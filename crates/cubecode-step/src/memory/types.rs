//! 检索请求与命中结果类型。

use cubecode_contracts::SessionId;

/// 默认召回条数（M5-4 可由配置覆盖）。
pub const DEFAULT_TOP_K: usize = 5;

/// 一次检索请求（按会话 + 当前用户文本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryQuery<'a> {
    pub session_id: &'a SessionId,
    pub user_text: &'a str,
    pub top_k: usize,
}

impl<'a> MemoryQuery<'a> {
    pub fn new(session_id: &'a SessionId, user_text: &'a str) -> Self {
        Self {
            session_id,
            user_text,
            top_k: DEFAULT_TOP_K,
        }
    }

    pub fn with_top_k(mut self, top_k: usize) -> Self {
        self.top_k = top_k;
        self
    }
}

/// 单条召回片段。
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryHit {
    pub id: String,
    pub content: String,
    /// 相关性分数（越高越相关）；占位实现可为 `None`。
    pub score: Option<f32>,
    /// 来源说明（文件路径、轮次等），日志摘要用。
    pub source: Option<String>,
}

/// 检索结果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MemoryRetrieveResult {
    pub hits: Vec<MemoryHit>,
}

impl MemoryRetrieveResult {
    pub fn empty() -> Self {
        Self { hits: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}
