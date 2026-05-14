//! 链尾兜底：若 [`llm_kit::flow::FlowContext::response`] 仍为 `None`，写入占位助手消息（可配置文案）。
//!
//! 建议放在 [`llm_kit::flow::FlowPipeline`] **最后一个**节点（builder 里最后 `push`）。
//!
//! **注意**：应用与节点 crate 应对 `llm-kit` 使用**同一条** path/workspace 依赖，否则会出现「两份 `llm-kit`」导致 [`FlowNode`] 无法为同一类型实现。`llm-kit` 自身集成测试改为内联节点，不依赖本 crate。

use llm_kit::{
    flow::{FlowContext, FlowNode},
    ChatMessage, FinishReason, GenerateResponse, LlmError, MessageRole,
};

#[derive(Debug, Clone)]
pub struct DefaultOutputNode {
    pub placeholder: String,
}

impl Default for DefaultOutputNode {
    fn default() -> Self {
        Self {
            placeholder: "(no output node produced assistant content)".to_owned(),
        }
    }
}

impl DefaultOutputNode {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_placeholder(placeholder: impl Into<String>) -> Self {
        Self {
            placeholder: placeholder.into(),
        }
    }
}

impl FlowNode for DefaultOutputNode {
    fn id(&self) -> &'static str {
        "default_output"
    }

    fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
        if ctx.response.is_some() {
            return Ok(());
        }
        ctx.response = Some(GenerateResponse {
            model: ctx.request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, self.placeholder.clone()),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        });
        Ok(())
    }
}
