use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub steam_api_url: String,
    pub check_interval_secs: u64,
    pub request_timeout_secs: u64,
    pub news_count: usize,
    pub max_source_chars: usize,
    pub max_message_chars: usize,
    pub translation_enabled: bool,
    pub translation_max_tokens: u32,
    pub ai_config_path: String,
    pub image_font_regular_path: String,
    pub image_font_bold_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            steam_api_url: "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/".to_string(),
            check_interval_secs: 300,
            request_timeout_secs: 120,
            news_count: 50,
            max_source_chars: 20_000,
            max_message_chars: 3_800,
            translation_enabled: true,
            translation_max_tokens: 2_500,
            ai_config_path: "../kovi-plugin-ai/config.toml".to_string(),
            image_font_regular_path: "../kovi-plugin-meme/resources/fonts/NotoSansSC-Regular.ttf"
                .to_string(),
            image_font_bold_path: "../kovi-plugin-meme/resources/fonts/NotoSansSC-Bold.ttf"
                .to_string(),
        }
    }
}

impl Config {
    pub fn load(data_path: &Path) -> Result<Self, String> {
        load_toml_data(Self::default(), data_path.join("config.toml"))
            .map_err(|e| format!("读取配置失败: {e}"))
    }

    pub fn check_interval_secs(&self) -> u64 {
        self.check_interval_secs.max(60)
    }

    pub fn news_count(&self) -> usize {
        self.news_count.clamp(10, 100)
    }

    pub fn max_source_chars(&self) -> usize {
        self.max_source_chars.clamp(1_000, 50_000)
    }

    pub fn max_message_chars(&self) -> usize {
        self.max_message_chars.clamp(800, 8_000)
    }

    pub fn ai_config_path(&self, data_path: &Path) -> PathBuf {
        resolve_path(data_path, &self.ai_config_path)
    }

    pub fn image_font_regular_path(&self, data_path: &Path) -> PathBuf {
        resolve_path(data_path, &self.image_font_regular_path)
    }

    pub fn image_font_bold_path(&self, data_path: &Path) -> PathBuf {
        resolve_path(data_path, &self.image_font_bold_path)
    }
}

fn resolve_path(data_path: &Path, path: &str) -> PathBuf {
    let configured = PathBuf::from(path);
    if configured.is_absolute() {
        configured
    } else {
        data_path.join(configured)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn unsafe_limits_are_clamped() {
        let config = Config {
            check_interval_secs: 1,
            news_count: 1_000,
            max_source_chars: 10,
            max_message_chars: 10,
            ..Config::default()
        };

        assert_eq!(config.check_interval_secs(), 60);
        assert_eq!(config.news_count(), 100);
        assert_eq!(config.max_source_chars(), 1_000);
        assert_eq!(config.max_message_chars(), 800);
    }
}
