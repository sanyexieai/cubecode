//! Markdown 文件记忆存储（每会话一个 `.md`）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use cubecode_contracts::SessionId;

use super::error::MemoryError;
use super::ranking::rank_chunks;
use super::retriever::MemoryChunk;
use super::store::MemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

const CHUNK_HEADER: &str = "## chunk: ";

/// `{root}/{session}.md` 追加片段。
#[derive(Debug)]
pub struct MarkdownMemoryStore {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl MarkdownMemoryStore {
    pub fn new(root: PathBuf) -> Result<Self, MemoryError> {
        fs::create_dir_all(&root).map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(Self {
            root,
            write_lock: Mutex::new(()),
        })
    }

    fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.root.join(format!("{}.md", sanitize_filename(session_id.as_str())))
    }

    fn read_chunks(&self, session_id: &SessionId) -> Result<Vec<MemoryChunk>, MemoryError> {
        let path = self.session_path(session_id);
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path).map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(parse_markdown_chunks(&text))
    }
}

impl MemoryStore for MarkdownMemoryStore {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn retrieve(&self, query: &MemoryQuery<'_>) -> Result<MemoryRetrieveResult, MemoryError> {
        if query.top_k == 0 {
            return Err(MemoryError::InvalidQuery("top_k 不能为 0".into()));
        }
        let chunks = self.read_chunks(query.session_id)?;
        let hits = rank_chunks(query.user_text, &chunks, query.top_k);
        tracing::info!(
            target: "cubecode.step.memory",
            session_id = %query.session_id,
            top_k = query.top_k,
            hits = hits.len(),
            path = %self.session_path(query.session_id).display(),
            "记忆检索完成（Markdown）"
        );
        Ok(MemoryRetrieveResult { hits })
    }

    fn remember(&self, session_id: &SessionId, chunk: MemoryChunk) -> Result<(), MemoryError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let path = self.session_path(session_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        let source = chunk.source.as_deref().unwrap_or("-");
        writeln!(file, "{CHUNK_HEADER}{}", chunk.id)
            .and_then(|_| writeln!(file, "source: {source}"))
            .and_then(|_| writeln!(file))
            .and_then(|_| writeln!(file, "{}", chunk.content.trim()))
            .and_then(|_| writeln!(file))
            .map_err(|e| MemoryError::Backend(e.to_string()))?;
        Ok(())
    }
}

fn sanitize_filename(session_key: &str) -> String {
    session_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_markdown_chunks(text: &str) -> Vec<MemoryChunk> {
    let mut chunks = Vec::new();
    let normalized = if text.starts_with(CHUNK_HEADER) {
        text.to_owned()
    } else {
        format!("{CHUNK_HEADER}{text}")
    };
    for section in normalized.split(CHUNK_HEADER).skip(1) {
        let Some(chunk) = parse_one_chunk(section) else {
            continue;
        };
        chunks.push(chunk);
    }
    chunks
}

fn parse_one_chunk(section: &str) -> Option<MemoryChunk> {
    let mut lines = section.lines();
    let id = lines.next()?.trim().to_owned();
    if id.is_empty() {
        return None;
    }
    let mut source = None;
    let mut body_lines = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("source:") {
            source = Some(rest.trim().to_owned());
            continue;
        }
        body_lines.push(line);
    }
    let content = body_lines.join("\n").trim().to_owned();
    if content.is_empty() {
        return None;
    }
    Some(MemoryChunk {
        id,
        content,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::SessionId;

    #[test]
    fn markdown_remember_and_retrieve() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = MarkdownMemoryStore::new(dir.path().to_path_buf()).expect("store");
        let session = SessionId::new("s-md");
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "u-1".into(),
                    content: "Rust 异步编程".into(),
                    source: Some("user".into()),
                },
            )
            .expect("write");
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "a-1".into(),
                    content: "今天天气很好".into(),
                    source: Some("assistant".into()),
                },
            )
            .expect("write");
        let q = MemoryQuery::new(&session, "Rust 编程").with_top_k(2);
        let r = store.retrieve(&q).expect("retrieve");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].id, "u-1");
    }
}
