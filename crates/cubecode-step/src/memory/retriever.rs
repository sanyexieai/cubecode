//! [`MemoryRetriever`]（检索侧别名）与 [`MemoryChunk`]。

use cubecode_contracts::SessionId;

use super::error::MemoryError;
use super::store::MemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

/// 与 [`MemoryStore`] 等价；保留名称供 pipeline / 导出兼容。
pub trait MemoryRetriever: MemoryStore {}

impl<T: MemoryStore + ?Sized> MemoryRetriever for T {}

/// 写入记忆库的片段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChunk {
    pub id: String,
    pub content: String,
    pub source: Option<String>,
}

/// 不召回、不持久化（未启用记忆时的占位）。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRetriever;

impl MemoryStore for NoopRetriever {
    fn id(&self) -> &'static str {
        "noop"
    }

    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError> {
        if query.top_k == 0 {
            return Err(MemoryError::InvalidQuery("top_k 不能为 0".into()));
        }
        tracing::debug!(
            target: "cubecode.step.memory",
            session_id = %query.session_id,
            top_k = query.top_k,
            "记忆检索（空实现）"
        );
        Ok(MemoryRetrieveResult::empty())
    }

    fn remember(&self, _session_id: &SessionId, _chunk: MemoryChunk) -> Result<(), MemoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::SessionId;

    #[test]
    fn noop_returns_empty() {
        let session = SessionId::new("s-noop");
        let q = MemoryQuery::new(&session, "hello");
        let r = NoopRetriever.retrieve(&q).expect("retrieve");
        assert!(r.is_empty());
    }
}
