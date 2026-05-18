//! 记忆存储 trait 与按配置构造实现。

use std::path::PathBuf;
use std::sync::Arc;

use cubecode_contracts::SessionId;

use super::config::{MemoryConfig, MemoryStorageMode};
use super::error::MemoryError;
use super::in_memory::InMemoryRetriever;
use super::markdown::MarkdownMemoryStore;
use super::retriever::MemoryChunk;
use super::sqlite::SqliteMemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

/// 写入 + 检索（各存储模式统一接口）。
pub trait MemoryStore: Send + Sync {
    fn id(&self) -> &'static str;
    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError>;
    fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) -> Result<(), MemoryError>;
}

impl MemoryStore for Arc<dyn MemoryStore> {
    fn id(&self) -> &'static str {
        self.as_ref().id()
    }

    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError> {
        self.as_ref().retrieve(query)
    }

    fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) -> Result<(), MemoryError> {
        self.as_ref().remember(session_id, chunk)
    }
}

/// 按 [`MemoryConfig`] 构造存储；未启用时返回 `None`。
pub fn memory_store_from_config(
    cfg: &MemoryConfig,
) -> Result<Option<Arc<dyn MemoryStore>>, MemoryError> {
    if !cfg.enabled {
        return Ok(None);
    }
    let store: Arc<dyn MemoryStore> = match cfg.storage {
        MemoryStorageMode::Memory => Arc::new(InMemoryRetriever::new()),
        MemoryStorageMode::Markdown => Arc::new(MarkdownMemoryStore::new(cfg.storage_path.clone())?),
        MemoryStorageMode::Sqlite => Arc::new(SqliteMemoryStore::new(cfg.storage_path.clone())?),
    };
    tracing::info!(
        target: "cubecode.step.memory",
        storage = store.id(),
        path = %cfg.storage_path.display(),
        top_k = cfg.top_k,
        "记忆存储已初始化"
    );
    Ok(Some(store))
}

pub fn default_storage_root() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".cubecode")
        .join("memory")
}

/// 挂载记忆 pipeline（会**替换**已有 pipeline）。
pub fn attach_memory_store(registry: &mut llm_kit::ProviderRegistry, store: Arc<dyn MemoryStore>) {
    super::pipeline::attach_memory_pipeline(registry, store);
}
