//! 跨 **①～⑥** 的最小契约：事件与路由提示。业务变体后续再扩展。

/// 进入 inbox 的语义事件（占位级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEvent {
    UserLine(String),
    Shutdown,
}

/// **③ Dispatcher** 的输出：供 **④ Orchestrator** 决策（占位级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteHint {
    UserTurn,
    Exit,
}
