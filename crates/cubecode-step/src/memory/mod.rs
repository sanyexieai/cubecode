//! 记忆 / RAG 检索接口（M5-1+）。
//!
//! 存储模式见 [`MemoryStorageMode`]：`memory` / `markdown` / `sqlite`。

mod config;
mod error;
mod in_memory;
mod markdown;
mod pipeline;
mod ranking;
mod retriever;
mod sqlite;
mod store;
mod types;

pub use config::{
    memory_enabled_from_env, memory_storage_mode_from_env, memory_storage_path_from_env,
    memory_top_k_from_env, MemoryConfig, MemoryStorageMode, ENV_MEMORY_ENABLED, ENV_MEMORY_PATH,
    ENV_MEMORY_STORAGE, ENV_MEMORY_TOP_K, META_MEMORY_TOP_K,
};
pub use error::MemoryError;
pub use in_memory::InMemoryRetriever;
pub use pipeline::{
    apply_memory_recall, attach_memory_pipeline, memory_pipeline, stamp_request_metadata,
    MemoryRecallStage, META_MEMORY_HIT_COUNT, META_MEMORY_INJECTED_BYTES, META_SESSION_ID,
    META_TURN_ID, META_USER_TEXT,
};
pub use retriever::{MemoryChunk, MemoryRetriever, NoopRetriever};
pub use store::{
    attach_memory_store, default_storage_root, memory_store_from_config, MemoryStore,
};
pub use types::{
    MemoryHit, MemoryQuery, MemoryRetrieveResult, DEFAULT_TOP_K,
};
