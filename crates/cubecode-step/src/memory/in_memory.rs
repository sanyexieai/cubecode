//! 进程内记忆存储。

use std::collections::BTreeMap;
use std::sync::Mutex;

use cubecode_contracts::SessionId;

use super::error::MemoryError;
use super::ranking::rank_chunks;
use super::retriever::MemoryChunk;
use super::store::MemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

/// 进程内、按会话分桶（默认 `memory` 模式）。
#[derive(Debug, Default)]
pub struct InMemoryRetriever {
    by_session: Mutex<BTreeMap<String, Vec<MemoryChunk>>>,
}

impl InMemoryRetriever {
    pub fn new() -> Self {
        Self::default()
    }

    fn load_chunks(&self, session_id: &SessionId) -> Result<Vec<MemoryChunk>, MemoryError> {
        let guard = self
            .by_session
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(guard
            .get(session_id.as_str())
            .cloned()
            .unwrap_or_default())
    }
}

impl MemoryStore for InMemoryRetriever {
    fn id(&self) -> &'static str {
        "in_memory"
    }

    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError> {
        if query.top_k == 0 {
            return Err(MemoryError::InvalidQuery("top_k 不能为 0".into()));
        }
        let chunks = self.load_chunks(query.session_id)?;
        let hits = rank_chunks(query.user_text, &chunks, query.top_k);
        tracing::info!(
            target: "cubecode.step.memory",
            session_id = %query.session_id,
            top_k = query.top_k,
            hits = hits.len(),
            "记忆检索完成（内存）"
        );
        Ok(MemoryRetrieveResult { hits })
    }

    fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) -> Result<(), MemoryError> {
        let key = session_id.as_str().to_owned();
        let mut guard = self
            .by_session
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        guard.entry(key).or_default().push(chunk);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::SessionId;

    #[test]
    fn in_memory_ranks_by_overlap() {
        let session = SessionId::new("s-mem");
        let store = InMemoryRetriever::new();
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "a".into(),
                    content: "Rust 异步编程".into(),
                    source: None,
                },
            )
            .expect("write");
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "b".into(),
                    content: "今天天气很好".into(),
                    source: None,
                },
            )
            .expect("write");
        let q = MemoryQuery::new(&session, "Rust 编程").with_top_k(2);
        let r = store.retrieve(&q).expect("retrieve");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].id, "a");
    }

    #[test]
    fn reject_zero_top_k() {
        let session = SessionId::new("s-zero");
        let q = MemoryQuery::new(&session, "x").with_top_k(0);
        let store = InMemoryRetriever::new();
        assert!(matches!(
            store.retrieve(&q),
            Err(MemoryError::InvalidQuery(_))
        ));
    }
}
