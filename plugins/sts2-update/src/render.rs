use crate::steam::{NewsItem, clip_chars};
use base64::{Engine, engine::general_purpose::STANDARD};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder, Rgb, RgbImage};
use kovi::Message;
use kovi::chrono::{DateTime, FixedOffset};
use kovi_onebot::MessageRegistrar;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const IMAGE_WIDTH: u32 = 1200;
const MAX_IMAGE_HEIGHT: u32 = 16_000;
const SIDE_PADDING: f32 = 72.0;
const TOP_PADDING: f32 = 70.0;
const BOTTOM_PADDING: f32 = 68.0;
const CONTENT_WIDTH: f32 = IMAGE_WIDTH as f32 - SIDE_PADDING * 2.0;
const MAX_BODY_LINES: usize = 240;

const BACKGROUND: Rgb<u8> = Rgb([249, 245, 238]);
const ACCENT: Rgb<u8> = Rgb([155, 72, 58]);
const HEADING: Rgb<u8> = Rgb([42, 35, 32]);
const BODY: Rgb<u8> = Rgb([55, 49, 46]);
const MUTED: Rgb<u8> = Rgb([110, 96, 89]);
const LINK: Rgb<u8> = Rgb([122, 87, 72]);

pub struct PatchRenderer {
    regular_font_path: PathBuf,
    bold_font_path: PathBuf,
}

impl PatchRenderer {
    pub fn new(regular_font_path: PathBuf, bold_font_path: PathBuf) -> Self {
        Self {
            regular_font_path,
            bold_font_path,
        }
    }

    pub fn render_message(
        &self,
        item: &NewsItem,
        body: &str,
        max_body_chars: usize,
    ) -> Result<Message, String> {
        let png = self.render_png(item, body, max_body_chars)?;
        let encoded = STANDARD.encode(png);
        Ok(Message::new().add_image(&format!("base64://{encoded}")))
    }

    fn render_png(
        &self,
        item: &NewsItem,
        body: &str,
        max_body_chars: usize,
    ) -> Result<Vec<u8>, String> {
        let fonts = load_fonts(&self.regular_font_path, &self.bold_font_path)?;
        let body = clipped_body(body, max_body_chars);
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: SIDE_PADDING,
            y: TOP_PADDING,
            max_width: Some(CONTENT_WIDTH),
            line_height: 1.48,
            ..LayoutSettings::default()
        });

        append(&mut layout, &fonts, "杀戮尖塔 2 更新\n", 28.0, 1, ACCENT);
        append(
            &mut layout,
            &fonts,
            &format!("{}\n", item.localized_title()),
            46.0,
            1,
            HEADING,
        );
        append(
            &mut layout,
            &fonts,
            &format!(
                "{}  ·  {}\n\n",
                item.branch_label(),
                formatted_date(item.date)
            ),
            23.0,
            0,
            MUTED,
        );
        append(&mut layout, &fonts, "更新内容\n", 25.0, 1, ACCENT);
        append(&mut layout, &fonts, &format!("{body}\n\n"), 27.0, 0, BODY);
        append(
            &mut layout,
            &fonts,
            &format!("Steam 原文：{}", item.url),
            20.0,
            0,
            LINK,
        );

        let requested_height = (layout.height() + BOTTOM_PADDING).ceil() as u32;
        if requested_height > MAX_IMAGE_HEIGHT {
            return Err(format!(
                "更新图片高度 {requested_height}px 超过上限 {MAX_IMAGE_HEIGHT}px"
            ));
        }

        let mut image = RgbImage::from_pixel(IMAGE_WIDTH, requested_height.max(520), BACKGROUND);
        for y in 0..18 {
            for x in 0..IMAGE_WIDTH {
                *image.get_pixel_mut(x, y) = ACCENT;
            }
        }
        draw_layout(&mut image, &fonts, &layout);
        encode_png(&image)
    }
}

fn clipped_body(body: &str, max_chars: usize) -> String {
    let clipped = clip_chars(body.trim(), max_chars);
    let lines = clipped.lines().take(MAX_BODY_LINES + 1).collect::<Vec<_>>();
    if lines.len() <= MAX_BODY_LINES {
        clipped
    } else {
        format!("{}\n…", lines[..MAX_BODY_LINES].join("\n"))
    }
}

fn append(
    layout: &mut Layout<Rgb<u8>>,
    fonts: &[Font; 2],
    text: &str,
    size: f32,
    font: usize,
    color: Rgb<u8>,
) {
    layout.append(fonts, &TextStyle::with_user_data(text, size, font, color));
}

fn draw_layout(image: &mut RgbImage, fonts: &[Font; 2], layout: &Layout<Rgb<u8>>) {
    let mut glyph_cache = HashMap::new();
    for glyph in layout.glyphs() {
        if glyph.width == 0 || glyph.height == 0 {
            continue;
        }
        let coverage = glyph_cache.entry(glyph.key).or_insert_with(|| {
            let (_metrics, coverage) = fonts[glyph.font_index].rasterize_config(glyph.key);
            coverage
        });
        for (index, alpha) in coverage.iter().copied().enumerate() {
            if alpha == 0 {
                continue;
            }
            let x = glyph.x as i32 + (index % glyph.width) as i32;
            let y = glyph.y as i32 + (index / glyph.width) as i32;
            if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
                continue;
            }
            blend(
                image.get_pixel_mut(x as u32, y as u32),
                glyph.user_data,
                alpha,
            );
        }
    }
}

fn blend(target: &mut Rgb<u8>, color: Rgb<u8>, alpha: u8) {
    let alpha = alpha as u16;
    let inverse = 255 - alpha;
    for channel in 0..3 {
        target[channel] =
            ((color[channel] as u16 * alpha + target[channel] as u16 * inverse) / 255) as u8;
    }
}

fn encode_png(image: &RgbImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ColorType::Rgb8.into(),
        )
        .map_err(|e| format!("编码更新 PNG 图片失败: {e}"))?;
    Ok(png)
}

fn formatted_date(timestamp: i64) -> String {
    let shanghai = FixedOffset::east_opt(8 * 60 * 60).expect("UTC+8 是有效时区");
    DateTime::from_timestamp(timestamp, 0)
        .map(|date| {
            date.with_timezone(&shanghai)
                .format("%Y-%m-%d %H:%M（UTC+8）")
                .to_string()
        })
        .unwrap_or_else(|| "时间未知".to_string())
}

fn load_fonts(regular_path: &Path, bold_path: &Path) -> Result<[Font; 2], String> {
    Ok([
        load_font(regular_path, "常规")?,
        load_font(bold_path, "粗体")?,
    ])
}

fn load_font(path: &Path, label: &str) -> Result<Font, String> {
    let bytes =
        fs::read(path).map_err(|e| format!("读取{label}中文字体 {} 失败: {e}", path.display()))?;
    Font::from_bytes(bytes, FontSettings::default())
        .map_err(|e| format!("无法解析{label}中文字体 {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{MAX_BODY_LINES, PatchRenderer, clipped_body};
    use crate::steam::NewsItem;
    use std::path::PathBuf;

    fn item() -> NewsItem {
        NewsItem {
            gid: "1".to_string(),
            title: "Beta Patch Notes - v0.111.0".to_string(),
            url: "https://example.com/source".to_string(),
            contents: String::new(),
            date: 1_786_669_622,
            tags: vec!["patchnotes".to_string()],
        }
    }

    fn local_font() -> Option<PathBuf> {
        [
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
    }

    #[test]
    fn renders_png_and_applies_body_limit() {
        let Some(font) = local_font() else {
            return;
        };
        let renderer = PatchRenderer::new(font.clone(), font);
        let png = renderer
            .render_png(&item(), &"加强 机械骑士\n".repeat(2_000), 800)
            .unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() > 10_000);
        if let Ok(path) = std::env::var("STS2_UPDATE_PREVIEW_PATH") {
            std::fs::write(path, png).unwrap();
        }
    }

    #[test]
    fn excessive_source_lines_are_clipped() {
        let body = (0..300).map(|_| "一行").collect::<Vec<_>>().join("\n");
        let clipped = clipped_body(&body, 8_000);
        assert_eq!(clipped.lines().count(), MAX_BODY_LINES + 1);
        assert!(clipped.ends_with('…'));
    }
}
