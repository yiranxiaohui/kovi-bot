//! 群图片进上下文:提取消息里的图片 → 下载转 base64 喂给视觉模型;
//! 多轮记忆里不存原图,只存「[图片: caption]」文字占位,控制上下文体积。

use kovi::Message;
use rig::OneOrMany;
use rig::message::{ImageDetail, ImageMediaType, UserContent};

/// 单条消息最多带进上下文的图片数,多余的忽略。
pub const MAX_IMAGES: usize = 2;
/// 单张图片下载大小上限(字节),超过跳过,避免 base64 撑爆请求。
const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
/// caption 最大长度(字符),超出截断,防模型话痨污染记忆。
const MAX_CAPTION_CHARS: usize = 100;

/// 取消息里所有图片的 http(s) url(napcat 的 image 段带 url 字段),按出现顺序。
pub fn image_urls(message: &Message) -> Vec<String> {
    message
        .iter()
        .filter(|seg| seg.kind == "image")
        .filter_map(|seg| {
            seg.data
                .get("url")
                .or_else(|| seg.data.get("file"))
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .map(|s| s.to_string())
        })
        .collect()
}

/// 从 get_msg 返回的原始 OneBot 段数组(字段名为 `type`)取所有图片 http(s) url。
pub fn image_urls_from_json_segments(segments: &[kovi::serde_json::Value]) -> Vec<String> {
    segments
        .iter()
        .filter(|seg| seg.get("type").and_then(|t| t.as_str()) == Some("image"))
        .filter_map(|seg| {
            seg.get("data")
                .and_then(|d| d.get("url").or_else(|| d.get("file")))
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
                .map(|s| s.to_string())
        })
        .collect()
}

/// 从 get_msg 返回的原始 OneBot 段数组拼出纯文本(text 段按序连接)。
pub fn text_from_json_segments(segments: &[kovi::serde_json::Value]) -> String {
    segments
        .iter()
        .filter(|seg| seg.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|seg| {
            seg.get("data")
                .and_then(|d| d.get("text"))
                .and_then(|v| v.as_str())
        })
        .collect()
}

/// 被引用消息的最大文字长度(字符),超出截断。
const MAX_QUOTE_CHARS: usize = 200;

/// 引用消息标注:带被引用人昵称与内容,喂给模型/存记忆用。
/// 文字超 200 字符截断;无文字的纯图引用标成 `(图)`。
pub fn format_quote(name: &str, text: &str, has_image: bool) -> String {
    let text = text.trim();
    if text.is_empty() {
        if has_image {
            format!("[引用 {name} 的消息(图)]")
        } else {
            format!("[引用 {name} 的消息]")
        }
    } else {
        format!("[引用 {name} 的消息: {}]", truncate_chars(text, MAX_QUOTE_CHARS))
    }
}

/// 按魔数嗅探图片格式,认不出返回 None(不再假 JPEG 兜底,
/// 否则 QQ 图床过期返回的 HTML 错误页会被硬发给上游吃 400)。
pub fn sniff_media_type(bytes: &[u8]) -> Option<ImageMediaType> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(ImageMediaType::PNG)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageMediaType::JPEG)
    } else if bytes.starts_with(b"GIF8") {
        Some(ImageMediaType::GIF)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageMediaType::WEBP)
    } else {
        None
    }
}

/// 把下载到的图片整理成可发给上游的 (字节, 格式):
/// grok2api 只认 JPG/PNG/WebP/ICO,GIF(QQ 表情包大头)取第一帧转码 PNG;
/// PNG/JPEG/WEBP 原样通过;认不出或转码失败报错(调用方跳过该图)。
pub fn prepare_image(bytes: &[u8]) -> Result<(Vec<u8>, ImageMediaType), String> {
    match sniff_media_type(bytes) {
        Some(ImageMediaType::GIF) => {
            let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Gif)
                .map_err(|e| format!("GIF 解码失败: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png)
                .map_err(|e| format!("PNG 编码失败: {e}"))?;
            Ok((out.into_inner(), ImageMediaType::PNG))
        }
        Some(media) => Ok((bytes.to_vec(), media)),
        None => Err("不是可识别的图片格式".to_string()),
    }
}

/// 把 caption 拼进记忆行:有 caption → `text [图片: caption]`;无 → `text [图片]`;
/// text 为空时只留图片标注。
pub fn with_image_note(text: &str, caption: Option<&str>) -> String {
    let note = match caption {
        Some(c) => format!("[图片: {c}]"),
        None => "[图片]".to_string(),
    };
    let text = text.trim();
    if text.is_empty() {
        note
    } else {
        format!("{text} {note}")
    }
}

/// 按字符数截断(不破坏 UTF-8),超长时结尾加 `…`。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// 组装当轮多模态用户消息:文本在前,图片在后;两者都空时退化为空文本。
pub fn build_user_message(text: &str, images: Vec<UserContent>) -> rig::completion::Message {
    let mut items = Vec::new();
    if !text.is_empty() || images.is_empty() {
        items.push(UserContent::text(text));
    }
    items.extend(images);
    rig::completion::Message::User {
        content: OneOrMany::many(items).expect("至少含一个文本项,不会为空"),
    }
}

/// 规整 caption:trim + 截断,空的返回 None。
pub fn clean_caption(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(truncate_chars(t, MAX_CAPTION_CHARS))
    }
}

/// 下载图片并转成视觉消息内容,单张失败只跳过不影响其余。
pub async fn fetch_images(urls: &[String]) -> Vec<UserContent> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            kovi::log::warn!("图片下载 client 构建失败: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for url in urls {
        match fetch_one(&client, url).await {
            Ok(content) => out.push(content),
            Err(e) => kovi::log::warn!("下载图片失败({url}): {e}"),
        }
    }
    out
}

async fn fetch_one(client: &reqwest::Client, url: &str) -> Result<UserContent, String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!("图片过大({} bytes)", bytes.len()));
    }
    let (bytes, media_type) = prepare_image(&bytes)?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(UserContent::image_base64(
        b64,
        Some(media_type),
        Some(ImageDetail::Auto),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kovi::message::Segment;
    use kovi::serde_json::json;
    use rig::message::UserContent;

    fn img_seg(url: &str) -> Segment {
        Segment::new("image", json!({"url": url, "file": "a.png"}))
    }

    #[test]
    fn json_image_urls_multi_http_only() {
        let segs = vec![
            json!({"type": "image", "data": {"url": "https://x/1.png"}}),
            json!({"type": "text", "data": {"text": "hi"}}),
            json!({"type": "image", "data": {"file": "local.png"}}),
            json!({"type": "image", "data": {"file": "https://x/2.jpg"}}),
        ];
        assert_eq!(
            image_urls_from_json_segments(&segs),
            vec!["https://x/1.png", "https://x/2.jpg"]
        );
    }

    #[test]
    fn json_text_concat_in_order() {
        let segs = vec![
            json!({"type": "text", "data": {"text": "早上"}}),
            json!({"type": "image", "data": {"url": "https://x/1.png"}}),
            json!({"type": "text", "data": {"text": "好"}}),
        ];
        assert_eq!(text_from_json_segments(&segs), "早上好");
        assert_eq!(text_from_json_segments(&[]), "");
    }

    #[test]
    fn quote_with_text() {
        assert_eq!(
            format_quote("小红", "早上好", false),
            "[引用 小红 的消息: 早上好]"
        );
    }

    #[test]
    fn quote_image_only() {
        assert_eq!(format_quote("小红", "", true), "[引用 小红 的消息(图)]");
        assert_eq!(format_quote("小红", "  ", true), "[引用 小红 的消息(图)]");
    }

    #[test]
    fn quote_text_and_image_keeps_text() {
        assert_eq!(
            format_quote("小红", "看这个", true),
            "[引用 小红 的消息: 看这个]"
        );
    }

    #[test]
    fn quote_empty_no_image() {
        assert_eq!(format_quote("小红", "", false), "[引用 小红 的消息]");
    }

    #[test]
    fn quote_long_text_truncated() {
        let long = "长".repeat(300);
        let q = format_quote("小红", &long, false);
        assert!(q.contains(&"长".repeat(200)));
        assert!(!q.contains(&"长".repeat(201)));
        assert!(q.contains('…'));
    }

    #[test]
    fn image_urls_collects_all_http_images_in_order() {
        let msg = Message::from(vec![
            Segment::new("text", json!({"text": "看这个"})),
            img_seg("https://x/1.png"),
            img_seg("http://x/2.jpg"),
        ]);
        assert_eq!(image_urls(&msg), vec!["https://x/1.png", "http://x/2.jpg"]);
    }

    #[test]
    fn image_urls_skips_non_http_and_non_image() {
        let msg = Message::from(vec![
            Segment::new("image", json!({"file": "local.png"})),
            Segment::new("face", json!({"id": "1"})),
        ]);
        assert!(image_urls(&msg).is_empty());
    }

    #[test]
    fn image_urls_falls_back_to_file_field_when_http() {
        let msg = Message::from(vec![Segment::new(
            "image",
            json!({"file": "https://x/f.jpg"}),
        )]);
        assert_eq!(image_urls(&msg), vec!["https://x/f.jpg"]);
    }

    #[test]
    fn sniff_known_magics() {
        assert_eq!(
            sniff_media_type(&[0x89, b'P', b'N', b'G', 0, 0]),
            Some(ImageMediaType::PNG)
        );
        assert_eq!(
            sniff_media_type(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageMediaType::JPEG)
        );
        assert_eq!(sniff_media_type(b"GIF89a...."), Some(ImageMediaType::GIF));
        let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        webp.extend_from_slice(&[0; 4]);
        assert_eq!(sniff_media_type(&webp), Some(ImageMediaType::WEBP));
    }

    #[test]
    fn sniff_unknown_is_none() {
        // 不再假 JPEG 兜底:QQ 图床过期返回的 HTML 错误页会被上游以
        // 「not a valid JPG...」拒绝,认不出的直接不发
        assert_eq!(sniff_media_type(b"<html>expired</html>"), None);
        assert_eq!(sniff_media_type(&[]), None);
    }

    /// 1x1 透明 GIF(经典 43 字节)。
    fn tiny_gif() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
            .unwrap()
    }

    #[test]
    fn prepare_gif_transcodes_to_png() {
        let (bytes, media) = prepare_image(&tiny_gif()).unwrap();
        // grok 上游不认 GIF,转成 PNG 第一帧
        assert_eq!(media, ImageMediaType::PNG);
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn prepare_passthrough_for_supported_formats() {
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
        let (bytes, media) = prepare_image(&png).unwrap();
        assert_eq!(media, ImageMediaType::PNG);
        assert_eq!(bytes, png.to_vec());
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 9];
        assert_eq!(prepare_image(&jpg).unwrap().1, ImageMediaType::JPEG);
    }

    #[test]
    fn prepare_rejects_unknown_and_broken_gif() {
        assert!(prepare_image(b"<html>404</html>").is_err());
        // GIF 魔数但内容坏掉:转码失败应报错而不是硬发
        assert!(prepare_image(b"GIF89a garbage").is_err());
    }

    #[test]
    fn with_note_text_and_caption() {
        assert_eq!(
            with_image_note("小明: 看", Some("一只橘猫")),
            "小明: 看 [图片: 一只橘猫]"
        );
    }

    #[test]
    fn with_note_no_caption_placeholder() {
        assert_eq!(with_image_note("看", None), "看 [图片]");
    }

    #[test]
    fn with_note_empty_text_only_note() {
        assert_eq!(with_image_note("", Some("风景照")), "[图片: 风景照]");
        assert_eq!(with_image_note("  ", None), "[图片]");
    }

    #[test]
    fn truncate_within_limit_unchanged() {
        assert_eq!(truncate_chars("短文本", 10), "短文本");
    }

    #[test]
    fn truncate_over_limit_adds_ellipsis() {
        assert_eq!(truncate_chars("一二三四五", 3), "一二三…");
    }

    #[test]
    fn clean_caption_trims_and_rejects_empty() {
        assert_eq!(clean_caption("  一只猫  "), Some("一只猫".to_string()));
        assert_eq!(clean_caption("   "), None);
        assert_eq!(clean_caption(""), None);
    }

    #[test]
    fn clean_caption_truncates_long() {
        let long = "很".repeat(200);
        let got = clean_caption(&long).unwrap();
        assert_eq!(got.chars().count(), MAX_CAPTION_CHARS + 1); // 100 字 + …
        assert!(got.ends_with('…'));
    }

    #[test]
    fn build_message_text_first_then_images() {
        let img = UserContent::image_base64("QUFB", Some(ImageMediaType::PNG), Some(ImageDetail::Auto));
        let msg = build_user_message("你好", vec![img.clone()]);
        let rig::completion::Message::User { content } = msg else {
            panic!("应为 User 消息");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], UserContent::Text(_)));
        assert!(matches!(items[1], UserContent::Image(_)));
    }

    #[test]
    fn build_message_empty_text_images_only() {
        let img = UserContent::image_base64("QUFB", Some(ImageMediaType::PNG), Some(ImageDetail::Auto));
        let msg = build_user_message("", vec![img]);
        let rig::completion::Message::User { content } = msg else {
            panic!("应为 User 消息");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], UserContent::Image(_)));
    }

    #[test]
    fn build_message_all_empty_degrades_to_empty_text() {
        let msg = build_user_message("", Vec::new());
        let rig::completion::Message::User { content } = msg else {
            panic!("应为 User 消息");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], UserContent::Text(_)));
    }
}
