use kovi::PluginBuilder;
use kovi_onebot::EventRegistrar;

const HELP_MAIN: &str = "\
🤖 机器人帮助菜单
━━━━━━━━━━━━━━
📰 资讯  /60s 每日60秒读懂世界
⚔️ 尖塔  杀戮尖塔2更新订阅与中文翻译
😆 表情  摸摸头 / 捶爆 / 小丑 / 手枪 / 上香 …
🌾 农场  登录农场 / 农场状态 / 农场在线数 …
🧠 AI    @我 直接对话 ｜ @我 生成图片<描述>
🛠 管理  .kovi help（管理员）
🎮 CS2   Steam 资料、时长和累计统计查询
━━━━━━━━━━━━━━
查看分类详情：
/help 表情 ｜ /help ai ｜ /help 60s ｜ /help 尖塔2 ｜ /help CS2 ｜ /help 农场 ｜ /help 管理";

const HELP_60S: &str = "\
📰 资讯模块
━━━━━━━━━━━━━━
/60s  —— 每日 60 秒读懂世界（图片日报）";

const HELP_STS2: &str = "\
⚔️ 杀戮尖塔 2 更新提醒
━━━━━━━━━━━━━━
尖塔2订阅 [全部/正式版/测试版]  —— 开启或修改本群订阅
尖塔2取消订阅  —— 关闭本群订阅
尖塔2最新 [全部/正式版/测试版]  —— 查看最新中文更新
尖塔2状态  —— 查看本群订阅状态
尖塔2帮助  —— 查看本说明
（订阅设置仅限群主、群管理员或机器人管理员）";

const HELP_MEME: &str = "\
😆 表情模块（对我发送，或 @某人 一起玩）
━━━━━━━━━━━━━━
摸摸头 ｜ 捶爆（爆捶）｜ 戒导
小丑 ｜ 小丑面具 ｜ 手枪
上香 ｜ 催眠app";

const HELP_AI: &str = "\
🧠 AI 模块（需要 @我 触发）
━━━━━━━━━━━━━━
@我 + 任意内容  —— 和我对话
@我 生成图片<描述>  —— AI 文生图";

const HELP_FARM: &str = "\
🌾 农场模块
━━━━━━━━━━━━━━
登录农场 ｜ 农场状态 ｜ 农场在线数
退出农场 ｜ 获取QQ名
（发送「农场帮助」查看完整说明）";

const HELP_CS2: &str = "🎮 CS2 Steam 查询\n━━━━━━━━━━━━━━\nCS2绑定 <Steam64或数字社区链接>\nCS2战绩 —— 查询已绑定账号\nCS2战绩 <Steam64或数字社区链接>\nCS2解绑\n（Steam Web API 基础查询，不含最近对局和段位）";
const HELP_ADMIN: &str = "\
🛠 管理模块（仅管理员）
━━━━━━━━━━━━━━
.kovi help  —— 查看管理命令
.kovi plugin list ｜ start/stop/restart <name>
.kovi status  —— 运行状态";

#[kovi::plugin]
async fn main() {
    PluginBuilder::on_msg(|event| async move {
        let text = match event.borrow_text() {
            Some(t) => t.trim(),
            None => return,
        };

        // 先匹配带参数的子命令，最后才是裸 /help，避免 "/help 表情" 误中总览
        let reply = match text {
            "/help 表情" | "表情帮助" | "/help meme" => Some(HELP_MEME),
            "/help ai" | "ai帮助" | "AI帮助" => Some(HELP_AI),
            "/help 60s" | "60s帮助" => Some(HELP_60S),
            "/help 尖塔2" | "尖塔2帮助" | "杀戮尖塔2帮助" => Some(HELP_STS2),
            "/help 农场" | "/help farm" => Some(HELP_FARM),
            "/help CS2" | "CS2帮助" | "cs2帮助" => Some(HELP_CS2),
            "/help 管理" | "/help admin" => Some(HELP_ADMIN),
            "/help" | "/menu" | "/菜单" | "菜单" => Some(HELP_MAIN),
            _ => None,
        };

        if let Some(msg) = reply {
            event.reply(msg);
        }
    });
}
