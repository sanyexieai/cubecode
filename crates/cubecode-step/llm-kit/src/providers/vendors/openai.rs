//! OpenAI 官方 API，以及通过 `OPENAI_BASE_URL` 指向的任意 OpenAI 兼容服务。

use std::env;

use crate::providers::preset::{ProviderPreset, WireProtocol};

pub const ID: &str = "openai";

pub const PRESET: ProviderPreset = ProviderPreset {
    id: ID,
    display_name: "OpenAI Compatible",
    default_base_url: "https://api.openai.com/v1",
    balanced_model: "gpt-4.1-mini",
    fast_model: "gpt-4.1-mini",
    coding_model: "gpt-4.1",
    wire: WireProtocol::ChatCompletions,
};

pub const API_KEY_ENV: &str = "OPENAI_API_KEY";
pub const BASE_URL_ENV: &str = "OPENAI_BASE_URL";

/// `LLM_*` / Honeycomb 风格密钥，或直连 OpenAI 的 `OPENAI_API_KEY`。
pub fn shared_llm_keys_present() -> bool {
    env::var("LLM_API_KEY").is_ok()
        || env::var("HC_LLM_API_KEY").is_ok()
        || env::var(API_KEY_ENV).is_ok()
}
