//! DeepSeek（OpenAI 兼容 Chat Completions）。

use crate::providers::preset::{ProviderPreset, WireProtocol};

pub const ID: &str = "deepseek";

pub const PRESET: ProviderPreset = ProviderPreset {
    id: ID,
    display_name: "DeepSeek Compatible",
    default_base_url: "https://api.deepseek.com",
    balanced_model: "deepseek-v4-flash",
    fast_model: "deepseek-v4-flash",
    coding_model: "deepseek-v4-pro",
    wire: WireProtocol::ChatCompletions,
};

pub const API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
pub const BASE_URL_ENV: &str = "DEEPSEEK_BASE_URL";
