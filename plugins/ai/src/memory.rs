use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use rig::completion::Message;

/// 会话历史的默认 token 预算(估算值)。
pub const DEFAULT_CONTEXT_BUDGET: usize = 12_000;

/// 存入历史的单条消息字符数上限。
/// 防投毒放大:越狱后模型会被要求每轮完整复述污染文本,若原样入库,
/// 窗口里每一轮都携带全量污染内容,滚动淘汰永远洗不掉;截断后放大链路被切断。
pub const MAX_STORED_CHARS: usize = 300;

/// 超过 [`MAX_STORED_CHARS`] 的文本按字符截断并补省略号(字符边界安全)。
pub fn clip_for_history(s: &str) -> String {
    match s.char_indices().nth(MAX_STORED_CHARS) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

/// 估算文本 token 数:CJK 等非 ASCII 字符按 1 token/字,ASCII 按 4 字符/token。
/// 没有真 tokenizer,预算控制用一致的估算口径即可。
pub fn estimate_tokens(s: &str) -> usize {
    let (ascii, other) = s.chars().fold((0usize, 0usize), |(a, o), c| {
        if c.is_ascii() { (a + 1, o) } else { (a, o + 1) }
    });
    ascii.div_ceil(4) + other
}

/// 按会话键(群/私聊)隔离的进程内多轮对话记忆,(role, content) 文本存储。
/// 容量按估算 token 预算控制,超预算由调用方走「LLM 压缩摘要」流程收缩。
#[derive(Default)]
pub struct ChatMemory {
    inner: Mutex<HashMap<String, VecDeque<(String, String)>>>,
}

impl ChatMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取该会话的历史副本(时间序),转换为模型消息。
    pub fn history(&self, key: &str) -> Vec<Message> {
        self.inner
            .lock()
            .unwrap()
            .get(key)
            .map(|q| {
                q.iter()
                    .map(|(role, content)| {
                        if role == "assistant" {
                            Message::assistant(content)
                        } else {
                            Message::user(content)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 追加一轮问答。不在此处淘汰:超预算由 compression_batch/replace_prefix 收缩。
    pub fn append(&self, key: &str, user_msg: &str, assistant_msg: &str) {
        let mut map = self.inner.lock().unwrap();
        let q = map.entry(key.to_string()).or_default();
        q.push_back(("user".to_string(), user_msg.to_string()));
        q.push_back(("assistant".to_string(), assistant_msg.to_string()));
    }

    /// 清空该会话的记忆。
    pub fn clear(&self, key: &str) {
        self.inner.lock().unwrap().remove(key);
    }

    /// 该会话当前的估算 token 总量。
    pub fn session_tokens(&self, key: &str) -> usize {
        self.inner
            .lock()
            .unwrap()
            .get(key)
            .map(|q| q.iter().map(|(_, c)| estimate_tokens(c)).sum())
            .unwrap_or(0)
    }

    /// 取一批待压缩的最旧消息:压掉它们之后,剩余部分 ≤ keep_tokens。
    /// 至少保留最近一轮不压;不需要压缩时返回 None。返回 (条数, 消息副本)。
    pub fn compression_batch(
        &self,
        key: &str,
        keep_tokens: usize,
    ) -> Option<(usize, Vec<(String, String)>)> {
        let map = self.inner.lock().unwrap();
        let q = map.get(key)?;
        let total: usize = q.iter().map(|(_, c)| estimate_tokens(c)).sum();
        if total <= keep_tokens {
            return None;
        }
        let mut acc = 0;
        let mut count = 0;
        for (_, c) in q.iter() {
            if total - acc <= keep_tokens {
                break;
            }
            acc += estimate_tokens(c);
            count += 1;
        }
        let count = count.min(q.len().saturating_sub(2));
        if count == 0 {
            return None;
        }
        Some((count, q.iter().take(count).cloned().collect()))
    }

    /// 把最前面 count 条消息替换为一条摘要(user 角色,置于队首),
    /// 并在同一把锁内返回替换后的完整窗口快照(供落库 compact,保证与内存一致)。
    /// count 超出现存长度时按现存长度截断,不会误删压缩期间新追加的消息。
    pub fn replace_prefix(
        &self,
        key: &str,
        count: usize,
        summary: &str,
    ) -> Vec<(String, String)> {
        let mut map = self.inner.lock().unwrap();
        match map.get_mut(key) {
            Some(q) => {
                let n = count.min(q.len());
                q.drain(..n);
                q.push_front(("user".to_string(), summary.to_string()));
                q.iter().cloned().collect()
            }
            None => Vec::new(),
        }
    }

    /// 直接丢弃最前面 count 条消息(压缩失败时的兜底,保证不无限增长)。
    pub fn drop_prefix(&self, key: &str, count: usize) {
        let mut map = self.inner.lock().unwrap();
        if let Some(q) = map.get_mut(key) {
            let n = count.min(q.len());
            q.drain(..n);
        }
    }

    /// 启动时从持久层回填(role, content),替换该会话现有窗口;
    /// 超出 budget 从头部丢弃,单条同样截断。仅供启动阶段一次性调用。
    pub fn restore(&self, key: &str, rows: &[(String, String)], budget: usize) {
        let mut q: VecDeque<(String, String)> = rows
            .iter()
            .map(|(role, content)| (role.clone(), clip_for_history(content)))
            .collect();
        while q.iter().map(|(_, c)| estimate_tokens(c)).sum::<usize>() > budget {
            if q.pop_front().is_none() {
                break;
            }
        }
        self.inner.lock().unwrap().insert(key.to_string(), q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_cjk_and_ascii() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("你好世界"), 4); // CJK 1 字 1 token
        assert_eq!(estimate_tokens("abcd"), 1); // ASCII 4 字符 1 token
        assert_eq!(estimate_tokens("abcde"), 2); // 向上取整
        assert_eq!(estimate_tokens("你好ab"), 3);
    }

    #[test]
    fn clip_short_text_unchanged() {
        assert_eq!(clip_for_history("你好"), "你好");
        let exact: String = "字".repeat(MAX_STORED_CHARS);
        assert_eq!(clip_for_history(&exact), exact);
    }

    #[test]
    fn clip_long_text_truncated_at_char_boundary() {
        let long: String = "污".repeat(MAX_STORED_CHARS + 500);
        let clipped = clip_for_history(&long);
        assert_eq!(clipped.chars().count(), MAX_STORED_CHARS + 1); // 上限字数 + 省略号
        assert!(clipped.ends_with('…'));
        assert!(clipped.starts_with('污'));
    }

    #[test]
    fn append_then_history_roundtrip() {
        let m = ChatMemory::new();
        m.append("group:1", "你好", "你好呀");
        let h = m.history("group:1");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0], Message::user("你好"));
        assert_eq!(h[1], Message::assistant("你好呀"));
    }

    #[test]
    fn history_of_unknown_key_is_empty() {
        let m = ChatMemory::new();
        assert!(m.history("group:404").is_empty());
    }

    #[test]
    fn session_tokens_accumulate() {
        let m = ChatMemory::new();
        assert_eq!(m.session_tokens("group:1"), 0);
        m.append("group:1", "你好", "你好呀"); // 2 + 3 = 5
        assert_eq!(m.session_tokens("group:1"), 5);
        m.append("group:1", "在吗", "在的"); // +4
        assert_eq!(m.session_tokens("group:1"), 9);
    }

    #[test]
    fn clear_removes_only_that_key() {
        let m = ChatMemory::new();
        m.append("group:1", "a", "b");
        m.append("user:9", "c", "d");
        m.clear("group:1");
        assert!(m.history("group:1").is_empty());
        assert_eq!(m.history("user:9").len(), 2);
    }

    #[test]
    fn compression_batch_none_when_under_keep() {
        let m = ChatMemory::new();
        m.append("group:1", "你好", "你好呀");
        assert!(m.compression_batch("group:1", 100).is_none());
        assert!(m.compression_batch("group:404", 100).is_none());
    }

    #[test]
    fn compression_batch_takes_oldest_until_under_keep() {
        let m = ChatMemory::new();
        for _ in 0..10 {
            // 每条 10 个 CJK 字 = 10 token,一轮 20 token,共 200
            m.append("group:1", &"问".repeat(10), &"答".repeat(10));
        }
        let (count, batch) = m.compression_batch("group:1", 60).unwrap();
        // 压掉之后剩余 ≤ 60 token:剩 6 条,压掉 14 条
        assert_eq!(count, 14);
        assert_eq!(batch.len(), 14);
        assert_eq!(batch[0].0, "user");
        // 剩余部分确实 ≤ 60
        let remaining = m.session_tokens("group:1")
            - batch.iter().map(|(_, c)| estimate_tokens(c)).sum::<usize>();
        assert!(remaining <= 60);
    }

    #[test]
    fn compression_batch_keeps_at_least_last_round() {
        let m = ChatMemory::new();
        m.append("group:1", &"问".repeat(50), &"答".repeat(50));
        // keep=0 也不能把仅有的一轮压掉
        assert!(m.compression_batch("group:1", 0).is_none());
        m.append("group:1", &"再".repeat(50), &"答".repeat(50));
        let (count, _) = m.compression_batch("group:1", 0).unwrap();
        assert_eq!(count, 2); // 只压第一轮,留最近一轮
    }

    #[test]
    fn replace_prefix_swaps_batch_for_summary() {
        let m = ChatMemory::new();
        m.append("group:1", "旧问1", "旧答1");
        m.append("group:1", "旧问2", "旧答2");
        m.append("group:1", "新问", "新答");
        m.replace_prefix("group:1", 4, "[早前对话摘要] 聊了旧话题");
        let h = m.history("group:1");
        assert_eq!(
            h,
            vec![
                Message::user("[早前对话摘要] 聊了旧话题"),
                Message::user("新问"),
                Message::assistant("新答"),
            ]
        );
    }

    #[test]
    fn replace_prefix_clamps_when_queue_shrank() {
        let m = ChatMemory::new();
        m.append("group:1", "a", "b");
        m.replace_prefix("group:1", 99, "[摘要]");
        assert_eq!(m.history("group:1"), vec![Message::user("[摘要]")]);
    }

    #[test]
    fn drop_prefix_discards_oldest() {
        let m = ChatMemory::new();
        m.append("group:1", "旧问", "旧答");
        m.append("group:1", "新问", "新答");
        m.drop_prefix("group:1", 2);
        assert_eq!(
            m.history("group:1"),
            vec![Message::user("新问"), Message::assistant("新答")]
        );
    }

    #[test]
    fn restore_rebuilds_history_in_order() {
        let m = ChatMemory::new();
        m.restore(
            "group:1",
            &[
                ("user".to_string(), "问".to_string()),
                ("assistant".to_string(), "答".to_string()),
            ],
            DEFAULT_CONTEXT_BUDGET,
        );
        let h = m.history("group:1");
        assert_eq!(h, vec![Message::user("问"), Message::assistant("答")]);
    }

    #[test]
    fn restore_replaces_rather_than_appends() {
        let m = ChatMemory::new();
        m.append("group:1", "旧问", "旧答");
        m.restore(
            "group:1",
            &[
                ("user".to_string(), "问".to_string()),
                ("assistant".to_string(), "答".to_string()),
            ],
            DEFAULT_CONTEXT_BUDGET,
        );
        let h = m.history("group:1");
        assert_eq!(h, vec![Message::user("问"), Message::assistant("答")]);
    }

    #[test]
    fn restore_trims_to_budget_from_front() {
        let m = ChatMemory::new();
        // 每条 10 token,共 10 条 = 100 token;预算 35 → 留最新 3 条
        let rows: Vec<(String, String)> = (0..10)
            .map(|i| {
                let role = if i % 2 == 0 { "user" } else { "assistant" };
                (role.to_string(), format!("{}{}", "字".repeat(9), i))
            })
            .collect();
        m.restore("group:1", &rows, 35);
        let h = m.history("group:1");
        assert_eq!(h.len(), 3);
        assert_eq!(
            h.last().unwrap(),
            &Message::assistant(format!("{}9", "字".repeat(9)))
        );
    }

    #[test]
    fn restore_clips_each_row() {
        let m = ChatMemory::new();
        let long = "污".repeat(MAX_STORED_CHARS + 100);
        m.restore(
            "group:1",
            &[("assistant".to_string(), long)],
            DEFAULT_CONTEXT_BUDGET,
        );
        let h = m.history("group:1");
        assert_eq!(h.len(), 1);
        // 存进来的是截断版
        assert_eq!(m.session_tokens("group:1"), MAX_STORED_CHARS + 1);
    }

    /// 打通 Store → ChatMemory::restore 的接缝:落库若干轮 → load_recent_all → restore,
    /// 断言 history() 与写入的 user/assistant 序列一致(角色映射正确、跨会话互不干扰)。
    #[test]
    fn store_load_then_memory_restore_roundtrip() {
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        store.append_round("group:1", "你好", "你好呀").unwrap();
        store.append_round("group:1", "在吗", "在的").unwrap();
        store.append_round("user:9", "hi", "hello").unwrap();

        let sessions = store.load_recent_all(400).unwrap();
        assert_eq!(sessions.len(), 2);

        let m = ChatMemory::new();
        for (key, rows) in &sessions {
            m.restore(key, rows, DEFAULT_CONTEXT_BUDGET);
        }

        assert_eq!(
            m.history("group:1"),
            vec![
                Message::user("你好"),
                Message::assistant("你好呀"),
                Message::user("在吗"),
                Message::assistant("在的"),
            ]
        );
        assert_eq!(
            m.history("user:9"),
            vec![Message::user("hi"), Message::assistant("hello")]
        );
    }
}
