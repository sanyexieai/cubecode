//! 记忆检索错误。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    /// 参数无效（如 `top_k == 0`）。
    InvalidQuery(String),
    /// 后端内部失败。
    Backend(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidQuery(msg) => write!(f, "记忆检索参数无效：{msg}"),
            Self::Backend(msg) => write!(f, "记忆检索失败：{msg}"),
        }
    }
}

impl std::error::Error for MemoryError {}
