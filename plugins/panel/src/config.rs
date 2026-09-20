use std::sync::Arc;
use kovi::RuntimeBot;
use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use toml::toml;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub token: String,
}

pub fn read_config(bot: Arc<RuntimeBot>) -> Config {
    let data_path = bot.get_data_path();
    let config_toml_path = data_path.join("config.toml");
    let default_config = toml! {
        port = 8080
        token = "change-me"
    };
    let config = load_toml_data(default_config, config_toml_path).unwrap();
    let config: Config = toml::from_str(&config.to_string()).unwrap();
    config
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_port_and_token() {
        let c: Config = toml::from_str("port = 9000\ntoken = \"abc\"\n").unwrap();
        assert_eq!(c.port, 9000);
        assert_eq!(c.token, "abc");
    }
}
