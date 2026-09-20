# ai 插件 ChatMemory SQLite 持久化设计

日期：2026-07-28
状态：已确认

## 背景与目标

`plugins/ai` 的多轮对话记忆 `ChatMemory` 目前是纯进程内结构
（`Mutex<HashMap<会话键, VecDeque<Message>>>`，每会话滚动保留 20 轮），
容器重启即全部丢失。目标：把对话历史持久化到 SQLite，重启后恢复最近上下文，
并保留全量聊天存档供未来长期记忆/摘要使用。

非目标：GroupContext（主动搭话缓冲）保持纯内存，不持久化；不做多机共享。

## 存储层

新增 `plugins/ai/src/store.rs`，依赖 `rusqlite`（`bundled` 特性，静态编译
SQLite，不依赖系统库）。数据库文件：`bot.get_data_path()/memory.db`
（与 config.toml 同目录，宿主 `data/ai/`，随 compose 卷持久化）。

```sql
CREATE TABLE IF NOT EXISTS messages (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  session_key TEXT NOT NULL,          -- group:<群号> / user:<QQ>
  role        TEXT NOT NULL,          -- 'user' | 'assistant'
  content     TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_key, id);
CREATE TABLE IF NOT EXISTS resets (
  session_key   TEXT PRIMARY KEY,
  last_reset_id INTEGER NOT NULL     -- 此 id 及之前的消息不再加载
);
```

## 行为语义

- **追加**：每轮问答插两行（role=user、role=assistant），只增不删，全量存档。
- **加载**：插件启动时一次性把所有会话在 `id > last_reset_id` 之后的最近
  20 轮（`MAX_ROUNDS`）载入内存窗口；运行期只写不读。取数按 `id` 倒序
  LIMIT 40 再反转为时间序。
- **重置**：用户发「重置」→ 清内存窗口 + 将该会话当前最大 `id` upsert 进
  `resets`。存档保留，只是不再进入上下文。
- 内存中重建为 `rig::completion::Message::user/assistant`（只存纯文本，
  与现有 `ChatMemory::append(&str, &str)` 签名一致）。

## 接入方式

- `rusqlite::Connection` 非 `Sync`，用 `Mutex<Connection>` 封装为
  `Store`，放入 `OnceLock<Store>` 静态量（DB 路径需等插件启动拿到
  `bot.get_data_path()` 后才可知）。
- `lib.rs` 改动点：
  1. 插件入口：初始化 `Store`，加载历史填充 `MEMORY`；
  2. 问答成功后 `MEMORY.append(...)` 处：追加落库；
  3. 「重置」分支 `MEMORY.clear(...)` 处：写重置标记。
- **错误处理**：DB 任何错误只 `log`（kovi 日志），不中断聊天流程——
  持久化失败时退化为现状的纯内存行为。

## 测试

`store.rs` 单测用 `Connection::open_in_memory()`：

1. 追加后加载回环（内容、顺序、role 正确）；
2. 超过 20 轮时只加载最近 20 轮；
3. 重置标记后旧消息不再加载，新消息正常加载；
4. 会话之间隔离；
5. 重复重置（upsert）语义正确。

`memory.rs` 既有测试不动。

## 实施约束

- 按 treeflow 规约在独立 worktree 开发；
- 本地只跑 `cargo test`/`cargo check`，不做构建打包；镜像照旧走 CI，
  服务器（LXC 1012）只 pull + 重启。
