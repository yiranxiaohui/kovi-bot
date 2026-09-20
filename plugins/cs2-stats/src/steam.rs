use serde::{Deserialize, de::DeserializeOwned};

pub struct SteamClient {
    client: reqwest::Client,
    base: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    response: T,
}
#[derive(Debug, Deserialize)]
struct Players {
    players: Vec<Player>,
}
#[derive(Debug, Deserialize)]
pub struct Player {
    pub personaname: String,
    pub personastate: Option<u8>,
}
#[derive(Debug, Deserialize)]
struct OwnedGames {
    games: Option<Vec<Game>>,
}
#[derive(Debug, Deserialize)]
pub struct Game {
    pub appid: u32,
    pub playtime_forever: Option<u64>,
    pub playtime_2weeks: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct StatsResponse {
    playerstats: Option<PlayerStats>,
}
#[derive(Debug, Deserialize)]
struct PlayerStats {
    stats: Option<Vec<Stat>>,
}
#[derive(Debug, Deserialize)]
pub struct Stat {
    pub name: String,
    pub value: u64,
}
#[derive(Debug)]
pub struct Cs2Stats {
    pub player: Player,
    pub game: Option<Game>,
    pub stats: Vec<Stat>,
    pub game_error: Option<String>,
    pub stats_error: Option<String>,
}

impl SteamClient {
    pub fn new(base: String, key: String, timeout: u64) -> Result<Self, String> {
        let url = reqwest::Url::parse(&base).map_err(|_| "Steam API 地址无效")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Steam API 地址必须是无凭据和查询参数的 HTTPS 地址".into());
        }
        let client = reqwest::Client::builder()
            .user_agent("kovi-plugin-cs2-stats/0.1")
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(timeout))
            .build()
            .map_err(|_| "创建 HTTP 客户端失败")?;
        Ok(Self {
            client,
            base: base.trim_end_matches('/').into(),
            key: key.trim().into(),
        })
    }

    // Never include reqwest errors or response bodies: request URLs contain the API key.
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, String> {
        if self.key.is_empty() {
            return Err(
                "尚未配置 Steam Web API Key，请管理员填写插件 config.toml 的 api_key 并重启插件。"
                    .into(),
            );
        }
        let response = self
            .client
            .get(format!("{}/{path}", self.base))
            .query(&[("key", self.key.as_str())])
            .query(params)
            .send()
            .await
            .map_err(|_| "Steam 网络请求失败或超时，请稍后重试。")?;
        match response.status().as_u16() {
            200..=299 => response
                .json()
                .await
                .map_err(|_| "Steam 返回的数据格式异常。".into()),
            401 | 403 => Err("Steam 拒绝访问，请检查 API Key 或资料权限。".into()),
            429 => Err("Steam 请求过于频繁，请稍后重试。".into()),
            _ => Err(format!(
                "Steam 接口暂不可用（HTTP {}）。",
                response.status().as_u16()
            )),
        }
    }

    pub async fn player(&self, id: &str) -> Result<Player, String> {
        self.get::<Envelope<Players>>("ISteamUser/GetPlayerSummaries/v0002/", &[("steamids", id)])
            .await?
            .response
            .players
            .into_iter()
            .next()
            .ok_or_else(|| "找不到该 Steam 用户。".into())
    }

    pub async fn fetch(&self, id: &str) -> Result<Cs2Stats, String> {
        let player = self.player(id).await?;
        let owned_params = [
            ("steamid", id),
            ("include_played_free_games", "1"),
            ("appids_filter[0]", "730"),
        ];
        let stats_params = [("steamid", id), ("appid", "730")];
        let (owned, stats) = kovi::tokio::join!(
            self.get::<Envelope<OwnedGames>>("IPlayerService/GetOwnedGames/v0001/", &owned_params),
            self.get::<StatsResponse>("ISteamUserStats/GetUserStatsForGame/v0002/", &stats_params)
        );
        let game_error = owned.as_ref().err().cloned();
        let stats_error = stats.as_ref().err().cloned();
        let game = owned
            .ok()
            .and_then(|r| r.response.games)
            .and_then(|games| games.into_iter().find(|g| g.appid == 730));
        let stats = stats
            .ok()
            .and_then(|r| r.playerstats)
            .and_then(|s| s.stats)
            .unwrap_or_default();
        Ok(Cs2Stats {
            player,
            game,
            stats,
            game_error,
            stats_error,
        })
    }
}

pub fn stat_value(stats: &[Stat], name: &str) -> Option<u64> {
    stats
        .iter()
        .find(|stat| stat.name == name)
        .map(|stat| stat.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_games_are_not_zero_hours() {
        let r: Envelope<OwnedGames> = serde_json::from_str(r#"{"response":{}}"#).unwrap();
        assert!(r.response.games.is_none());
        let r: StatsResponse =
            serde_json::from_str(r#"{"playerstats":{"success":false}}"#).unwrap();
        assert!(r.playerstats.unwrap().stats.is_none());
    }
    #[test]
    fn reads_actual_steam_stat_names() {
        let r: StatsResponse = serde_json::from_str(r#"{"playerstats":{"stats":[{"name":"total_kills_headshot","value":12},{"name":"total_wins","value":100}]}}"#).unwrap();
        let stats = r.playerstats.unwrap().stats.unwrap();
        assert_eq!(stat_value(&stats, "total_kills_headshot"), Some(12));
        assert_eq!(stat_value(&stats, "total_deaths"), None);
    }
}
