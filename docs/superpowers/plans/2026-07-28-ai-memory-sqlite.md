# ai 插件 ChatMemory SQLite 持久化 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 plugins/ai 的多轮对话记忆落盘到 SQLite，重启后恢复每会话最近 20 轮上下文，全量存档只增不删。

**Architecture:** 新增 `store.rs`（`Mutex<Connection>` 封装的 Store，messages 全量存档 + resets 重置水位表）；`memory.rs` 加 `restore` 方法供启动回填；`lib.rs` 用 `OnceLock<Store>` 持有，在 main 里打开 DB 并预热内存，在 append/重置两处各加一行落库调用。DB 错误只 log 不中断聊天。

**Tech Stack:** Rust, kovi 0.13, rusqlite 0.32 (bundled), rig-core 0.27。

**Spec:** `docs/superpowers/specs/2026-07-28-ai-memory-sqlite-design.md`

## Global Constraints

- 本地只跑 `cargo test` / `cargo check`，不构建镜像、不打包（镜像走 CI，服务器只 pull）。
- DB 文件路径：`bot.get_data_path().join("memory.db")`（宿主 `data/ai/memory.db`）。
- 每会话内存窗口沿用 `memory::MAX_ROUNDS = 20`，DB 全量存档不裁剪。
- GroupContext 不持久化，不改动。
- 所有 DB 错误用 `kovi::log::error!` 记录后继续，绝不 panic / 中断消息处理。

---

### Task 1: store.rs 存储层（含 Cargo 依赖）

**Files:**
- Modify: `plugins/ai/Cargo.toml`（dependencies 末尾加一行）
- Create: `plugins/ai/src/store.rs`（含 `#[cfg(test)]` 单测）
- Modify: `plugins/ai/src/lib.rs:1-5`（仅加 `mod store;`，本任务不做其他接线）

**Interfaces:**
- Consumes: 无（独立模块）。
- Produces（Task 3 依赖，签名逐字）：
  - `Store::open(path: &std::path::Path) -> rusqlite::Result<Store>`
  - `Store::append_round(&self, key: &str, user: &str, assistant: &str) -> rusqlite::Result<()>`
  - `Store::mark_reset(&self, key: &str) -> rusqlite::Result<()>`
  - `Store::load_recent_all(&self, max_rounds: usize) -> rusqlite::Result<Vec<(String, Vec<(String, String)>)>>`（元素为 `(session_key, 时间序[(role, content)])`，role 取值 `"user"`/`"assistant"`）

- [ ] **Step 1: 加依赖**

在 `plugins/ai/Cargo.toml` 的 `[dependencies]` 末尾（`toml = "0.8.23"` 之后）加：

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

- [ ] **Step 2: 写失败测试**

创建 `plugins/ai/src/store.rs`，先只写测试骨架 + 空实现占位（让编译器先报缺失项也可以，推荐直接写好测试再补实现）。测试内容：

```rust
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
    fn load_caps_at_max_rounds_keeping_newest() {
        let s = mem_store();
        for i in 0..25 {
            s.append_round("group:1", &format!("问{i}"), &format!("答{i}")).unwrap();
        }
        let all = s.load_recent_all(20).unwrap();
        let rows = &all[0].1;
        assert_eq!(rows.len(), 40);
        // 最旧 5 轮被截掉,现存第一条是「问5」,最后一条是「答24」
        assert_eq!(rows[0], ("user".to_string(), "问5".to_string()));
        assert_eq!(rows[39], ("assistant".to_string(), "答24".to_string()));
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
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p kovi-plugin-ai store:: 2>&1 | tail -20`（在仓库根执行）
Expected: 编译错误（`Store` 未定义）——这就是本步的"失败"。

- [ ] **Step 4: 写实现**

`store.rs` 测试模块上方的完整实现：

```rust
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
    fn open_in_memory() -> rusqlite::Result<Self> {
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

    /// 所有会话在重置水位之后的最近 max_rounds 轮,时间序 (role, content);空会话不返回。
    pub fn load_recent_all(
        &self,
        max_rounds: usize,
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
                .query_map(params![key, (max_rounds * 2) as i64], |r| {
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
```

并在 `plugins/ai/src/lib.rs` 顶部模块声明区（`mod group_context;` 之后）加：

```rust
mod store;
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test -p kovi-plugin-ai 2>&1 | tail -20`
Expected: store 的 5 个新测试 + 既有测试全部 PASS（首次编译 bundled sqlite 较慢属正常）。

- [ ] **Step 6: Commit**

```bash
git add plugins/ai/Cargo.toml plugins/ai/src/store.rs plugins/ai/src/lib.rs Cargo.lock
git commit -m "feat(ai): SQLite 对话存档层 store.rs(全量存档+重置水位)"
```

---

### Task 2: memory.rs 增加 restore 回填

**Files:**
- Modify: `plugins/ai/src/memory.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: 无。
- Produces（Task 3 依赖）：`ChatMemory::restore(&self, key: &str, rows: &[(String, String)])` — rows 为时间序 `(role, content)`，role `"assistant"` 建为 assistant 消息、其余建为 user 消息；回填后窗口超过 `MAX_ROUNDS * 2` 条时从头部截断。

- [ ] **Step 1: 写失败测试**

在 `memory.rs` 的 tests 模块追加：

```rust
    #[test]
    fn restore_rebuilds_history_in_order() {
        let m = ChatMemory::new();
        m.restore(
            "group:1",
            &[
                ("user".to_string(), "问".to_string()),
                ("assistant".to_string(), "答".to_string()),
            ],
        );
        let h = m.history("group:1");
        assert_eq!(h, vec![Message::user("问"), Message::assistant("答")]);
    }

    #[test]
    fn restore_truncates_to_window() {
        let m = ChatMemory::new();
        let rows: Vec<(String, String)> = (0..(MAX_ROUNDS + 5))
            .flat_map(|i| {
                [
                    ("user".to_string(), format!("问{i}")),
                    ("assistant".to_string(), format!("答{i}")),
                ]
            })
            .collect();
        m.restore("group:1", &rows);
        let h = m.history("group:1");
        assert_eq!(h.len(), MAX_ROUNDS * 2);
        assert_eq!(h[0], Message::user("问5"));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p kovi-plugin-ai memory:: 2>&1 | tail -10`
Expected: 编译错误（`restore` 方法不存在）。

- [ ] **Step 3: 写实现**

在 `impl ChatMemory` 的 `clear` 方法之后加：

```rust
    /// 启动时从持久层回填历史(时间序 (role, content));超窗从头部截断。
    pub fn restore(&self, key: &str, rows: &[(String, String)]) {
        let mut map = self.inner.lock().unwrap();
        let q = map.entry(key.to_string()).or_default();
        for (role, content) in rows {
            let msg = if role == "assistant" {
                Message::assistant(content)
            } else {
                Message::user(content)
            };
            q.push_back(msg);
        }
        while q.len() > MAX_ROUNDS * 2 {
            q.pop_front();
        }
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p kovi-plugin-ai 2>&1 | tail -10`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add plugins/ai/src/memory.rs
git commit -m "feat(ai): ChatMemory::restore 启动回填历史"
```

---

### Task 3: lib.rs 接线（初始化/预热/落库/重置）

**Files:**
- Modify: `plugins/ai/src/lib.rs`（main 入口 ~182 行、重置分支 ~271 行、append 处 ~327 行）

**Interfaces:**
- Consumes: Task 1 的 `Store::{open, append_round, mark_reset, load_recent_all}`、Task 2 的 `ChatMemory::restore`、既有 `memory::MAX_ROUNDS`。
- Produces: 无（终端任务）。本任务无新增单测（接线逻辑依赖 kovi runtime，由既有测试 + CI 编译把关）。

- [ ] **Step 1: 静态量与 use**

`lib.rs` 顶部：`use std::sync::LazyLock;` 改为 `use std::sync::{LazyLock, OnceLock};`；在 `use crate::group_context::GroupContext;` 之后加 `use crate::store::Store;`；在 `static GROUP_CTX...` 之后加：

```rust
static STORE: OnceLock<Store> = OnceLock::new();

/// 落库失败只 log,不影响聊天流程。
fn persist(f: impl FnOnce(&Store) -> rusqlite::Result<()>) {
    if let Some(s) = STORE.get() {
        if let Err(e) = f(s) {
            error!("memory db error: {e}");
        }
    }
}
```

（`rusqlite` 已是直接依赖，路径可用。）

- [ ] **Step 2: main 里初始化并预热**

在 `let bot = PluginBuilder::get_runtime_bot();`（lib.rs:183）之后、`PluginBuilder::on_msg(...)` 之前插入：

```rust
    // 打开 SQLite 存档并把各会话最近历史回填进内存窗口;失败则退化为纯内存模式
    let db_path = bot.get_data_path().join("memory.db");
    match Store::open(&db_path) {
        Ok(store) => {
            match store.load_recent_all(memory::MAX_ROUNDS) {
                Ok(sessions) => {
                    for (key, rows) in sessions {
                        MEMORY.restore(&key, &rows);
                    }
                }
                Err(e) => error!("memory db load error: {e}"),
            }
            let _ = STORE.set(store);
        }
        Err(e) => error!("memory db open error: {e}, 本次运行不持久化"),
    }
```

- [ ] **Step 3: 重置与追加处落库**

重置分支（原 lib.rs:270-274）改为：

```rust
                if is_reset(&text) {
                    MEMORY.clear(&key);
                    persist(|s| s.mark_reset(&key));
                    event.reply("记忆已清空，我们重新开始吧");
                    return;
                }
```

append 处（原 lib.rs:327）`MEMORY.append(&key, &stored, &response);` 之后加一行：

```rust
                        persist(|s| s.append_round(&key, &stored, &response));
```

- [ ] **Step 4: 编译 + 全量测试**

Run: `cargo check -p kovi-plugin-ai && cargo test -p kovi-plugin-ai 2>&1 | tail -10`
Expected: 编译通过，全部测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add plugins/ai/src/lib.rs
git commit -m "feat(ai): 对话记忆接入 SQLite 持久化(启动回填/追加落库/重置水位)"
```

---

## 收尾（不在任务内自动执行）

合并回 main 后走既有 CI 构建镜像；LXC 1012 上 `docker compose pull && docker compose up -d` 更新。上线后验证：`data/ai/memory.db` 出现且 @机器人对话后 `messages` 表有行；重启容器后追问上一轮内容确认记忆仍在。
