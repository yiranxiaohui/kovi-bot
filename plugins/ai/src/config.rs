use std::sync::Arc;
use kovi::log::debug;
use kovi::RuntimeBot;
use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use toml::toml;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Config {
    pub kind: Kind,
    pub model: String,
    pub api_key: String,
    // 聊天(OpenAI 兼容协议)使用的接口地址;Gemini 分支忽略此字段
    #[serde(default = "default_base_url")]
    pub base_url: String,
    // 生图(OpenAI 兼容,默认 yunnet 中转站 gpt-image-2)
    pub image_base_url: String,
    pub image_api_key: String,
    pub image_model: String,
    pub image_size: String,
    // 主动搭话模式(免@,按群白名单启用)
    #[serde(default)]
    pub proactive_groups: Vec<i64>,
    #[serde(default = "default_proactive_cooldown")]
    pub proactive_cooldown: u64,
    #[serde(default = "default_proactive_probability")]
    pub proactive_probability: f64,
    #[serde(default = "default_proactive_context_size")]
    pub proactive_context_size: usize,
    #[serde(default = "default_proactive_persona")]
    pub proactive_persona: String,
    // 会话历史 token 预算(估算),超出触发 LLM 压缩摘要
    #[serde(default = "default_context_budget")]
    pub context_budget: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Kind {
    Gemini,
    OpenAI,
}

fn default_base_url() -> String {
    "https://api.yunnet.top/v1".to_string()
}

fn default_context_budget() -> usize {
    crate::memory::DEFAULT_CONTEXT_BUDGET
}

fn default_proactive_cooldown() -> u64 {
    60
}

fn default_proactive_probability() -> f64 {
    0.3
}

fn default_proactive_context_size() -> usize {
    20
}

fn default_proactive_persona() -> String {
    "你叫小云,是这个群里的一个普通群友。说话口语、简短、随意,像真人发消息一样,\
一次只说一两句,不用 markdown,不用书面语。你不是 AI,绝不承认自己是机器人或程序。\
如果当前话题和你无关、或没有你想说的,就安静待着。"
        .to_string()
}

pub fn read_config(bot: Arc<RuntimeBot>) -> Config {
    let data_path = bot.get_data_path();
    let config_toml_path = data_path.join("config.toml");
    let default_config = toml! {
        kind = "OpenAI"
        model = "grok-4.5"
        api_key = "${API_KEY}"
        base_url = "https://api.yunnet.top/v1"
        image_base_url = "https://api.yunnet.top/v1"
        image_api_key = "${IMAGE_API_KEY}"
        image_model = "gpt-image-2"
        image_size = "1024x1024"
    };
    let config = load_toml_data(default_config, config_toml_path).unwrap();
    debug!("{}", config.to_string());
    let config: Config = toml::from_str(&config.to_string()).unwrap();
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_budget_defaults_to_12k() {
        let cfg: Config = toml::from_str(
            r#"
            kind = "OpenAI"
            model = "grok-4.5"
            api_key = "k"
            image_base_url = "https://x/v1"
            image_api_key = "k"
            image_model = "gpt-image-2"
            image_size = "1024x1024"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.context_budget, 12_000);
    }
}
