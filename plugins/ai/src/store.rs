use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

/// 对话历史持久层:messages 全量存档 + resets 重置水位。所有方法内部短锁,可跨线程共享。
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
               id          INTEGER PRIMARY KEY AUTOINCREMENT,
               session_key TEXT NOT NULL,
               role        TEXT NOT NULL,
               content     TEXT NOT NULL,
               created_at  TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_key, id);
             CREATE TABLE IF NOT EXISTS resets (
               session_key   TEXT PRIMARY KEY,
               last_reset_id INTEGER NOT NULL
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// 追加一轮问答(user+assistant 两行),同一事务保证成对。
    pub fn append_round(&self, key: &str, user: &str, assistant: &str) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO messages (session_key, role, content) VALUES (?1, 'user', ?2)",
            params![key, user],
        )?;
        tx.execute(
            "INSERT INTO messages (session_key, role, content) VALUES (?1, 'assistant', ?2)",
            params![key, assistant],
        )?;
        tx.commit()
    }

    /// 重置:把该会话当前最大 id 记为水位,之前的消息不再被加载;存档保留。
    pub fn mark_reset(&self, key: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO resets (session_key, last_reset_id)
             VALUES (?1, COALESCE((SELECT MAX(id) FROM messages WHERE session_key = ?1), 0))
             ON CONFLICT(session_key) DO UPDATE SET last_reset_id = excluded.last_reset_id",
            params![key],
        )?;
        Ok(())
    }

    /// 压缩落库:把该会话现有消息全部隐藏到水位之下,再把压缩后的完整窗口
    /// (摘要 + 保留的近期原文)按序重写到水位之上,保证重启回填与内存严格一致。
    /// 旧消息在存档里保留,只是不再被加载。
    pub fn compact(&self, key: &str, rows: &[(String, String)]) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO resets (session_key, last_reset_id)
             VALUES (?1, COALESCE((SELECT MAX(id) FROM messages WHERE session_key = ?1), 0))
             ON CONFLICT(session_key) DO UPDATE SET last_reset_id = excluded.last_reset_id",
            params![key],
        )?;
        for (role, content) in rows {
            tx.execute(
                "INSERT INTO messages (session_key, role, content) VALUES (?1, ?2, ?3)",
                params![key, role, content],
            )?;
        }
        tx.commit()
    }

    /// 所有会话在重置水位之后的最近 max_rows 条消息,时间序 (role, content);空会话不返回。
    pub fn load_recent_all(
        &self,
        max_rows: usize,
    ) -> rusqlite::Result<Vec<(String, Vec<(String, String)>)>> {
        let conn = self.conn.lock().unwrap();
        let keys: Vec<String> = conn
            .prepare("SELECT DISTINCT session_key FROM messages")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        for key in keys {
            let mut rows: Vec<(String, String)> = conn
                .prepare(
                    "SELECT role, content FROM messages
                     WHERE session_key = ?1
                       AND id > COALESCE(
                             (SELECT last_reset_id FROM resets WHERE session_key = ?1), 0)
                     ORDER BY id DESC LIMIT ?2",
                )?
                .query_map(params![key, max_rows as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?
                .collect::<Result<_, _>>()?;
            rows.reverse();
            if !rows.is_empty() {
                out.push((key, rows));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn append_then_load_roundtrip() {
        let s = mem_store();
        s.append_round("group:1", "你好", "你好呀").unwrap();
        let all = s.load_recent_all(20).unwrap();
        assert_eq!(all.len(), 1);
        let (key, rows) = &all[0];
        assert_eq!(key, "group:1");
        assert_eq!(
            rows,
            &vec![
                ("user".to_string(), "你好".to_string()),
                ("assistant".to_string(), "你好呀".to_string()),
            ]
        );
    }

    #[test]
    fn load_caps_at_max_rows_keeping_newest() {
        let s = mem_store();
        for i in 0..25 {
            s.append_round("group:1", &format!("问{i}"), &format!("答{i}")).unwrap();
        }
        let all = s.load_recent_all(40).unwrap();
        let rows = &all[0].1;
        assert_eq!(rows.len(), 40);
        // 最旧 5 轮被截掉,现存第一条是「问5」,最后一条是「答24」
        assert_eq!(rows[0], ("user".to_string(), "问5".to_string()));
        assert_eq!(rows[39], ("assistant".to_string(), "答24".to_string()));
    }

    #[test]
    fn compact_rewrites_window_and_hides_old_rows() {
        let s = mem_store();
        s.append_round("group:1", "旧问1", "旧答1").unwrap();
        s.append_round("group:1", "旧问2", "旧答2").unwrap();
        // 压缩:旧问1/旧答1 被压成摘要,旧问2/旧答2 作为近期原文保留
        s.compact(
            "group:1",
            &[
                ("user".to_string(), "[早前对话摘要] 聊了旧话题".to_string()),
                ("user".to_string(), "旧问2".to_string()),
                ("assistant".to_string(), "旧答2".to_string()),
            ],
        )
        .unwrap();
        s.append_round("group:1", "新问", "新答").unwrap();
        let all = s.load_recent_all(400).unwrap();
        assert_eq!(
            all[0].1,
            vec![
                ("user".to_string(), "[早前对话摘要] 聊了旧话题".to_string()),
                ("user".to_string(), "旧问2".to_string()),
                ("assistant".to_string(), "旧答2".to_string()),
                ("user".to_string(), "新问".to_string()),
                ("assistant".to_string(), "新答".to_string()),
            ]
        );
    }

    #[test]
    fn reset_hides_prior_messages_only() {
        let s = mem_store();
        s.append_round("group:1", "旧问", "旧答").unwrap();
        s.mark_reset("group:1").unwrap();
        assert!(s.load_recent_all(20).unwrap().is_empty());
        s.append_round("group:1", "新问", "新答").unwrap();
        let all = s.load_recent_all(20).unwrap();
        assert_eq!(
            all[0].1,
            vec![
                ("user".to_string(), "新问".to_string()),
                ("assistant".to_string(), "新答".to_string()),
            ]
        );
    }

    #[test]
    fn repeated_reset_upserts() {
        let s = mem_store();
        s.append_round("user:9", "a", "b").unwrap();
        s.mark_reset("user:9").unwrap();
        s.append_round("user:9", "c", "d").unwrap();
        s.mark_reset("user:9").unwrap();
        assert!(s.load_recent_all(20).unwrap().is_empty());
        // 对空会话重置也不报错
        s.mark_reset("group:404").unwrap();
    }

    #[test]
    fn sessions_are_isolated() {
        let s = mem_store();
        s.append_round("group:1", "a", "b").unwrap();
        s.append_round("user:9", "c", "d").unwrap();
        s.mark_reset("group:1").unwrap();
        let all = s.load_recent_all(20).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, "user:9");
    }
}
