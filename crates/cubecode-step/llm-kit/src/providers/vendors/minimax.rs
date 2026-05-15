//! MiniMax（OpenAI 兼容 Chat Completions）。

use crate::providers::preset::{ProviderPreset, WireProtocol};

pub const ID: &str = "minimax";

pub const PRESET: ProviderPreset = ProviderPreset {
    id: ID,
    display_name: "MiniMax Compatible",
    default_base_url: "https://api.minimaxi.com/v1",
    balanced_model: "MiniMax-M2.5",
    fast_model: "MiniMax-M2.5-HighSpeed",
    coding_model: "MiniMax-M2.1",
    wire: WireProtocol::ChatCompletions,
};

pub const API_KEY_ENV: &str = "MINIMAX_API_KEY";
pub const BASE_URL_ENV: &str = "MINIMAX_BASE_URL";
