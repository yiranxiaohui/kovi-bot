# panel 文件浏览器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 panel 插件中增加 `data/` 目录的文件浏览器:浏览目录、查看/编辑文本文件、保存后可一键重启对应插件。

**Architecture:** 后端在 `plugins/panel` 新增 `fs.rs` 模块(纯函数:路径校验/列目录/读/写,便于单测),`api.rs` 挂 3 个薄 handler(`GET /api/fs/list`、`GET /api/fs/read`、`PUT /api/fs/write`)于现有 Bearer auth 中间件之后。前端在 `frontend/src` 新增 `pages/Files.tsx`(浏览+编辑一体,内部状态机),由 `Dashboard.tsx` 挂入。

**Tech Stack:** Rust(axum 0.7 / kovi 0.13)、React 19 + Vite + Tailwind v4、vitest、bun(前端包管理,**本地不跑 build,只跑 test**)。

## Global Constraints

- 设计文档:`docs/superpowers/specs/2026-07-28-panel-file-browser-design.md`。
- 根目录 = `bot.get_data_path()` 的**父目录**(即 `data/`);所有路径为相对路径,逐段校验后 canonicalize 前缀检查;校验失败一律 400、错误信息统一 `"路径无效"`(不区分不存在/越界)。
- 文件读写上限 1MB(`MAX_FILE_SIZE = 1024*1024`);read 仅接受合法 UTF-8;write 只能覆盖已存在的普通文件,原子写(临时文件+rename)。
- 不做:上传/下载/删除/新建/重命名、diff、语法高亮。
- 包管理用 bun;服务器与本地都不执行打包构建(cargo test / vitest 可以)。
- 开发在独立 worktree 进行(treeflow),分支名建议 `panel-file-browser`。

---

### Task 1: 后端 fs 模块(路径校验 + 列/读/写纯函数)

**Files:**
- Create: `plugins/panel/src/fs.rs`
- Modify: `plugins/panel/src/lib.rs`(加 `mod fs;`)
- Modify: `plugins/panel/Cargo.toml`(dev-dependencies 加 `tempfile = "3"`)
- Test: `fs.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `crate::api::ApiError`(已存在:`NotFound/BadRequest/Internal`)。
- Produces(Task 2 依赖,签名逐字):
  - `pub const MAX_FILE_SIZE: u64 = 1024 * 1024;`
  - `pub struct FsEntry { pub name: String, pub is_dir: bool, pub size: u64 }`(derive `Serialize, Debug, PartialEq`)
  - `pub fn resolve_path(root: &Path, rel: &str) -> Result<PathBuf, ApiError>`
  - `pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<FsEntry>, ApiError>`
  - `pub fn read_file(root: &Path, rel: &str) -> Result<(String, Option<String>), ApiError>`(返回 `(content, 顶层目录名)`;文件直接位于根下时为 `None`)
  - `pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), ApiError>`

- [ ] **Step 1: 写失败测试**

`plugins/panel/src/fs.rs`(先写骨架+测试;函数体先 `todo!()` 或直接空文件只写测试也可,推荐骨架):

```rust
use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;
use crate::api::ApiError;

pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

#[derive(Serialize, Debug, PartialEq)]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

fn invalid() -> ApiError {
    ApiError::BadRequest("路径无效".into())
}

pub fn resolve_path(root: &Path, rel: &str) -> Result<PathBuf, ApiError> {
    todo!()
}

pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<FsEntry>, ApiError> {
    todo!()
}

pub fn read_file(root: &Path, rel: &str) -> Result<(String, Option<String>), ApiError> {
    todo!()
}

pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), ApiError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("kovi-plugin-ai")).unwrap();
        fs::write(root.join("kovi-plugin-ai/config.toml"), "model = \"grok\"\n").unwrap();
        fs::write(root.join("kovi-plugin-ai/blob.bin"), [0u8, 159, 146, 150]).unwrap();
        fs::write(root.join("top.txt"), "hi").unwrap();
        dir
    }

    fn err_msg(e: ApiError) -> String {
        match e {
            ApiError::BadRequest(m) | ApiError::NotFound(m) | ApiError::Internal(m) => m,
        }
    }

    #[test]
    fn resolve_ok_and_root() {
        let d = setup();
        assert!(resolve_path(d.path(), "").is_ok());
        assert!(resolve_path(d.path(), "kovi-plugin-ai/config.toml").is_ok());
    }

    #[test]
    fn resolve_rejects_escape() {
        let d = setup();
        for bad in ["..", "a/../../b", "/etc/passwd", "a//b", ".", "a/./b"] {
            let e = resolve_path(d.path(), bad).unwrap_err();
            assert_eq!(err_msg(e), "路径无效", "case: {bad}");
        }
        // 不存在的路径同样报"路径无效"
        let e = resolve_path(d.path(), "no-such-dir/x.toml").unwrap_err();
        assert_eq!(err_msg(e), "路径无效");
    }

    #[test]
    fn resolve_rejects_symlink_escape() {
        let d = setup();
        symlink("/etc", d.path().join("evil")).unwrap();
        let e = resolve_path(d.path(), "evil/passwd").unwrap_err();
        assert_eq!(err_msg(e), "路径无效");
    }

    #[test]
    fn list_dir_sorted_dirs_first() {
        let d = setup();
        let entries = list_dir(d.path(), "").unwrap();
        assert_eq!(entries[0].name, "kovi-plugin-ai");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "top.txt");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, 2);
    }

    #[test]
    fn list_dir_on_file_is_error() {
        let d = setup();
        assert!(list_dir(d.path(), "top.txt").is_err());
    }

    #[test]
    fn read_text_file_with_plugin() {
        let d = setup();
        let (content, plugin) = read_file(d.path(), "kovi-plugin-ai/config.toml").unwrap();
        assert_eq!(content, "model = \"grok\"\n");
        assert_eq!(plugin.as_deref(), Some("kovi-plugin-ai"));
        let (_, plugin) = read_file(d.path(), "top.txt").unwrap();
        assert_eq!(plugin, None);
    }

    #[test]
    fn read_rejects_binary_and_oversize() {
        let d = setup();
        let e = read_file(d.path(), "kovi-plugin-ai/blob.bin").unwrap_err();
        assert_eq!(err_msg(e), "非文本文件");
        fs::write(d.path().join("big.txt"), "a".repeat((MAX_FILE_SIZE + 1) as usize)).unwrap();
        let e = read_file(d.path(), "big.txt").unwrap_err();
        assert_eq!(err_msg(e), "文件过大");
    }

    #[test]
    fn write_overwrites_existing_only() {
        let d = setup();
        write_file(d.path(), "top.txt", "new content").unwrap();
        assert_eq!(fs::read_to_string(d.path().join("top.txt")).unwrap(), "new content");
        // 不能新建
        assert!(write_file(d.path(), "brand-new.txt", "x").is_err());
        // 不能写目录
        assert!(write_file(d.path(), "kovi-plugin-ai", "x").is_err());
    }
}
```

`Cargo.toml` 追加:

```toml
[dev-dependencies]
tempfile = "3"
```

`lib.rs` 第 1-3 行的 mod 声明处加 `mod fs;`。
另:`api.rs` 的 `ApiError` 枚举字段需可被测试模式匹配——它已是 `pub enum` 且变体带 `String`,无需改动。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd plugins/panel && cargo test fs::`
Expected: FAIL(`todo!()` panic 或编译通过后断言失败)

- [ ] **Step 3: 实现**

```rust
pub fn resolve_path(root: &Path, rel: &str) -> Result<PathBuf, ApiError> {
    let mut p = root.to_path_buf();
    if !rel.is_empty() {
        for seg in rel.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\0') || seg.contains('\\') {
                return Err(invalid());
            }
            p.push(seg);
        }
    }
    let canon_root = root.canonicalize().map_err(|_| invalid())?;
    let canon = p.canonicalize().map_err(|_| invalid())?;
    if !canon.starts_with(&canon_root) {
        return Err(invalid());
    }
    Ok(canon)
}

pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<FsEntry>, ApiError> {
    let dir = resolve_path(root, rel)?;
    if !dir.is_dir() {
        return Err(invalid());
    }
    let mut entries = Vec::new();
    for e in fs::read_dir(&dir).map_err(|e| ApiError::Internal(e.to_string()))? {
        let e = e.map_err(|e| ApiError::Internal(e.to_string()))?;
        let meta = e.metadata().map_err(|e| ApiError::Internal(e.to_string()))?;
        entries.push(FsEntry {
            name: e.file_name().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            size: meta.len(),
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

pub fn read_file(root: &Path, rel: &str) -> Result<(String, Option<String>), ApiError> {
    let path = resolve_path(root, rel)?;
    if !path.is_file() {
        return Err(invalid());
    }
    let meta = path.metadata().map_err(|e| ApiError::Internal(e.to_string()))?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(ApiError::BadRequest("文件过大".into()));
    }
    let bytes = fs::read(&path).map_err(|e| ApiError::Internal(e.to_string()))?;
    let content = String::from_utf8(bytes).map_err(|_| ApiError::BadRequest("非文本文件".into()))?;
    let plugin = rel.split('/').next().filter(|_| rel.contains('/')).map(str::to_string);
    Ok((content, plugin))
}

pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), ApiError> {
    let path = resolve_path(root, rel)?;
    if !path.is_file() {
        return Err(invalid());
    }
    let dir = path.parent().ok_or_else(invalid)?;
    let file_name = path.file_name().ok_or_else(invalid)?.to_string_lossy();
    let tmp = dir.join(format!(".{file_name}.panel-tmp"));
    fs::write(&tmp, content).map_err(|e| ApiError::Internal(e.to_string()))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        ApiError::Internal(e.to_string())
    })?;
    Ok(())
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd plugins/panel && cargo test`
Expected: 全部 PASS(含既有 api/config 测试)

- [ ] **Step 5: Commit**

```bash
git add plugins/panel/src/fs.rs plugins/panel/src/lib.rs plugins/panel/Cargo.toml Cargo.lock
git commit -m "feat(panel): fs 模块——data 目录路径校验与列/读/写纯函数"
```

---

### Task 2: 后端 API 接线(路由 + AppState.data_root)

**Files:**
- Modify: `plugins/panel/src/api.rs`
- Modify: `plugins/panel/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 的 `fs::{list_dir, read_file, write_file, FsEntry, MAX_FILE_SIZE}`。
- Produces(前端依赖的 HTTP 契约):
  - `GET /api/fs/list?path=<rel>` → `200 [{"name","is_dir","size"}]` / `400 {"error":"路径无效"}`
  - `GET /api/fs/read?path=<rel>` → `200 {"content": string, "plugin": string|null}` / `400 {"error":"非文本文件"|"文件过大"|"路径无效"}`
  - `PUT /api/fs/write?path=<rel>`(body = 原文 text)→ `200` / `400`
  - 三者均在 Bearer auth 之后;`path` 缺省视为 `""`。

- [ ] **Step 1: api.rs 增加 handler 与路由**

`AppState` 加字段(api.rs:15-19):

```rust
#[derive(Clone)]
pub struct AppState {
    pub bot: Arc<RuntimeBot>,
    pub token: String,
    pub self_name: String,
    pub data_root: std::path::PathBuf,
}
```

api.rs 新增(置于 `status` handler 之后):

```rust
use axum::extract::Query;

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

async fn fs_write(State(st): State<AppState>, Query(q): Query<FsQuery>, body: String) -> Result<StatusCode, ApiError> {
    if body.len() as u64 > crate::fs::MAX_FILE_SIZE {
        return Err(ApiError::BadRequest("文件过大".into()));
    }
    crate::fs::write_file(&st.data_root, &q.path, &body)?;
    Ok(StatusCode::OK)
}
```

`build_router` 的 protected Router 加三行(api.rs:113-119 区域):

```rust
        .route("/fs/list", get(fs_list))
        .route("/fs/read", get(fs_read))
        .route("/fs/write", axum::routing::put(fs_write))
```

- [ ] **Step 2: lib.rs 求 data_root 并传入**

`main()` 中(lib.rs:24-28 区域):

```rust
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
```

- [ ] **Step 3: 跑测试与编译检查**

Run: `cd plugins/panel && cargo test && cargo clippy --all-targets -- -D warnings 2>/dev/null || cargo test`
Expected: 全部 PASS、编译无错(clippy 若项目未装可忽略,以 `cargo test` 通过为准)

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/src/api.rs plugins/panel/src/lib.rs
git commit -m "feat(panel): /api/fs list|read|write 三端点(auth 后,限 data 根)"
```

---

### Task 3: 前端 fs API 封装

**Files:**
- Modify: `plugins/panel/frontend/src/api.ts`
- Test: `plugins/panel/frontend/src/api.test.ts`

**Interfaces:**
- Consumes: 既有 `apiFetch`(自动带 Bearer、401 清 token 抛 `unauthorized`)。
- Produces(Task 4 依赖,签名逐字):
  - `export interface FsEntry { name: string; is_dir: boolean; size: number }`
  - `export async function fsList(path: string): Promise<FsEntry[]>`
  - `export async function fsRead(path: string): Promise<{ content: string; plugin: string | null }>`
  - `export async function fsWrite(path: string, content: string): Promise<void>`

- [ ] **Step 1: 写失败测试**

`api.test.ts` 追加:

```ts
import { fsList, fsRead, fsWrite } from "./api";

describe("fs api", () => {
  it("fsList encodes path and parses entries", async () => {
    setToken("secret");
    const entries = [{ name: "config.toml", is_dir: false, size: 12 }];
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify(entries), { status: 200 })
    );
    vi.stubGlobal("fetch", fetchMock);
    const got = await fsList("kovi-plugin-ai/子目录");
    expect(got).toEqual(entries);
    expect(fetchMock.mock.calls[0][0]).toBe(
      `/api/fs/list?path=${encodeURIComponent("kovi-plugin-ai/子目录")}`
    );
  });

  it("fsRead surfaces server error message", async () => {
    setToken("secret");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ error: "非文本文件" }), { status: 400 })
    ));
    await expect(fsRead("a/blob.bin")).rejects.toThrow("非文本文件");
  });

  it("fsWrite PUTs raw text body", async () => {
    setToken("secret");
    const fetchMock = vi.fn().mockResolvedValue(new Response("", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await fsWrite("a/config.toml", "x = 1\n");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe(`/api/fs/write?path=${encodeURIComponent("a/config.toml")}`);
    expect(init.method).toBe("PUT");
    expect(init.body).toBe("x = 1\n");
    expect(init.headers["Content-Type"]).toBe("text/plain");
  });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd plugins/panel/frontend && bun run test`
Expected: FAIL(`fsList` 等未导出)

- [ ] **Step 3: 实现**

`api.ts` 追加:

```ts
export interface FsEntry {
  name: string;
  is_dir: boolean;
  size: number;
}

async function fsError(res: Response, fallback: string): Promise<Error> {
  try {
    const data = await res.json();
    return new Error(data.error ?? fallback);
  } catch {
    return new Error(fallback);
  }
}

export async function fsList(path: string): Promise<FsEntry[]> {
  const res = await apiFetch(`/api/fs/list?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw await fsError(res, "读取目录失败");
  return res.json();
}

export async function fsRead(path: string): Promise<{ content: string; plugin: string | null }> {
  const res = await apiFetch(`/api/fs/read?path=${encodeURIComponent(path)}`);
  if (!res.ok) throw await fsError(res, "读取文件失败");
  return res.json();
}

export async function fsWrite(path: string, content: string): Promise<void> {
  const res = await apiFetch(`/api/fs/write?path=${encodeURIComponent(path)}`, {
    method: "PUT",
    headers: { "Content-Type": "text/plain" },
    body: content,
  });
  if (!res.ok) throw await fsError(res, "保存失败");
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd plugins/panel/frontend && bun run test`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add plugins/panel/frontend/src/api.ts plugins/panel/frontend/src/api.test.ts
git commit -m "feat(panel/fe): fs API 封装(list/read/write)"
```

---

### Task 4: 前端 Files 页 + Dashboard 集成

**Files:**
- Create: `plugins/panel/frontend/src/pages/Files.tsx`
- Modify: `plugins/panel/frontend/src/pages/Dashboard.tsx`

**Interfaces:**
- Consumes: Task 3 的 `fsList/fsRead/fsWrite/FsEntry`;既有 `restartPlugin`、shadcn `Button/Card/Table`、`sonner` toast。
- Produces: `export default function Files({ initialPath, pluginNames, onBack }: { initialPath: string; pluginNames: string[]; onBack: () => void })`。

- [ ] **Step 1: 实现 Files.tsx**

```tsx
import { useEffect, useState } from "react";
import { fsList, fsRead, fsWrite, restartPlugin, type FsEntry } from "@/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { toast } from "sonner";

const SELF = "kovi-plugin-panel";

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

interface Editing {
  path: string;
  content: string;
  original: string;
  plugin: string | null;
}

export default function Files({
  initialPath, pluginNames, onBack,
}: { initialPath: string; pluginNames: string[]; onBack: () => void }) {
  const [path, setPath] = useState(initialPath);
  const [entries, setEntries] = useState<FsEntry[]>([]);
  const [editing, setEditing] = useState<Editing | null>(null);

  const dirty = editing !== null && editing.content !== editing.original;

  async function load(p: string) {
    try {
      setEntries(await fsList(p));
      setPath(p);
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  useEffect(() => {
    load(initialPath);
  }, [initialPath]);

  async function openFile(p: string) {
    try {
      const { content, plugin } = await fsRead(p);
      setEditing({ path: p, content, original: content, plugin });
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  function confirmDiscard(): boolean {
    return !dirty || window.confirm("有未保存的修改,确定放弃?");
  }

  async function save(restart: boolean) {
    if (!editing) return;
    try {
      await fsWrite(editing.path, editing.content);
      setEditing({ ...editing, original: editing.content });
      toast.success("已保存");
      if (restart && editing.plugin) {
        await restartPlugin(editing.plugin);
        toast.success(`${editing.plugin} 已重启`);
      }
    } catch (e) {
      toast.error((e as Error).message);
    }
  }

  const crumbs = path === "" ? [] : path.split("/");
  const canRestart =
    editing?.plugin && editing.plugin !== SELF && pluginNames.includes(editing.plugin);

  if (editing) {
    return (
      <div className="min-h-screen bg-muted p-4 md:p-8">
        <div className="max-w-4xl mx-auto space-y-4">
          <div className="flex items-center justify-between gap-2 flex-wrap">
            <div className="text-sm text-muted-foreground break-all">
              data / {editing.path.split("/").join(" / ")}
              {dirty && <span className="ml-2 text-orange-500">●未保存</span>}
            </div>
            <div className="flex gap-2">
              <Button variant="outline" onClick={() => { if (confirmDiscard()) setEditing(null); }}>
                返回
              </Button>
              <Button onClick={() => save(false)} disabled={!dirty}>保存</Button>
              {canRestart && (
                <Button variant="secondary" onClick={() => save(true)}>
                  保存并重启 {editing.plugin}
                </Button>
              )}
            </div>
          </div>
          <textarea
            className="w-full h-[70vh] font-mono text-sm p-3 rounded-md border bg-background resize-none"
            value={editing.content}
            onChange={(e) => setEditing({ ...editing, content: e.target.value })}
            spellCheck={false}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-muted p-4 md:p-8">
      <div className="max-w-3xl mx-auto space-y-4">
        <div className="flex items-center justify-between">
          <div className="text-sm break-all">
            <button className="underline-offset-2 hover:underline" onClick={() => load("")}>data</button>
            {crumbs.map((seg, i) => (
              <span key={i}>
                {" / "}
                <button
                  className="underline-offset-2 hover:underline"
                  onClick={() => load(crumbs.slice(0, i + 1).join("/"))}
                >
                  {seg}
                </button>
              </span>
            ))}
          </div>
          <Button variant="outline" onClick={onBack}>返回插件列表</Button>
        </div>
        <Card className="p-0 overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>名称</TableHead>
                <TableHead className="text-right">大小</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {entries.map((e) => {
                const child = path === "" ? e.name : `${path}/${e.name}`;
                return (
                  <TableRow
                    key={e.name}
                    className="cursor-pointer"
                    onClick={() => (e.is_dir ? load(child) : openFile(child))}
                  >
                    <TableCell className="font-medium">
                      {e.is_dir ? "📁" : "📄"} {e.name}
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {e.is_dir ? "—" : fmtSize(e.size)}
                    </TableCell>
                  </TableRow>
                );
              })}
              {entries.length === 0 && (
                <TableRow>
                  <TableCell colSpan={2} className="text-center text-muted-foreground">
                    空目录
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Dashboard 集成**

`Dashboard.tsx` 改动:

```tsx
// import 区追加
import Files from "@/pages/Files";

// 组件内 state 追加
const [filesPath, setFilesPath] = useState<string | null>(null);

// return 之前追加
if (filesPath !== null) {
  return (
    <Files
      initialPath={filesPath}
      pluginNames={plugins.map((p) => p.name)}
      onBack={() => setFilesPath(null)}
    />
  );
}
```

标题行加总入口(`登出` 按钮旁):

```tsx
<div className="flex gap-2">
  <Button variant="outline" onClick={() => setFilesPath("")}>文件</Button>
  <Button variant="outline" onClick={logout}>登出</Button>
</div>
```

插件表格「操作」列的重启按钮旁加:

```tsx
<Button size="sm" variant="ghost" onClick={() => setFilesPath(p.name)}>
  文件
</Button>
```

注:`data/<插件名>/` 目录可能不存在(插件从未写过数据)——fsList 会报"路径无效",toast 提示即可,属预期行为。

- [ ] **Step 3: 跑前端测试 + 类型检查**

Run: `cd plugins/panel/frontend && bun run test && bunx tsc --noEmit`
Expected: 测试 PASS、tsc 无错误

- [ ] **Step 4: Commit**

```bash
git add plugins/panel/frontend/src/pages/Files.tsx plugins/panel/frontend/src/pages/Dashboard.tsx
git commit -m "feat(panel/fe): 文件浏览器页(浏览/编辑/保存并重启)"
```

---

### Task 5: 合并与上线验证

**Files:** 无代码改动;流程任务。

- [ ] **Step 1: 全量测试**

Run: `cd plugins/panel && cargo test && cd frontend && bun run test`
Expected: 全部 PASS

- [ ] **Step 2: 合并到 main 并推送**

按 superpowers:finishing-a-development-branch 流程(worktree 分支 → main,squash 或 merge 按其引导),push 触发 CI 构建镜像。⚠️ 本地与服务器都不跑 build;镜像由 CI(Github-Runner 主机)出。

- [ ] **Step 3: 服务器部署**

```bash
ssh root@10.1.39.1 'nohup sh -c "docker compose -f /opt/kovi-bot/docker-compose.yml --project-directory /opt/kovi-bot pull kovi-bot && docker compose -f /opt/kovi-bot/docker-compose.yml --project-directory /opt/kovi-bot up -d kovi-bot && echo DEPLOY_DONE" > /tmp/kovi-deploy.log 2>&1 &'
# 轮询 /tmp/kovi-deploy.log 直到 DEPLOY_DONE(镜像大,pull 常 >150s,勿用短超时前台 ssh)
```

- [ ] **Step 4: 线上验证**

```bash
TOKEN=b88793d878fdcd0b3769e155081bcec3
curl -s -H "Authorization: Bearer $TOKEN" "http://10.1.39.1:8080/api/fs/list?path=" | head -c 300
curl -s -H "Authorization: Bearer $TOKEN" "http://10.1.39.1:8080/api/fs/read?path=kovi-plugin-ai/config.toml" | head -c 200
# 越界必须 400:
curl -s -o /dev/null -w "%{http_code}\n" -H "Authorization: Bearer $TOKEN" "http://10.1.39.1:8080/api/fs/read?path=../kovi.conf.toml"
# 无 token 必须 401:
curl -s -o /dev/null -w "%{http_code}\n" "http://10.1.39.1:8080/api/fs/list?path="
# 机器人仍在线:
ssh root@10.1.39.1 "docker logs kovi-bot --tail 5 2>&1 | grep -c 'Bot connection successful'" || true
```

Expected: list 返回 JSON 数组、read 返回 config 内容、越界 400、无 token 401、bot 在线。

- [ ] **Step 5: 更新项目记忆**

`panel-plugin.md` 补一行:文件浏览器已上线(/api/fs 三端点,限 data 根,1MB/UTF-8,只覆盖不新建)。
