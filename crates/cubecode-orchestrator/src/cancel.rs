//! 用户取消进行中的轮次（M4-7）。

use cubecode_adapter::cancel_session;
use cubecode_inbox::Inbox;

use crate::Orchestrator;

/// 与 [`run_full_turn`] 取消路径一致的错误文案。
pub const USER_CANCELLED_MSG: &str = "用户已取消当前轮次";

/// 清空 ② 中该会话待处理事件，并使 ④ 从 `RunningTurn` / `AwaitingTool` 回到 `Idle`。
///
/// 若当前无进行中的轮次，返回 `false` 且不修改状态。
pub fn cancel_active_turn(orchestrator: &mut Orchestrator, inbox: &mut Inbox) -> bool {
    if !orchestrator.is_turn_active() {
        tracing::info!(
            target: "cubecode.orchestrator",
            session_id = %orchestrator.session_id,
            "④编排层：取消请求（无进行中的轮次）"
        );
        return false;
    }
    let session_id = orchestrator.session_id.clone();
    let removed = cancel_session(inbox, &session_id);
    if let Err(e) = orchestrator.abort_user_turn() {
        tracing::warn!(
            target: "cubecode.orchestrator",
            session_id = %session_id,
            error = %e,
            "④编排层：取消时中止轮次失败"
        );
    }
    tracing::info!(
        target: "cubecode.orchestrator",
        session_id = %session_id,
        inbox_removed = removed,
        "④编排层：用户取消，已回到 Idle"
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::{SessionId, TurnId};
    use cubecode_inbox::Inbox;

    #[test]
    fn cancel_from_running_turn() {
        let session = SessionId::new("sess-cancel-run");
        let mut orch = Orchestrator::new(session.clone());
        let mut inbox = Inbox::with_capacity(4);
        orch.begin_user_turn(TurnId::FIRST).unwrap();
        assert!(cancel_active_turn(&mut orch, &mut inbox));
        assert!(orch.is_idle());
    }

    #[test]
    fn cancel_from_awaiting_tool() {
        use crate::TurnFinished;

        let session = SessionId::new("sess-cancel-await");
        let mut orch = Orchestrator::new(session.clone());
        let mut inbox = Inbox::with_capacity(4);
        orch.begin_user_turn(TurnId::FIRST).unwrap();
        orch.finish_turn_with(&TurnFinished::pending_tool(
            TurnId::FIRST,
            "c1",
            "read_file",
            "{}",
            "{}",
        ))
        .unwrap();
        assert!(cancel_active_turn(&mut orch, &mut inbox));
        assert!(orch.is_idle());
    }

    #[test]
    fn cancel_when_idle_is_noop() {
        let session = SessionId::new("sess-cancel-idle");
        let mut orch = Orchestrator::new(session.clone());
        let mut inbox = Inbox::with_capacity(4);
        assert!(!cancel_active_turn(&mut orch, &mut inbox));
        assert!(orch.is_idle());
    }
}
