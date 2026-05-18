//! ④ **编排层**：编排 ⑤ 执行层 → ⑥ 输出层；[`run_full_turn`] 串联 ①～⑥。

mod cancel;
mod flow;
pub mod outcome;
pub mod session_meta;
pub mod state;
pub mod tool_call;

use cubecode_contracts::{ControlEvent, RouteHint, TurnContext};

pub use outcome::{classify_step_result, StepOutcome, TurnFinished};
pub use tool_call::{parse_tool_call_from_model_output, ParsedToolCall};
pub use state::{
    transition, InvalidTransition, Orchestrator, OrchestratorSignal, OrchestratorState,
};
use cubecode_sink::{
    begin_assistant_stream, emit_assistant, emit_chunk, emit_line, end_assistant_stream,
};
use cubecode_step::{
    append_assistant_reply, append_tool_exchange, llm_turn, llm_turn_stream, placeholder_turn,
    ChatMessage, LlmStepContext, ModelRef, ProviderRegistry,
};

pub use cancel::{cancel_active_turn, USER_CANCELLED_MSG};
pub use session_meta::{SessionMetadata, META_ROUTE, META_TOOL_CALL_ID};
pub use flow::{
    execute_pending_tool, new_cancel_flag, run_full_turn, run_shutdown_turn,
    MAX_TOOL_FOLLOW_UP_ROUNDS,
};

/// 单轮 ⑤ 后端：占位或 LLM（消息列表在工具多圈间由编排层更新）。
pub enum TurnRunner<'a> {
    Placeholder,
    Llm {
        registry: &'a ProviderRegistry,
        model: ModelRef,
        messages: &'a mut Vec<ChatMessage>,
        stream: bool,
        session_meta: &'a mut SessionMetadata,
    },
}

impl<'a> TurnRunner<'a> {
    pub fn placeholder() -> Self {
        Self::Placeholder
    }

    pub fn llm(
        registry: &'a ProviderRegistry,
        model: ModelRef,
        messages: &'a mut Vec<ChatMessage>,
        stream: bool,
        session_meta: &'a mut SessionMetadata,
    ) -> Self {
        Self::Llm {
            registry,
            model,
            messages,
            stream,
            session_meta,
        }
    }

    pub fn step_backend(&mut self) -> StepBackend<'_> {
        match self {
            TurnRunner::Placeholder => StepBackend::Placeholder,
            TurnRunner::Llm {
                registry,
                model,
                messages,
                stream,
                session_meta,
            } => StepBackend::Llm {
                registry: *registry,
                model: model.clone(),
                messages: &**messages,
                stream: *stream,
                session_meta,
            },
        }
    }

    pub fn record_tool_exchange(
        &mut self,
        assistant_content: &str,
        call_id: &str,
        tool_output: &str,
    ) {
        if let TurnRunner::Llm { messages, .. } = self {
            append_tool_exchange(messages, assistant_content, call_id, tool_output);
            tracing::info!(
                target: "cubecode.orchestrator",
                %call_id,
                transcript_len = messages.len(),
                "④编排层：工具交换已写入会话 transcript"
            );
        }
    }

    pub fn record_final_assistant(&mut self, content: &str) {
        if let TurnRunner::Llm { messages, .. } = self {
            append_assistant_reply(messages, content);
        }
    }
}

/// ⑤ 执行后端。
pub enum StepBackend<'a> {
    Placeholder,
    Llm {
        registry: &'a ProviderRegistry,
        model: ModelRef,
        messages: &'a [ChatMessage],
        /// `true` 时走 [`llm_turn_stream`] + ⑥ 流式写出（仅 `UserTurn` / `ChatTurn`）。
        stream: bool,
        session_meta: &'a mut SessionMetadata,
    },
}

/// ⑥ 输出样式。
#[derive(Clone, Copy)]
pub enum SinkStyle {
    /// `[label] …`（演示）
    Prefixed,
    /// 聊天正文
    Assistant,
}

pub(crate) fn emit_body(turn_ctx: &TurnContext, sink: SinkStyle, label: &str, body: &str) {
    match sink {
        SinkStyle::Prefixed => emit_line(turn_ctx, label, body),
        SinkStyle::Assistant => emit_assistant(turn_ctx, body),
    }
}

fn prepare_session_metadata(
    session_meta: &mut SessionMetadata,
    turn_ctx: &TurnContext,
    event: &ControlEvent,
) {
    session_meta.prepare_for_step(turn_ctx, event);
}

/// ⑤ 聊天：先分类再写出 ⑥（工具调用 JSON 不直接展示）。
fn run_chat_llm(
    turn_ctx: &TurnContext,
    event: &ControlEvent,
    registry: &ProviderRegistry,
    model: ModelRef,
    messages: &[ChatMessage],
    session_meta: &mut SessionMetadata,
    sink: SinkStyle,
    stream: bool,
) -> Result<(String, StepOutcome), String> {
    prepare_session_metadata(session_meta, turn_ctx, event);
    let ctx = LlmStepContext {
        registry,
        model,
        messages,
        request_metadata: Some(session_meta.as_map()),
    };
    if stream {
        tracing::info!(
            target: "cubecode.orchestrator",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            "④编排层 → ⑤执行层（聊天·流式）"
        );
        return run_chat_llm_stream(turn_ctx, event, &ctx, sink);
    }
    let body = llm_turn(&ctx, turn_ctx, event)?;
    let outcome = classify_step_result(&body);
    outcome::emit_assistant_if_text(turn_ctx, sink, &outcome);
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        out_bytes = body.len(),
        ?outcome,
        "④编排层 → ⑥输出层"
    );
    Ok((body, outcome))
}

fn run_chat_llm_stream(
    turn_ctx: &TurnContext,
    event: &ControlEvent,
    ctx: &LlmStepContext<'_>,
    sink: SinkStyle,
) -> Result<(String, StepOutcome), String> {
    let mut buffered = String::new();
    let full = llm_turn_stream(ctx, turn_ctx, event, &mut |delta| {
        buffered.push_str(delta);
        Ok(())
    })?;
    let body = if full.is_empty() { buffered } else { full };
    let outcome = classify_step_result(&body);
    match (&outcome, sink) {
        (StepOutcome::Text(text), SinkStyle::Assistant) => {
            begin_assistant_stream(turn_ctx);
            emit_chunk(turn_ctx, text);
            end_assistant_stream(turn_ctx);
        }
        (StepOutcome::Text(text), SinkStyle::Prefixed) => {
            emit_line(turn_ctx, "助手(流式)", text);
        }
        (StepOutcome::PendingTool { .. }, _) => {
            tracing::info!(
                target: "cubecode.orchestrator",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "④编排层：流式结果实为工具调用，已抑制正文输出"
            );
        }
        _ => {}
    }
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        out_bytes = body.len(),
        ?outcome,
        "④编排层 → ⑥输出层（流式已完成）"
    );
    Ok((body, outcome))
}

/// `ToolFollowUp`：⑤ 消费 [`ControlEvent::ToolResult`] 并分类写出 ⑥。
fn run_tool_follow_up(
    turn_ctx: &TurnContext,
    event: &ControlEvent,
    backend: &mut StepBackend<'_>,
    sink: SinkStyle,
) -> Result<StepOutcome, String> {
    let ControlEvent::ToolResult {
        call_id,
        output,
        ..
    } = event
    else {
        return Err(format!("工具回灌路由收到非 ToolResult 事件：{event:?}"));
    };
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        %call_id,
        out_bytes = output.len(),
        "④编排层：处理工具结果"
    );
    match backend {
        StepBackend::Placeholder => {
            let body = placeholder_turn(turn_ctx, event)?;
            let outcome = classify_step_result(&body);
            outcome::emit_assistant_if_text(turn_ctx, sink, &outcome);
            Ok(outcome)
        }
        StepBackend::Llm {
            registry,
            model,
            messages,
            stream,
            session_meta,
        } => run_chat_llm(
            turn_ctx,
            event,
            registry,
            model.clone(),
            messages,
            session_meta,
            sink,
            *stream,
        )
        .map(|(_, o)| o),
    }
}

/// **④ → ⑤ → ⑥**（③ 路由结果传入）。
pub fn run_pipeline(
    turn_ctx: &TurnContext,
    route: RouteHint,
    event: &ControlEvent,
    backend: &mut StepBackend<'_>,
    sink: SinkStyle,
) -> Result<TurnFinished, String> {
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        ?route,
        "④编排层：进入"
    );
    let result = match route {
        RouteHint::Exit => {
            tracing::info!(
                target: "cubecode.orchestrator",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "④编排层 → ⑥输出层（退出，跳过⑤执行层）"
            );
            emit_line(turn_ctx, "输出层", "编排层：退出路由（未调用执行层）");
            TurnFinished::no_reply(turn_ctx.turn_id)
        }
        RouteHint::ChatTurn => {
            let outcome = match backend {
                StepBackend::Placeholder => {
                    tracing::info!(
                        target: "cubecode.orchestrator",
                        session_id = %turn_ctx.session_id,
                        turn_id = %turn_ctx.turn_id,
                        "④编排层 → ⑤执行层（聊天）"
                    );
                    let body = placeholder_turn(turn_ctx, event)?;
                    let outcome = classify_step_result(&body);
                    outcome::emit_assistant_if_text(turn_ctx, sink, &outcome);
                    outcome
                }
                StepBackend::Llm {
                    registry,
                    model,
                    messages,
                    stream,
                    session_meta,
                } => {
                    tracing::info!(
                        target: "cubecode.orchestrator",
                        session_id = %turn_ctx.session_id,
                        turn_id = %turn_ctx.turn_id,
                        stream,
                        "④编排层 → ⑤执行层（聊天）"
                    );
                    run_chat_llm(
                        turn_ctx,
                        event,
                        registry,
                        model.clone(),
                        messages,
                        session_meta,
                        sink,
                        *stream,
                    )?
                    .1
                }
            };
            TurnFinished::new(turn_ctx.turn_id, outcome)
        }
        RouteHint::ToolFollowUp => {
            tracing::info!(
                target: "cubecode.orchestrator",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "④编排层 → ⑤执行层（工具回灌）"
            );
            let outcome = run_tool_follow_up(turn_ctx, event, backend, sink)?;
            TurnFinished::new(turn_ctx.turn_id, outcome)
        }
        RouteHint::SubAgent => {
            tracing::info!(
                target: "cubecode.orchestrator",
                session_id = %turn_ctx.session_id,
                turn_id = %turn_ctx.turn_id,
                "④编排层 → ⑤执行层（子 Agent，占位）"
            );
            let body = "（编排层占位）子 Agent 路由尚未实现".to_string();
            emit_body(turn_ctx, sink, "子Agent", &body);
            TurnFinished::text(turn_ctx.turn_id, body)
        }
    };
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %turn_ctx.session_id,
        turn_id = %turn_ctx.turn_id,
        ?result.outcome,
        "④编排层：离开"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::{SessionId, TurnId};

    use crate::flow::run_full_turn;
    use cubecode_step::{ChatMessage, MessageRole, ToolRegistry};

    #[test]
    fn chat_turn_placeholder_ignores_stream_flag() {
        let session = SessionId::new("sess-orch");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let event = ControlEvent::user_turn(&ctx, "hi");
        let mut backend = StepBackend::Placeholder;
        let finished = run_pipeline(
            &ctx,
            RouteHint::ChatTurn,
            &event,
            &mut backend,
            SinkStyle::Prefixed,
        )
        .expect("pipeline");
        assert!(finished
            .user_reply()
            .is_some_and(|s| s.contains("占位")));
    }

    #[test]
    fn pipeline_classifies_tool_call_poc() {
        let ctx = TurnContext::new(SessionId::new("sess-poc"), TurnId::FIRST);
        let event = ControlEvent::user_turn(&ctx, "x");
        let json = r#"{"tool_call":{"id":"c1","name":"read_file"}}"#;
        let mut backend = StepBackend::Placeholder;
        let finished = run_pipeline(
            &ctx,
            RouteHint::ChatTurn,
            &event,
            &mut backend,
            SinkStyle::Prefixed,
        )
        .expect("pipeline");
        // placeholder returns non-json; test classify directly
        let _ = finished;
        assert!(matches!(
            classify_step_result(json),
            StepOutcome::PendingTool { .. }
        ));
    }

    #[test]
    fn full_turn_updates_orchestrator_state() {
        let session = SessionId::new("sess-orch-flow");
        let mut orch = Orchestrator::new(session.clone());
        let ctx = TurnContext::new(session, TurnId::FIRST);
        let mut inbox = cubecode_inbox::Inbox::with_capacity(4);
        let tools = ToolRegistry::new(".");
        run_full_turn(
            &ctx,
            &mut orch,
            &mut inbox,
            "hi",
            &tools,
            TurnRunner::placeholder(),
            SinkStyle::Prefixed,
            None,
        )
        .expect("turn");
        assert!(orch.is_idle());
    }

    #[test]
    fn full_turn_llm_tool_then_reply_same_turn_id() {
        use std::fs;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        use llm_kit::{
            FinishReason, GenerateRequest, GenerateResponse, LlmError, LlmProvider, ProviderInfo,
            ProviderRegistry,
        };

        struct ToolThenAnswer {
            calls: Arc<AtomicUsize>,
            tool_json: String,
        }

        impl LlmProvider for ToolThenAnswer {
            fn info(&self) -> ProviderInfo {
                ProviderInfo {
                    id: "tool-then-answer".to_owned(),
                    display_name: "Tool Then Answer".to_owned(),
                    supports_chat: true,
                    supports_streaming: false,
                }
            }

            fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    return Ok(GenerateResponse {
                        model: request.model.clone(),
                        message: ChatMessage::new(MessageRole::Assistant, self.tool_json.clone()),
                        finish_reason: FinishReason::Stop,
                        usage: None,
                        raw: None,
                    });
                }
                let tool_body = request
                    .messages
                    .iter()
                    .find(|m| matches!(m.role, MessageRole::Tool))
                    .map(|m| m.content.as_str())
                    .unwrap_or("");
                let reply = format!("已读取文件，内容为：{tool_body}");
                Ok(GenerateResponse {
                    model: request.model.clone(),
                    message: ChatMessage::new(MessageRole::Assistant, reply),
                    finish_reason: FinishReason::Stop,
                    usage: None,
                    raw: None,
                })
            }
        }

        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("data.txt"), "FROM_DISK").expect("write");
        let tools = ToolRegistry::new(dir.path());

        let tool_json = r#"{"tool_call":{"id":"c1","name":"read_file","arguments":{"path":"data.txt"}}}"#;
        let mut registry = ProviderRegistry::new();
        registry.register(ToolThenAnswer {
            calls: Arc::new(AtomicUsize::new(0)),
            tool_json: tool_json.to_owned(),
        });

        let session = SessionId::new("sess-m4-6");
        let mut orch = Orchestrator::new(session.clone());
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut inbox = cubecode_inbox::Inbox::with_capacity(8);
        let mut messages = vec![ChatMessage::new(MessageRole::User, "请读取 data.txt")];
        let model = ModelRef::new("tool-then-answer", "mock");
        let mut session_meta = SessionMetadata::new(session.clone());
        let runner = TurnRunner::llm(
            &registry,
            model,
            &mut messages,
            false,
            &mut session_meta,
        );

        let finished = run_full_turn(
            &ctx,
            &mut orch,
            &mut inbox,
            "请读取 data.txt",
            &tools,
            runner,
            SinkStyle::Prefixed,
            None,
        )
        .expect("full turn with tool loop");

        assert!(matches!(finished.outcome, StepOutcome::Text(_)));
        assert!(
            finished
                .user_reply()
                .is_some_and(|s| s.contains("FROM_DISK")),
            "reply should include tool output"
        );
        assert!(orch.is_idle());
        assert!(
            messages.iter().any(|m| {
                matches!(m.role, MessageRole::Tool) && m.content.contains("FROM_DISK")
            }),
            "transcript should contain tool result"
        );
    }
}

/// 占位闭环（兼容旧调用）：**④ → ⑤ → ⑥**。
pub fn run_minimal_pipeline(_route: RouteHint, _event: &ControlEvent) -> Result<(), String> {
    use cubecode_contracts::{SessionId, TurnId};

    let session = SessionId::generate();
    let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
    let mut orch = Orchestrator::new(session);
    let mut inbox = cubecode_inbox::Inbox::with_capacity(4);
    let tools = cubecode_step::ToolRegistry::new(".");
    run_full_turn(
        &ctx,
        &mut orch,
        &mut inbox,
        "minimal",
        &tools,
        TurnRunner::placeholder(),
        SinkStyle::Prefixed,
        None,
    )
    .map(|_| ())
}
