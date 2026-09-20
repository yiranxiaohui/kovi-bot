# panel 插件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 kovi-bot 新增一个 `plugins/panel` 插件,在 bot 进程内起 axum HTTP server,提供 React 单页管理面板,运行时管理已加载插件(列表 / 启用 / 禁用 / 重启),单 token 鉴权。

**Architecture:** 插件启动时读 `data/config.toml`(port + token),用 `PluginBuilder::get_runtime_bot()` 拿 `Arc<RuntimeBot>` 注入 axum `State`,后台 tokio task 起 server。API 调 `RuntimeBot` 的 `get_plugin_info / enable_plugin / disable_plugin / restart_plugin`。前端(Vite+React+TS+Tailwind+shadcn/ui)构建产物用 `rust-embed`(`debug-embed` 特性,编译期嵌入)随二进制分发,axum SPA fallback 提供。

**Tech Stack:** Rust / axum 0.7 / rust-embed / kovi 0.13;前端 Vite + React 18 + TypeScript + Tailwind + shadcn/ui,包管理用 bun。

**参考 spec:** `docs/superpowers/specs/2026-07-20-panel-plugin-design.md`

**关键 API 事实(已核对 kovi 0.13 源码):**
- 插件入口:`#[kovi::plugin] async fn main() { let bot = PluginBuilder::get_runtime_bot(); ... }`,`get_runtime_bot()` 返回 `Arc<RuntimeBot>`。
- `PluginBuilder::get_plugin_name() -> String` 拿本插件名(用于自锤护栏)。
- `bot.get_data_path()` 返回本插件数据目录;配置模式见 `plugins/ai/src/config.rs`(`kovi::utils::load_toml_data`)。
- `RuntimeBot::get_plugin_info() -> Result<Vec<PluginInfo>, _>`;`PluginInfo` 字段:`name: String`、`version: String`、`enabled: bool`(还有 feature-gated 的 access_control 字段,v1 不用)。
- `enable_plugin(name) -> Result<(), _>`(sync)、`disable_plugin(name) -> Result<Option<JoinHandle>, _>`(sync)、`restart_plugin(name) -> Result<(), _>`(**async**)、`is_plugin_enable(name) -> Result<bool, _>`。
- 错误统一用 `.map_err(|e| e.to_string())` 转字符串,避免依赖具体 `BotError` 路径。
- `RuntimeBot` 在 ai 插件里被 clone 进 `Send + Sync` 的 async 闭包,故 `Arc<RuntimeBot>` 可直接作 axum State(若真遇 Send/Sync 编译错误,见 Task 7 备注的 channel 降级方案)。

**文件结构:**
```
plugins/panel/
  Cargo.toml
  src/
    lib.rs        # 插件入口:读配置、拿 RuntimeBot、spawn server
    config.rs     # Config { port, token } + read_config
    api.rs        # AppState、router、handlers、auth、DTO、错误
    embed.rs      # rust-embed Assets + SPA static_handler
  frontend/
    package.json / vite.config.ts / tsconfig*.json / tailwind 配置 / components.json
    index.html
    src/main.tsx / App.tsx / api.ts / api.test.ts / pages/Login.tsx / pages/Dashboard.tsx
    src/components/ui/...   # shadcn 生成
    dist/index.html          # 占位,保证 cargo build 时 rust-embed 有目录;CI 用真构建覆盖
```

---

## Task 1: Scaffold `plugins/panel` crate 并接入 workspace

**Files:**
- Create: `plugins/panel/Cargo.toml`
- Create: `plugins/panel/src/lib.rs`
- Create: `plugins/panel/frontend/dist/index.html`(占位)
- Modify: `Cargo.toml`(根,workspace + deps)
- Modify: `src/main.rs`(build_bot! 加插件)
- Modify: `kovi.plugin.toml`(加 panel 段)
- Modify: `.gitignore`

- [ ] **Step 1: 创建 crate `Cargo.toml`**

`plugins/panel/Cargo.toml`:
```toml
[package]
name = "kovi-plugin-panel"
version = "0.0.1"
edition = "2024"
description = "kovi plugin: Web 管理面板(运行时插件管理)"
license = "MIT OR Apache-2.0"

[dependencies]
kovi = ">=0.13"
axum = "0.7"
rust-embed = { version = "8", features = ["debug-embed"] }
mime_guess = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
```

- [ ] **Step 2: 占位前端产物**（保证 rust-embed 目录存在)

`plugins/panel/frontend/dist/index.html`:
```html
<!doctype html><html><head><meta charset="utf-8"><title>panel</title></head>
<body>panel frontend not built yet</body></html>
```

- [ ] **Step 3: 最小插件入口**（只打印日志,先跑通编译与加载)

`plugins/panel/src/lib.rs`:
```rust
use kovi::PluginBuilder;
use kovi::log::info;

#[kovi::plugin]
async fn main() {
    let _bot = PluginBuilder::get_runtime_bot();
    info!("panel 插件已加载(server 尚未实现)");
}
```

- [ ] **Step 4: 接入根 `Cargo.toml`**

把 `members` 改为包含 panel,并加依赖:
```toml
[workspace]
members = ["plugins/help", "plugins/ai", "plugins/panel"]
```
`[dependencies]` 末尾加:
```toml
kovi-plugin-panel = { path = "plugins/panel" }
```

- [ ] **Step 5: 接入 `src/main.rs`**

把 `build_bot!` 那行的插件列表末尾加 `, kovi_plugin_panel`:
```rust
    let bot = kovi::build_bot!(driver; kovi_plugin_cmd, kovi_plugin_60s, kovi_plugin_meme, kovi_plugin_ai, kovi_plugin_help, kovi_plugin_panel);
```

- [ ] **Step 6: `kovi.plugin.toml` 加 panel 段**

文件末尾追加:
```toml
[kovi-plugin-panel]
enable_on_startup = true
access_control = false
list_mode = "WhiteList"

[kovi-plugin-panel.access_list]
friends = []
groups = []
```

- [ ] **Step 7: `.gitignore` 追加**

在 `.gitignore` 末尾追加:
```
plugins/panel/frontend/node_modules
plugins/panel/frontend/dist/assets
```
（保留 `dist/index.html` 占位入库;CI 构建生成的 `dist/assets/` 不入库。)

- [ ] **Step 8: 编译验证**

Run: `cargo build`
Expected: 编译成功(可能有 unused 警告)。

- [ ] **Step 9: Commit**

```bash
git add plugins/panel Cargo.toml src/main.rs kovi.plugin.toml .gitignore
git commit -m "feat(panel): scaffold 面板插件并接入 workspace"
```

---

## Task 2: Config 模块

**Files:**
- Create: `plugins/panel/src/config.rs`
- Modify: `plugins/panel/src/lib.rs`(声明 mod、调用)

- [ ] **Step 1: 写失败测试**

`plugins/panel/src/config.rs`:
```rust
use std::sync::Arc;
use kovi::RuntimeBot;
use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use toml::toml;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub token: String,
}

pub fn read_config(bot: Arc<RuntimeBot>) -> Config {
    let data_path = bot.get_data_path();
    let config_toml_path = data_path.join("config.toml");
    let default_config = toml! {
        port = 8080
        token = "change-me"
    };
    let config = load_toml_data(default_config, config_toml_path).unwrap();
    let config: Config = toml::from_str(&config.to_string()).unwrap();
    config
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_port_and_token() {
        let c: Config = toml::from_str("port = 9000\ntoken = \"abc\"\n").unwrap();
        assert_eq!(c.port, 9000);
        assert_eq!(c.token, "abc");
    }
}
```

- [ ] **Step 2: 声明 mod**

`plugins/panel/src/lib.rs` 顶部加 `mod config;`(在 `use` 之前)。

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test -p kovi-plugin-panel config`
Expected: `parses_port_and_token ... ok`

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/src
git commit -m "feat(panel): config 模块(port/token, load_toml_data)"
```

---

## Task 3: DTO 与 auth 纯逻辑(可单测)

**Files:**
- Create: `plugins/panel/src/api.rs`
- Modify: `plugins/panel/src/lib.rs`(`mod api;`)

- [ ] **Step 1: 写 DTO + auth 检查 + 测试**

`plugins/panel/src/api.rs`(本任务只放可单测的纯逻辑,handler 下一任务加):
```rust
use serde::Serialize;

#[derive(Serialize, Debug, PartialEq)]
pub struct PluginDto {
    pub name: String,
    pub version: String,
    pub enabled: bool,
}

/// 校验 `Authorization` 头是否是 `Bearer <token>` 且 token 匹配。
pub fn check_bearer(header: Option<&str>, token: &str) -> bool {
    header
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t == token)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_ok() {
        assert!(check_bearer(Some("Bearer secret"), "secret"));
    }

    #[test]
    fn bearer_wrong_token() {
        assert!(!check_bearer(Some("Bearer nope"), "secret"));
    }

    #[test]
    fn bearer_missing_or_malformed() {
        assert!(!check_bearer(None, "secret"));
        assert!(!check_bearer(Some("secret"), "secret"));
        assert!(!check_bearer(Some("Basic secret"), "secret"));
    }

    #[test]
    fn dto_serializes() {
        let d = PluginDto { name: "ai".into(), version: "0.1".into(), enabled: true };
        let j = serde_json::to_string(&d).unwrap();
        assert_eq!(j, r#"{"name":"ai","version":"0.1","enabled":true}"#);
    }
}
```

- [ ] **Step 2: 声明 mod**

`plugins/panel/src/lib.rs` 加 `mod api;`。

- [ ] **Step 3: 运行测试**

Run: `cargo test -p kovi-plugin-panel api`
Expected: 4 个测试全 `ok`。

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/src
git commit -m "feat(panel): PluginDto + Bearer 校验纯逻辑(含单测)"
```

---

## Task 4: API router、handlers、State、错误

**Files:**
- Modify: `plugins/panel/src/api.rs`

- [ ] **Step 1: 在 `api.rs` 顶部补 imports 与 AppState**

在 `api.rs` 现有内容**上方**插入:
```rust
use std::sync::Arc;
use axum::{
    Router, Json,
    extract::{State, Path, Request},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use kovi::RuntimeBot;
use serde::Deserialize;
use serde_json::json;

#[derive(Clone)]
pub struct AppState {
    pub bot: Arc<RuntimeBot>,
    pub token: String,
    pub self_name: String,
}
```

- [ ] **Step 2: 错误类型**

在 `api.rs` 追加:
```rust
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, msg) = match self {
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (code, Json(json!({ "error": msg }))).into_response()
    }
}
```

- [ ] **Step 3: auth 中间件 + login handler**

追加:
```rust
async fn auth_mw(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let header = req.headers().get(AUTHORIZATION).and_then(|v| v.to_str().ok());
    if check_bearer(header, &st.token) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" }))).into_response()
    }
}

#[derive(Deserialize)]
struct LoginReq {
    password: String,
}

async fn login(State(st): State<AppState>, Json(body): Json<LoginReq>) -> Response {
    if body.password == st.token {
        (StatusCode::OK, Json(json!({ "ok": true, "token": st.token }))).into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Json(json!({ "error": "密码错误" }))).into_response()
    }
}
```

- [ ] **Step 4: 插件管理 handlers**

追加:
```rust
async fn list_plugins(State(st): State<AppState>) -> Result<Json<Vec<PluginDto>>, ApiError> {
    let infos = st.bot.get_plugin_info().map_err(|e| ApiError::Internal(e.to_string()))?;
    let dtos = infos
        .into_iter()
        .map(|p| PluginDto { name: p.name, version: p.version, enabled: p.enabled })
        .collect();
    Ok(Json(dtos))
}

async fn enable(State(st): State<AppState>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    st.bot.enable_plugin(&name).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn disable(State(st): State<AppState>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    if name == st.self_name {
        return Err(ApiError::BadRequest("不能禁用面板插件自身".into()));
    }
    st.bot.disable_plugin(&name).map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn restart(State(st): State<AppState>, Path(name): Path<String>) -> Result<StatusCode, ApiError> {
    if name == st.self_name {
        return Err(ApiError::BadRequest("不能重启面板插件自身".into()));
    }
    st.bot.restart_plugin(&name).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn status(State(st): State<AppState>) -> Json<serde_json::Value> {
    let count = st.bot.get_plugin_info().map(|v| v.len()).unwrap_or(0);
    Json(json!({ "online": true, "plugin_count": count }))
}
```

- [ ] **Step 5: router 组装(导出 `build_router`)**

追加:
```rust
pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/plugins", get(list_plugins))
        .route("/plugins/:name/enable", post(enable))
        .route("/plugins/:name/disable", post(disable))
        .route("/plugins/:name/restart", post(restart))
        .route("/status", get(status))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_mw));

    let api = Router::new().route("/login", post(login)).merge(protected);

    Router::new()
        .nest("/api", api)
        .fallback(crate::embed::static_handler)
        .with_state(state)
}
```

- [ ] **Step 6: 编译验证**（embed 模块下一任务建,先临时留桩)

在 `plugins/panel/src/lib.rs` 加 `mod embed;`,并先创建 `plugins/panel/src/embed.rs` 桩:
```rust
use axum::response::{Html, IntoResponse, Response};

pub async fn static_handler() -> Response {
    Html("<!doctype html><title>panel</title>panel").into_response()
}
```
Run: `cargo build -p kovi-plugin-panel`
Expected: 编译成功。

- [ ] **Step 7: Commit**

```bash
git add plugins/panel/src
git commit -m "feat(panel): axum router/handlers/auth/错误 + static 桩"
```

---

## Task 5: rust-embed 静态资源 + SPA fallback

**Files:**
- Modify: `plugins/panel/src/embed.rs`

- [ ] **Step 1: 实现真正的 static_handler**

用真实现替换 `plugins/panel/src/embed.rs` 全文:
```rust
use axum::{
    extract::Uri,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Assets;

/// 静态资源;找不到就回退 index.html(SPA 前端路由)。
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
    }

    match Assets::get("index.html") {
        Some(content) => (
            [(header::CONTENT_TYPE, "text/html")],
            content.data,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
```

- [ ] **Step 2: 编译验证**（`frontend/dist/index.html` 占位已存在)

Run: `cargo build -p kovi-plugin-panel`
Expected: 编译成功。

- [ ] **Step 3: Commit**

```bash
git add plugins/panel/src/embed.rs
git commit -m "feat(panel): rust-embed 静态资源 + SPA fallback"
```

---

## Task 6: 插件入口起 server

**Files:**
- Modify: `plugins/panel/src/lib.rs`

- [ ] **Step 1: 用完整入口替换 `lib.rs`**

`plugins/panel/src/lib.rs` 全文:
```rust
mod config;
mod api;
mod embed;

use kovi::PluginBuilder;
use kovi::log::{error, info};
use crate::api::{build_router, AppState};
use crate::config::read_config;

#[kovi::plugin]
async fn main() {
    let bot = PluginBuilder::get_runtime_bot();
    let self_name = PluginBuilder::get_plugin_name();
    let cfg = read_config(bot.clone());
    let port = cfg.port;

    let state = AppState {
        bot,
        token: cfg.token,
        self_name,
    };
    let app = build_router(state);

    kovi::tokio::spawn(async move {
        let addr = ("0.0.0.0", port);
        match kovi::tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                info!("panel server 监听 0.0.0.0:{port}");
                if let Err(e) = axum::serve(listener, app).await {
                    error!("panel server 退出: {e}");
                }
            }
            Err(e) => error!("panel server 绑定 :{port} 失败: {e}"),
        }
    });
}
```

**备注(Send/Sync 降级方案):** 若 `AppState` 因 `Arc<RuntimeBot>` 报 `Send`/`Sync` 编译错误,不要改架构主线——改为:在 `main` 里建一个 `tokio::sync::mpsc` 命令 channel,server task 只发命令(`{ List, Enable(name), ... }` + oneshot 回执),在插件自身 task 里 `while let Some(cmd) = rx.recv()` 调 `RuntimeBot`。State 里改存 `mpsc::Sender`(Send+Sync)。handler 逻辑不变,只是经 channel 转一手。

- [ ] **Step 2: 编译验证**

Run: `cargo build`
Expected: 整个 workspace 编译成功。

- [ ] **Step 3: 单测回归**

Run: `cargo test -p kovi-plugin-panel`
Expected: config + api 的单测全通过。

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/src/lib.rs
git commit -m "feat(panel): 插件入口读配置并 spawn axum server"
```

---

## Task 7: 前端脚手架(Vite + React + TS + Tailwind + shadcn/ui)

**Files:**
- Create: `plugins/panel/frontend/`(整套)

> 说明:以下命令在 `plugins/panel/frontend/` 下执行,包管理一律用 **bun**。这是本地开发脚手架步骤,不违反"不在服务器构建"(仅本地生成源码与配置)。

- [ ] **Step 1: 初始化 Vite React-TS 工程**

```bash
cd plugins/panel/frontend
bun create vite . --template react-ts
bun install
```
(若目录非空提示,选择在当前目录初始化;保留已存在的 `dist/index.html` 占位。)

- [ ] **Step 2: 装 Tailwind + 依赖**

```bash
bun add -d tailwindcss postcss autoprefixer
bunx tailwindcss init -p
bun add class-variance-authority clsx tailwind-merge lucide-react sonner
```

- [ ] **Step 3: 配置 Tailwind**

`tailwind.config.js` 的 `content` 改为:
```js
content: ["./index.html", "./src/**/*.{ts,tsx}"],
```
`src/index.css` 顶部替换为:
```css
@tailwind base;
@tailwind components;
@tailwind utilities;
```

- [ ] **Step 4: 配置路径别名(shadcn 需要)**

`vite.config.ts` 全文:
```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
  server: {
    proxy: {
      "/api": "http://localhost:8080",
    },
  },
});
```
`tsconfig.json` 的 `compilerOptions` 加:
```json
"baseUrl": ".",
"paths": { "@/*": ["./src/*"] }
```

- [ ] **Step 5: 初始化 shadcn 并加组件**

```bash
bunx shadcn@latest init -d
bunx shadcn@latest add button switch table card input
```
Expected: 生成 `src/components/ui/{button,switch,table,card,input}.tsx` 与 `components.json`、`src/lib/utils.ts`。

- [ ] **Step 6: 构建冒烟**

Run: `bun run build`
Expected: 生成 `dist/index.html` 与 `dist/assets/`,无报错。

- [ ] **Step 7: Commit**

```bash
git add plugins/panel/frontend
git commit -m "feat(panel): 前端脚手架 Vite+React+TS+Tailwind+shadcn"
```

---

## Task 8: 前端 API 客户端(TDD)

**Files:**
- Create: `plugins/panel/frontend/src/api.ts`
- Create: `plugins/panel/frontend/src/api.test.ts`
- Modify: `plugins/panel/frontend/package.json`(vitest)

- [ ] **Step 1: 装 vitest**

```bash
cd plugins/panel/frontend
bun add -d vitest
```
`package.json` 的 `scripts` 加:`"test": "vitest run"`。

- [ ] **Step 2: 写失败测试**

`plugins/panel/frontend/src/api.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setToken, getToken, apiFetch, clearToken } from "./api";

beforeEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("token storage", () => {
  it("stores and reads token", () => {
    setToken("abc");
    expect(getToken()).toBe("abc");
  });
});

describe("apiFetch", () => {
  it("attaches bearer header", async () => {
    setToken("secret");
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ ok: true }), { status: 200 })
    );
    vi.stubGlobal("fetch", fetchMock);
    await apiFetch("/api/plugins");
    const headers = fetchMock.mock.calls[0][1].headers;
    expect(headers.Authorization).toBe("Bearer secret");
  });

  it("clears token and throws on 401", async () => {
    setToken("secret");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("", { status: 401 })));
    await expect(apiFetch("/api/plugins")).rejects.toThrow("unauthorized");
    expect(getToken()).toBeNull();
  });
});
```

- [ ] **Step 3: 运行确认失败**

Run: `bun run test`
Expected: FAIL(`./api` 模块不存在)。

- [ ] **Step 4: 实现 `api.ts`**

`plugins/panel/frontend/src/api.ts`:
```ts
const TOKEN_KEY = "panel_token";

export function setToken(t: string) {
  localStorage.setItem(TOKEN_KEY, t);
}
export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}
export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export async function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const token = getToken();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(init.headers as Record<string, string>),
  };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    clearToken();
    throw new Error("unauthorized");
  }
  return res;
}

export interface Plugin {
  name: string;
  version: string;
  enabled: boolean;
}

export async function login(password: string): Promise<string> {
  const res = await fetch("/api/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password }),
  });
  if (!res.ok) throw new Error("密码错误");
  const data = await res.json();
  return data.token as string;
}

export async function listPlugins(): Promise<Plugin[]> {
  const res = await apiFetch("/api/plugins");
  return res.json();
}

export async function setPluginEnabled(name: string, enabled: boolean): Promise<void> {
  const action = enabled ? "enable" : "disable";
  const res = await apiFetch(`/api/plugins/${encodeURIComponent(name)}/${action}`, { method: "POST" });
  if (!res.ok) throw new Error(await res.text());
}

export async function restartPlugin(name: string): Promise<void> {
  const res = await apiFetch(`/api/plugins/${encodeURIComponent(name)}/restart`, { method: "POST" });
  if (!res.ok) throw new Error(await res.text());
}

export interface Status {
  online: boolean;
  plugin_count: number;
}
export async function getStatus(): Promise<Status> {
  const res = await apiFetch("/api/status");
  return res.json();
}
```

- [ ] **Step 5: 运行确认通过**

Run: `bun run test`
Expected: 全部 PASS。（若 `localStorage` 未定义,在 `package.json` 加 `"vitest": { "environment": "jsdom" }` 并 `bun add -d jsdom`,重跑。)

- [ ] **Step 6: Commit**

```bash
git add plugins/panel/frontend/src/api.ts plugins/panel/frontend/src/api.test.ts plugins/panel/frontend/package.json
git commit -m "feat(panel): 前端 api 客户端(Bearer/401,含 vitest)"
```

---

## Task 9: 登录页

**Files:**
- Create: `plugins/panel/frontend/src/pages/Login.tsx`

- [ ] **Step 1: 实现 Login**

`plugins/panel/frontend/src/pages/Login.tsx`:
```tsx
import { useState } from "react";
import { login, setToken } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { toast } from "sonner";

export default function Login({ onLogin }: { onLogin: () => void }) {
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setLoading(true);
    try {
      const token = await login(password);
      setToken(token);
      onLogin();
    } catch {
      toast.error("密码错误");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-muted p-4">
      <Card className="w-full max-w-sm p-6 space-y-4">
        <h1 className="text-xl font-semibold text-center">kovi 面板</h1>
        <form onSubmit={submit} className="space-y-3">
          <Input
            type="password"
            placeholder="访问密码"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoFocus
          />
          <Button type="submit" className="w-full" disabled={loading}>
            {loading ? "登录中…" : "登录"}
          </Button>
        </form>
      </Card>
    </div>
  );
}
```

- [ ] **Step 2: 编译冒烟**

Run: `bun run build`
Expected: 成功(Login 未被引用会有 unused 警告,忽略;下个任务接线)。

- [ ] **Step 3: Commit**

```bash
git add plugins/panel/frontend/src/pages/Login.tsx
git commit -m "feat(panel): 登录页"
```

---

## Task 10: Dashboard 页

**Files:**
- Create: `plugins/panel/frontend/src/pages/Dashboard.tsx`

- [ ] **Step 1: 实现 Dashboard**

`plugins/panel/frontend/src/pages/Dashboard.tsx`:
```tsx
import { useEffect, useState } from "react";
import {
  listPlugins, setPluginEnabled, restartPlugin, getStatus,
  clearToken, type Plugin, type Status,
} from "@/api";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Card } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { toast } from "sonner";

export default function Dashboard({ onLogout }: { onLogout: () => void }) {
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [status, setStatus] = useState<Status | null>(null);

  async function refresh() {
    try {
      setPlugins(await listPlugins());
      setStatus(await getStatus());
    } catch (e) {
      if ((e as Error).message === "unauthorized") onLogout();
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function toggle(p: Plugin) {
    try {
      await setPluginEnabled(p.name, !p.enabled);
      toast.success(`${p.name} 已${p.enabled ? "禁用" : "启用"}`);
      refresh();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  async function doRestart(p: Plugin) {
    try {
      await restartPlugin(p.name);
      toast.success(`${p.name} 已重启`);
      refresh();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  function logout() {
    clearToken();
    onLogout();
  }

  return (
    <div className="min-h-screen bg-muted p-4 md:p-8">
      <div className="max-w-3xl mx-auto space-y-4">
        <div className="flex items-center justify-between">
          <h1 className="text-2xl font-semibold">kovi 插件管理</h1>
          <Button variant="outline" onClick={logout}>登出</Button>
        </div>
        <Card className="p-4 flex gap-6 text-sm">
          <span>状态:{status?.online ? "🟢 在线" : "⚪ 未知"}</span>
          <span>插件数:{status?.plugin_count ?? "—"}</span>
        </Card>
        <Card className="p-0 overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>插件</TableHead>
                <TableHead>版本</TableHead>
                <TableHead className="text-center">启用</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {plugins.map((p) => (
                <TableRow key={p.name}>
                  <TableCell className="font-medium">{p.name}</TableCell>
                  <TableCell className="text-muted-foreground">{p.version}</TableCell>
                  <TableCell className="text-center">
                    <Switch checked={p.enabled} onCheckedChange={() => toggle(p)} />
                  </TableCell>
                  <TableCell className="text-right">
                    <Button size="sm" variant="outline" onClick={() => doRestart(p)}>
                      重启
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add plugins/panel/frontend/src/pages/Dashboard.tsx
git commit -m "feat(panel): Dashboard 插件管理页"
```

---

## Task 11: App 装配(登录门 + Toaster)

**Files:**
- Modify: `plugins/panel/frontend/src/App.tsx`
- Modify: `plugins/panel/frontend/src/main.tsx`(确认渲染 App)

- [ ] **Step 1: 实现 App**

`plugins/panel/frontend/src/App.tsx` 全文:
```tsx
import { useState } from "react";
import { getToken } from "@/api";
import Login from "@/pages/Login";
import Dashboard from "@/pages/Dashboard";
import { Toaster } from "sonner";

export default function App() {
  const [authed, setAuthed] = useState(() => !!getToken());
  return (
    <>
      {authed ? (
        <Dashboard onLogout={() => setAuthed(false)} />
      ) : (
        <Login onLogin={() => setAuthed(true)} />
      )}
      <Toaster richColors position="top-center" />
    </>
  );
}
```

- [ ] **Step 2: 确认 `main.tsx`**

`plugins/panel/frontend/src/main.tsx` 应为(如与 Vite 模板不同则改成):
```tsx
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App.tsx";
import "./index.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
```

- [ ] **Step 3: 构建 + 测试**

Run: `bun run build && bun run test`
Expected:构建产出 `dist/`;vitest 全通过。

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/frontend/src/App.tsx plugins/panel/frontend/src/main.tsx
git commit -m "feat(panel): App 登录门 + Toaster 装配"
```

---

## Task 12: Dockerfile 前端构建阶段 + compose 端口

**Files:**
- Modify: `Dockerfile`
- Modify: `docker-compose.yml`

- [ ] **Step 1: Dockerfile 加 bun 前端阶段**

在 `Dockerfile` **最顶部**(现有 `### 构建阶段` 之前)插入:
```dockerfile
### 前端构建阶段
FROM oven/bun:1 AS fe
WORKDIR /fe
COPY plugins/panel/frontend/ .
RUN bun install && bun run build
```

- [ ] **Step 2: 把 dist 注入 Rust 构建阶段**

在 Rust `builder` 阶段的 `COPY . .` **之后**、`RUN cargo build --release` **之前**插入:
```dockerfile
COPY --from=fe /fe/dist ./plugins/panel/frontend/dist
```
(覆盖占位 `dist/index.html`,供 rust-embed 编译期嵌入真实前端。)

- [ ] **Step 3: compose 暴露端口**

`docker-compose.yml` 的 `kovi-bot` 服务加(与 `volumes` 同级):
```yaml
    ports:
      - "8080:8080"
```

- [ ] **Step 4: 本地构建验证(Docker)**

Run: `docker build -t kovi-bot:panel-test .`
Expected: 前端阶段 `bun run build` 成功,Rust 阶段编译成功,镜像出成。
（若本机无 Docker/不便,跳过并在部署前于 CI 验证。)

- [ ] **Step 5: Commit**

```bash
git add Dockerfile docker-compose.yml
git commit -m "build(panel): Dockerfile bun 前端阶段 + compose 8080 端口"
```

---

## Task 13: 端到端手工验证

**Files:** 无(验证任务)

- [ ] **Step 1: 本地起 bot(或容器)**

设置 `data/config.toml`(面板插件数据目录,路径同其他插件规则)含:
```toml
port = 8080
token = "test-secret"
```
启动 bot,日志应见 `panel server 监听 0.0.0.0:8080`。

- [ ] **Step 2: 未鉴权访问被拒**

Run: `curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/api/plugins`
Expected: `401`

- [ ] **Step 3: 登录拿 token**

Run: `curl -s -X POST http://localhost:8080/api/login -H 'Content-Type: application/json' -d '{"password":"test-secret"}'`
Expected: `{"ok":true,"token":"test-secret"}`

- [ ] **Step 4: 列插件**

Run: `curl -s http://localhost:8080/api/plugins -H 'Authorization: Bearer test-secret'`
Expected: JSON 数组,含 `kovi-plugin-ai`、`kovi-plugin-panel` 等,字段 `name/version/enabled`。

- [ ] **Step 5: 禁用/启用/重启**

```bash
curl -s -X POST http://localhost:8080/api/plugins/kovi-plugin-60s/disable -H 'Authorization: Bearer test-secret' -o /dev/null -w "%{http_code}\n"   # 200
curl -s -X POST http://localhost:8080/api/plugins/kovi-plugin-60s/enable  -H 'Authorization: Bearer test-secret' -o /dev/null -w "%{http_code}\n"   # 200
curl -s -X POST http://localhost:8080/api/plugins/kovi-plugin-panel/disable -H 'Authorization: Bearer test-secret' -w "\n"   # 400 不能禁用面板自身
```

- [ ] **Step 6: 前端页面**

浏览器开 `http://localhost:8080/` → 登录页 → 输入密码 → 看到插件表格,开关/重启可用,toast 有反馈,刷新后状态持久(localStorage token)。

- [ ] **Step 7: 记录结果**

把实际输出贴进本任务勾选项旁;若某步不符,进 systematic-debugging,勿跳过。

---

## Self-Review 检查记录

- **Spec 覆盖:** 列表/启用/禁用/重启(Task 4)、状态条(Task 4 status + Task 10)、单 token 鉴权(Task 3/4/8)、rust-embed 前端(Task 5/12)、React+Tailwind+shadcn 栈(Task 7-11)、Dockerfile bun 阶段与 compose 端口(Task 12)、自锤护栏(Task 4 disable/restart)——均有对应任务。
- **占位符:** 无 TBD/TODO;所有代码步骤给出完整代码。
- **类型一致:** `PluginDto`/`Plugin`(name/version/enabled)前后端一致;`AppState`{bot,token,self_name} 在 Task 4 定义、Task 6 构造一致;api.ts 的 `setPluginEnabled`/`restartPlugin`/`listPlugins`/`login`/`getStatus` 在 Dashboard/Login 中调用签名一致。
- **已知风险:** `Arc<RuntimeBot>` 的 Send/Sync(Task 6 备注给 channel 降级方案);rust-embed 需 `dist/` 目录存在(占位 index.html + `debug-embed` 特性解决)。
