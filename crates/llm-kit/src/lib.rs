//! 通用可插拔 LLM 核心：多种线路协议（Chat Completions、Anthropic Messages）、流式、重试与注册表。
//!
//! 厂商与协议见 [`providers`] 模块文档。
//!
//! ## 源码目录（与 `src/` 下文件夹一致）
//! - `core/` — 错误、类型、[`LlmProvider`] 契约
//! - `runtime/` — 环境变量与重试策略
//! - `registry/` — 多后端注册与带重试的调用（可选 pipeline）
//! - `sanitize/` — 助手文本清洗
//! - `providers/` — 厂商预设与线路协议实现
//! - `pipeline/` — 可选「单次 generate + 前后钩子」
//! - `flow/` — 可选「节点链 + 共享 [`LlmProvider`]」，无中心 LLM 调用
//!
//! 环境变量优先读取 `LLM_*`，并兼容 Honeycomb 风格的 `HC_LLM_*`。
//!
//! 库本身只读进程环境，不读取 `.env` 文件；**`apps/llm-cli` 提供的 `llm-kit` 命令** 会在启动时向上查找并加载 `.env`。

mod core;
pub mod flow;
pub mod pipeline;
pub mod providers;
mod registry;
mod runtime;
mod sanitize;

pub use core::error::LlmError;
pub use core::provider::LlmProvider;
pub use core::types::{
    ChatMessage, FinishReason, GenerateRequest, GenerateResponse, MessageRole, ModelRef,
    ProviderInfo, StreamChunk, TokenUsage,
};
pub use flow::{FlowContext, FlowNode, FlowPipeline, FlowPipelineBuilder};
pub use pipeline::{Pipeline, PipelineBuilder, PipelineContext, PipelineStage};
pub use providers::{
    provider_api_key_var_name, provider_base_url_var_name, provider_preset, provider_presets,
    AnthropicMessagesProvider, OpenAiCompatibleProvider, ProtocolBinding, ProviderPreset,
    WireProtocol,
};
pub use registry::ProviderRegistry;
pub use runtime::env::{
    default_base_url_for_provider, default_model_for_provider, default_model_from_env,
    default_provider_from_env, provider_api_key_env_source, provider_api_key_from_env,
    provider_base_url_env_source, provider_base_url_from_env, provider_requires_api_key,
};
pub use runtime::retry::{is_retryable_llm_error, is_retryable_provider_failure_message, LlmRetryPolicy};
pub use sanitize::{sanitize_assistant_text, strip_assistant_hidden_blocks};

#[cfg(test)]
pub(crate) use providers::protocols::chat_completions::{
    content_to_string, message_content_to_string, OpenAiChatResponse,
};

pub fn default_registry_from_env() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let provider_id = default_provider_from_env();
    let api_key = provider_api_key_from_env(&provider_id);
    let base_url = provider_base_url_from_env(&provider_id);

    let Some(api_key) = api_key else {
        return registry;
    };

    let display_name = provider_preset(&provider_id)
        .map(|preset| preset.display_name.to_owned())
        .unwrap_or_else(|| format!("{provider_id} compatible"));

    let wire = provider_preset(&provider_id)
        .map(|preset| preset.wire)
        .unwrap_or(WireProtocol::ChatCompletions);

    match wire {
        WireProtocol::AnthropicMessages => {
            if let Ok(provider) =
                AnthropicMessagesProvider::new(provider_id.clone(), display_name, base_url, api_key)
            {
                registry.register(provider);
            }
        }
        WireProtocol::ChatCompletions => {
            if let Ok(provider) =
                OpenAiCompatibleProvider::new(provider_id.clone(), display_name, base_url, api_key)
            {
                registry.register(provider);
            }
        }
    }

    registry
}

pub fn is_timeout_error(error: &LlmError) -> bool {
    match error {
        LlmError::ProviderFailure(message) => {
            let lowered = message.to_ascii_lowercase();
            lowered.contains("timed out") || lowered.contains("timeout")
        }
        LlmError::ProviderNotFound(_) | LlmError::InvalidRequest(_) => false,
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
