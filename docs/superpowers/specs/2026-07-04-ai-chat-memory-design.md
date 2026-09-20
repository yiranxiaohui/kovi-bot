# ai 插件多轮上下文记忆 — 设计

日期：2026-07-04
状态：已确认

## 目标

给 ai 插件的 @机器人 聊天加多轮上下文记忆，让同一个群里的对话可以接力进行；私聊按用户独立记忆。

## 需求（已与用户确认）

- **隔离粒度**：按群隔离，整个群共享一条对话线；私聊按用户各一条。
- **记忆内容**：只存 @机器人 的问答对，不旁听群内其他消息。
- **保留策略**：内存保留最近 20 轮（40 条消息），FIFO 淘汰；重启即忘，不落盘。
- **重置指令**：@机器人 发「重置」清空当前会话记忆并回复确认。

## 方案

采用 rig 的 `Chat` trait（`agent.chat(prompt, history)`）+ 进程内 HashMap 存历史。

### 新模块 `plugins/ai/src/memory.rs`

```rust
pub struct ChatMemory {
    inner: Mutex<HashMap<String, VecDeque<rig::completion::Message>>>,
}
```

- `history(key) -> Vec<Message>`：取该会话的历史副本。
- `append(key, user_msg, assistant_msg)`：追加一轮问答；超过 `MAX_ROUNDS = 20` 轮时从头部淘汰最旧一轮。
- `clear(key)`：清空该会话。
- 全局实例：`static MEMORY: LazyLock<ChatMemory>`（或 main 里 `Arc` 持有并 clone 进闭包）。

### `lib.rs` AI 对话分支改动

1. 会话键：群聊 `group:{group_id}`，私聊 `user:{user_id}`。
2. @机器人 且文本 trim 后等于「重置」→ `clear(key)`，回复「记忆已清空，我们重新开始吧」，返回。
3. 否则：`let history = MEMORY.history(&key)` → `agent.chat(text, history)`。
4. 成功：回复响应，并 `append(key, text, response)`。
5. 失败：回复现有错误文案，**不写入记忆**（避免污染上下文）。

## 错误处理

- Gemini 调用失败：记忆保持原样。
- 锁：用同步 `std::sync::Mutex`，临界区只做取/写内存操作（不跨 await），无死锁风险。

## 测试

- `memory.rs` 单元测试：append 后 history 可取回、超 20 轮淘汰最旧、clear 生效、不同 key 互不影响。
- 「重置」指令的文本匹配逻辑单元测试。
- 端到端：部署后在群里实际验证接力对话与重置。

## 明确不做（YAGNI）

- 不落盘持久化。
- 不做 token 预算截断。
- 轮数不做成配置项（写死 20）。
- 不旁听群聊消息。
