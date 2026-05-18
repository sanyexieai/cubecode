//! `read_file`：只读读取工作区内普通文件。

use std::fs;
use std::path::{Path, PathBuf};

use super::error::ToolError;

/// 单文件最大读取字节（默认 512 KiB）。
pub const DEFAULT_MAX_BYTES: u64 = 512 * 1024;

/// 解析 `read_file` 参数：JSON `{"path":"…"}` 或裸路径字符串。
pub fn parse_path_argument(arguments: &str) -> Result<PathBuf, ToolError> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Err(ToolError::InvalidArguments("缺少 path".into()));
    }
    if trimmed.starts_with('{') {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args = serde_json::from_str(trimmed)
            .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        return Ok(PathBuf::from(args.path));
    }
    Ok(PathBuf::from(trimmed))
}

/// 将用户路径解析为工作区内规范路径；越界则拒绝。
pub fn resolve_whitelisted_path(
    workspace_root: &Path,
    user_path: &Path,
) -> Result<PathBuf, ToolError> {
    let workspace = workspace_root
        .canonicalize()
        .map_err(|e| ToolError::Io(format!("无法解析工作区根目录：{e}")))?;
    let joined = if user_path.is_absolute() {
        user_path.to_path_buf()
    } else {
        workspace.join(user_path)
    };
    let canonical = joined
        .canonicalize()
        .map_err(|e| ToolError::Io(format!("无法解析路径 {}：{e}", joined.display())))?;
    if !canonical.starts_with(&workspace) {
        return Err(ToolError::PathNotAllowed {
            path: canonical,
            workspace,
        });
    }
    Ok(canonical)
}

/// 读取文件全文（UTF-8 有损则按无效字节替换）。
pub fn read_file(
    workspace_root: &Path,
    arguments: &str,
    max_bytes: u64,
) -> Result<String, ToolError> {
    let user_path = parse_path_argument(arguments)?;
    let path = resolve_whitelisted_path(workspace_root, &user_path)?;
    let meta = fs::metadata(&path).map_err(|e| ToolError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(ToolError::NotAFile(path));
    }
    if meta.len() > max_bytes {
        return Err(ToolError::FileTooLarge {
            path,
            max_bytes,
        });
    }
    let bytes = fs::read(&path).map_err(|e| ToolError::Io(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let file = root.join("allowed.txt");
        fs::write(&file, "hello cubecode").unwrap();
        (dir, root)
    }

    #[test]
    fn reads_file_under_workspace() {
        let (_guard, root) = temp_workspace();
        let out = read_file(&root, "allowed.txt", DEFAULT_MAX_BYTES).expect("read");
        assert_eq!(out, "hello cubecode");
    }

    #[test]
    fn reads_json_arguments() {
        let (_guard, root) = temp_workspace();
        let out = read_file(&root, r#"{"path":"allowed.txt"}"#, DEFAULT_MAX_BYTES).expect("read");
        assert_eq!(out, "hello cubecode");
    }

    #[test]
    fn rejects_path_outside_workspace() {
        let (_guard, root) = temp_workspace();
        let outside = std::env::temp_dir().join("cubecode_tool_escape_test.txt");
        fs::write(&outside, "secret").unwrap();
        let err = read_file(&root, outside.to_string_lossy().as_ref(), DEFAULT_MAX_BYTES)
            .expect_err("escape");
        assert!(matches!(err, ToolError::PathNotAllowed { .. }));
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn rejects_directory() {
        let (_guard, root) = temp_workspace();
        let err = read_file(&root, ".", DEFAULT_MAX_BYTES).expect_err("dir");
        assert!(matches!(err, ToolError::NotAFile(_)));
    }

    #[test]
    fn rejects_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let big = root.join("big.bin");
        let mut f = fs::File::create(&big).unwrap();
        f.write_all(&[0u8; 64]).unwrap();
        let err = read_file(root, "big.bin", 32).expect_err("too big");
        assert!(matches!(err, ToolError::FileTooLarge { .. }));
    }
}
