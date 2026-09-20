# kovi-plugin-ai 本地化 + 生图功能 设计

日期:2026-06-25

## 背景与目标

`kovi-plugin-ai`(crates.io 0.0.2)当前是外部依赖,内置一个需 @ 机器人、以 `生成图片`
开头、基于 Gemini `gemini-3-pro-image-preview` 的生图分支。

需求:用户直接发 `生图 xxx` 即可出图,**无需 @**;生图后端改用 yunnet 中转站的
`gpt-image-2`(OpenAI 兼容)。

外部 crate 无法直接改源码,因此将其 **fork 到本地 workspace 成员 `plugins/ai`** 再改造。

## 方案

### 1. 工程结构

- 新建 `plugins/ai`,包名仍为 `kovi-plugin-ai`(保持 `main.rs` 中 `kovi_plugin_ai` 标识符不变)。
- 拷贝 crate 的 `Cargo.toml(.orig)` / `src/lib.rs` / `src/image.rs` / `src/config.rs` 作为起点。
- 根 `Cargo.toml`:
  - `members` 增加 `"plugins/ai"`。
  - 依赖 `kovi-plugin-ai = "0.0.2"` 改为 `kovi-plugin-ai = { path = "plugins/ai" }`。

### 2. 触发逻辑(`lib.rs`)

`on_msg` 内按顺序判断:

1. **生图(无需 @)**:`event.get_text().trim()` 以 `生图` 开头时:
   - 取前缀之后的内容为 prompt;为空则回复用法提示(如「用法:生图 <描述>」),return。
   - 先 `event.reply("正在生成中…")` 给等待反馈。
   - 调 `gen_image(prompt, &config)`;成功 → `event.reply(Message::new().add_image(img))`;
     失败/空 → 回复失败提示。
   - return,不再走对话分支。
2. **AI 对话(仍需 @)**:沿用原逻辑 —— 仅当消息 @ 了机器人
   (`get_qq_number(...) == self_id`)时调用 Gemini 对话。
- 删除原 @ 门内的 `生成图片` 分支(被新生图取代)。

### 3. 生图后端(`image.rs` 重写)

OpenAI 兼容调用:
- `POST {image_base_url}/images/generations`
- Header:`Authorization: Bearer {image_api_key}`、`Content-Type: application/json`
- Body:`{ "model": image_model, "prompt": prompt, "size": image_size, "n": 1 }`
- 解析 `data[0].b64_json` → 返回 `base64://<b64>`(供 `add_image` 使用)。
- 请求失败 / data 为空 / 无 b64 → 返回空串,由调用方回复失败提示。
- 移除原先仅服务于图片的全局 `API_KEY` OnceLock,改为按参数传入。

### 4. 配置(`config.rs`)

`Config` 在原 `kind/model/api_key`(Gemini 对话用)基础上新增独立生图字段:

```toml
kind = "Gemini"
model = "gemini-3-pro-preview"
api_key = "${API_KEY}"
# 新增:生图(yunnet 中转站 gpt-image-2)
image_base_url = "https://api.yunnet.top/v1"
image_api_key  = "${IMAGE_API_KEY}"
image_model    = "gpt-image-2"
image_size     = "1024x1024"
```

部署时在 `data/config.toml` 填入真实的 `image_api_key`。

### 5. 测试

网络调用不单测,聚焦两个纯函数:
- 生图前缀解析:`生图 猫` → prompt `猫`;`生图`(无内容)→ 空。
- 响应解析:从 `{"data":[{"b64_json":"..."}]}` 取出 b64。

## 影响范围

- 改:根 `Cargo.toml`。
- 增:`plugins/ai/`(Cargo.toml + src 三文件)。
- 不改:`main.rs`、其余插件。
