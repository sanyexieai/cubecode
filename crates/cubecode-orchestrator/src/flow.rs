//! ①～⑥ 单轮流转（日志顺序与调用顺序一致）。

use cubecode_adapter::{enqueue_shutdown, enqueue_user_line};
use cubecode_dispatch::route;
use cubecode_inbox::Inbox;

use crate::{run_pipeline, SinkStyle, StepBackend};

/// 从用户一行输入跑完 ①～⑥。
pub fn run_full_turn<'a>(
    turn: u32,
    inbox: &mut Inbox,
    user_line: impl Into<String>,
    backend: StepBackend<'a>,
    sink: SinkStyle,
) -> Result<Option<String>, String> {
    let line = user_line.into();
    tracing::info!(
        target: "cubecode.cli",
        turn,
        bytes = line.len(),
        "════ 本轮开始 ════"
    );

    tracing::info!(target: "cubecode.adapter", turn, "①适配层：进入");
    enqueue_user_line(inbox, line);
    tracing::info!(target: "cubecode.adapter", turn, "①适配层 → ②收件箱");

    tracing::info!(target: "cubecode.inbox", turn, "②收件箱：进入（出队）");
    let event = inbox
        .pop()
        .ok_or_else(|| "②收件箱：适配层入队后队列为空".to_owned())?;
    tracing::info!(target: "cubecode.inbox", turn, ?event, "②收件箱 → ③调度层");

    tracing::info!(target: "cubecode.dispatch", turn, "③调度层：进入（路由）");
    let route_hint = route(&event);
    tracing::info!(
        target: "cubecode.dispatch",
        turn,
        ?route_hint,
        "③调度层 → ④编排层"
    );

    let reply = run_pipeline(route_hint, &event, backend, sink)?;

    tracing::info!(
        target: "cubecode.cli",
        turn,
        has_reply = reply.is_some(),
        "════ 本轮结束 ════"
    );
    Ok(reply)
}

/// 退出：shutdown 事件走同一流水线。
pub fn run_shutdown_turn(turn: u32, inbox: &mut Inbox) -> Result<(), String> {
    tracing::info!(target: "cubecode.cli", turn, "════ 退出轮次 ════");
    tracing::info!(target: "cubecode.adapter", turn, "①适配层：进入（关闭）");
    enqueue_shutdown(inbox);
    tracing::info!(target: "cubecode.adapter", turn, "①适配层 → ②收件箱");
    tracing::info!(target: "cubecode.inbox", turn, "②收件箱：进入（出队）");
    let event = inbox
        .pop()
        .ok_or_else(|| "②收件箱：队列为空".to_owned())?;
    tracing::info!(target: "cubecode.inbox", turn, ?event, "②收件箱 → ③调度层");
    tracing::info!(target: "cubecode.dispatch", turn, "③调度层：进入（路由）");
    let route_hint = route(&event);
    tracing::info!(
        target: "cubecode.dispatch",
        turn,
        ?route_hint,
        "③调度层 → ④编排层"
    );
    run_pipeline(
        route_hint,
        &event,
        StepBackend::Placeholder,
        SinkStyle::Prefixed,
    )?;
    tracing::info!(target: "cubecode.cli", turn, "════ 退出完成 ════");
    Ok(())
}
