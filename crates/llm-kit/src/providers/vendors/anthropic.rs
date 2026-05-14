//! Anthropic Claude（[Messages API](https://docs.anthropic.com/en/api/messages)）。

use crate::providers::preset::{ProviderPreset, WireProtocol};

pub const ID: &str = "anthropic";

pub const PRESET: ProviderPreset = ProviderPreset {
    id: ID,
    display_name: "Anthropic Messages",
    default_base_url: "https://api.anthropic.com",
    balanced_model: "claude-3-5-sonnet-20241022",
    fast_model: "claude-3-5-haiku-20241022",
    coding_model: "claude-3-5-sonnet-20241022",
    wire: WireProtocol::AnthropicMessages,
};

pub const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
pub const BASE_URL_ENV: &str = "ANTHROPIC_BASE_URL";
