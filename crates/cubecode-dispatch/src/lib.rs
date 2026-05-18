//! ③ **调度层**：事件类型 → 路由意图。

use cubecode_contracts::{ControlEvent, RouteHint};

pub fn route(event: &ControlEvent) -> RouteHint {
    let hint = match event {
        ControlEvent::UserLine(_) => RouteHint::UserTurn,
        ControlEvent::Shutdown => RouteHint::Exit,
    };
    tracing::info!(
        target: "cubecode.dispatch",
        ?event,
        ?hint,
        "③调度层：路由完成"
    );
    hint
}
