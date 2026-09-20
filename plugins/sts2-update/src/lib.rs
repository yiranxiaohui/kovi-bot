mod config;
mod render;
mod state;
mod steam;
mod terminology;
mod translate;

use config::Config;
use kovi::log::{error, info, warn};
use kovi::tokio::sync::Mutex;
use kovi::{Message, PluginBuilder, RuntimeBot};
use kovi_onebot::{EventRegistrar, GroupMsgEvent, OnebotTrait};
use render::PatchRenderer;
use state::{BranchMode, PersistentState, Subscription};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use steam::{NewsItem, clean_contents, clip_chars};

const TRANSLATION_CACHE_NAMESPACE: &str = "official-zhs-v1";

const HELP: &str = "\
⚔️ 杀戮尖塔 2 更新提醒
━━━━━━━━━━━━━━
尖塔2订阅 [全部/正式版/测试版]  —— 开启或修改本群订阅
尖塔2取消订阅  —— 关闭本群订阅
尖塔2最新 [全部/正式版/测试版]  —— 查看最新中文更新
尖塔2状态  —— 查看本群订阅状态
尖塔2帮助  —— 查看本说明
（订阅设置仅限群主、群管理员或机器人管理员）";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Subscribe(BranchMode),
    Unsubscribe,
    Latest(BranchMode),
    Status,
    Help,
    InvalidMode,
}

struct Notifier {
    bot: Arc<RuntimeBot>,
    config: Config,
    client: reqwest::Client,
    state_path: PathBuf,
    ai_config_path: PathBuf,
    renderer: PatchRenderer,
    state: Mutex<PersistentState>,
}

impl Notifier {
    fn new(bot: Arc<RuntimeBot>, config: Config, state: PersistentState) -> Result<Self, String> {
        let data_path = bot.get_data_path();
        let client = reqwest::Client::builder()
            .user_agent("kovi-sts2-update/0.1")
            .timeout(Duration::from_secs(config.request_timeout_secs.max(10)))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
        let renderer = PatchRenderer::new(
            config.image_font_regular_path(&data_path),
            config.image_font_bold_path(&data_path),
        );
        Ok(Self {
            bot,
            ai_config_path: config.ai_config_path(&data_path),
            config,
            client,
            renderer,
            state_path: data_path.join("state.json"),
            state: Mutex::new(state),
        })
    }

    async fn fetch_news(&self) -> Result<Vec<NewsItem>, String> {
        steam::fetch_news(
            &self.client,
            &self.config.steam_api_url,
            self.config.news_count(),
        )
        .await
    }

    async fn subscribe(&self, group_id: i64, mode: BranchMode) -> Result<String, String> {
        {
            let state = self.state.lock().await;
            if state
                .subscription(group_id)
                .is_some_and(|subscription| subscription.mode == mode)
            {
                return Ok(format!("本群已经订阅：{}", mode.label()));
            }
        }

        let baseline = self.fetch_news().await;
        let mut state = self.state.lock().await;
        let subscription = match state.subscription_mut(group_id) {
            Some(subscription) => {
                subscription.mode = mode;
                subscription.initialized = false;
                subscription
            }
            None => {
                state.subscriptions.push(Subscription::new(group_id, mode));
                state
                    .subscription_mut(group_id)
                    .expect("刚插入的订阅必须存在")
            }
        };
        let baseline_ok = match baseline {
            Ok(items) => {
                subscription
                    .seen_gids
                    .extend(items.into_iter().map(|item| item.gid));
                subscription.initialized = true;
                true
            }
            Err(e) => {
                warn!("订阅群 {group_id} 时建立 Steam 基线失败: {e}");
                false
            }
        };
        state.save(&self.state_path)?;

        let suffix = if baseline_ok {
            "已记录当前版本，之后有新补丁就会自动翻译并提醒。"
        } else {
            "Steam 当前不可用，插件会继续自动重试，建立基线后开始提醒。"
        };
        Ok(format!("订阅成功：{}\n{suffix}", mode.label()))
    }

    async fn unsubscribe(&self, group_id: i64) -> Result<bool, String> {
        let mut state = self.state.lock().await;
        let old_len = state.subscriptions.len();
        state
            .subscriptions
            .retain(|subscription| subscription.group_id != group_id);
        let removed = state.subscriptions.len() != old_len;
        if removed {
            state.save(&self.state_path)?;
        }
        Ok(removed)
    }

    async fn status(&self, group_id: i64) -> String {
        let state = self.state.lock().await;
        match state.subscription(group_id) {
            Some(subscription) => format!(
                "本群已订阅杀戮尖塔 2 更新\n范围：{}\n检查间隔：{} 分钟\n已记录公告：{} 条",
                subscription.mode.label(),
                self.config.check_interval_secs() / 60,
                subscription.seen_gids.len()
            ),
            None => "本群尚未订阅杀戮尖塔 2 更新。\n发送“尖塔2订阅”即可开启。".to_string(),
        }
    }

    async fn latest(&self, mode: BranchMode) -> Result<Message, String> {
        let item = self
            .fetch_news()
            .await?
            .into_iter()
            .filter(|item| mode.accepts(item.is_beta()))
            .max_by_key(|item| item.date)
            .ok_or_else(|| "Steam 暂时没有符合条件的补丁公告".to_string())?;
        self.render_news(&item).await
    }

    async fn check_updates(&self) -> Result<(), String> {
        let items = self.fetch_news().await?;
        let mut state_changed = false;
        let deliveries = {
            let mut state = self.state.lock().await;
            let mut deliveries = Vec::new();
            for subscription in &mut state.subscriptions {
                if !subscription.initialized {
                    subscription
                        .seen_gids
                        .extend(items.iter().map(|item| item.gid.clone()));
                    subscription.initialized = true;
                    state_changed = true;
                    info!("已为群 {} 建立杀戮尖塔 2 公告基线", subscription.group_id);
                    continue;
                }
                deliveries.extend(
                    items
                        .iter()
                        .filter(|item| {
                            subscription.mode.accepts(item.is_beta())
                                && !subscription.seen_gids.contains(&item.gid)
                        })
                        .cloned()
                        .map(|item| (subscription.group_id, item)),
                );
            }
            if state_changed {
                state.save(&self.state_path)?;
            }
            deliveries
        };

        for (group_id, item) in deliveries {
            if !self.still_wants(group_id, &item).await {
                continue;
            }
            let message = match self.render_news(&item).await {
                Ok(message) => message,
                Err(e) => {
                    error!(
                        "生成杀戮尖塔 2 补丁 {} 图片失败，将在下轮重试: {e}",
                        item.gid
                    );
                    continue;
                }
            };
            match self.bot.send_group_msg_return(group_id, message).await {
                Ok(_) => {
                    let mut state = self.state.lock().await;
                    if let Some(subscription) = state.subscription_mut(group_id) {
                        subscription.seen_gids.insert(item.gid.clone());
                        if let Err(e) = state.save(&self.state_path) {
                            error!("补丁 {} 已发送，但去重状态保存失败: {e}", item.gid);
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "向群 {group_id} 推送杀戮尖塔 2 补丁 {} 失败，将在下轮重试: {:?}",
                        item.gid, e
                    );
                }
            }
        }
        Ok(())
    }

    async fn still_wants(&self, group_id: i64, item: &NewsItem) -> bool {
        self.state
            .lock()
            .await
            .subscription(group_id)
            .is_some_and(|subscription| {
                subscription.mode.accepts(item.is_beta())
                    && !subscription.seen_gids.contains(&item.gid)
            })
    }

    async fn render_news(&self, item: &NewsItem) -> Result<Message, String> {
        let cache_key = format!("{TRANSLATION_CACHE_NAMESPACE}:{}", item.gid);
        let cached = {
            self.state
                .lock()
                .await
                .translations
                .get(&cache_key)
                .cloned()
        };
        let body = match cached {
            Some(body) => body,
            None if self.config.translation_enabled => {
                match translate::translate_news(
                    &self.client,
                    &self.ai_config_path,
                    item,
                    self.config.max_source_chars(),
                    self.config.translation_max_tokens,
                )
                .await
                {
                    Ok(body) => {
                        let mut state = self.state.lock().await;
                        state.translations.insert(cache_key, body.clone());
                        if let Err(e) = state.save(&self.state_path) {
                            error!("保存补丁 {} 的翻译缓存失败: {e}", item.gid);
                        }
                        body
                    }
                    Err(e) => {
                        warn!("补丁 {} 翻译失败，发送英文回退内容: {e}", item.gid);
                        english_fallback(item)
                    }
                }
            }
            None => english_fallback(item),
        };
        self.renderer
            .render_message(item, &body, self.config.max_message_chars())
    }

    fn can_manage(&self, event: &GroupMsgEvent) -> bool {
        if matches!(event.sender.role.as_deref(), Some("owner" | "admin")) {
            return true;
        }
        self.bot.get_all_admin().is_ok_and(|admins| {
            admins
                .iter()
                .any(|id| id.to_string() == event.user_id.to_string())
        })
    }
}

#[kovi::plugin]
async fn main() {
    let bot = PluginBuilder::get_runtime_bot();
    let data_path = bot.get_data_path();
    let config = match Config::load(&data_path) {
        Ok(config) => config,
        Err(e) => {
            error!("杀戮尖塔 2 更新插件无法启动: {e}");
            return;
        }
    };
    let state_path = data_path.join("state.json");
    let state = match PersistentState::load(&state_path) {
        Ok(mut state) => {
            if state.retain_translation_namespace(TRANSLATION_CACHE_NAMESPACE)
                && let Err(e) = state.save(&state_path)
            {
                error!("清理旧版杀戮尖塔 2 翻译缓存失败: {e}");
                return;
            }
            state
        }
        Err(e) => {
            error!("杀戮尖塔 2 更新插件无法启动: {e}");
            return;
        }
    };
    let notifier = match Notifier::new(bot, config, state) {
        Ok(notifier) => Arc::new(notifier),
        Err(e) => {
            error!("杀戮尖塔 2 更新插件无法启动: {e}");
            return;
        }
    };

    let scheduled = notifier.clone();
    kovi::spawn(async move {
        let mut interval = kovi::tokio::time::interval(Duration::from_secs(
            scheduled.config.check_interval_secs(),
        ));
        interval.set_missed_tick_behavior(kovi::tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = scheduled.check_updates().await {
                warn!("检查杀戮尖塔 2 更新失败: {e}");
            }
        }
    });

    PluginBuilder::on_group_msg(move |event| {
        let notifier = notifier.clone();
        async move {
            let Some(command) = event.borrow_text().and_then(parse_command) else {
                return;
            };
            match command {
                Command::Subscribe(mode) => {
                    if !notifier.can_manage(&event) {
                        event.reply("只有群主、群管理员或机器人管理员可以修改订阅。");
                        return;
                    }
                    match notifier.subscribe(event.group_id, mode).await {
                        Ok(message) => event.reply(message),
                        Err(e) => {
                            error!("修改群 {} 的尖塔 2 订阅失败: {e}", event.group_id);
                            event.reply("订阅保存失败，请稍后再试。")
                        }
                    }
                }
                Command::Unsubscribe => {
                    if !notifier.can_manage(&event) {
                        event.reply("只有群主、群管理员或机器人管理员可以修改订阅。");
                        return;
                    }
                    match notifier.unsubscribe(event.group_id).await {
                        Ok(true) => event.reply("已取消本群的杀戮尖塔 2 更新提醒。"),
                        Ok(false) => event.reply("本群原本就没有订阅。"),
                        Err(e) => {
                            error!("取消群 {} 的尖塔 2 订阅失败: {e}", event.group_id);
                            event.reply("取消订阅失败，请稍后再试。")
                        }
                    }
                }
                Command::Latest(mode) => {
                    event.reply("正在获取并翻译最新补丁，请稍等…");
                    match notifier.latest(mode).await {
                        Ok(message) => event.reply(message),
                        Err(e) => {
                            warn!("手动查询杀戮尖塔 2 最新补丁失败: {e}");
                            event.reply(format!("获取最新补丁失败：{e}"))
                        }
                    }
                }
                Command::Status => event.reply(notifier.status(event.group_id).await),
                Command::Help => event.reply(HELP),
                Command::InvalidMode => event.reply("订阅范围只能是：全部、正式版或测试版。"),
            }
        }
    });
}

fn parse_command(text: &str) -> Option<Command> {
    let text = text.trim().trim_start_matches('/').trim();
    let mut parts = text.split_whitespace();
    let name = parts.next()?;
    let argument = parts.next();
    if parts.next().is_some() {
        return None;
    }

    match name {
        "尖塔2订阅" | "杀戮尖塔2订阅" => parse_mode(argument)
            .map(Command::Subscribe)
            .or(Some(Command::InvalidMode)),
        "尖塔2取消订阅" | "杀戮尖塔2取消订阅" => Some(Command::Unsubscribe),
        "尖塔2最新" | "杀戮尖塔2最新" => parse_mode(argument)
            .map(Command::Latest)
            .or(Some(Command::InvalidMode)),
        "尖塔2状态" | "杀戮尖塔2状态" => Some(Command::Status),
        "尖塔2帮助" | "杀戮尖塔2帮助" => Some(Command::Help),
        _ => None,
    }
}

fn parse_mode(argument: Option<&str>) -> Option<BranchMode> {
    match argument.unwrap_or("全部").to_ascii_lowercase().as_str() {
        "全部" | "all" => Some(BranchMode::All),
        "正式" | "正式版" | "stable" => Some(BranchMode::Stable),
        "测试" | "测试版" | "beta" => Some(BranchMode::Beta),
        _ => None,
    }
}

fn english_fallback(item: &NewsItem) -> String {
    format!(
        "中文翻译暂时不可用，以下为英文内容：\n{}",
        clip_chars(&clean_contents(&item.contents), 1_200)
    )
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};
    use crate::state::BranchMode;

    #[test]
    fn chinese_commands_and_modes_are_parsed() {
        assert_eq!(
            parse_command("尖塔2订阅"),
            Some(Command::Subscribe(BranchMode::All))
        );
        assert_eq!(
            parse_command("/尖塔2订阅 正式版"),
            Some(Command::Subscribe(BranchMode::Stable))
        );
        assert_eq!(
            parse_command("杀戮尖塔2最新 beta"),
            Some(Command::Latest(BranchMode::Beta))
        );
        assert_eq!(parse_command("尖塔2订阅 其他"), Some(Command::InvalidMode));
        assert_eq!(parse_command("普通聊天"), None);
    }
}
