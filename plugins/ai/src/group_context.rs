use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 单个群的主动搭话状态:最近消息滚动窗口 + 上次咨询 LLM 的时刻。
struct GroupState {
    messages: VecDeque<String>,
    last_consult: Option<Instant>,
}

impl GroupState {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            last_consult: None,
        }
    }
}

/// 按群隔离的进程内群聊上下文缓冲(供主动搭话判断使用)。与 ChatMemory 独立。
#[derive(Default)]
pub struct GroupContext {
    inner: Mutex<HashMap<i64, GroupState>>,
}

impl GroupContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条群消息(建议格式 `昵称: 内容`),超过 `cap` 条时淘汰最旧。
    pub fn push(&self, group_id: i64, line: String, cap: usize) {
        let mut map = self.inner.lock().unwrap();
        let st = map.entry(group_id).or_insert_with(GroupState::new);
        st.messages.push_back(line);
        while st.messages.len() > cap {
            st.messages.pop_front();
        }
    }

    /// 取该群最近消息(时间序副本)。
    pub fn recent(&self, group_id: i64) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .get(&group_id)
            .map(|s| s.messages.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 冷却→概率门控:决定这条消息是否应咨询 LLM。命中则把 `last_consult` 更新为 `now`。
    /// `roll` 为 `[0,100)` 的掷骰值(由调用方注入,便于测试与避免引入 rand 依赖)。
    pub fn try_consult(
        &self,
        group_id: i64,
        cooldown: Duration,
        probability: f64,
        roll: u32,
        now: Instant,
    ) -> bool {
        let mut map = self.inner.lock().unwrap();
        let st = map.entry(group_id).or_insert_with(GroupState::new);
        if !should_consult(st.last_consult, now, cooldown, roll, probability) {
            return false;
        }
        st.last_consult = Some(now);
        true
    }
}

/// 纯函数门控:冷却未过 → false;否则按概率(`roll < probability*100`)判定。
/// `last_consult == None`(该群从未咨询过)视为冷却已过。
pub fn should_consult(
    last_consult: Option<Instant>,
    now: Instant,
    cooldown: Duration,
    roll: u32,
    probability: f64,
) -> bool {
    if let Some(last) = last_consult {
        if now.duration_since(last) < cooldown {
            return false;
        }
    }
    (roll as f64) < probability * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_evicts_oldest_beyond_cap() {
        let ctx = GroupContext::new();
        for i in 0..5 {
            ctx.push(1, format!("消息{i}"), 3);
        }
        let recent = ctx.recent(1);
        assert_eq!(recent, vec!["消息2", "消息3", "消息4"]);
    }

    #[test]
    fn recent_isolated_per_group() {
        let ctx = GroupContext::new();
        ctx.push(1, "群一".to_string(), 10);
        ctx.push(2, "群二".to_string(), 10);
        assert_eq!(ctx.recent(1), vec!["群一"]);
        assert_eq!(ctx.recent(2), vec!["群二"]);
        assert!(ctx.recent(3).is_empty());
    }

    #[test]
    fn should_consult_first_time_depends_on_probability() {
        let now = Instant::now();
        let cd = Duration::from_secs(60);
        // 无 last_consult:仅看概率。roll 30 < 0.3*100=30? 否(严格小于)
        assert!(!should_consult(None, now, cd, 30, 0.3));
        assert!(should_consult(None, now, cd, 29, 0.3));
        // 概率 1.0 时任何 roll 都命中
        assert!(should_consult(None, now, cd, 99, 1.0));
        // 概率 0 时都不命中
        assert!(!should_consult(None, now, cd, 0, 0.0));
    }

    #[test]
    fn should_consult_within_cooldown_rejected() {
        let now = Instant::now();
        let cd = Duration::from_secs(60);
        let last = now - Duration::from_secs(30); // 冷却内
        // 即便掷骰必中(概率 1.0)也应拒绝
        assert!(!should_consult(Some(last), now, cd, 0, 1.0));
    }

    #[test]
    fn should_consult_after_cooldown_then_probability() {
        let now = Instant::now();
        let cd = Duration::from_secs(60);
        let last = now - Duration::from_secs(90); // 冷却已过
        assert!(should_consult(Some(last), now, cd, 10, 0.3));
        assert!(!should_consult(Some(last), now, cd, 50, 0.3));
    }

    #[test]
    fn try_consult_updates_last_consult_on_hit() {
        let ctx = GroupContext::new();
        let now = Instant::now();
        let cd = Duration::from_secs(60);
        // 首次命中(概率 1.0)
        assert!(ctx.try_consult(7, cd, 1.0, 0, now));
        // 紧接着冷却内:必拒
        assert!(!ctx.try_consult(7, cd, 1.0, 0, now));
        // 冷却过后再命中
        let later = now + Duration::from_secs(61);
        assert!(ctx.try_consult(7, cd, 1.0, 0, later));
    }
}
