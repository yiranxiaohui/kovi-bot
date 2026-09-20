use kovi::tokio;
use kovi_onebot::{OneBotDriver, load_local_conf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let driver_config = load_local_conf()?;
    let driver = OneBotDriver::new(driver_config);

    let bot = kovi::build_bot!(driver; kovi_plugin_cmd, kovi_plugin_60s, kovi_plugin_meme, kovi_plugin_ai, kovi_plugin_help, kovi_plugin_panel, kovi_plugin_sts2_update, kovi_plugin_cs2_stats);

    bot.run().await;
    Ok(())
}
