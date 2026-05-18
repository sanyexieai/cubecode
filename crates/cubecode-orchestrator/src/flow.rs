//! ①～⑥ 单轮流转（日志顺序与调用顺序一致）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cubecode_adapter::{enqueue_shutdown, enqueue_tool_result, enqueue_user_line};
use cubecode_contracts::TurnContext;
use cubecode_dispatch::route;
use cubecode_inbox::Inbox;
use cubecode_sink::emit_error;
use cubecode_step::ToolRegistry;

use crate::{
    cancel::{cancel_active_turn, USER_CANCELLED_MSG},
    run_pipeline, Orchestrator, SinkStyle, StepOutcome, TurnFinished, TurnRunner,
};

/// 单轮用户输入内允许的工具跟进次数上限。
pub const MAX_TOOL_FOLLOW_UP_ROUNDS: usize = 8;

fn map_turn_error(ctx: &TurnContext, err: impl ToString) -> String {
    let message = err.to_string();
    emit_error(ctx, &message);
    message
}

/// 若 `cancel` 已置位，清空 ② 待处理事件并使 ④ 回到 `Idle`。
fn abort_if_cancelled(
    ctx: &TurnContext,
    orchestrator: &mut Orchestrator,
    inbox: &mut Inbox,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let Some(flag) = cancel else {
        return Ok(());
    };
    if !flag.load(Ordering::SeqCst) {
        return Ok(());
    }
    flag.store(false, Ordering::SeqCst);
    cancel_active_turn(orchestrator, inbox);
    Err(map_turn_error(ctx, USER_CANCELLED_MSG))
}

/// ② 出队 → ③ 路由 → ④ 流水线。
fn dequeue_and_run_pipeline<'a>(
    ctx: &TurnContext,
    inbox: &mut Inbox,
    runner: &mut TurnRunner<'a>,
    sink: SinkStyle,
) -> Result<TurnFinished, String> {
    tracing::info!(
        target: "cubecode.inbox",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "②收件箱：进入（出队）"
    );
    let event = inbox
        .pop()
        .ok_or_else(|| map_turn_error(ctx, "②收件箱：队列为空"))?;
    tracing::info!(
        target: "cubecode.inbox",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        ?event,
        "②收件箱 → ③调度层"
    );
    tracing::info!(
        target: "cubecode.dispatch",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "③调度层：进入（路由）"
    );
    let route_hint = route(ctx, &event);
    tracing::info!(
        target: "cubecode.dispatch",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        ?route_hint,
        "③调度层 → ④编排层"
    );
    let mut backend = runner.step_backend();
    run_pipeline(ctx, route_hint, &event, &mut backend, sink).map_err(|e| map_turn_error(ctx, e))
}

/// 执行待处理工具并将结果作为 [`ControlEvent::ToolResult`] 再入队 ②；返回工具输出正文。
pub fn execute_pending_tool(
    ctx: &TurnContext,
    orchestrator: &mut Orchestrator,
    inbox: &mut Inbox,
    tools: &ToolRegistry,
    call_id: &str,
    tool_name: &str,
    arguments: &str,
) -> Result<String, String> {
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        %call_id,
        %tool_name,
        "④编排层：执行工具"
    );
    let output = tools
        .execute(tool_name, arguments)
        .map_err(|e| map_turn_error(ctx, e))?;
    enqueue_tool_result(inbox, ctx, call_id, output.clone())
        .map_err(|e| map_turn_error(ctx, e))?;
    orchestrator.tool_result_ready()?;
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "④编排层：工具结果已入队 ②"
    );
    Ok(output)
}

/// 从用户一行输入跑完 ①～⑥（含工具多圈：PendingTool → 执行 → ToolResult → 再跑 ③④）。
pub fn run_full_turn<'a>(
    ctx: &TurnContext,
    orchestrator: &mut Orchestrator,
    inbox: &mut Inbox,
    user_line: impl Into<String>,
    tools: &'a ToolRegistry,
    mut runner: TurnRunner<'a>,
    sink: SinkStyle,
    cancel: Option<&AtomicBool>,
) -> Result<TurnFinished, String> {
    abort_if_cancelled(ctx, orchestrator, inbox, cancel)?;
    orchestrator.begin_user_turn(ctx.turn_id)?;
    let line = user_line.into();
    tracing::info!(
        target: "cubecode.cli",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        bytes = line.len(),
        "════ 本轮开始 ════"
    );

    tracing::info!(
        target: "cubecode.adapter",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "①适配层：进入"
    );
    enqueue_user_line(inbox, ctx, line).map_err(|e| map_turn_error(ctx, e))?;
    tracing::info!(
        target: "cubecode.adapter",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "①适配层 → ②收件箱"
    );

    abort_if_cancelled(ctx, orchestrator, inbox, cancel)?;

    let mut finished = match dequeue_and_run_pipeline(ctx, inbox, &mut runner, sink) {
        Ok(f) => f,
        Err(e) => {
            orchestrator.abort_user_turn()?;
            return Err(e);
        }
    };

    orchestrator.finish_turn_with(&finished)?;

    let mut tool_rounds = 0usize;
    while let StepOutcome::PendingTool {
        call_id,
        tool_name,
        arguments,
        assistant_content,
    } = &finished.outcome
    {
        tool_rounds += 1;
        if tool_rounds > MAX_TOOL_FOLLOW_UP_ROUNDS {
            let msg = format!("工具跟进超过上限（{MAX_TOOL_FOLLOW_UP_ROUNDS} 次）");
            emit_error(ctx, &msg);
            orchestrator.abort_user_turn()?;
            return Err(msg);
        }
        abort_if_cancelled(ctx, orchestrator, inbox, cancel)?;
        let tool_output = execute_pending_tool(
            ctx,
            orchestrator,
            inbox,
            tools,
            call_id,
            tool_name,
            arguments,
        )?;
        runner.record_tool_exchange(assistant_content, call_id, &tool_output);
        abort_if_cancelled(ctx, orchestrator, inbox, cancel)?;
        finished = match dequeue_and_run_pipeline(ctx, inbox, &mut runner, sink) {
            Ok(f) => f,
            Err(e) => {
                orchestrator.abort_user_turn()?;
                return Err(e);
            }
        };
        orchestrator.finish_turn_with(&finished)?;
    }

    if let StepOutcome::Text(content) = &finished.outcome {
        runner.record_final_assistant(content);
    }

    tracing::info!(
        target: "cubecode.cli",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        ?finished.outcome,
        tool_rounds,
        has_user_reply = finished.user_reply().is_some(),
        "════ 本轮结束 ════"
    );
    Ok(finished)
}

/// 供 CLI 安装的 Ctrl+C 取消标志（跨线程置位，编排层在步骤间检查）。
pub fn new_cancel_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

/// 退出：shutdown 事件走同一流水线。
pub fn run_shutdown_turn(
    ctx: &TurnContext,
    orchestrator: &mut Orchestrator,
    inbox: &mut Inbox,
) -> Result<(), String> {
    orchestrator.request_shutdown()?;
    tracing::info!(
        target: "cubecode.cli",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "════ 退出轮次 ════"
    );
    tracing::info!(
        target: "cubecode.adapter",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "①适配层：进入（关闭）"
    );
    enqueue_shutdown(inbox, ctx).map_err(|e| map_turn_error(ctx, e))?;
    tracing::info!(
        target: "cubecode.adapter",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "①适配层 → ②收件箱"
    );
    let mut runner = TurnRunner::placeholder();
    dequeue_and_run_pipeline(ctx, inbox, &mut runner, SinkStyle::Prefixed)
    .map_err(|e| map_turn_error(ctx, e))?;
    orchestrator.complete_shutdown()?;
    tracing::info!(
        target: "cubecode.cli",
        session_id = %ctx.session_id,
        turn_id = %ctx.turn_id,
        "════ 退出完成 ════"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use cubecode_contracts::{ControlEvent, RouteHint, SessionId, TurnId};
    use cubecode_dispatch::route;

    #[test]
    fn run_full_turn_aborts_when_cancel_flag_set() {
        use std::sync::atomic::AtomicBool;

        let session = SessionId::new("sess-cancel-flow");
        let mut orch = Orchestrator::new(session.clone());
        let ctx = TurnContext::new(session, TurnId::FIRST);
        let mut inbox = Inbox::with_capacity(4);
        let tools = ToolRegistry::new(".");
        let cancel = AtomicBool::new(true);
        let err = run_full_turn(
            &ctx,
            &mut orch,
            &mut inbox,
            "hi",
            &tools,
            TurnRunner::placeholder(),
            SinkStyle::Prefixed,
            Some(&cancel),
        )
        .expect_err("cancelled");
        assert!(err.contains(USER_CANCELLED_MSG));
        assert!(orch.is_idle());
    }

    #[test]
    fn tool_result_event_routes_to_follow_up() {
        let session = SessionId::new("sess-tool-route");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut inbox = Inbox::with_capacity(4);
        enqueue_tool_result(&mut inbox, &ctx, "c1", "file body").expect("enqueue");
        let event = inbox.pop().expect("pop");
        assert_eq!(route(&ctx, &event), RouteHint::ToolFollowUp);
    }

    #[test]
    fn execute_pending_tool_requeues_result() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data.txt");
        fs::write(&path, "FROM_DISK").expect("write");
        let tools = ToolRegistry::new(dir.path());
        let session = SessionId::new("sess-tool-exec");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut orch = Orchestrator::new(session);
        let mut inbox = Inbox::with_capacity(4);
        orch.begin_user_turn(TurnId::FIRST).unwrap();
        orch.finish_turn_with(&TurnFinished::pending_tool(
            TurnId::FIRST,
            "c1",
            "read_file",
            "data.txt",
            r#"{"tool_call":{"id":"c1","name":"read_file","arguments":{"path":"data.txt"}}}"#,
        ))
        .unwrap();
        let out = execute_pending_tool(&ctx, &mut orch, &mut inbox, &tools, "c1", "read_file", "data.txt")
            .expect("execute");
        assert_eq!(out, "FROM_DISK");
        assert!(matches!(orch.state(), crate::OrchestratorState::RunningTurn { .. }));
        let event = inbox.pop().expect("tool result in inbox");
        match event {
            ControlEvent::ToolResult { output, call_id, .. } => {
                assert_eq!(call_id, "c1");
                assert_eq!(output, "FROM_DISK");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
