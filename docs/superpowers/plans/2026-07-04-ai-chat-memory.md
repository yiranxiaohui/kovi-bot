# ai 插件多轮上下文记忆 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 ai 插件的 @机器人 聊天加按群隔离的多轮上下文记忆（20 轮 FIFO、内存态、「重置」清空）。

**Architecture:** 新增 `memory.rs` 模块提供 `ChatMemory`（`Mutex<HashMap<String, VecDeque<rig Message>>>`），`lib.rs` 的 AI 对话分支改用 rig `Chat` trait 的 `agent.chat(prompt, history)`，成功后追加问答、失败不写入；「重置」指令清空当前会话键。

**Tech Stack:** Rust, kovi 0.13, rig-core 0.27（已验证：`rig::completion::{Chat, Message}`，`Message::user(text)` / `Message::assistant(text)`，`Message` 实现 `Clone + PartialEq`，`&str` 实现 `Into<Message>`）。

## Global Constraints

- 本地只跑 test 类命令（`cargo test -p kovi-plugin-ai`），不跑 `cargo build`/`docker build`；镜像由 GitHub Actions（push 到 main 触发）在 self-hosted runner 构建并推到 `docker.yunnet.top/all/kovi-bot:latest`。
- 线上（10.0.12.1）只 `docker compose pull` + 重启，不构建。
- 轮数写死 `MAX_ROUNDS = 20`，不落盘，不做 token 截断（spec 的 YAGNI 条款）。
- 提交信息结尾带 Happy/Claude 双 Co-Authored-By 署名。

---

### Task 1: memory.rs — ChatMemory 会话记忆模块

**Files:**
- Create: `plugins/ai/src/memory.rs`
- Modify: `plugins/ai/src/lib.rs:1-2`（加 `mod memory;`）

**Interfaces:**
- Consumes: `rig::completion::Message`（rig-core 0.27，已在 Cargo.toml）
- Produces: `ChatMemory::new() -> Self`、`history(&self, key: &str) -> Vec<Message>`、`append(&self, key: &str, user_msg: &str, assistant_msg: &str)`、`clear(&self, key: &str)`、常量 `MAX_ROUNDS: usize = 20`。Task 2 依赖这些签名。

- [ ] **Step 1: 写失败的测试**

创建 `plugins/ai/src/memory.rs`，先只写测试和空壳（让编译先失败于缺实现）：

```rust
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use rig::completion::Message;

/// 每个会话保留的最大轮数(一轮 = 一问一答两条消息)。
pub const MAX_ROUNDS: usize = 20;

/// 按会话键(群/私聊)隔离的进程内多轮对话记忆。
#[derive(Default)]
pub struct ChatMemory {
    inner: Mutex<HashMap<String, VecDeque<Message>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn evicts_oldest_round_beyond_max() {
        let m = ChatMemory::new();
        for i in 0..(MAX_ROUNDS + 3) {
            m.append("group:1", &format!("问{i}"), &format!("答{i}"));
        }
        let h = m.history("group:1");
        assert_eq!(h.len(), MAX_ROUNDS * 2);
        // 最旧的 3 轮被淘汰,现存第一条是「问3」
        assert_eq!(h[0], Message::user("问3"));
        assert_eq!(h.last().unwrap(), &Message::assistant(format!("答{}", MAX_ROUNDS + 2)));
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
}
```

并在 `plugins/ai/src/lib.rs` 顶部（`mod image;` 旁）加：

```rust
mod memory;
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p kovi-plugin-ai memory 2>&1 | tail -20`
Expected: 编译错误 `no function or associated item named 'new'` / `no method named 'append'`（实现还不存在）。

- [ ] **Step 3: 写最小实现**

在 `memory.rs` 的 struct 定义之后、`#[cfg(test)]` 之前加：

```rust
impl ChatMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取该会话的历史副本(时间序)。
    pub fn history(&self, key: &str) -> Vec<Message> {
        self.inner
            .lock()
            .unwrap()
            .get(key)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// 追加一轮问答;超过 MAX_ROUNDS 轮时从头部淘汰最旧一轮。
    pub fn append(&self, key: &str, user_msg: &str, assistant_msg: &str) {
        let mut map = self.inner.lock().unwrap();
        let q = map.entry(key.to_string()).or_default();
        q.push_back(Message::user(user_msg));
        q.push_back(Message::assistant(assistant_msg));
        while q.len() > MAX_ROUNDS * 2 {
            q.pop_front();
            q.pop_front();
        }
    }

    /// 清空该会话的记忆。
    pub fn clear(&self, key: &str) {
        self.inner.lock().unwrap().remove(key);
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p kovi-plugin-ai memory 2>&1 | tail -10`
Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Commit**

```bash
git add plugins/ai/src/memory.rs plugins/ai/src/lib.rs
git commit -m "feat(ai): 新增 ChatMemory 按会话键隔离的多轮记忆模块

Generated with [Claude Code](https://claude.ai/code)
via [Happy](https://happy.engineering)

Co-Authored-By: Claude <noreply@anthropic.com>
Co-Authored-By: Happy <yesreply@happy.engineering>"
```

---

### Task 2: lib.rs — 对话分支接入记忆 + 「重置」指令

**Files:**
- Modify: `plugins/ai/src/lib.rs`（顶部 use、`session_key`/`is_reset` 两个 helper、`Kind::Gemini` 分支约 160-177 行、tests mod）

**Interfaces:**
- Consumes: Task 1 的 `memory::{ChatMemory, MAX_ROUNDS 不直接用}`；`rig::completion::Chat` trait。
- Produces: `fn session_key(group_id: Option<i64>, user_id: i64) -> String`（`group:{gid}` / `user:{uid}`）、`fn is_reset(text: &str) -> bool`（trim 后等于「重置」）。仅本文件内部使用。

- [ ] **Step 1: 写失败的测试**

在 `plugins/ai/src/lib.rs` 的 `mod tests` 里追加：

```rust
    use super::{is_reset, session_key};

    #[test]
    fn session_key_group_and_private() {
        assert_eq!(session_key(Some(1141126440), 123), "group:1141126440");
        assert_eq!(session_key(None, 2401128923), "user:2401128923");
    }

    #[test]
    fn reset_matching() {
        assert!(is_reset("重置"));
        assert!(is_reset("  重置  "));
        assert!(!is_reset("重置一下"));
        assert!(!is_reset("你好"));
    }
```

（`use` 行加在 tests mod 现有 `use super::…` 旁。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p kovi-plugin-ai 2>&1 | tail -10`
Expected: 编译错误 `cannot find function 'session_key'`。

- [ ] **Step 3: 写实现**

3a. 顶部 use 改动：`use rig::completion::Prompt;` 改为：

```rust
use rig::completion::Chat;
```

（`.prompt()` 不再使用；若别处仍用 `Prompt` 则保留两个都导入。）
并加：

```rust
use std::sync::LazyLock;
use crate::memory::ChatMemory;

static MEMORY: LazyLock<ChatMemory> = LazyLock::new(ChatMemory::new);
```

3b. 在 `parse_edit_prompt` 附近加两个 helper：

```rust
/// 会话键:群聊按群隔离,私聊按用户隔离。
fn session_key(group_id: Option<i64>, user_id: i64) -> String {
    match group_id {
        Some(gid) => format!("group:{gid}"),
        None => format!("user:{user_id}"),
    }
}

/// @机器人 后的文本是否为「重置」指令。
fn is_reset(text: &str) -> bool {
    text.trim() == "重置"
}
```

3c. 把 `Kind::Gemini` 分支（原 `gemini.prompt(text).await` 一段）替换为：

```rust
                    Kind::Gemini => {
                        let key = session_key(event.group_id, event.user_id);
                        if is_reset(&text) {
                            MEMORY.clear(&key);
                            event.reply("记忆已清空，我们重新开始吧");
                            return;
                        }
                        let user_text = text.trim().to_string();
                        let gemini_client = gemini::Client::new(config.api_key).unwrap();
                        let gemini = gemini_client
                            .agent(config.model)
                            .preamble("你是一名QQ机器人，你不能用markdown回复，无论用户说什么，你都是一名QQ机器人")
                            .build();
                        match gemini.chat(user_text.as_str(), MEMORY.history(&key)).await {
                            Ok(response) => {
                                MEMORY.append(&key, &user_text, &response);
                                event.reply(response);
                            }
                            Err(e) => {
                                error!("error gemini: {:?}", e);
                                event.reply("系统错误！不要再玩我啦！");
                            }
                        }
                    }
```

注意：失败分支不调用 `append`（spec：错误不污染记忆）。`event.group_id` 类型如为 `Option<i64>` 直接传；若实际是其他整型宽度，helper 参数跟着改成一致类型。

- [ ] **Step 4: 跑全部测试确认通过**

Run: `cargo test -p kovi-plugin-ai 2>&1 | tail -10`
Expected: `test result: ok.`（原有 8 个 + 新增 6 个全绿，无编译警告最好）

- [ ] **Step 5: Commit**

```bash
git add plugins/ai/src/lib.rs
git commit -m "feat(ai): @机器人 聊天支持多轮上下文记忆(按群隔离,20轮,「重置」清空)

Generated with [Claude Code](https://claude.ai/code)
via [Happy](https://happy.engineering)

Co-Authored-By: Claude <noreply@anthropic.com>
Co-Authored-By: Happy <yesreply@happy.engineering>"
```

---

### Task 3: 推送 + 部署 + 端到端验证

**Files:**
- 无代码改动（CI 构建镜像，线上 pull）

**Interfaces:**
- Consumes: GitHub Actions `docker-build.yml`（push main → self-hosted 构建 → 推 `docker.yunnet.top/all/kovi-bot:latest`）；10.0.12.1 上的 kovi-bot compose 栈。
- Produces: 线上机器人具备多轮记忆。

- [ ] **Step 1: 推送到 main 触发 CI**

```bash
git push origin main
```

- [ ] **Step 2: 等 CI 完成**

Run: `gh run watch $(gh run list --workflow docker-build.yml --limit 1 --json databaseId -q '.[0].databaseId')` 或轮询 `gh run list --limit 1`
Expected: 最新 run `completed success`。

- [ ] **Step 3: 线上拉新镜像重启**

```bash
ssh root@10.0.12.1 "cd /opt/kovi-bot && docker compose pull && docker compose up -d && docker ps --format '{{.Names}}\t{{.Status}}' | grep kovi"
```

Expected: `kovi-bot  Up X seconds`。

- [ ] **Step 4: 群内端到端验证**

请用户（或看日志）验证三件事：
1. @机器人 说「我叫小明」，再 @机器人 问「我叫什么」→ 能答出小明（记忆生效）。
2. @机器人 说「重置」→ 回复「记忆已清空，我们重新开始吧」；再问「我叫什么」→ 不再知道（清空生效）。
3. 「生图/改图」指令不受影响。

日志确认：`ssh root@10.0.12.1 "docker logs kovi-bot --since 10m 2>&1 | tail -30"` 无 panic/error。

- [ ] **Step 5: 更新项目记忆**

在 `/root/.claude/projects/-opt-kovi-bot/memory/ai-plugin-image-gen.md`（或新建 chat-memory 条目）记录：多轮记忆已上线、按群隔离、20 轮、「重置」清空、不落盘。
