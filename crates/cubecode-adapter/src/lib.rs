//! ① **适配层**：将用户输入转为语义事件写入收件箱。

mod adapter;
mod drain;
mod error;
mod http_json;
mod mock;
mod terminal;

pub use adapter::Adapter;
pub use drain::{drain_adapter, push_events};
pub use error::{AdapterError, DrainError};
pub use http_json::{HttpJsonAdapter, JsonRpcEchoResult, JsonRpcRequest};
pub use mock::MockAdapter;
pub use terminal::{TerminalAdapter, TerminalPoll};

use cubecode_contracts::{ControlEvent, SessionId, TurnContext};
use cubecode_inbox::{Inbox, InboxFull};

pub fn enqueue_user_line(
    inbox: &mut Inbox,
    turn_ctx: &TurnContext,
    line: impl Into<String>,
) -> Result<(), InboxFull> {
    push_events(inbox, [ControlEvent::user_turn(turn_ctx, line)])?;
    Ok(())
}

pub fn enqueue_tool_result(
    inbox: &mut Inbox,
    turn_ctx: &TurnContext,
    call_id: impl Into<String>,
    output: impl Into<String>,
) -> Result<(), InboxFull> {
    push_events(
        inbox,
        [ControlEvent::tool_result(turn_ctx, call_id, output)],
    )?;
    Ok(())
}

pub fn enqueue_shutdown(inbox: &mut Inbox, turn_ctx: &TurnContext) -> Result<(), InboxFull> {
    push_events(inbox, [ControlEvent::shutdown(turn_ctx.session_id.clone())])?;
    Ok(())
}

/// 丢弃收件箱中该会话尚未出队的事件（如用户 `/cancel` 或 Ctrl+C 预处理）。
pub fn cancel_session(inbox: &mut Inbox, session_id: &SessionId) -> usize {
    tracing::info!(
        target: "cubecode.adapter",
        %session_id,
        "①适配层：请求取消会话待处理事件"
    );
    inbox.cancel_session(session_id)
}

/// 清空收件箱全部待处理事件（关闭或重置时用）。
pub fn clear_inbox(inbox: &mut Inbox) -> usize {
    tracing::info!(target: "cubecode.adapter", "①适配层：请求清空收件箱");
    inbox.clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::{SessionId, TurnId};

    #[test]
    fn mock_adapter_poll_drains_into_inbox() {
        let session = SessionId::new("s-mock");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut adapter = MockAdapter::with_events([
            ControlEvent::user_turn(&ctx, "hello"),
            ControlEvent::shutdown(session.clone()),
        ]);
        let mut inbox = Inbox::with_capacity(4);

        assert_eq!(drain_adapter(&mut adapter, &mut inbox).expect("drain"), 2);
        assert!(adapter.poll_events().expect("empty").is_empty());
        assert_eq!(drain_adapter(&mut adapter, &mut inbox).expect("noop"), 0);

        let first = inbox.pop().expect("user");
        assert!(matches!(first, ControlEvent::UserTurn { .. }));
        let second = inbox.pop().expect("shutdown");
        assert!(matches!(second, ControlEvent::Shutdown { .. }));
    }

    #[test]
    fn push_events_respects_inbox_capacity() {
        let session = SessionId::new("s-full");
        let ctx = TurnContext::new(session, TurnId::FIRST);
        let mut inbox = Inbox::with_capacity(1);
        push_events(&mut inbox, [ControlEvent::user_turn(&ctx, "a")]).expect("one");
        let err = push_events(&mut inbox, [ControlEvent::user_turn(&ctx, "b")]);
        assert!(err.is_err());
    }

    #[test]
    fn enqueue_helpers_delegate_to_push_events() {
        let session = SessionId::new("s-enq");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut inbox = Inbox::with_capacity(4);
        enqueue_user_line(&mut inbox, &ctx, "hi").expect("user");
        enqueue_shutdown(&mut inbox, &ctx).expect("shutdown");
        assert_eq!(inbox.len(), 2);
    }
}
