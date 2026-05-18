//! [`MemoryRetriever`] trait 与内置实现。

use std::collections::BTreeMap;
use std::sync::Mutex;

use cubecode_contracts::SessionId;

use super::error::MemoryError;
use super::types::{MemoryHit, MemoryQuery, MemoryRetrieveResult};

/// 记忆检索接口（M5-1）；具体索引/向量库在后续里程碑实现。
pub trait MemoryRetriever: Send + Sync {
    /// 稳定标识，用于日志与配置。
    fn id(&self) -> &'static str;

    /// 按会话与当前用户输入召回相关片段。
    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError>;
}

/// 不召回任何内容（默认关闭记忆时的占位）。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopRetriever;

impl MemoryRetriever for NoopRetriever {
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
}

/// 写入记忆库的片段（供 [`InMemoryRetriever`] 测试与 POC）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChunk {
    pub id: String,
    pub content: String,
    pub source: Option<String>,
}

/// 进程内、按会话分桶的简易检索（关键词重叠打分，非向量）。
#[derive(Debug, Default)]
pub struct InMemoryRetriever {
    by_session: Mutex<BTreeMap<String, Vec<MemoryChunk>>>,
}

impl InMemoryRetriever {
    pub fn new() -> Self {
        Self::default()
    }

    /// 向会话追加一条可检索片段。
    pub fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) {
        let key = session_id.as_str().to_owned();
        let mut guard = self.by_session.lock().expect("memory lock");
        guard.entry(key).or_default().push(chunk);
    }

    fn score_chunk(query: &str, content: &str) -> f32 {
        let q = query.to_lowercase();
        let c = content.to_lowercase();
        if q.is_empty() {
            return 0.0;
        }
        if c.contains(&q) {
            return 1.0;
        }
        let words: Vec<&str> = q
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .collect();
        if words.is_empty() {
            return 0.0;
        }
        let matched = words.iter().filter(|w| c.contains(*w)).count();
        matched as f32 / words.len() as f32
    }
}

impl MemoryRetriever for InMemoryRetriever {
    fn id(&self) -> &'static str {
        "in_memory"
    }

    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError> {
        if query.top_k == 0 {
            return Err(MemoryError::InvalidQuery("top_k 不能为 0".into()));
        }
        let guard = self
            .by_session
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let Some(chunks) = guard.get(query.session_id.as_str()) else {
            tracing::info!(
                target: "cubecode.step.memory",
                session_id = %query.session_id,
                top_k = query.top_k,
                hits = 0,
                "记忆检索完成（无会话数据）"
            );
            return Ok(MemoryRetrieveResult::empty());
        };

        let mut scored: Vec<(f32, &MemoryChunk)> = chunks
            .iter()
            .map(|c| (Self::score_chunk(query.user_text, &c.content), c))
            .filter(|(s, _)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let hits: Vec<MemoryHit> = scored
            .into_iter()
            .take(query.top_k)
            .map(|(score, c)| MemoryHit {
                id: c.id.clone(),
                content: c.content.clone(),
                score: Some(score),
                source: c.source.clone(),
            })
            .collect();

        let hit_summary: String = hits
            .iter()
            .map(|h| format!("{}:{}B", h.id, h.content.len()))
            .collect::<Vec<_>>()
            .join(",");
        tracing::info!(
            target: "cubecode.step.memory",
            session_id = %query.session_id,
            top_k = query.top_k,
            user_text_bytes = query.user_text.len(),
            hits = hits.len(),
            hit_summary = %hit_summary,
            "记忆检索完成"
        );
        Ok(MemoryRetrieveResult { hits })
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

    #[test]
    fn in_memory_ranks_by_overlap() {
        let session = SessionId::new("s-mem");
        let store = InMemoryRetriever::new();
        store.remember(
            &session,
            MemoryChunk {
                id: "a".into(),
                content: "Rust 异步编程".into(),
                source: None,
            },
        );
        store.remember(
            &session,
            MemoryChunk {
                id: "b".into(),
                content: "今天天气很好".into(),
                source: None,
            },
        );
        let q = MemoryQuery::new(&session, "Rust 编程").with_top_k(2);
        let r = store.retrieve(&q).expect("retrieve");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].id, "a");
        assert!(r.hits[0].score.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn reject_zero_top_k() {
        let session = SessionId::new("s-zero");
        let q = MemoryQuery::new(&session, "x").with_top_k(0);
        assert!(matches!(
            NoopRetriever.retrieve(&q),
            Err(MemoryError::InvalidQuery(_))
        ));
    }
}
