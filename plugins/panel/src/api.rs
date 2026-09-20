use std::sync::Arc;
use axum::{
    Router, Json,
    extract::{State, Path, Request, Query},
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
    pub data_root: std::path::PathBuf,
}

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

#[derive(Debug)]
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

#[derive(Deserialize)]
struct FsQuery {
    #[serde(default)]
    path: String,
}

async fn fs_list(State(st): State<AppState>, Query(q): Query<FsQuery>) -> Result<Json<Vec<crate::fs::FsEntry>>, ApiError> {
    crate::fs::list_dir(&st.data_root, &q.path).map(Json)
}

async fn fs_read(State(st): State<AppState>, Query(q): Query<FsQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let (content, plugin) = crate::fs::read_file(&st.data_root, &q.path)?;
    Ok(Json(json!({ "content": content, "plugin": plugin })))
}

/// 手动限长读 body:超过 `MAX_FILE_SIZE` 返回 400 "文件过大",非 UTF-8 返回 400 "非文本文件"。
///
/// 不用 `body: String` 提取器——那会走 axum 默认 2MB body limit,超限在进
/// handler 前就被拒成 413 纯文本,违反统一 JSON 400 的接口约定。
async fn read_body_limited(body: axum::body::Body) -> Result<String, ApiError> {
    use http_body_util::BodyExt;
    let limited = http_body_util::Limited::new(body, crate::fs::MAX_FILE_SIZE as usize);
    let bytes = match limited.collect().await {
        Ok(c) => c.to_bytes(),
        Err(_) => return Err(ApiError::BadRequest("文件过大".into())),
    };
    String::from_utf8(bytes.to_vec()).map_err(|_| ApiError::BadRequest("非文本文件".into()))
}

async fn fs_write(State(st): State<AppState>, Query(q): Query<FsQuery>, req: axum::extract::Request) -> Result<StatusCode, ApiError> {
    let content = read_body_limited(req.into_body()).await?;
    crate::fs::write_file(&st.data_root, &q.path, &content)?;
    Ok(StatusCode::OK)
}

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/plugins", get(list_plugins))
        .route("/plugins/:name/enable", post(enable))
        .route("/plugins/:name/disable", post(disable))
        .route("/plugins/:name/restart", post(restart))
        .route("/status", get(status))
        .route("/fs/list", get(fs_list))
        .route("/fs/read", get(fs_read))
        .route("/fs/write", axum::routing::put(fs_write))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_mw));

    let api = Router::new().route("/login", post(login)).merge(protected);

    Router::new()
        .nest("/api", api)
        .fallback(crate::embed::static_handler)
        .with_state(state)
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

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        kovi::tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn bad_request_msg(e: ApiError) -> String {
        match e {
            ApiError::BadRequest(m) => m,
            other => panic!("期望 BadRequest,得到 {other:?}"),
        }
    }

    #[test]
    fn body_limited_ok_at_exact_limit() {
        assert_eq!(block_on(read_body_limited(axum::body::Body::from("hello"))).unwrap(), "hello");
        let exact = "a".repeat(crate::fs::MAX_FILE_SIZE as usize);
        assert_eq!(block_on(read_body_limited(axum::body::Body::from(exact.clone()))).unwrap(), exact);
    }

    #[test]
    fn body_limited_rejects_oversize_as_400() {
        let big = "a".repeat(crate::fs::MAX_FILE_SIZE as usize + 1);
        let e = block_on(read_body_limited(axum::body::Body::from(big))).unwrap_err();
        assert_eq!(bad_request_msg(e), "文件过大");
    }

    #[test]
    fn body_limited_rejects_non_utf8() {
        let e = block_on(read_body_limited(axum::body::Body::from(vec![0u8, 159, 146, 150]))).unwrap_err();
        assert_eq!(bad_request_msg(e), "非文本文件");
    }
}
