//! ④ **Orchestrator**：占位编排——根据 [`cubecode_contracts::RouteHint`] 调用 **⑤** 再交给 **⑥**。

use cubecode_contracts::{ControlEvent, RouteHint};
use cubecode_sink::emit_line;
use cubecode_step::placeholder_turn;

/// 最小闭环：**④ → ⑤ → ⑥**（路由由上游 `dispatch` 产生）。
pub fn run_minimal_pipeline(route: RouteHint, event: &ControlEvent) -> Result<(), String> {
    match route {
        RouteHint::Exit => {
            emit_line("sink", "orchestrator: exit route (no step)");
            Ok(())
        }
        RouteHint::UserTurn => {
            let body = placeholder_turn(event)?;
            emit_line("sink", &body);
            Ok(())
        }
    }
}
