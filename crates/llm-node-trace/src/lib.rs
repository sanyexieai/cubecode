//! Pipeline 节点：在 `before_generate` / `after_generate` 打 [`tracing::debug!`] 点。
//!
//! 过滤日志：`RUST_LOG=llm_node_trace=debug` 或 `tracing` 全局级别。

use llm_kit::{LlmError, PipelineContext, PipelineStage};

const DEFAULT_TARGET: &str = "llm_node_trace";

#[derive(Debug, Default, Clone, Copy)]
pub struct TraceStage;

impl TraceStage {
    pub fn new() -> Self {
        Self
    }
}

impl PipelineStage for TraceStage {
    fn id(&self) -> &'static str {
        "trace"
    }

    fn before_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        tracing::debug!(
            target: DEFAULT_TARGET,
            provider = %ctx.request.model.provider,
            model = %ctx.request.model.model,
            messages = ctx.request.messages.len(),
            "llm pipeline before_generate"
        );
        Ok(())
    }

    fn after_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        let chars = ctx
            .response
            .as_ref()
            .map(|r| r.message.content.chars().count())
            .unwrap_or(0);
        tracing::debug!(
            target: DEFAULT_TARGET,
            assistant_chars = chars,
            "llm pipeline after_generate"
        );
        Ok(())
    }
}
