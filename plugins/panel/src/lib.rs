mod config;
mod api;
mod embed;
mod fs;

use kovi::PluginBuilder;
use kovi::log::{error, info, warn};
use crate::api::{build_router, AppState};
use crate::config::read_config;

#[kovi::plugin]
async fn main() {
    let bot = PluginBuilder::get_runtime_bot();
    let self_name = PluginBuilder::get_plugin_name();
    let cfg = read_config(bot.clone());
    let port = cfg.port;

    if cfg.token == "change-me" {
        warn!(
            "panel 仍在使用默认 token \"change-me\",任何能访问 :{port} 的人都可开关插件;\
             请修改 data/kovi-plugin-panel/config.toml 的 token 后再对外暴露端口"
        );
    }

    let data_root = bot
        .get_data_path()
        .parent()
        .expect("data path 应有父目录")
        .to_path_buf();

    let state = AppState {
        bot,
        token: cfg.token,
        self_name,
        data_root,
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
