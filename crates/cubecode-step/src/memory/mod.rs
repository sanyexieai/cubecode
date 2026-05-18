//! 记忆 / RAG 检索接口（M5-1）。
//!
//! 编排层或 `llm-kit` pipeline 在进 ⑤ 前调用 [`MemoryRetriever`]；
//! 具体向量库、持久化在后续里程碑扩展。

mod config;
mod error;
mod pipeline;
mod retriever;
mod types;

pub use config::{
    memory_enabled_from_env, memory_top_k_from_env, MemoryConfig, ENV_MEMORY_ENABLED,
    ENV_MEMORY_TOP_K, META_MEMORY_TOP_K,
};
pub use error::MemoryError;
pub use pipeline::{
    apply_memory_recall, attach_memory_pipeline, memory_pipeline, stamp_request_metadata,
    MemoryRecallStage, META_MEMORY_HIT_COUNT, META_MEMORY_INJECTED_BYTES, META_SESSION_ID,
    META_TURN_ID, META_USER_TEXT,
};
pub use retriever::{InMemoryRetriever, MemoryChunk, MemoryRetriever, NoopRetriever};
pub use types::{
    MemoryHit, MemoryQuery, MemoryRetrieveResult, DEFAULT_TOP_K,
};
