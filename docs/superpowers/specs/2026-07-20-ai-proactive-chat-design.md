# AI 插件:免@主动搭话模式(proactive chat)设计

## 目标

给 kovi-bot 的 ai 插件新增一个模式:启用后**无需 @机器人**,机器人根据群消息内容
自行判断是否需要开口,模拟真人在群里聊天的效果。默认关闭,按群白名单启用。

## 触发位置

在现有 `plugins/ai/src/lib.rs` 的 `on_msg` 处理链**末尾**新增一个分支。仅当满足全部条件才进入:

- 群消息(`event.group_id.is_some()`),私聊不参与;
- 该群号在 `proactive_groups` 白名单内;
- 不是 `生图` / `改图` / `重置` 指令(已被前面分支处理并 `return`);
- 未 @机器人(@ 已被 `3) AI 对话` 分支处理)。

自身消息仍由处理链开头的 `event.user_id == event.self_id` 护栏拦掉。
白名单为空时该分支零开销直接跳过。

## 组件

### 1. 群上下文缓冲 `group_context.rs`(新模块)

- 每个群维护一个滚动窗口,存最近 N 条(默认 20)消息,格式 `昵称: 内容`。
  - 昵称优先取群名片/昵称,拿不到则回退到 QQ 号。
- 每条群消息都先入缓冲(即使这次不搭话),为 LLM 判断提供上下文。
- 机器人自己主动说出的话也追加进缓冲,保证连贯、避免复读。
- 结构:`GroupContext`,内部 `Mutex<HashMap<i64, GroupState>>`;
  `GroupState { messages: VecDeque<String>, last_consult: Option<Instant> }`。
  `LazyLock` 单例,与现有 `MEMORY`(ChatMemory)同风格,**两者完全独立**。

### 2. 节流:冷却 + 概率(成本硬约束)

顺序:**冷却 → 概率 → LLM**。

- **冷却**:每群两次「LLM 咨询」的最短间隔 `proactive_cooldown`(默认 60s)。
  冷却期内的消息只入缓冲,**不调 LLM**。
- **概率**:冷却过后来新消息时先掷骰,命中 `proactive_probability`(默认 0.3)才调 LLM。
  概率用 `SystemTime` 纳秒取模实现(`nanos % 100 < probability*100`),**不引入新 crate**
  (避免 CI 拉 crates.io 偶发超时)。
- 无论 LLM 判定「说」还是「不说」,**都重置该群 `last_consult`** → 每群 LLM 调用上限
  约「每 `cooldown` 秒一次」,兜住 grok 付费与上游不稳的成本。

门控逻辑抽成纯函数便于单测:`should_consult(last_consult, now, cooldown, roll, probability) -> bool`
(`roll` 与 `now` 作为参数注入)。

### 3. LLM 判断(纯 LLM 决策)

- 复用现有聊天客户端构建方式(`Kind::OpenAI` 走 `openai::CompletionsClient`,
  `model/base_url/api_key` 复用现有配置,当前即 grok-4.5)。
- Preamble 用 `proactive_persona`(真人人设)。
- 把群上下文窗口拼成一段最近对话,作为用户输入交给 agent。
- 指令约定:**若此刻不该插话,只回复 `[skip]`**;否则直接输出要说的话
  (口语、简短、不用 markdown)。
- 解析 `parse_proactive_reply(raw) -> Option<String>`:trim 后等于 `[skip]`(忽略大小写)
  或为空 → `None`(保持沉默);否则 `Some(text)`。
- `None` → 不发送;`Some(text)` → `event.reply(text)` + 追加进群缓冲。
- LLM 调用出错(上游不稳)时:记 `error!` 日志,**静默不发**(主动模式下不应刷「系统错误」)。

### 4. 人设(可配置真人)

- 新增 `proactive_persona` 字段,默认一段像真人的性格:有名字、口语化、短句、
  偶尔选择不接话、明确「不要暴露自己是 AI/机器人」、不使用 markdown。
- 与现有 @对话人设「你是一名QQ机器人」**分开**,互不影响。

## 配置新增(`config.rs`)

全部带 `#[serde(default = ...)]`,老 `config.toml` 不填也能解析。

| 字段 | 类型 | 默认 | 含义 |
|---|---|---|---|
| `proactive_groups` | `Vec<i64>` | `[]` | 群号白名单,空=关闭 |
| `proactive_cooldown` | `u64` | `60` | 每群最短咨询间隔(秒) |
| `proactive_probability` | `f64` | `0.3` | 冷却后掷骰命中概率(0~1) |
| `proactive_context_size` | `usize` | `20` | 上下文窗口条数 |
| `proactive_persona` | `String` | 见下 | 真人人设 preamble |

`proactive_persona` 默认值(示意,可后续调):
> 你叫小云,是这个群里的一个普通群友。说话口语、简短、随意,像真人发消息一样,
> 一次只说一两句,不用 markdown,不用书面语。你不是 AI,绝不承认自己是机器人或程序。
> 如果当前话题和你无关、或没有你想说的,就安静待着。

## 数据流

```
群消息到达 on_msg
  └─ 前置分支(自身消息/生图/改图/@对话)都未命中
     └─ 主动分支:群在白名单?
        ├─ 否 → return
        └─ 是 → 入群上下文缓冲
                └─ should_consult(冷却→概率)?
                   ├─ 否 → return(仅缓冲)
                   └─ 是 → 重置 last_consult
                           └─ 调 LLM(persona + 上下文窗口)
                              └─ parse_proactive_reply
                                 ├─ None  → 沉默
                                 └─ Some  → 发送 + 追加缓冲
```

## 错误处理

- LLM 上游错误:`error!` 记录,静默不发(不打扰群)。
- 拿不到昵称:回退 QQ 号。
- 白名单为空 / 群不在白名单:立即返回,无任何开销。

## 测试

纯函数单测(`cargo test -p kovi-plugin-ai`):

- `parse_proactive_reply`:`[skip]`、大小写、空串、含实际内容各分支。
- `GroupContext`:入队、超过 `context_size` 淘汰最旧、按群隔离、格式化输出。
- `should_consult`:冷却内拒绝、冷却外掷骰命中/未命中、首次(无 `last_consult`)。

LLM 实际判断质量靠部署后在白名单群里实测。

## 部署

沿用现有链路:worktree 开发 → 合并 `main` → GitHub Actions(`Github-Runner`)
构建推 Harbor → 10.1.12.1 `docker compose pull && up -d`。
容器 `/opt/kovi-bot/data/kovi-plugin-ai/config.toml` 补齐新字段(至少配 `proactive_groups`)。

## 非目标(YAGNI)

- 不做多模型/多 persona 切换,复用现有 grok-4.5 与单一 persona。
- 不做跨重启持久化上下文(内存态即可,重启清空)。
- 不做图片/表情理解,仅基于文本消息判断。
