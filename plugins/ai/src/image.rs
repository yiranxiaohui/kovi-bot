use kovi::serde_json::{json, Value};
use reqwest::{multipart, Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use crate::config::Config;

/// 单次请求超时(秒)
const REQUEST_TIMEOUT_SECS: u64 = 150;
/// 最大尝试次数(yunnet gpt-image-2 上游偶发 502,需重试)
const MAX_ATTEMPTS: u32 = 3;

#[derive(Serialize, Debug, Deserialize)]
pub struct ImageResponse {
    pub data: Vec<ImageData>,
}

#[derive(Serialize, Debug, Deserialize)]
pub struct ImageData {
    pub b64_json: Option<String>,
}

/// 从响应中取出第一张图的 base64,空则返回空串。
pub fn extract_b64(resp: &ImageResponse) -> String {
    resp.data
        .first()
        .and_then(|d| d.b64_json.clone())
        .unwrap_or_default()
}

/// 从错误响应体里提取 `error.message`,取不到则给出兜底文案。
pub fn extract_error(text: &str, status: u16) -> String {
    kovi::serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| format!("HTTP {} 无图片数据", status))
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP client 构建失败: {e}"))
}

/// 文生图:调用 OpenAI 兼容的 images/generations,带超时与重试。
/// 成功返回 `Ok(base64://<b64>)`,失败返回 `Err(错误信息)`。
pub async fn gen_image(prompt: &str, config: &Config) -> Result<String, String> {
    let url = format!(
        "{}/images/generations",
        config.image_base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": config.image_model,
        "prompt": prompt,
        "size": config.image_size,
        "n": 1,
    });
    let client = build_client()?;
    let key = &config.image_api_key;
    send_with_retry(|| {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {key}"))
            .header("Content-Type", "application/json")
            .json(&body)
    })
    .await
}

/// 改图:下载来源图片后,multipart 调用 images/edits 按描述编辑。
/// 成功返回 `Ok(base64://<b64>)`,失败返回 `Err(错误信息)`。
pub async fn edit_image(prompt: &str, image_url: &str, config: &Config) -> Result<String, String> {
    let client = build_client()?;
    // 下载来源图片
    let bytes = client
        .get(image_url)
        .send()
        .await
        .map_err(|e| format!("下载图片失败: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取图片失败: {e}"))?
        .to_vec();

    let url = format!(
        "{}/images/edits",
        config.image_base_url.trim_end_matches('/')
    );
    let key = &config.image_api_key;
    send_with_retry(|| {
        // multipart Form 不可复用,每次尝试重新构造(image bytes 可克隆)
        let part = multipart::Part::bytes(bytes.clone())
            .file_name("image.png")
            .mime_str("image/png")
            .expect("固定 mime 必定合法");
        let form = multipart::Form::new()
            .text("model", config.image_model.clone())
            .text("prompt", prompt.to_string())
            .text("size", config.image_size.clone())
            .text("n", "1")
            .part("image", part);
        client
            .post(&url)
            .header("Authorization", format!("Bearer {key}"))
            .multipart(form)
    })
    .await
}

/// 共用的发送+重试逻辑:`build` 每次尝试构造一个新的请求。
/// 成功返回 `Ok(base64://<b64>)`,失败返回 `Err(错误信息)`。
async fn send_with_retry<F>(build: F) -> Result<String, String>
where
    F: Fn() -> RequestBuilder,
{
    let mut last_err = String::from("未知错误");
    for attempt in 1..=MAX_ATTEMPTS {
        match try_once(build()).await {
            Ok(b64) if !b64.is_empty() => return Ok(format!("base64://{b64}")),
            Ok(_) => last_err = "响应中无图片数据".to_string(),
            // 终态错误(如内容审核拒绝 / 参数错误):是确定结果,立即返回不再重试
            Err((false, msg)) => return Err(msg),
            // 可重试错误(502 / 超时 / 网络):继续下一次尝试
            Err((true, msg)) => last_err = msg,
        }
        kovi::log::warn!("图片请求第 {attempt}/{MAX_ATTEMPTS} 次失败(可重试): {last_err}");
    }
    Err(last_err)
}

/// 单次请求。返回:
/// - `Ok(b64)`:成功(b64 可能为空串)
/// - `Err((retryable, msg))`:retryable=true 表示传输/服务端错误可重试,
///   false 表示模型给出了确定结果(审核拒绝、参数错误等 4xx),不应重试
async fn try_once(req: RequestBuilder) -> Result<String, (bool, String)> {
    let resp = req.send().await.map_err(|e| {
        // 网络错误 / 超时:可重试
        let msg = if e.is_timeout() {
            "请求超时".to_string()
        } else {
            format!("请求错误: {e}")
        };
        (true, msg)
    })?;

    let status = resp.status();
    let code = status.as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| (true, format!("读取响应失败: {e}")))?;

    if let Ok(img) = kovi::serde_json::from_str::<ImageResponse>(&text) {
        let b64 = extract_b64(&img);
        if !b64.is_empty() {
            return Ok(b64);
        }
    }
    // 5xx 服务端错误 / 429 限流 → 可重试;4xx(内容审核、参数)→ 终态不重试
    let retryable = status.is_server_error() || code == 429;
    Err((retryable, extract_error(&text, code)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_b64_ok() {
        let resp = ImageResponse {
            data: vec![ImageData { b64_json: Some("AAAA".to_string()) }],
        };
        assert_eq!(extract_b64(&resp), "AAAA");
    }

    #[test]
    fn extract_b64_empty() {
        let resp = ImageResponse { data: vec![] };
        assert_eq!(extract_b64(&resp), "");
        let resp = ImageResponse { data: vec![ImageData { b64_json: None }] };
        assert_eq!(extract_b64(&resp), "");
    }

    #[test]
    fn parse_from_json() {
        let raw = r#"{"data":[{"b64_json":"Zm9v"}]}"#;
        let resp: ImageResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(extract_b64(&resp), "Zm9v");
    }

    #[test]
    fn extract_error_from_upstream() {
        let raw = r#"{"error":{"message":"image generation failed","type":"server_error","code":"upstream_error"}}"#;
        assert_eq!(extract_error(raw, 502), "image generation failed");
    }

    #[test]
    fn extract_error_fallback() {
        assert_eq!(extract_error("not json", 502), "HTTP 502 无图片数据");
    }
}
