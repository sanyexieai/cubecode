//! SQLite 记忆存储（单库多会话）。

use std::path::PathBuf;
use std::sync::Mutex;

use cubecode_contracts::SessionId;
use rusqlite::{params, Connection};

use super::error::MemoryError;
use super::ranking::rank_chunks;
use super::retriever::MemoryChunk;
use super::store::MemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

/// `{path}/memory.db`（`path` 为 [`MemoryConfig::storage_path`] 目录）。
#[derive(Debug)]
pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,
}

impl SqliteMemoryStore {
    pub fn new(root: PathBuf) -> Result<Self, MemoryError> {
        std::fs::create_dir_all(&root).map_err(|e| MemoryError::Backend(e.to_string()))?;
        let db_path = root.join("memory.db");
        let conn = Connection::open(&db_path).map_err(|e| MemoryError::Backend(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_chunks (
                session_id TEXT NOT NULL,
                chunk_id   TEXT NOT NULL,
                content    TEXT NOT NULL,
                source     TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (session_id, chunk_id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_session ON memory_chunks(session_id);",
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn load_chunks(&self, session_id: &SessionId) -> Result<Vec<MemoryChunk>, MemoryError> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let mut stmt = guard
            .prepare(
                "SELECT chunk_id, content, source FROM memory_chunks
                 WHERE session_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let rows = stmt
            .query_map(params![session_id.as_str()], |row| {
                Ok(MemoryChunk {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    source: row.get(2)?,
                })
            })
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row.map_err(|e| MemoryError::Backend(e.to_string()))?);
        }
        Ok(chunks)
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn id(&self) -> &'static str {
        "sqlite"
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
            "记忆检索完成（SQLite）"
        );
        Ok(MemoryRetrieveResult { hits })
    }

    fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) -> Result<(), MemoryError> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        guard
            .execute(
                "INSERT INTO memory_chunks (session_id, chunk_id, content, source)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(session_id, chunk_id) DO UPDATE SET
                   content = excluded.content,
                   source = excluded.source,
                   created_at = datetime('now')",
                params![
                    session_id.as_str(),
                    chunk.id,
                    chunk.content,
                    chunk.source,
                ],
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::SessionId;

    #[test]
    fn sqlite_remember_and_retrieve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteMemoryStore::new(dir.path().to_path_buf()).expect("store");
        let session = SessionId::new("s-sql");
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "u-1".into(),
                    content: "向量数据库".into(),
                    source: Some("user".into()),
                },
            )
            .expect("write");
        let q = MemoryQuery::new(&session, "向量").with_top_k(3);
        let r = store.retrieve(&q).expect("retrieve");
        assert_eq!(r.hits.len(), 1);
    }
}
