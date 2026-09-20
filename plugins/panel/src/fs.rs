//! panel 文件浏览器的纯函数层:data/ 目录内的路径校验与列/读/写。
//!
//! # 已知限制:resolve_path 的 TOCTOU
//!
//! `resolve_path` 用 `canonicalize` 校验路径落在根目录内,但校验通过后、
//! 实际读写发生前,目录结构理论上可能被替换为指向根外的符号链接
//! (time-of-check to time-of-use 窗口)。当前威胁模型下面板管理员自己
//! 控制 data/ 目录,能在该窗口内改目录结构的人本就拥有更高权限,利用
//! 面极窄,故接受该限制、不引入 openat 等平台相关的加固手段。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use serde::Serialize;
use crate::api::ApiError;

/// 写临时文件名的进程内唯一计数器,避免并发写同一文件时共享 tmp 路径。
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    let tmp = dir.join(format!(
        ".{file_name}.{}.panel-tmp",
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, content).map_err(|e| ApiError::Internal(e.to_string()))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        ApiError::Internal(e.to_string())
    })?;
    Ok(())
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

    #[test]
    fn write_concurrent_same_file_no_corruption() {
        let d = setup();
        let root = d.path().to_path_buf();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let root = root.clone();
                std::thread::spawn(move || {
                    let content = format!("writer-{i}-").repeat(200);
                    for _ in 0..20 {
                        write_file(&root, "top.txt", &content).unwrap();
                    }
                    content
                })
            })
            .collect();
        let contents: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // 最终内容必须是某个线程写入的完整值,不能是交错/污染的混合体
        let last = fs::read_to_string(root.join("top.txt")).unwrap();
        assert!(contents.contains(&last), "文件内容被并发写污染");
        // 不残留临时文件
        for e in fs::read_dir(&root).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.ends_with(".panel-tmp"), "残留临时文件: {name}");
        }
    }
}
