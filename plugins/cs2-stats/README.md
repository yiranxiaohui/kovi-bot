# CS2 Steam 查询插件

这个插件通过 Steam Web API 查询公开 Steam 资料、CS2 游戏时长和 Steam 累计统计。
Steam Web API 不提供可靠的最近对局、Premier Rating 或段位数据，因此这些内容暂不显示。

## 配置

插件首次启动后，在运行时数据目录下创建或编辑：

```toml
# data/kovi-plugin-cs2-stats/config.toml
api_key = "在 steamcommunity.com/dev/apikey 申请的 Key"
api_base_url = "https://api.steampowered.com"
request_timeout_secs = 15
```

API Key 不要提交到 Git，不要写入镜像或日志。修改配置后重启插件。

## 命令

```text
CS2绑定 <17 位 Steam64 ID>
CS2绑定 https://steamcommunity.com/profiles/<Steam64>
CS2战绩
CS2战绩 <17 位 Steam64 ID>
CS2解绑
CS2帮助
```

Steam 个人资料、游戏详情和游戏统计需要公开；资料设为私密时，接口会返回不可用或缺失数据。
