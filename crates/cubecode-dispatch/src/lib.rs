//! ③ **Dispatcher**：占位路由表。

use cubecode_contracts::{ControlEvent, RouteHint};

pub fn route(event: &ControlEvent) -> RouteHint {
    match event {
        ControlEvent::UserLine(_) => RouteHint::UserTurn,
        ControlEvent::Shutdown => RouteHint::Exit,
    }
}
