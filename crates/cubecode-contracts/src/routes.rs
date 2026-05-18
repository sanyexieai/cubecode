//! **③ 调度层** → **④ 编排层** 的路由提示。

use serde::{Deserialize, Serialize};

/// 编排层应走的流程分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteHint {
    /// 用户聊天轮次（原 `UserTurn`）。
    ChatTurn,
    /// 结束会话。
    Exit,
    /// 工具执行结果回灌后的后续轮次（占位，M4 接线）。
    ToolFollowUp,
    /// 子 Agent / 委派任务（占位）。
    SubAgent,
}

impl RouteHint {
    /// 是否需要进入 **⑤ 执行层**（`Exit` 为 false）。
    pub fn needs_step(self) -> bool {
        matches!(
            self,
            RouteHint::ChatTurn | RouteHint::ToolFollowUp | RouteHint::SubAgent
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_hint_json_roundtrip() {
        for hint in [
            RouteHint::ChatTurn,
            RouteHint::Exit,
            RouteHint::ToolFollowUp,
            RouteHint::SubAgent,
        ] {
            let json = serde_json::to_string(&hint).expect("serialize");
            let back: RouteHint = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, hint);
        }
    }

    #[test]
    fn needs_step_only_exit_is_false() {
        assert!(RouteHint::ChatTurn.needs_step());
        assert!(!RouteHint::Exit.needs_step());
        assert!(RouteHint::ToolFollowUp.needs_step());
    }
}
