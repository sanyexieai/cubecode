//! [`llm_kit::PipelineStage`]：在 `before_generate` 注入记忆召回（M5-2）。

use std::sync::Arc;

use cubecode_contracts::SessionId;
use llm_kit::{
    ChatMessage, GenerateRequest, LlmError, MessageRole, Pipeline, PipelineBuilder, PipelineContext,
    PipelineStage, ProviderRegistry,
};

use super::config::META_MEMORY_TOP_K;
use super::error::MemoryError;
use super::store::MemoryStore;
use super::types::{MemoryQuery, MemoryRetrieveResult};

/// 写入 [`GenerateRequest::metadata`]，供本阶段与 M5-3 编排层使用。
pub const META_SESSION_ID: &str = "cubecode.session_id";
pub const META_TURN_ID: &str = "cubecode.turn_id";
pub const META_USER_TEXT: &str = "cubecode.user_text";
/// 召回条数（日志摘要，不存全文）。
pub const META_MEMORY_HIT_COUNT: &str = "cubecode.memory.hit_count";
pub const META_MEMORY_INJECTED_BYTES: &str = "cubecode.memory.injected_bytes";

const MEMORY_BLOCK_HEADER: &str = "以下是与会话相关的记忆片段，供参考：\n\n";

/// 将编排层 / ⑤ 上下文写入请求 metadata（须在 `generate` 前调用）。
pub fn stamp_request_metadata(
    request: &mut GenerateRequest,
    session_id: &SessionId,
    turn_id: cubecode_contracts::TurnId,
    user_text: Option<&str>,
) {
    request
        .metadata
        .insert(META_SESSION_ID.into(), session_id.as_str().into());
    request
        .metadata
        .insert(META_TURN_ID.into(), turn_id.to_string());
    if let Some(text) = user_text.filter(|s| !s.is_empty()) {
        request
            .metadata
            .insert(META_USER_TEXT.into(), text.to_owned());
    }
}

/// 根据 metadata 与消息列表执行召回，并把结果注入 `request.messages`。
pub fn apply_memory_recall(
    request: &mut GenerateRequest,
    store: &dyn MemoryStore,
) -> Result<(), MemoryError> {
    let Some(session_key) = request.metadata.get(META_SESSION_ID).cloned() else {
        tracing::debug!(
            target: "cubecode.step.memory",
            "pipeline：跳过记忆（无 session_id metadata）"
        );
        return Ok(());
    };
    let session = SessionId::new(&session_key);
    let user_text = request
        .metadata
        .get(META_USER_TEXT)
        .map(String::as_str)
        .or_else(|| last_user_message_text(&request.messages))
        .unwrap_or("");
    if user_text.is_empty() {
        tracing::debug!(
            target: "cubecode.step.memory",
            session_id = %session_key,
            "pipeline：跳过记忆（无用户文本）"
        );
        return Ok(());
    }

    let user_text_bytes = user_text.len();
    let top_k = top_k_from_metadata(&request.metadata);
    let query = MemoryQuery::new(&session, user_text).with_top_k(top_k);
    let MemoryRetrieveResult { hits } = store.retrieve(&query)?;

    request.metadata.insert(
        META_MEMORY_HIT_COUNT.into(),
        hits.len().to_string(),
    );

    if hits.is_empty() {
        tracing::info!(
            target: "cubecode.step.memory",
            session_id = %session_key,
            storage = store.id(),
            top_k,
            user_text_bytes,
            "pipeline：记忆召回完成（无命中）"
        );
        request
            .metadata
            .insert(META_MEMORY_INJECTED_BYTES.into(), "0".into());
        return Ok(());
    }

    let block = format_memory_block(&hits);
    let injected_bytes = block.len();
    inject_memory_block(&mut request.messages, &block);
    request.metadata.insert(
        META_MEMORY_INJECTED_BYTES.into(),
        injected_bytes.to_string(),
    );

    let hit_summary = summarize_hits(&hits);
    tracing::info!(
        target: "cubecode.step.memory",
        session_id = %session_key,
        storage = store.id(),
        top_k,
        user_text_bytes,
        hits = hits.len(),
        injected_bytes,
        hit_summary = %hit_summary,
        "pipeline：记忆召回已注入"
    );
    Ok(())
}

fn summarize_hits(hits: &[super::types::MemoryHit]) -> String {
    hits.iter()
        .map(|h| {
            let src = h.source.as_deref().unwrap_or("-");
            format!("{}:{}B@{}", h.id, h.content.len(), src)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn top_k_from_metadata(metadata: &std::collections::BTreeMap<String, String>) -> usize {
    metadata
        .get(META_MEMORY_TOP_K)
        .and_then(|s| s.parse().ok())
        .filter(|&k| k > 0)
        .unwrap_or(super::types::DEFAULT_TOP_K)
}

fn last_user_message_text(messages: &[ChatMessage]) -> Option<&str> {
    messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::User))
        .map(|m| m.content.as_str())
}

fn format_memory_block(hits: &[super::types::MemoryHit]) -> String {
    let mut block = MEMORY_BLOCK_HEADER.to_owned();
    for (i, hit) in hits.iter().enumerate() {
        let label = hit
            .source
            .as_deref()
            .map(|s| format!("[记忆{}·{}]", i + 1, s))
            .unwrap_or_else(|| format!("[记忆{}]", i + 1));
        block.push_str(&label);
        block.push('\n');
        block.push_str(hit.content.trim());
        block.push_str("\n\n");
    }
    block
}

fn inject_memory_block(messages: &mut Vec<ChatMessage>, block: &str) {
    if let Some(pos) = messages
        .iter()
        .position(|m| matches!(m.role, MessageRole::System))
    {
        if messages[pos].content.is_empty() {
            messages[pos].content = block.to_owned();
        } else {
            messages[pos].content = format!("{}\n\n{}", messages[pos].content, block);
        }
    } else {
        messages.insert(0, ChatMessage::new(MessageRole::System, block));
    }
}

/// 记忆召回 pipeline 阶段。
pub struct MemoryRecallStage {
    store: Arc<dyn MemoryStore>,
}

impl MemoryRecallStage {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

impl PipelineStage for MemoryRecallStage {
    fn id(&self) -> &'static str {
        "memory_recall"
    }

    fn before_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        apply_memory_recall(&mut ctx.request, self.store.as_ref())
            .map_err(|e| LlmError::InvalidRequest(e.to_string()))
    }
}

/// 仅含记忆召回的 pipeline。
pub fn memory_pipeline(store: Arc<dyn MemoryStore>) -> Pipeline {
    PipelineBuilder::default()
        .push(MemoryRecallStage::new(store))
        .build()
}

/// 在 registry 上挂载记忆召回（会**替换**已有 pipeline）。
pub fn attach_memory_pipeline(registry: &mut ProviderRegistry, store: Arc<dyn MemoryStore>) {
    let storage_id = store.id();
    registry.set_pipeline(Some(memory_pipeline(store)));
    tracing::info!(
        target: "cubecode.step.memory",
        storage = storage_id,
        "已挂载记忆召回 pipeline"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InMemoryRetriever, MemoryChunk, MemoryStore, NoopRetriever};
    use cubecode_contracts::TurnId;
    use llm_kit::ModelRef;

    #[test]
    fn apply_memory_injects_system_message() {
        let session = SessionId::new("s-inject");
        let store = InMemoryRetriever::new();
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "m1".into(),
                    content: "本项目使用 Rust 开发".into(),
                    source: Some("doc".into()),
                },
            )
            .expect("remember");
        let mut request = GenerateRequest::new(
            ModelRef::new("p", "m"),
            vec![ChatMessage::new(MessageRole::User, "Rust 开发")],
        );
        stamp_request_metadata(&mut request, &session, TurnId::FIRST, Some("Rust 开发"));
        apply_memory_recall(&mut request, &store).expect("recall");
        let system = request
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::System))
            .expect("system");
        assert!(system.content.contains("Rust"));
        assert_eq!(request.metadata.get(META_MEMORY_HIT_COUNT).map(String::as_str), Some("1"));
    }

    #[test]
    fn pipeline_stage_delegates_to_apply() {
        let session = SessionId::new("s-stage");
        let store: Arc<dyn MemoryStore> = Arc::new(InMemoryRetriever::new());
        store
            .remember(
                &session,
                MemoryChunk {
                    id: "a".into(),
                    content: "关键事实".into(),
                    source: None,
                },
            )
            .expect("remember");
        let stage = MemoryRecallStage::new(store);
        let mut ctx = PipelineContext::new(GenerateRequest::new(
            ModelRef::new("p", "m"),
            vec![ChatMessage::new(MessageRole::User, "关键")],
        ));
        stamp_request_metadata(&mut ctx.request, &session, TurnId::FIRST, Some("关键"));
        stage.before_generate(&mut ctx).expect("stage");
        assert!(ctx
            .request
            .messages
            .iter()
            .any(|m| m.content.contains("关键事实")));
    }

    #[test]
    fn top_k_from_metadata_overrides_default() {
        let session = SessionId::new("s-topk");
        let store = InMemoryRetriever::new();
        for i in 0..4 {
            store
                .remember(
                    &session,
                    MemoryChunk {
                        id: format!("m{i}"),
                        content: format!("关键词条目 {i}"),
                        source: None,
                    },
                )
                .expect("remember");
        }
        let mut request = GenerateRequest::new(
            ModelRef::new("p", "m"),
            vec![ChatMessage::new(MessageRole::User, "关键词")],
        );
        stamp_request_metadata(&mut request, &session, TurnId::FIRST, Some("关键词"));
        request
            .metadata
            .insert(META_MEMORY_TOP_K.into(), "2".into());
        apply_memory_recall(&mut request, &store).expect("recall");
        assert_eq!(request.metadata.get(META_MEMORY_HIT_COUNT).map(String::as_str), Some("2"));
    }

    #[test]
    fn noop_retriever_leaves_messages_unchanged() {
        let session = SessionId::new("s-noop");
        let mut request = GenerateRequest::new(
            ModelRef::new("p", "m"),
            vec![ChatMessage::new(MessageRole::User, "hi")],
        );
        stamp_request_metadata(&mut request, &session, TurnId::FIRST, Some("hi"));
        let len_before = request.messages.len();
        apply_memory_recall(&mut request, &NoopRetriever).expect("noop");
        assert_eq!(request.messages.len(), len_before);
    }
}
