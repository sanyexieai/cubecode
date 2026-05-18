//! 工具执行错误。

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    UnknownTool(String),
    InvalidArguments(String),
    PathNotAllowed { path: PathBuf, workspace: PathBuf },
    NotAFile(PathBuf),
    Io(String),
    FileTooLarge { path: PathBuf, max_bytes: u64 },
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTool(name) => write!(f, "未知工具：{name}"),
            Self::InvalidArguments(msg) => write!(f, "工具参数无效：{msg}"),
            Self::PathNotAllowed { path, workspace } => {
                write!(
                    f,
                    "路径不在白名单工作区内：{}（工作区 {}）",
                    path.display(),
                    workspace.display()
                )
            }
            Self::NotAFile(path) => write!(f, "不是普通文件：{}", path.display()),
            Self::Io(msg) => write!(f, "读取失败：{msg}"),
            Self::FileTooLarge { path, max_bytes } => write!(
                f,
                "文件过大：{}（上限 {} 字节）",
                path.display(),
                max_bytes
            ),
        }
    }
}

impl std::error::Error for ToolError {}
