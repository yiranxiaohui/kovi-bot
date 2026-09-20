# panel 文件浏览器设计(2026-07-28)

## 背景与目标

panel 插件(plugins/panel)已提供运行时插件管理(列表/启停/重启)。各插件的配置文件存放在
`data/<插件名>/`(线上为 `/opt/kovi-bot/kovi_data`,容器内 `/app/data`),目前只能 SSH 进服务器改。
本功能在面板中增加一个**文件浏览器**:浏览 `data/` 下任意目录、查看/编辑文本文件,保存后可一键重启对应插件。

明确不做(YAGNI):上传、下载、删除、重命名、新建文件/目录;diff/历史版本;语法高亮(用等宽 textarea)。

## 范围与安全边界

- **根目录限定为 `data/`**。由 `bot.get_data_path()` 的父目录求得。不暴露 `/app` 其余内容
  (`kovi.conf.toml` 是 `:ro` bind 挂载,改不了;程序本体无浏览意义)。
- **路径校验(核心安全点)**:客户端传相对路径,逐段校验——拒绝绝对路径、`..`、空段、含 NUL 的段;
  通过后再对实际路径 `canonicalize`,校验结果必须以 data 根(同样 canonicalize)为前缀,防符号链接逃逸。
  校验失败一律 400,不区分"不存在"与"越界"(避免探测)。
- **只能覆盖已存在的文件**,不能创建新文件。
- 所有新 API 挂在现有 Bearer token auth 中间件之后。

## 后端 API(plugins/panel/src/api.rs 或拆新模块 fs.rs)

1. `GET /api/fs/list?path=<相对路径>`
   - path 为空 = data 根。返回 `[{ name, is_dir, size }]`,目录在前、各自按名排序。
   - path 指向文件或不存在 → 400。
2. `GET /api/fs/read?path=<相对路径>`
   - 文件 ≤ 1MB 且内容为合法 UTF-8 → `{ content, plugin }`。
     `plugin` = path 的顶层目录名(如 `kovi-plugin-ai`),供前端判断是否显示重启按钮。
   - 二进制(非 UTF-8)或 >1MB → 400,错误信息区分"文件过大"/"非文本文件"。
3. `PUT /api/fs/write?path=<相对路径>`
   - body 为新文件内容(text/plain)。目标必须已存在且为普通文件。
   - 原子写:写同目录临时文件后 rename 覆盖。成功 200。
   - 内容大小同样限 1MB(axum body limit)。

## 前端(plugins/panel/frontend)

- **入口**:插件列表页每行加「文件」按钮直达 `data/<插件名>/`;页面顶部另有总入口进 `data/` 根。
- **浏览页**:面包屑 + 单栏列表(非树控件)。目录可点进;文件显示大小;
  文本文件点击进编辑器;read 返回 400 的文件置灰标注"不可查看"(实现上:列表阶段不预判,
  点击后由 read 接口报错再提示即可,避免列表页 N 次探测)。
- **编辑页**:等宽 `<textarea>` 撑满,顶部面包屑 + 「保存」+「保存并重启 <插件名>」。
  重启按钮仅当 `plugin` 字段匹配当前运行插件列表、且不是 panel 自身时显示
  (沿用后端已有的不能重启自身护栏)。有未保存改动时,路由离开前 confirm。
- 保存/重启结果沿用现有提示样式。

## 测试

- 后端单测:路径校验(正常/`..`/绝对路径/空段/符号链接逃逸)、list/read/write 各接口的
  成功与错误分支(二进制、超 1MB、写不存在的文件、写目录)。
- 前端:沿用 `api.test.ts` 模式补 fs API 封装的单测。
- 本地冒烟沿用已知坑:用 dummy WS server 顶住 `kovi.conf.toml` 的 server 地址让进程存活,再 curl。

## 部署

链路不变:merge 到 main → CI 构建镜像推 Harbor → 服务器(10.1.39.1)pull + up -d。
