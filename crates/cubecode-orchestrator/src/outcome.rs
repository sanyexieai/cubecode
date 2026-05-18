//! ⑤ → ④ 结构化结果与轮次收束（M4-2）。

use cubecode_contracts::TurnId;

use crate::tool_call::{parse_tool_call_from_model_output, ParsedToolCall};

/// 单次 **⑤ 执行层** 交给 **④** 的结果种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// 无用户可见正文（如 `Exit` 路由）。
    NoReply,
    /// 助手文本已写出 / 将写出。
    Text(String),
    /// 模型请求调用工具，等待 `ToolResult` 回灌（M4-5 接线）。
    PendingTool {
        call_id: String,
        tool_name: String,
        arguments: String,
        /// 模型原始输出（写回 transcript 的助手消息）。
        assistant_content: String,
    },
    /// ⑤ 或 ④ 判定失败（错误文案已由 ⑥ 展示时可留空）。
    Failed {
        message: String,
    },
}

impl StepOutcome {
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    pub fn needs_tool_follow_up(&self) -> bool {
        matches!(self, Self::PendingTool { .. })
    }

    /// 供 CLI 写入会话历史的助手正文。
    pub fn user_visible_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn from_parsed_tool_call(call: ParsedToolCall, assistant_content: impl Into<String>) -> Self {
        Self::PendingTool {
            call_id: call.call_id,
            tool_name: call.tool_name,
            arguments: call.arguments,
            assistant_content: assistant_content.into(),
        }
    }
}

/// 一轮编排内对 ⑤ 结果的收束（同一 `turn_id` 可多次 step，M4-6 扩展）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFinished {
    pub turn_id: TurnId,
    pub outcome: StepOutcome,
}

impl TurnFinished {
    pub fn new(turn_id: TurnId, outcome: StepOutcome) -> Self {
        Self { turn_id, outcome }
    }

    pub fn no_reply(turn_id: TurnId) -> Self {
        Self::new(turn_id, StepOutcome::NoReply)
    }

    pub fn text(turn_id: TurnId, content: impl Into<String>) -> Self {
        Self::new(turn_id, StepOutcome::Text(content.into()))
    }

    pub fn pending_tool(
        turn_id: TurnId,
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: impl Into<String>,
        assistant_content: impl Into<String>,
    ) -> Self {
        Self::new(
            turn_id,
            StepOutcome::PendingTool {
                call_id: call_id.into(),
                tool_name: tool_name.into(),
                arguments: arguments.into(),
                assistant_content: assistant_content.into(),
            },
        )
    }

    pub fn failed(turn_id: TurnId, message: impl Into<String>) -> Self {
        Self::new(
            turn_id,
            StepOutcome::Failed {
                message: message.into(),
            },
        )
    }

    pub fn user_reply(&self) -> Option<&str> {
        self.outcome.user_visible_text()
    }
}

/// 将 ⑤ 返回的字符串分类为 [`StepOutcome`]。
///
/// 协议（M4-4）：见 [`crate::tool_call::parse_tool_call_from_model_output`]。
pub fn classify_step_result(step_output: &str) -> StepOutcome {
    if let Some(call) = parse_tool_call_from_model_output(step_output) {
        return StepOutcome::from_parsed_tool_call(call, step_output);
    }
    StepOutcome::Text(step_output.to_owned())
}

/// 仅当结果为普通文本时写出 ⑥（工具调用 JSON 不展示给用户）。
pub(crate) fn emit_assistant_if_text(
    turn_ctx: &cubecode_contracts::TurnContext,
    sink: crate::SinkStyle,
    outcome: &StepOutcome,
) {
    if let StepOutcome::Text(body) = outcome {
        crate::emit_body(turn_ctx, sink, "助手", body);
    } else if matches!(outcome, StepOutcome::PendingTool { .. }) {
        tracing::info!(
            target: "cubecode.orchestrator",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            "④编排层：识别为工具调用，跳过助手正文输出"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_plain_text() {
        let o = classify_step_result("你好");
        assert_eq!(o, StepOutcome::Text("你好".into()));
    }

    #[test]
    fn classify_tool_call_poc_json() {
        let json = r#"{"tool_call":{"id":"call-1","name":"read_file","arguments":{"path":"a.txt"}}}"#;
        let o = classify_step_result(json);
        assert_eq!(
            o,
            StepOutcome::PendingTool {
                call_id: "call-1".into(),
                tool_name: "read_file".into(),
                arguments: r#"{"path":"a.txt"}"#.into(),
                assistant_content: json.into(),
            }
        );
    }

    #[test]
    fn turn_finished_user_reply_only_for_text() {
        let t = TurnFinished::text(TurnId::FIRST, "hi");
        assert_eq!(t.user_reply(), Some("hi"));
        let p = TurnFinished::pending_tool(TurnId::FIRST, "c1", "read_file", "{}", "{}");
        assert_eq!(p.user_reply(), None);
    }
}
