mod config;
mod state;
mod steam;

use config::Config;
use kovi::{PluginBuilder, log::error, tokio::sync::Mutex};
use kovi_onebot::EventRegistrar;
use state::State;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use steam::{Cs2Stats, SteamClient, stat_value};

const HELP: &str = "🎮 CS2 Steam 查询\nCS2绑定 <Steam64或数字社区链接>\nCS2战绩 —— 查询自己的绑定\nCS2战绩 <Steam64或数字社区链接> —— 直接查询\nCS2解绑\nCS2帮助\n支持 https://steamcommunity.com/profiles/<Steam64>，暂不支持 /id/ 自定义链接。绑定只保存查询目标，不验证账号所有权。\n公开时长与历史累计数据可能包含 CS:GO 数据；不提供最近对局、段位或赛季战绩。";

struct Cs2Plugin {
    api: SteamClient,
    state_path: PathBuf,
    state: Mutex<State>,
    // Serializes network queries and caps upstream load for all users together.
    queries: Mutex<QueryState>,
}
#[derive(Default)]
struct QueryState {
    last_request: Option<Instant>,
    cache: HashMap<String, (Instant, String)>,
}
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Bind(String),
    Stats(Option<String>),
    Unbind,
    Help,
}

impl Cs2Plugin {
    async fn handle(&self, user_id: i64, command: Command) -> Result<String, String> {
        match command {
            Command::Help => Ok(HELP.into()),
            Command::Unbind => self
                .update_binding(user_id, None)
                .await
                .map(|_| "已解除 Steam 绑定。".into()),
            Command::Bind(input) => {
                let id = require_id(&input)?;
                // Validate existence using the same cached lookup, while permitting unavailable game details.
                self.lookup(&id).await?;
                self.update_binding(user_id, Some(id.clone())).await?;
                Ok(format!("已绑定查询目标：{id}\n发送“CS2战绩”即可查询。"))
            }
            Command::Stats(input) => {
                let id = match input {
                    Some(input) => require_id(&input)?,
                    None => self
                        .state
                        .lock()
                        .await
                        .bindings
                        .get(&user_id)
                        .cloned()
                        .ok_or("尚未绑定，请发送：CS2绑定 <Steam64或数字社区链接>")?,
                };
                self.lookup(&id).await
            }
        }
    }

    async fn update_binding(&self, user_id: i64, id: Option<String>) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        match id {
            Some(id) => {
                next.bindings.insert(user_id, id);
            }
            None => {
                next.bindings.remove(&user_id);
            }
        }
        next.save(&self.state_path)
            .map_err(|_| "绑定保存失败，请稍后重试。")?;
        *state = next;
        Ok(())
    }

    async fn lookup(&self, id: &str) -> Result<String, String> {
        let mut queries = self
            .queries
            .try_lock()
            .map_err(|_| "正在查询 Steam，请稍后再试。")?;
        queries
            .cache
            .retain(|_, (time, _)| time.elapsed() < Duration::from_secs(60));
        if let Some((_, result)) = queries.cache.get(id) {
            return Ok(result.clone());
        }
        if queries
            .last_request
            .is_some_and(|time| time.elapsed() < Duration::from_secs(3))
        {
            return Err("查询过于频繁，请稍等 3 秒再试。".into());
        }
        queries.last_request = Some(Instant::now());
        let result = format_stats(id, &self.api.fetch(id).await?);
        queries
            .cache
            .insert(id.into(), (Instant::now(), result.clone()));
        Ok(result)
    }
}

fn parse_command(text: &str) -> Option<Command> {
    let text = text.trim();
    let text = text
        .get(..3)
        .filter(|p| p.eq_ignore_ascii_case("cs2"))
        .map(|_| &text[3..])?;
    match text {
        "帮助" | " help" => Some(Command::Help),
        "解绑" => Some(Command::Unbind),
        "战绩" => Some(Command::Stats(None)),
        "绑定" => Some(Command::Help),
        _ => {
            let (verb, arg) = text.split_once(char::is_whitespace)?;
            let arg = arg.trim();
            match verb {
                "绑定" if !arg.is_empty() => Some(Command::Bind(arg.into())),
                "战绩" if !arg.is_empty() => Some(Command::Stats(Some(arg.into()))),
                _ => None,
            }
        }
    }
}

fn normalize_steam_id(input: &str) -> Option<String> {
    let input = input.trim();
    let id = if input.starts_with("https://") {
        input
            .strip_prefix("https://steamcommunity.com/profiles/")?
            .trim_end_matches('/')
    } else {
        input
    };
    let value: u64 = id.parse().ok()?;
    // Public universe, individual account, desktop instance; nonzero 32-bit account id.
    if id.len() == 17
        && id.bytes().all(|c| c.is_ascii_digit())
        && value >> 32 == 0x01100001
        && value as u32 != 0
    {
        Some(id.into())
    } else {
        None
    }
}
fn require_id(input: &str) -> Result<String, String> {
    normalize_steam_id(input).ok_or_else(|| "请使用有效的 Steam64 或 https://steamcommunity.com/profiles/<Steam64> 数字链接；暂不支持自定义 /id/ 链接。".into())
}

fn format_stats(steam_id: &str, stats: &Cs2Stats) -> String {
    let status = match stats.player.personastate {
        Some(0) => "离线或隐身",
        Some(1) => "在线",
        Some(2) => "忙碌",
        Some(3) => "离开",
        Some(4) => "打盹",
        Some(5) => "想交易",
        Some(6) => "想游玩",
        _ => "不可用",
    };
    let hours = |minutes: Option<u64>| {
        minutes
            .map(|m| format!("{:.1} 小时", m as f64 / 60.0))
            .unwrap_or_else(|| "不可用".into())
    };
    let total = hours(stats.game.as_ref().and_then(|g| g.playtime_forever));
    let recent = hours(stats.game.as_ref().and_then(|g| g.playtime_2weeks));
    let value = |key| {
        stat_value(&stats.stats, key)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "不可用".into())
    };
    let name: String = stats
        .player
        .personaname
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let mut text = format!(
        "🎮 CS2 Steam 资料\n玩家：{name}\n状态：{status}\nSteam64：{steam_id}\n总时长：{total}\n近两周时长：{recent}\n\n累计击杀：{}\n累计死亡：{}\n累计爆头击杀：{}\n累计获胜回合：{}\n\nhttps://steamcommunity.com/profiles/{steam_id}\n历史累计数据可能包含 CS:GO，不代表当前赛季；不提供最近对局或段位。",
        value("total_kills"),
        value("total_deaths"),
        value("total_kills_headshot"),
        value("total_wins")
    );
    if let Some(error) = &stats.game_error {
        text.push_str(&format!("\n时长查询失败：{error}"));
    }
    if let Some(error) = &stats.stats_error {
        text.push_str(&format!("\n累计统计查询失败：{error}"));
    }
    if stats.game.is_none() || stats.stats.is_empty() {
        text.push_str(
            "\n缺失数据可能因游戏详情未公开、未游玩或接口未提供；请检查 Steam 隐私设置。",
        );
    }
    text
}

#[kovi::plugin]
async fn main() {
    let bot = PluginBuilder::get_runtime_bot();
    let data_path = bot.get_data_path();
    let setup = || -> Result<Cs2Plugin, String> {
        let config = Config::load(&data_path)?;
        let timeout = config.timeout_secs();
        let api = SteamClient::new(config.api_base_url, config.api_key, timeout)?;
        let state_path = data_path.join("state.json");
        let state = State::load(&state_path)?;
        Ok(Cs2Plugin {
            api,
            state_path,
            state: Mutex::new(state),
            queries: Mutex::new(QueryState::default()),
        })
    };
    let plugin = match setup() {
        Ok(plugin) => Arc::new(plugin),
        Err(_) => {
            error!("CS2 插件无法启动，请检查配置格式、API 地址及数据目录权限。");
            return;
        }
    };
    PluginBuilder::on_msg(move |event| {
        let plugin = plugin.clone();
        async move {
            if event.user_id == event.self_id {
                return;
            }
            if let Some(command) = event.borrow_text().and_then(parse_command) {
                let reply = plugin
                    .handle(event.user_id, command)
                    .await
                    .unwrap_or_else(|e| format!("操作失败：{e}"));
                event.reply(reply);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_require_boundaries() {
        assert_eq!(
            parse_command("cs2绑定 76561198000000000"),
            Some(Command::Bind("76561198000000000".into()))
        );
        assert_eq!(parse_command("CS2战绩"), Some(Command::Stats(None)));
        assert_eq!(parse_command("Cs2解绑"), Some(Command::Unbind));
        assert_eq!(parse_command("CS2战绩很好"), None);
        assert_eq!(parse_command("中文聊天"), None);
    }
    #[test]
    fn validates_individual_steam64_and_exact_host() {
        assert_eq!(
            normalize_steam_id("76561198000000000"),
            Some("76561198000000000".into())
        );
        assert_eq!(
            normalize_steam_id("https://steamcommunity.com/profiles/76561198000000000/"),
            Some("76561198000000000".into())
        );
        for input in [
            "https://steamcommunity.com/id/test",
            "https://evil.test/profiles/76561198000000000",
            "11111111111111111",
            "76561197960265728",
            "76561198000000000/path",
        ] {
            assert!(normalize_steam_id(input).is_none(), "{input}");
        }
    }
    #[test]
    fn missing_data_is_not_zero_or_recent_match_history() {
        let data = Cs2Stats {
            player: steam::Player {
                personaname: "测试".into(),
                personastate: None,
            },
            game: None,
            stats: vec![],
            game_error: None,
            stats_error: None,
        };
        let text = format_stats("76561198000000000", &data);
        assert!(text.contains("总时长：不可用"));
        assert!(text.contains("累计爆头击杀：不可用"));
        assert!(text.contains("累计获胜回合"));
        assert!(text.contains("CS:GO"));
    }
}
