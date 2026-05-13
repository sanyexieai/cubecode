//! 通用可插拔 LLM 核心：OpenAI 兼容 Chat Completions、流式输出、重试与注册表。
//!
//! 环境变量优先读取 `LLM_*`，并兼容 Honeycomb 风格的 `HC_LLM_*`。

mod env;
mod error;
mod openai;
mod provider;
mod registry;
mod retry;
mod sanitize;
mod types;

pub use env::{
    default_base_url_for_provider, default_model_for_provider, default_model_from_env,
    default_provider_from_env, provider_api_key_env_source, provider_api_key_from_env,
    provider_api_key_var_name, provider_base_url_env_source, provider_base_url_from_env,
    provider_base_url_var_name, provider_preset, provider_presets, provider_requires_api_key,
    ProviderPreset,
};
pub use error::LlmError;
pub use openai::OpenAiCompatibleProvider;
pub use provider::LlmProvider;
pub use registry::ProviderRegistry;
pub use retry::{is_retryable_llm_error, is_retryable_provider_failure_message, LlmRetryPolicy};
pub use sanitize::{sanitize_assistant_text, strip_assistant_hidden_blocks};
pub use types::{
    ChatMessage, FinishReason, GenerateRequest, GenerateResponse, MessageRole, ModelRef,
    ProviderInfo, StreamChunk, TokenUsage,
};

#[cfg(test)]
pub(crate) use openai::{content_to_string, message_content_to_string, OpenAiChatResponse};

pub fn default_registry_from_env() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    let provider_id = default_provider_from_env();
    let api_key = provider_api_key_from_env(&provider_id);
    let base_url = provider_base_url_from_env(&provider_id);

    if let Some(api_key) = api_key {
        if let Ok(provider) = OpenAiCompatibleProvider::new(
            provider_id.clone(),
            provider_preset(&provider_id)
                .map(|preset| preset.display_name.to_owned())
                .unwrap_or_else(|| format!("{provider_id} compatible")),
            base_url,
            api_key,
        ) {
            registry.register(provider);
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
