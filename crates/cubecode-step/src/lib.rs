//! ⑤ **执行层**：占位或真实 LLM 执行。

pub mod memory;
pub mod tools;
pub mod transcript;

use std::collections::BTreeMap;

use cubecode_contracts::{ControlEvent, TurnContext};

pub use llm_kit::{ChatMessage, MessageRole, ModelRef, ProviderRegistry};
pub use memory::{
    apply_memory_recall, attach_memory_pipeline, attach_memory_store, default_storage_root,
    memory_enabled_from_env, memory_pipeline, memory_storage_mode_from_env,
    memory_storage_path_from_env,
    memory_store_from_config, memory_top_k_from_env, stamp_request_metadata, InMemoryRetriever,
    MemoryChunk, MemoryConfig, MemoryError, MemoryHit, MemoryQuery, MemoryRecallStage,
    MemoryRetrieveResult, MemoryRetriever, MemoryStorageMode, MemoryStore, NoopRetriever,
    DEFAULT_TOP_K, ENV_MEMORY_ENABLED, ENV_MEMORY_PATH, ENV_MEMORY_STORAGE,
    ENV_MEMORY_TOP_K, META_MEMORY_HIT_COUNT, META_MEMORY_INJECTED_BYTES, META_MEMORY_TOP_K,
    META_SESSION_ID, META_TURN_ID, META_USER_TEXT,
};
pub use tools::{ToolError, ToolRegistry};
pub use transcript::{
    append_assistant_reply, append_assistant_tool_request, append_tool_exchange,
    append_tool_result,
};
use llm_kit::{GenerateRequest, LlmError, StreamChunk};

/// ⑤ 调用 LLM 时由编排层注入的上下文。
pub struct LlmStepContext<'a> {
    pub registry: &'a ProviderRegistry,
    pub model: ModelRef,
    pub messages: &'a [ChatMessage],
    /// ④ 写入的 session 范围 metadata（M5-3）；合并进 [`GenerateRequest`] 后供 pipeline 消费。
    pub request_metadata: Option<&'a std::collections::BTreeMap<String, String>>,
}

impl<'a> LlmStepContext<'a> {
    pub fn new(
        registry: &'a ProviderRegistry,
        model: ModelRef,
        messages: &'a [ChatMessage],
    ) -> Self {
        Self {
            registry,
            model,
            messages,
            request_metadata: None,
        }
    }
}

/// 占位：不访问网络，只生成可读的摘要字符串。
pub fn placeholder_turn(turn_ctx: &TurnContext, event: &ControlEvent) -> Result<String, String> {
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        ?event,
        "⑤执行层：进入（占位）"
    );
    let out = match event {
        ControlEvent::UserTurn { text, .. } => {
            format!("（执行层占位）用户消息 {} 字节", text.len())
        }
        ControlEvent::Shutdown { .. } => {
            tracing::warn!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "⑤执行层：拒绝关闭事件"
            );
            return Err("shutdown".into());
        }
        ControlEvent::ToolResult { output, call_id, .. } => {
            format!("（执行层占位）工具结果 call_id={call_id} 字节={}", output.len())
        }
    };
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        bytes = out.len(),
        "⑤执行层：离开（占位完成）"
    );
    Ok(out)
}

/// 真实 LLM：用当前消息列表调用 registry。
pub fn llm_turn(
    ctx: &LlmStepContext<'_>,
    turn_ctx: &TurnContext,
    event: &ControlEvent,
) -> Result<String, String> {
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        provider = %ctx.model.provider,
        model = %ctx.model.model,
        messages = ctx.messages.len(),
        "⑤执行层：进入（调用大模型）"
    );
    match event {
        ControlEvent::UserTurn { .. } => {
            tracing::info!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "⑤执行层：用户轮次 → 大模型"
            );
        }
        ControlEvent::ToolResult { call_id, .. } => {
            tracing::info!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                %call_id,
                messages = ctx.messages.len(),
                "⑤执行层：工具回灌 → 大模型"
            );
        }
        ControlEvent::Shutdown { .. } => {
            tracing::warn!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "⑤执行层：拒绝关闭事件"
            );
            return Err("shutdown".into());
        }
    }
    let request = build_generate_request(ctx, turn_ctx, event);
    if let Some(hits) = request.metadata.get(META_MEMORY_HIT_COUNT) {
        let injected = request
            .metadata
            .get(META_MEMORY_INJECTED_BYTES)
            .map(String::as_str)
            .unwrap_or("0");
        tracing::info!(
            target: "cubecode.step",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            memory_hits = %hits,
            memory_injected_bytes = %injected,
            "⑤执行层：记忆 pipeline 已执行（before_generate）"
        );
    }
    let response = ctx
        .registry
        .generate(&request)
        .map_err(|e| e.to_string())?;
    let content = response.message.content;
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        out_bytes = content.len(),
        "⑤执行层：离开（大模型返回成功）"
    );
    Ok(content)
}

fn user_text_for_event(event: &ControlEvent, messages: &[ChatMessage]) -> Option<String> {
    match event {
        ControlEvent::UserTurn { text, .. } => Some(text.clone()),
        _ => messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, MessageRole::User))
            .map(|m| m.content.clone()),
    }
}

/// 将 metadata 合并进请求（④ 优先；无 map 时由 ⑤ 按 turn 补戳，兼容测试）。
pub fn merge_request_metadata(
    request: &mut GenerateRequest,
    orchestrator_meta: Option<&BTreeMap<String, String>>,
    turn_ctx: &TurnContext,
    event: &ControlEvent,
    messages: &[ChatMessage],
) {
    if let Some(meta) = orchestrator_meta {
        for (k, v) in meta {
            request.metadata.insert(k.clone(), v.clone());
        }
        return;
    }
    stamp_request_metadata(
        request,
        &turn_ctx.session_id,
        turn_ctx.turn_id,
        user_text_for_event(event, messages).as_deref(),
    );
}

fn build_generate_request(
    ctx: &LlmStepContext<'_>,
    turn_ctx: &TurnContext,
    event: &ControlEvent,
) -> GenerateRequest {
    let mut request = GenerateRequest::new(ctx.model.clone(), ctx.messages.to_vec());
    merge_request_metadata(
        &mut request,
        ctx.request_metadata,
        turn_ctx,
        event,
        ctx.messages,
    );
    request
}

/// 流式 LLM：调用 [`ProviderRegistry::generate_stream`]，每收到一块文本调用 `on_chunk`。
///
/// 返回与 [`llm_turn`] 相同：最终助手全文（优先用响应体中的 `message.content`，为空则用已拼接的 delta）。
pub fn llm_turn_stream(
    ctx: &LlmStepContext<'_>,
    turn_ctx: &TurnContext,
    event: &ControlEvent,
    on_chunk: &mut dyn FnMut(&str) -> Result<(), String>,
) -> Result<String, String> {
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        provider = %ctx.model.provider,
        model = %ctx.model.model,
        messages = ctx.messages.len(),
        "⑤执行层：进入（流式调用大模型）"
    );
    match event {
        ControlEvent::UserTurn { .. } => {
            tracing::info!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "⑤执行层：用户轮次 → 大模型（流式）"
            );
        }
        ControlEvent::ToolResult { call_id, .. } => {
            tracing::info!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                %call_id,
                messages = ctx.messages.len(),
                "⑤执行层：工具回灌 → 大模型（流式）"
            );
        }
        ControlEvent::Shutdown { .. } => {
            tracing::warn!(
                target: "cubecode.step",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "⑤执行层：拒绝关闭事件"
            );
            return Err("shutdown".into());
        }
    }
    let request = build_generate_request(ctx, turn_ctx, event);
    if let Some(hits) = request.metadata.get(META_MEMORY_HIT_COUNT) {
        let injected = request
            .metadata
            .get(META_MEMORY_INJECTED_BYTES)
            .map(String::as_str)
            .unwrap_or("0");
        tracing::info!(
            target: "cubecode.step",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            memory_hits = %hits,
            memory_injected_bytes = %injected,
            "⑤执行层：记忆 pipeline 已执行（before_generate·流式）"
        );
    }
    let mut streamed = String::new();
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        "⑤执行层：流式开始"
    );
    let response = ctx
        .registry
        .generate_stream(&request, &mut |chunk: StreamChunk| {
            if !chunk.delta.is_empty() {
                on_chunk(&chunk.delta)
                    .map_err(|e| LlmError::InvalidRequest(e))?;
                streamed.push_str(&chunk.delta);
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    let content = response.message.content;
    let final_text = if content.is_empty() {
        streamed
    } else {
        content
    };
    tracing::info!(
        target: "cubecode.step",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        out_bytes = final_text.len(),
        "⑤执行层：流式结束"
    );
    Ok(final_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::{SessionId, TurnId};
    use llm_kit::{
        FinishReason, GenerateResponse, LlmProvider, MessageRole, ProviderInfo,
    };

    struct StreamEchoProvider;

    impl LlmProvider for StreamEchoProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "stream-echo".to_owned(),
                display_name: "Stream Echo".to_owned(),
                supports_chat: true,
                supports_streaming: true,
            }
        }

        fn generate(&self, _request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
            Err(LlmError::InvalidRequest(
                "use generate_stream in test".into(),
            ))
        }

        fn generate_stream(
            &self,
            request: &GenerateRequest,
            on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
        ) -> Result<GenerateResponse, LlmError> {
            for delta in ["你", "好"] {
                on_chunk(StreamChunk {
                    delta: delta.to_owned(),
                    finish_reason: None,
                })?;
            }
            Ok(GenerateResponse {
                model: request.model.clone(),
                message: ChatMessage::new(MessageRole::Assistant, "你好"),
                finish_reason: FinishReason::Stop,
                usage: None,
                raw: None,
            })
        }
    }

    #[test]
    fn llm_turn_stream_calls_registry_and_delivers_chunks() {
        let mut registry = ProviderRegistry::new();
        registry.register(StreamEchoProvider);
        let model = ModelRef::new("stream-echo", "mock");
        let messages = [ChatMessage::new(MessageRole::User, "hi")];
        let ctx = LlmStepContext::new(&registry, model.clone(), &messages);
        let turn_ctx = TurnContext::new(SessionId::new("sess-stream"), TurnId::FIRST);
        let event = ControlEvent::user_turn(&turn_ctx, "hi");
        let mut chunks: Vec<String> = Vec::new();
        let full = llm_turn_stream(&ctx, &turn_ctx, &event, &mut |delta| {
            chunks.push(delta.to_owned());
            Ok(())
        })
        .expect("stream turn");
        assert_eq!(chunks, vec!["你", "好"]);
        assert_eq!(full, "你好");
    }
}
