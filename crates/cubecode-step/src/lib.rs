//! ⑤ **执行层**：占位或真实 LLM 执行。

use cubecode_contracts::ControlEvent;
use llm_kit::{
    ChatMessage, GenerateRequest, ModelRef, ProviderRegistry,
};

/// ⑤ 调用 LLM 时由编排层注入的上下文。
pub struct LlmStepContext<'a> {
    pub registry: &'a ProviderRegistry,
    pub model: ModelRef,
    pub messages: &'a [ChatMessage],
}

/// 占位：不访问网络，只生成可读的摘要字符串。
pub fn placeholder_turn(event: &ControlEvent) -> Result<String, String> {
    tracing::info!(target: "cubecode.step", ?event, "⑤执行层：进入（占位）");
    let out = match event {
        ControlEvent::UserLine(s) => {
            format!("（执行层占位）用户消息 {} 字节", s.len())
        }
        ControlEvent::Shutdown => {
            tracing::warn!(target: "cubecode.step", "⑤执行层：拒绝关闭事件");
            return Err("shutdown".into());
        }
    };
    tracing::info!(
        target: "cubecode.step",
        bytes = out.len(),
        "⑤执行层：离开（占位完成）"
    );
    Ok(out)
}

/// 真实 LLM：用当前消息列表调用 registry。
pub fn llm_turn(ctx: &LlmStepContext<'_>, event: &ControlEvent) -> Result<String, String> {
    tracing::info!(
        target: "cubecode.step",
        provider = %ctx.model.provider,
        model = %ctx.model.model,
        messages = ctx.messages.len(),
        "⑤执行层：进入（调用大模型）"
    );
    match event {
        ControlEvent::UserLine(_) => {
            let request =
                GenerateRequest::new(ctx.model.clone(), ctx.messages.to_vec());
            let response = ctx
                .registry
                .generate(&request)
                .map_err(|e| e.to_string())?;
            let content = response.message.content;
            tracing::info!(
                target: "cubecode.step",
                out_bytes = content.len(),
                "⑤执行层：离开（大模型返回成功）"
            );
            Ok(content)
        }
        ControlEvent::Shutdown => {
            tracing::warn!(target: "cubecode.step", "⑤执行层：拒绝关闭事件");
            Err("shutdown".into())
        }
    }
}
