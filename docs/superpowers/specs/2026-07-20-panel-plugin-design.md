# panel 插件设计 — Web 管理面板

日期:2026-07-20
分支:`worktree-panel-plugin`

## 背景 / 目标

给 kovi-bot 新建一个 **Web 管理面板**,本身作为一个 kovi 插件 `plugins/panel` 实现。
在 bot 进程内起一个 HTTP server,前端用 React 单页应用,用于**运行时管理已加载的 kovi 插件**。

前端栈已定:**Vite + React + TypeScript + Tailwind + shadcn/ui**(极简、组件自持、运行时零重型依赖)。

### v1 范围

**做:**
- 列出所有已加载插件(名字、版本、启用状态)
- 启用 / 禁用 单个插件(运行时,不重启进程)
- 重启单个插件
- 顶部 bot 状态条(在线 / 连接基本信息)

**不做(留 phase 2):**
- 日志流
- AI 插件配置编辑
- 访问控制白名单(群/好友)编辑
- 多轮上下文记忆管理

## 可行性(已验证)

kovi 0.13 的 `RuntimeBot` 内置全套运行时插件管理 API,面板后端直接调用,无需重启进程:

- `get_plugin_info() -> Result<Vec<PluginInfo>, BotError>` — 列出插件
- `enable_plugin(name)` / `disable_plugin(name)`
- `is_plugin_enable(name) -> Result<bool, BotError>`
- `restart_plugin(name) -> Result<(), BotError>`(async)
- `set_plugin_access_control*` 一套(phase 2 用)

插件通过 `PluginBuilder::get_runtime_bot() -> Arc<RuntimeBot>` 拿到句柄。

## 架构

- workspace 新成员 `plugins/panel`,依赖 `axum` + `tokio` + `rust-embed` + `serde` + `toml`
- 插件启动流程:
  1. 读配置(`data/config.toml`,见下)
  2. `PluginBuilder::get_runtime_bot()` 拿 `Arc<RuntimeBot>`
  3. 后台 tokio task 起 axum server,监听 `0.0.0.0:<port>`
  4. `RuntimeBot` 注入 axum `State`
- 前端 `dist/` 用 `rust-embed` 在**编译期**嵌入二进制;axum 用 SPA fallback(找不到静态文件时回 `index.html`)提供前端

### 目录结构

```
plugins/panel/
  Cargo.toml
  src/
    lib.rs        # 插件入口:读配置、起 server
    config.rs     # Config 结构 + load_toml_data
    api.rs        # axum router + handlers + auth 中间件
    embed.rs      # rust-embed 静态资源 + SPA fallback
  frontend/
    package.json          # bun
    vite.config.ts        # dev proxy /api -> :8080
    tsconfig.json
    tailwind.config.ts
    components.json        # shadcn
    index.html
    src/
      main.tsx
      App.tsx
      api.ts              # fetch 封装 + Bearer + 401 处理
      pages/Login.tsx
      pages/Dashboard.tsx
      components/ui/...    # shadcn 生成组件
    dist/                 # 构建产物,.gitignore,CI 生成
```

## 后端 API(`/api` 前缀,Bearer token)

| Method | Path | 说明 |
|---|---|---|
| POST | `/api/login` | body `{password}`,校验 `config.token`,通过返回 `{ok:true}` |
| GET | `/api/plugins` | 返回 `[{name, version, enabled}]`,由 `get_plugin_info()` 映射 |
| POST | `/api/plugins/:name/enable` | `enable_plugin(name)` |
| POST | `/api/plugins/:name/disable` | `disable_plugin(name)` |
| POST | `/api/plugins/:name/restart` | `restart_plugin(name)` |
| GET | `/api/status` | bot 在线 / 连接基本信息 |

### 鉴权

- 单密码/token,存 `data/config.toml` 的 `token` 字段
- token 即密码本身:前端登录成功后把密码存 localStorage,后续请求带 `Authorization: Bearer <token>`
- axum 中间件:除 `/api/login` 外,所有 `/api/*` 校验 Bearer,不匹配返回 401

### 安全护栏

- **禁止对面板插件自身执行 disable / restart**(否则面板把自己锤掉)。handler 里对 `name == 面板插件名` 直接返回 400。

## 前端

- Vite + React + TS + Tailwind + shadcn/ui
- 两屏:`Login`(密码输入)+ `Dashboard`(插件管理)
- token 存 localStorage;`api.ts` 统一带 Bearer;任何 401 → 清 token 回登录页
- Dashboard:
  - 顶部 shadcn `Card` 状态条(bot 在线状态)
  - shadcn `Table` 列出插件;每行 `Switch`(enable/disable)+ `Button`(重启)
  - 操作反馈用 `sonner` toast
- dev 流程:`bun run dev` 起 Vite(5173),`vite.config.ts` proxy `/api` → `http://localhost:8080`

## 配置(`data/config.toml`,load_toml_data 模式)

沿用 ai 插件模式:`bot.get_data_path().join("config.toml")` + `kovi::utils::load_toml_data(default, path)`。

```toml
port = 8080
token = "change-me"
```

## 部署改动

- **Dockerfile**:新增前端构建阶段
  ```dockerfile
  FROM oven/bun AS fe
  WORKDIR /fe
  COPY plugins/panel/frontend/ .
  RUN bun install && bun run build      # 产出 /fe/dist
  ```
  Rust builder 阶段在 `cargo build` 前 `COPY --from=fe /fe/dist ./plugins/panel/frontend/dist`(供 rust-embed 编译期读取)
- **main.rs**:`build_bot!` 追加 `kovi_plugin_panel`
- **根 Cargo.toml**:workspace `members` 加 `plugins/panel`;`dependencies` 加 `kovi-plugin-panel = { path = "plugins/panel" }`
- **kovi.plugin.toml**:新增 `[kovi-plugin-panel]` 段(`enable_on_startup = true`)
- **docker-compose.yml**:加 `ports: ["8080:8080"]`
- **.gitignore**:加 `plugins/panel/frontend/dist` 与 `node_modules`

## 测试

- **Rust**:
  - auth 中间件纯逻辑单测(带/不带/错误 Bearer → 200/401)
  - `PluginInfo -> {name,version,enabled}` 映射单测
  - RuntimeBot 侧调用抽一薄接口(trait)以便隔离测试 handler 逻辑
- **前端**:vitest 轻量测 `api.ts`(Bearer 注入、401 清 token 跳转)

## 未决 / phase 2

- 访问控制白名单编辑(kovi 已有 `set_plugin_access_control*` API)
- 日志流、AI 配置编辑、记忆管理
- token 签发 / 会话过期(当前 token==密码,够用即可)
