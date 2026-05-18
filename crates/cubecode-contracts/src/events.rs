//! 进入 **② 收件箱** 的语义事件（serde 可序列化）。

use serde::{Deserialize, Serialize};

use crate::{SessionId, TurnContext, TurnId};

/// [`ControlEvent`] 变体种类（③ 路由表键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlEventKind {
    UserTurn,
    Shutdown,
    ToolResult,
}

/// 进入 inbox 的语义事件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlEvent {
    /// 用户提交一轮对话（取代原 `UserLine(String)`）。
    UserTurn {
        session_id: SessionId,
        turn_id: TurnId,
        text: String,
    },
    /// 请求结束会话。
    Shutdown {
        session_id: SessionId,
    },
    /// 预留：工具执行结果回灌 inbox（M4 接线；路由见 M1-3 `ToolFollowUp`）。
    ToolResult {
        session_id: SessionId,
        turn_id: TurnId,
        call_id: String,
        output: String,
    },
}

impl ControlEvent {
    pub fn user_turn(ctx: &TurnContext, text: impl Into<String>) -> Self {
        Self::UserTurn {
            session_id: ctx.session_id.clone(),
            turn_id: ctx.turn_id,
            text: text.into(),
        }
    }

    pub fn shutdown(session_id: SessionId) -> Self {
        Self::Shutdown { session_id }
    }

    pub fn tool_result(
        ctx: &TurnContext,
        call_id: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        Self::ToolResult {
            session_id: ctx.session_id.clone(),
            turn_id: ctx.turn_id,
            call_id: call_id.into(),
            output: output.into(),
        }
    }

    /// 用户轮次正文；非 `UserTurn` 时返回 `None`。
    pub fn user_text(&self) -> Option<&str> {
        match self {
            Self::UserTurn { text, .. } => Some(text.as_str()),
            _ => None,
        }
    }

    /// 事件种类（用于 ③ 路由表查找）。
    pub fn kind(&self) -> ControlEventKind {
        match self {
            Self::UserTurn { .. } => ControlEventKind::UserTurn,
            Self::Shutdown { .. } => ControlEventKind::Shutdown,
            Self::ToolResult { .. } => ControlEventKind::ToolResult,
        }
    }

    /// 事件所属会话（所有变体均携带 `session_id`）。
    pub fn session_id(&self) -> &SessionId {
        match self {
            Self::UserTurn { session_id, .. }
            | Self::Shutdown { session_id }
            | Self::ToolResult { session_id, .. } => session_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_turn_json_roundtrip() {
        let event = ControlEvent::UserTurn {
            session_id: SessionId::generate(),
            turn_id: TurnId::FIRST,
            text: "你好".into(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("user_turn"));
        let back: ControlEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, event);
    }

    #[test]
    fn shutdown_json_roundtrip() {
        let event = ControlEvent::shutdown(SessionId::generate());
        let back: ControlEvent =
            serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn kind_matches_variant() {
        let sid = SessionId::new("sess-kind");
        let ctx = TurnContext::new(sid.clone(), TurnId::FIRST);
        assert_eq!(
            ControlEvent::user_turn(&ctx, "x").kind(),
            ControlEventKind::UserTurn
        );
        assert_eq!(
            ControlEvent::shutdown(sid.clone()).kind(),
            ControlEventKind::Shutdown
        );
        assert_eq!(
            ControlEvent::ToolResult {
                session_id: sid,
                turn_id: TurnId::FIRST,
                call_id: "c".into(),
                output: "".into(),
            }
            .kind(),
            ControlEventKind::ToolResult
        );
    }

    #[test]
    fn session_id_on_all_variants() {
        let sid = SessionId::generate();
        let ctx = TurnContext::new(sid.clone(), TurnId::FIRST);
        assert_eq!(ControlEvent::user_turn(&ctx, "x").session_id(), &sid);
        assert_eq!(ControlEvent::shutdown(sid.clone()).session_id(), &sid);
        assert_eq!(
            ControlEvent::ToolResult {
                session_id: sid.clone(),
                turn_id: TurnId::FIRST,
                call_id: "c".into(),
                output: "".into(),
            }
            .session_id(),
            &sid
        );
    }

    #[test]
    fn tool_result_placeholder_serializes() {
        let event = ControlEvent::ToolResult {
            session_id: SessionId::generate(),
            turn_id: TurnId::FIRST,
            call_id: "call-1".into(),
            output: "{}".into(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("tool_result"));
    }
}
