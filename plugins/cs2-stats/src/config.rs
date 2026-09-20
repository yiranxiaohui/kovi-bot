use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub api_key: String,
    pub api_base_url: String,
    pub request_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_base_url: "https://api.steampowered.com".to_string(),
            request_timeout_secs: 15,
        }
    }
}

impl Config {
    pub fn load(data_path: &Path) -> Result<Self, String> {
        load_toml_data(Self::default(), data_path.join("config.toml"))
            .map_err(|e| format!("读取配置失败: {e}"))
    }

    pub fn timeout_secs(&self) -> u64 {
        self.request_timeout_secs.clamp(5, 120)
    }
}
