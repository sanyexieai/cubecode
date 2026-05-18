//! ③ **调度层**：可注册路由表，将 [`ControlEvent`] 映射为 [`RouteHint`]。

use std::collections::HashMap;

use cubecode_contracts::{ControlEvent, ControlEventKind, RouteHint, TurnContext};

/// 内置默认路由条目（`with_defaults` / 文档 / 测试共用）。
pub const DEFAULT_ROUTES: &[(ControlEventKind, RouteHint)] = &[
    (ControlEventKind::UserTurn, RouteHint::ChatTurn),
    (ControlEventKind::Shutdown, RouteHint::Exit),
    (ControlEventKind::ToolResult, RouteHint::ToolFollowUp),
];

/// 路由表中没有对应 [`ControlEventKind`] 的条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnregisteredRoute(pub ControlEventKind);

impl std::fmt::Display for UnregisteredRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "未注册的路由：{:?}", self.0)
    }
}

impl std::error::Error for UnregisteredRoute {}

/// 可注册的事件种类 → 路由意图表。
#[derive(Debug, Clone)]
pub struct Router {
    table: HashMap<ControlEventKind, RouteHint>,
}

impl Router {
    /// 空表；需自行 [`register`](Self::register) 或 [`with_defaults`](Self::with_defaults)。
    pub fn new() -> Self {
        Self {
            table: HashMap::new(),
        }
    }

    /// 内置默认映射（与 M1 硬编码行为一致）。
    pub fn with_defaults() -> Self {
        let mut router = Self::new();
        for &(kind, hint) in DEFAULT_ROUTES {
            router.register(kind, hint);
        }
        router
    }

    /// 注册或覆盖某事件种类对应的路由意图。
    pub fn register(&mut self, kind: ControlEventKind, hint: RouteHint) -> &mut Self {
        self.table.insert(kind, hint);
        self
    }

    /// 是否已注册该事件种类。
    pub fn contains(&self, kind: ControlEventKind) -> bool {
        self.table.contains_key(&kind)
    }

    /// 查表路由；未注册时返回 [`UnregisteredRoute`]。
    pub fn try_route(
        &self,
        turn_ctx: &TurnContext,
        event: &ControlEvent,
    ) -> Result<RouteHint, UnregisteredRoute> {
        let kind = event.kind();
        let hint = self
            .table
            .get(&kind)
            .copied()
            .ok_or(UnregisteredRoute(kind))?;
        tracing::info!(
            target: "cubecode.dispatch",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            ?kind,
            ?event,
            ?hint,
            "③调度层：路由完成"
        );
        Ok(hint)
    }

    /// 查表路由；未注册时 panic（仅用于已填满默认表的生产路径）。
    pub fn route(&self, turn_ctx: &TurnContext, event: &ControlEvent) -> RouteHint {
        self.try_route(turn_ctx, event)
            .unwrap_or_else(|e| panic!("{e}"))
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 使用默认路由表查表（CLI / 编排层常用入口）。
pub fn route(turn_ctx: &TurnContext, event: &ControlEvent) -> RouteHint {
    Router::default().route(turn_ctx, event)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use cubecode_contracts::{SessionId, TurnId};

    fn sample_events(session: SessionId, ctx: &TurnContext) -> Vec<(ControlEvent, RouteHint)> {
        vec![
            (
                ControlEvent::user_turn(ctx, "用户消息"),
                RouteHint::ChatTurn,
            ),
            (ControlEvent::shutdown(session.clone()), RouteHint::Exit),
            (
                ControlEvent::ToolResult {
                    session_id: session,
                    turn_id: ctx.turn_id,
                    call_id: "call-42".into(),
                    output: r#"{"ok":true}"#.into(),
                },
                RouteHint::ToolFollowUp,
            ),
        ]
    }

    /// M2-4：三种事件种类在默认表下分别落到不同 `RouteHint`。
    #[test]
    fn multi_event_types_route_to_distinct_hints() {
        let router = Router::with_defaults();
        let session = SessionId::new("sess-m2-4-multi");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut distinct_hints = HashSet::new();

        for (event, expected) in sample_events(session, &ctx) {
            assert_eq!(event.kind(), expected_event_kind(expected));
            let hint = router.route(&ctx, &event);
            assert_eq!(hint, expected, "事件 {:?} 应路由到 {:?}", event.kind(), expected);
            distinct_hints.insert(hint);
        }

        assert_eq!(
            distinct_hints.len(),
            3,
            "UserTurn / Shutdown / ToolResult 应对应三种不同 hint"
        );
    }

    fn expected_event_kind(hint: RouteHint) -> ControlEventKind {
        DEFAULT_ROUTES
            .iter()
            .find(|&&(_, h)| h == hint)
            .map(|(k, _)| *k)
            .expect("test helper: unknown hint")
    }

    /// M2-4：连续出队式路由时，hint 随事件种类切换（模拟 ②→③）。
    #[test]
    fn sequential_events_change_route_hint() {
        let router = Router::with_defaults();
        let session = SessionId::new("sess-m2-4-seq");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let events: Vec<ControlEvent> = sample_events(session, &ctx)
            .into_iter()
            .map(|(e, _)| e)
            .collect();

        let hints: Vec<RouteHint> = events
            .iter()
            .map(|ev| router.route(&ctx, ev))
            .collect();

        assert_eq!(
            hints,
            vec![
                RouteHint::ChatTurn,
                RouteHint::Exit,
                RouteHint::ToolFollowUp,
            ]
        );
    }

    /// M2-4：默认表条目与 `DEFAULT_ROUTES` 一致且全覆盖。
    #[test]
    fn default_table_covers_all_event_kinds() {
        let router = Router::with_defaults();
        for &(kind, hint) in DEFAULT_ROUTES {
            assert!(router.contains(kind));
            let session = SessionId::new("sess-m2-4-cover");
            let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
            let event = match kind {
                ControlEventKind::UserTurn => ControlEvent::user_turn(&ctx, "x"),
                ControlEventKind::Shutdown => ControlEvent::shutdown(session),
                ControlEventKind::ToolResult => ControlEvent::ToolResult {
                    session_id: ctx.session_id.clone(),
                    turn_id: ctx.turn_id,
                    call_id: "c".into(),
                    output: "".into(),
                },
            };
            assert_eq!(router.route(&ctx, &event), hint);
        }
    }

    /// M2-4：路由结果与 `RouteHint::needs_step` 语义一致。
    #[test]
    fn routed_hints_match_needs_step_contract() {
        let router = Router::with_defaults();
        let session = SessionId::new("sess-m2-4-needs-step");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);

        for (event, expected) in sample_events(session, &ctx) {
            let hint = router.route(&ctx, &event);
            assert_eq!(hint, expected);
            match expected {
                RouteHint::Exit => assert!(!hint.needs_step(), "退出不应进 ⑤"),
                RouteHint::ChatTurn | RouteHint::ToolFollowUp | RouteHint::SubAgent => {
                    assert!(hint.needs_step(), "{expected:?} 应进 ⑤");
                }
            }
        }
    }

    #[test]
    fn register_overrides_hint() {
        let mut router = Router::with_defaults();
        router.register(ControlEventKind::UserTurn, RouteHint::SubAgent);
        let ctx = TurnContext::new(SessionId::new("sess-override"), TurnId::FIRST);
        assert_eq!(
            router.route(&ctx, &ControlEvent::user_turn(&ctx, "x")),
            RouteHint::SubAgent
        );
        assert_eq!(
            router.route(&ctx, &ControlEvent::shutdown(ctx.session_id.clone())),
            RouteHint::Exit
        );
        assert_eq!(
            router.route(
                &ctx,
                &ControlEvent::ToolResult {
                    session_id: ctx.session_id.clone(),
                    turn_id: ctx.turn_id,
                    call_id: "c".into(),
                    output: "".into(),
                },
            ),
            RouteHint::ToolFollowUp
        );
    }

    #[test]
    fn try_route_errors_when_unregistered() {
        let router = Router::new();
        let ctx = TurnContext::new(SessionId::new("sess-empty"), TurnId::FIRST);
        let err = router
            .try_route(&ctx, &ControlEvent::user_turn(&ctx, "a"))
            .expect_err("empty table");
        assert_eq!(err.0, ControlEventKind::UserTurn);
    }

    /// M2-4：顶层 `route()` 对三种事件均走默认表。
    #[test]
    fn free_function_routes_all_event_kinds() {
        let session = SessionId::new("sess-free-fn");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        for (event, expected) in sample_events(session, &ctx) {
            assert_eq!(route(&ctx, &event), expected);
        }
    }
}
