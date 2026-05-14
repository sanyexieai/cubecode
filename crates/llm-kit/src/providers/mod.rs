//! 模型厂商接入入口。
//!
//! ## 线路协议（wire）
//!
//! | 协议 | 实现模块 | 典型厂商 |
//! |------|----------|----------|
//! | Chat Completions（OpenAI 兼容形） | [`protocols::chat_completions`] | OpenAI、MiniMax、DeepSeek |
//! | Anthropic Messages | [`protocols::anthropic_messages`] | Claude（Anthropic） |
//!
//! 各厂商默认 URL、模型名、环境变量见 [`vendors`]；[`ProviderPreset::wire`] 决定用哪套协议。
//!
//! 新增厂商时：在 [`vendors`] 下增加模块，在 [`provider_presets`] 中注册，并设置正确的 [`WireProtocol`]。

pub mod protocols;
mod preset;

pub mod vendors;

pub use preset::{ProviderPreset, WireProtocol};
pub use protocols::{AnthropicMessagesProvider, OpenAiCompatibleProvider, ProtocolBinding};

use std::env;

use vendors::{anthropic, deepseek, minimax, openai};

/// 所有内置厂商预设（顺序即展示/遍历顺序）。
pub fn provider_presets() -> &'static [ProviderPreset] {
    &[
        openai::PRESET,
        minimax::PRESET,
        deepseek::PRESET,
        anthropic::PRESET,
    ]
}

pub fn provider_preset(provider: &str) -> Option<&'static ProviderPreset> {
    provider_presets()
        .iter()
        .find(|preset| preset.id.eq_ignore_ascii_case(provider.trim()))
}

pub fn provider_api_key_var_name(provider_id: &str) -> &'static str {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        id if id == minimax::ID => minimax::API_KEY_ENV,
        id if id == deepseek::ID => deepseek::API_KEY_ENV,
        id if id == anthropic::ID => anthropic::API_KEY_ENV,
        _ => openai::API_KEY_ENV,
    }
}

pub fn provider_base_url_var_name(provider_id: &str) -> &'static str {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        id if id == minimax::ID => minimax::BASE_URL_ENV,
        id if id == deepseek::ID => deepseek::BASE_URL_ENV,
        id if id == anthropic::ID => anthropic::BASE_URL_ENV,
        _ => openai::BASE_URL_ENV,
    }
}

/// 未显式设置 `LLM_PROVIDER` 时，根据已存在的环境变量猜测默认厂商 id。
pub(crate) fn infer_provider_id_from_env_keys() -> Option<&'static str> {
    if openai::shared_llm_keys_present() {
        return Some(openai::ID);
    }
    if env::var(minimax::API_KEY_ENV).is_ok() {
        return Some(minimax::ID);
    }
    if env::var(deepseek::API_KEY_ENV).is_ok() {
        return Some(deepseek::ID);
    }
    if env::var(anthropic::API_KEY_ENV).is_ok() {
        return Some(anthropic::ID);
    }
    None
}
