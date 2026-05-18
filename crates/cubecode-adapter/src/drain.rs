//! 将 [`Adapter::poll_events`] 结果写入 ②。

use cubecode_contracts::ControlEvent;
use cubecode_inbox::{Inbox, InboxFull};

use crate::adapter::Adapter;
use crate::error::DrainError;

/// 调用 [`Adapter::poll_events`] 并将事件依次 `try_push` 进收件箱。
pub fn drain_adapter(adapter: &mut dyn Adapter, inbox: &mut Inbox) -> Result<usize, DrainError> {
    let events = adapter.poll_events()?;
    push_events(inbox, events).map_err(DrainError::from)
}

/// 将已构造的语义事件写入 ②（与具体 `Adapter` 实现无关的公共路径）。
pub fn push_events(
    inbox: &mut Inbox,
    events: impl IntoIterator<Item = ControlEvent>,
) -> Result<usize, InboxFull> {
    let mut count = 0usize;
    for event in events {
        log_enqueue(&event);
        inbox.try_push(event)?;
        count += 1;
    }
    if count > 0 {
        tracing::debug!(
            target: "cubecode.adapter",
            count,
            "①适配层：批量入队完成"
        );
    }
    Ok(count)
}

fn log_enqueue(event: &ControlEvent) {
    match event {
        ControlEvent::UserTurn {
            session_id,
            turn_id,
            text,
        } => {
            tracing::info!(
                target: "cubecode.adapter",
                session_id = %session_id,
                turn_id = %turn_id,
                bytes = text.len(),
                "①适配层：入队用户轮次"
            );
        }
        ControlEvent::Shutdown { session_id } => {
            tracing::info!(
                target: "cubecode.adapter",
                session_id = %session_id,
                "①适配层：入队关闭事件"
            );
        }
        ControlEvent::ToolResult {
            session_id,
            turn_id,
            call_id,
            output,
        } => {
            tracing::info!(
                target: "cubecode.adapter",
                session_id = %session_id,
                turn_id = %turn_id,
                %call_id,
                out_bytes = output.len(),
                "①适配层：入队工具结果"
            );
        }
    }
}
