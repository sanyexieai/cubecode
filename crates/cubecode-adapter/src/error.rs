//! ① 适配层错误类型。

/// 从输入源拉取事件时的失败（如 stdin / socket 读失败）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterError {
    Io(String),
    /// JSON / JSON-RPC 请求体无效（HTTP 占位适配器）。
    InvalidRequest(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "适配层 I/O 错误：{msg}"),
            Self::InvalidRequest(msg) => write!(f, "适配层请求无效：{msg}"),
        }
    }
}

impl std::error::Error for AdapterError {}

/// [`drain_adapter`] 可能因 poll 或入队失败而返回。
#[derive(Debug)]
pub enum DrainError {
    Adapter(AdapterError),
    Inbox(cubecode_inbox::InboxFull),
}

impl std::fmt::Display for DrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(e) => write!(f, "{e}"),
            Self::Inbox(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DrainError {}

impl From<AdapterError> for DrainError {
    fn from(value: AdapterError) -> Self {
        Self::Adapter(value)
    }
}

impl From<cubecode_inbox::InboxFull> for DrainError {
    fn from(value: cubecode_inbox::InboxFull) -> Self {
        Self::Inbox(value)
    }
}
