//! ④ 编排层会话状态机（纯 Rust；M4-1 占位，供 M4-2+ 工具多圈扩展）。

use cubecode_contracts::{SessionId, TurnId};

/// 单会话编排状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorState {
    /// 可接受新的用户轮次。
    Idle,
    /// 正在处理一轮（⑤ / ⑥ 或后续工具链）。
    RunningTurn {
        turn_id: TurnId,
    },
    /// 已派发工具，等待 `ToolResult` 回灌（M4-5 接线）。
    AwaitingTool {
        turn_id: TurnId,
    },
    /// 已收到关闭意图，处理 shutdown 流水线中。
    ShuttingDown,
    /// 会话已结束，不再接受事件。
    Ended,
}

/// 触发状态迁移的外部信号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorSignal {
    /// 开始处理用户轮次（`Idle` → `RunningTurn`）。
    UserTurnStarted(TurnId),
    /// 本轮正常结束（`RunningTurn` / `AwaitingTool` → `Idle`）。
    TurnFinished,
    /// 本轮失败或取消，释放占用（→ `Idle`）。
    TurnAborted,
    /// 模型侧已派发工具（`RunningTurn` → `AwaitingTool`，M4+）。
    ToolDispatched,
    /// 工具结果已入队并将继续编排（`AwaitingTool` → `RunningTurn`，M4+）。
    ToolResultReady,
    /// 用户/适配层请求关闭会话。
    ShutdownRequested,
    /// shutdown 流水线完成。
    ShutdownComplete,
}

/// 非法迁移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: OrchestratorState,
    pub signal: OrchestratorSignal,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "非法状态迁移：{:?} + {:?}",
            self.from, self.signal
        )
    }
}

impl std::error::Error for InvalidTransition {}

/// 纯函数：根据当前状态与信号计算下一状态。
pub fn transition(
    from: OrchestratorState,
    signal: OrchestratorSignal,
) -> Result<OrchestratorState, InvalidTransition> {
    use OrchestratorSignal::*;
    use OrchestratorState::*;

    let next = match (from.clone(), signal) {
        (Idle, UserTurnStarted(turn_id)) => RunningTurn { turn_id },
        (RunningTurn { .. } | AwaitingTool { .. }, TurnFinished | TurnAborted) => Idle,
        (RunningTurn { turn_id }, ToolDispatched) => AwaitingTool { turn_id },
        (AwaitingTool { turn_id }, ToolResultReady) => RunningTurn { turn_id },
        (Idle, ShutdownRequested) => ShuttingDown,
        (ShuttingDown, ShutdownComplete) => Ended,
        (from, signal) => {
            return Err(InvalidTransition { from, signal });
        }
    };
    Ok(next)
}

/// 持有状态的编排器（按会话一个实例）。
#[derive(Debug, Clone)]
pub struct Orchestrator {
    pub session_id: SessionId,
    state: OrchestratorState,
}

impl Orchestrator {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            state: OrchestratorState::Idle,
        }
    }

    pub fn state(&self) -> &OrchestratorState {
        &self.state
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, OrchestratorState::Idle)
    }

    /// 是否正在处理用户轮次（含等待工具结果）。
    pub fn is_turn_active(&self) -> bool {
        matches!(
            self.state,
            OrchestratorState::RunningTurn { .. } | OrchestratorState::AwaitingTool { .. }
        )
    }

    pub fn apply(&mut self, signal: OrchestratorSignal) -> Result<(), InvalidTransition> {
        let from = self.state.clone();
        let to = transition(from.clone(), signal)?;
        tracing::info!(
            target: "cubecode.orchestrator",
            session_id = %self.session_id,
            ?from,
            ?signal,
            ?to,
            "④编排层：状态迁移"
        );
        self.state = to;
        Ok(())
    }

    /// 进入用户轮次；非 `Idle` 时返回可读错误。
    pub fn begin_user_turn(&mut self, turn_id: TurnId) -> Result<(), String> {
        self.apply(OrchestratorSignal::UserTurnStarted(turn_id))
            .map_err(|e| e.to_string())
    }

    /// 结束用户轮次（成功或失败后均应调用以回到 `Idle`）。
    pub fn end_user_turn(&mut self) -> Result<(), String> {
        let signal = if matches!(
            self.state,
            OrchestratorState::RunningTurn { .. } | OrchestratorState::AwaitingTool { .. }
        ) {
            OrchestratorSignal::TurnFinished
        } else {
            return Ok(());
        };
        self.apply(signal).map_err(|e| e.to_string())
    }

    /// 按 [`crate::TurnFinished`] 收束本轮：`PendingTool` → `AwaitingTool`，其余 → `Idle`。
    pub fn finish_turn_with(
        &mut self,
        finished: &crate::TurnFinished,
    ) -> Result<(), String> {
        use crate::StepOutcome;
        match &finished.outcome {
            StepOutcome::PendingTool {
                call_id,
                tool_name,
                arguments,
                assistant_content: _,
            } => {
                tracing::info!(
                    target: "cubecode.orchestrator",
                    session_id = %self.session_id,
                    turn_id = %finished.turn_id,
                    %call_id,
                    %tool_name,
                    args_bytes = arguments.len(),
                    "④编排层：待工具跟进"
                );
                self.apply(OrchestratorSignal::ToolDispatched)
                    .map_err(|e| e.to_string())
            }
            StepOutcome::Failed { .. } => self
                .apply(OrchestratorSignal::TurnAborted)
                .map_err(|e| e.to_string()),
            StepOutcome::Text(_) | StepOutcome::NoReply => self.end_user_turn(),
        }
    }

    /// 工具结果已入队，从 `AwaitingTool` 回到 `RunningTurn`（M4-5）。
    pub fn tool_result_ready(&mut self) -> Result<(), String> {
        self.apply(OrchestratorSignal::ToolResultReady)
            .map_err(|e| e.to_string())
    }

    /// 流水线失败时中止本轮。
    pub fn abort_user_turn(&mut self) -> Result<(), String> {
        if matches!(
            self.state,
            OrchestratorState::RunningTurn { .. } | OrchestratorState::AwaitingTool { .. }
        ) {
            self.apply(OrchestratorSignal::TurnAborted)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 请求关闭；`RunningTurn` 时需先结束或取消当前轮（M4-7）。
    pub fn request_shutdown(&mut self) -> Result<(), String> {
        self.apply(OrchestratorSignal::ShutdownRequested)
            .map_err(|e| e.to_string())
    }

    pub fn complete_shutdown(&mut self) -> Result<(), String> {
        self.apply(OrchestratorSignal::ShutdownComplete)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_to_running_to_idle() {
        let mut o = Orchestrator::new(SessionId::new("s1"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        assert!(matches!(o.state(), OrchestratorState::RunningTurn { .. }));
        o.end_user_turn().unwrap();
        assert!(o.is_idle());
    }

    #[test]
    fn shutdown_lifecycle() {
        let mut o = Orchestrator::new(SessionId::new("s2"));
        o.request_shutdown().unwrap();
        assert!(matches!(o.state(), OrchestratorState::ShuttingDown));
        o.complete_shutdown().unwrap();
        assert!(matches!(o.state(), OrchestratorState::Ended));
    }

    #[test]
    fn cannot_start_turn_while_running() {
        let mut o = Orchestrator::new(SessionId::new("s3"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        assert!(o.begin_user_turn(TurnId::FIRST.next()).is_err());
        o.end_user_turn().unwrap();
    }

    #[test]
    fn tool_round_trip_placeholder() {
        let mut o = Orchestrator::new(SessionId::new("s4"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        o.apply(OrchestratorSignal::ToolDispatched).unwrap();
        assert!(matches!(o.state(), OrchestratorState::AwaitingTool { .. }));
        o.apply(OrchestratorSignal::ToolResultReady).unwrap();
        assert!(matches!(o.state(), OrchestratorState::RunningTurn { .. }));
        o.end_user_turn().unwrap();
    }

    #[test]
    fn turn_aborted_from_running() {
        let mut o = Orchestrator::new(SessionId::new("s5"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        o.apply(OrchestratorSignal::TurnAborted).unwrap();
        assert!(o.is_idle());
    }

    #[test]
    fn reject_shutdown_while_running() {
        let mut o = Orchestrator::new(SessionId::new("s6"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        assert!(o.request_shutdown().is_err());
    }

    #[test]
    fn finish_turn_with_pending_tool_keeps_awaiting() {
        use crate::{StepOutcome, TurnFinished};

        let mut o = Orchestrator::new(SessionId::new("s7"));
        o.begin_user_turn(TurnId::FIRST).unwrap();
        let finished = TurnFinished::new(
            TurnId::FIRST,
            StepOutcome::PendingTool {
                call_id: "c1".into(),
                tool_name: "read_file".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
                assistant_content: "{}".into(),
            },
        );
        o.finish_turn_with(&finished).unwrap();
        assert!(matches!(o.state(), OrchestratorState::AwaitingTool { .. }));
    }
}
