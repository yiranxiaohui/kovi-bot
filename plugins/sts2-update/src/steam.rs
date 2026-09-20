use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

const APP_ID: u32 = 2_868_840;

#[derive(Clone, Debug, Deserialize)]
pub struct NewsItem {
    pub gid: String,
    pub title: String,
    pub url: String,
    pub contents: String,
    pub date: i64,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl NewsItem {
    pub fn is_patch(&self) -> bool {
        let title = self.title.to_ascii_lowercase();
        self.tags.iter().any(|tag| tag == "patchnotes")
            || title.contains("patch notes")
            || title.contains("hotfix")
            || title.starts_with("major update")
    }

    pub fn is_beta(&self) -> bool {
        self.title.to_ascii_lowercase().starts_with("beta ")
    }

    pub fn branch_label(&self) -> &'static str {
        if self.is_beta() {
            "测试分支（Beta）"
        } else {
            "正式分支"
        }
    }

    pub fn localized_title(&self) -> String {
        let replacements = [
            ("Beta Hotfix Patch Notes", "测试分支热修补丁说明"),
            ("Beta Patch Notes", "测试分支补丁说明"),
            ("Hotfix Patch Notes", "热修补丁说明"),
            ("Patch Notes", "补丁说明"),
            ("Major Update", "重大更新"),
        ];
        for (english, chinese) in replacements {
            if let Some(rest) = self.title.strip_prefix(english) {
                return format!("{chinese}{rest}");
            }
        }
        self.title.clone()
    }
}

#[derive(Debug, Deserialize)]
struct SteamResponse {
    appnews: AppNews,
}

#[derive(Debug, Deserialize)]
struct AppNews {
    newsitems: Vec<NewsItem>,
}

pub async fn fetch_news(
    client: &reqwest::Client,
    api_url: &str,
    count: usize,
) -> Result<Vec<NewsItem>, String> {
    let response = client
        .get(api_url)
        .query(&[
            ("appid", APP_ID.to_string()),
            ("count", count.to_string()),
            ("maxlength", "0".to_string()),
            ("feeds", "steam_community_announcements".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("请求 Steam 新闻失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Steam 新闻接口返回错误: {e}"))?;
    let mut items = response
        .json::<SteamResponse>()
        .await
        .map_err(|e| format!("解析 Steam 新闻失败: {e}"))?
        .appnews
        .newsitems;
    items.retain(NewsItem::is_patch);
    items.sort_by_key(|item| item.date);
    Ok(items)
}

pub fn clean_contents(input: &str) -> String {
    static STEAM_IMAGE: OnceLock<Regex> = OnceLock::new();
    static HTML_TAG: OnceLock<Regex> = OnceLock::new();
    static BBCODE_TAG: OnceLock<Regex> = OnceLock::new();
    static BLANK_LINES: OnceLock<Regex> = OnceLock::new();

    let text = STEAM_IMAGE
        .get_or_init(|| Regex::new(r"\{STEAM_CLAN_IMAGE\}/\S+").unwrap())
        .replace_all(input, " ");
    let text = HTML_TAG
        .get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap())
        .replace_all(&text, " ");
    let text = BBCODE_TAG
        .get_or_init(|| Regex::new(r"(?i)\[/?[a-z][^\]]*\]").unwrap())
        .replace_all(&text, "");
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    BLANK_LINES
        .get_or_init(|| Regex::new(r"\n[\t ]*\n(?:[\t ]*\n)+").unwrap())
        .replace_all(text.trim(), "\n\n")
        .to_string()
}

pub fn clip_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut clipped = input.chars().take(max_chars).collect::<String>();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::{NewsItem, clean_contents, clip_chars};

    fn item(title: &str, tags: &[&str]) -> NewsItem {
        NewsItem {
            gid: "1".to_string(),
            title: title.to_string(),
            url: "https://example.com".to_string(),
            contents: String::new(),
            date: 1,
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    #[test]
    fn patch_filter_excludes_newsletters() {
        assert!(item("Beta Patch Notes - v0.111.0", &[]).is_patch());
        assert!(item("Anything", &["patchnotes"]).is_patch());
        assert!(item("Major Update #2", &[]).is_patch());
        assert!(!item("The Neowsletter - August 2026", &[]).is_patch());
    }

    #[test]
    fn branch_and_title_are_localized() {
        let beta = item("Beta Hotfix Patch Notes - v0.109.1", &[]);
        assert!(beta.is_beta());
        assert_eq!(beta.localized_title(), "测试分支热修补丁说明 - v0.109.1");

        let stable = item("Major Update #2 - v0.107.1", &[]);
        assert!(!stable.is_beta());
        assert_eq!(stable.localized_title(), "重大更新 #2 - v0.107.1");
    }

    #[test]
    fn steam_markup_is_removed() {
        let source = "[h1]Changes[/h1]\n{STEAM_CLAN_IMAGE}/123/a.png\n<b>Fix</b> &amp; polish";
        assert_eq!(clean_contents(source), "Changes\n \n Fix  & polish");
    }

    #[test]
    fn clipping_respects_utf8_boundaries() {
        assert_eq!(clip_chars("杀戮尖塔", 3), "杀戮尖…");
        assert_eq!(clip_chars("short", 10), "short");
    }

    #[test]
    fn steam_api_fixture_deserializes() {
        let response: super::SteamResponse = serde_json::from_str(
            r#"{
                "appnews": {
                    "appid": 2868840,
                    "newsitems": [{
                        "gid": "1840944183778277",
                        "title": "Beta Patch Notes - v0.111.0",
                        "url": "https://example.com/patch",
                        "contents": "CONTENT & BALANCE",
                        "date": 1786669622,
                        "feedname": "steam_community_announcements",
                        "tags": ["patchnotes"]
                    }],
                    "count": 1
                }
            }"#,
        )
        .unwrap();

        let patch = &response.appnews.newsitems[0];
        assert_eq!(patch.gid, "1840944183778277");
        assert!(patch.is_patch());
        assert!(patch.is_beta());
    }
}
