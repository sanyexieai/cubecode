//! ⑤ 工具执行（v1：`read_file` 只读 + 工作区白名单）。

mod error;
mod read_file;

pub use error::ToolError;
pub use read_file::{parse_path_argument, resolve_whitelisted_path, DEFAULT_MAX_BYTES};

use std::path::{Path, PathBuf};

/// 工具注册表（v1 仅内置 `read_file`）。
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    workspace_root: PathBuf,
    max_read_bytes: u64,
}

impl ToolRegistry {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            max_read_bytes: DEFAULT_MAX_BYTES,
        }
    }

    /// 默认工作区：环境变量 `CUBECODE_WORKSPACE_ROOT`，否则当前目录。
    pub fn from_env() -> Self {
        Self::new(workspace_root_from_env())
    }

    pub fn with_max_read_bytes(mut self, max_bytes: u64) -> Self {
        self.max_read_bytes = max_bytes;
        self
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// 列出 v1 已注册工具名。
    pub fn tool_names(&self) -> &'static [&'static str] {
        &["read_file"]
    }

    /// 执行工具；返回 UTF-8 文本结果（供 `ToolResult` / 模型回灌）。
    pub fn execute(&self, tool_name: &str, arguments: &str) -> Result<String, ToolError> {
        tracing::info!(
            target: "cubecode.step.tools",
            %tool_name,
            workspace = %self.workspace_root.display(),
            args_bytes = arguments.len(),
            "⑤执行层：调用工具"
        );
        let result = match tool_name {
            "read_file" => read_file::read_file(
                &self.workspace_root,
                arguments,
                self.max_read_bytes,
            ),
            other => Err(ToolError::UnknownTool(other.to_owned())),
        };
        match &result {
            Ok(body) => tracing::info!(
                target: "cubecode.step.tools",
                %tool_name,
                out_bytes = body.len(),
                "⑤执行层：工具完成"
            ),
            Err(e) => tracing::warn!(
                target: "cubecode.step.tools",
                %tool_name,
                error = %e,
                "⑤执行层：工具失败"
            ),
        }
        result
    }
}

/// 从环境变量解析工作区根目录。
pub fn workspace_root_from_env() -> PathBuf {
    std::env::var("CUBECODE_WORKSPACE_ROOT")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn registry_executes_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "A").unwrap();
        let registry = ToolRegistry::new(root);
        let out = registry.execute("read_file", "a.txt").expect("ok");
        assert_eq!(out, "A");
    }

    #[test]
    fn registry_rejects_unknown_tool() {
        let registry = ToolRegistry::new(".");
        let err = registry.execute("write_file", "{}").expect_err("unknown");
        assert!(matches!(err, ToolError::UnknownTool(_)));
    }
}
