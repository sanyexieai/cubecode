use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub balanced_model: &'static str,
    pub fast_model: &'static str,
    pub coding_model: &'static str,
}

pub fn provider_presets() -> &'static [ProviderPreset] {
    &[
        ProviderPreset {
            id: "openai",
            display_name: "OpenAI Compatible",
            default_base_url: "https://api.openai.com/v1",
            balanced_model: "gpt-4.1-mini",
            fast_model: "gpt-4.1-mini",
            coding_model: "gpt-4.1",
        },
        ProviderPreset {
            id: "minimax",
            display_name: "MiniMax Compatible",
            default_base_url: "https://api.minimaxi.com/v1",
            balanced_model: "MiniMax-M2.5",
            fast_model: "MiniMax-M2.5-HighSpeed",
            coding_model: "MiniMax-M2.1",
        },
        ProviderPreset {
            id: "deepseek",
            display_name: "DeepSeek Compatible",
            default_base_url: "https://api.deepseek.com",
            balanced_model: "deepseek-v4-flash",
            fast_model: "deepseek-v4-flash",
            coding_model: "deepseek-v4-pro",
        },
    ]
}

pub fn provider_preset(provider: &str) -> Option<&'static ProviderPreset> {
    provider_presets()
        .iter()
        .find(|preset| preset.id.eq_ignore_ascii_case(provider.trim()))
}

pub fn default_provider_from_env() -> String {
    if let Some(provider) = env_var_any(&["LLM_PROVIDER", "HC_LLM_PROVIDER"]) {
        if !provider.trim().eq_ignore_ascii_case("mock") {
            return provider.trim().to_owned();
        }
    }

    if env::var("LLM_API_KEY").is_ok()
        || env::var("HC_LLM_API_KEY").is_ok()
        || env::var("OPENAI_API_KEY").is_ok()
    {
        return "openai".to_owned();
    }

    if env::var("MINIMAX_API_KEY").is_ok() {
        return "minimax".to_owned();
    }

    if env::var("DEEPSEEK_API_KEY").is_ok() {
        return "deepseek".to_owned();
    }

    "openai".to_owned()
}

pub fn default_model_from_env() -> String {
    let provider = default_provider_from_env();
    if let Some(model) = env_var_any(&["LLM_MODEL", "HC_LLM_MODEL"]) {
        if !using_legacy_mock_config() {
            return model;
        }
    }

    let model_type = env_var_any(&["LLM_MODEL_TYPE", "HC_LLM_MODEL_TYPE"])
        .unwrap_or_else(|| "balanced".to_owned());
    default_model_for_provider(&provider, &model_type)
}

pub fn provider_api_key_from_env(provider_id: &str) -> Option<String> {
    provider_api_key_env_source(provider_id).and_then(|source| env::var(source).ok())
}

pub fn provider_base_url_from_env(provider_id: &str) -> String {
    provider_base_url_env_source(provider_id)
        .and_then(|source| env::var(source).ok())
        .unwrap_or_else(|| default_base_url_for_provider(provider_id))
}

pub fn provider_api_key_env_source(provider_id: &str) -> Option<&'static str> {
    non_empty_env_source_any(&["LLM_API_KEY", "HC_LLM_API_KEY"])
        .or_else(|| non_empty_env_source(provider_api_key_var_name(provider_id)))
}

pub fn provider_base_url_env_source(provider_id: &str) -> Option<&'static str> {
    non_empty_env_source_any(&["LLM_BASE_URL", "HC_LLM_BASE_URL"])
        .or_else(|| non_empty_env_source(provider_base_url_var_name(provider_id)))
}

pub fn provider_requires_api_key(provider_id: &str) -> bool {
    !provider_id.trim().eq_ignore_ascii_case("ollama")
}

pub fn default_base_url_for_provider(provider: &str) -> String {
    provider_preset(provider)
        .map(|preset| preset.default_base_url.to_owned())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_owned())
}

pub fn default_model_for_provider(provider: &str, model_type: &str) -> String {
    if let Some(preset) = provider_preset(provider) {
        return match model_type {
            "fast" => preset.fast_model.to_owned(),
            "coding" => preset.coding_model.to_owned(),
            _ => preset.balanced_model.to_owned(),
        };
    }

    match model_type {
        "fast" => "gpt-4.1-mini".to_owned(),
        "coding" => "gpt-4.1".to_owned(),
        _ => "gpt-4.1-mini".to_owned(),
    }
}

pub fn provider_api_key_var_name(provider_id: &str) -> &'static str {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "minimax" => "MINIMAX_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        _ => "OPENAI_API_KEY",
    }
}

pub fn provider_base_url_var_name(provider_id: &str) -> &'static str {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "minimax" => "MINIMAX_BASE_URL",
        "deepseek" => "DEEPSEEK_BASE_URL",
        _ => "OPENAI_BASE_URL",
    }
}

fn env_var_any(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| env::var(key).ok())
}

fn non_empty_env_source(name: &'static str) -> Option<&'static str> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|_| name)
}

fn non_empty_env_source_any(names: &[&'static str]) -> Option<&'static str> {
    names
        .iter()
        .copied()
        .find(|name| non_empty_env_source(name).is_some())
}

fn using_legacy_mock_config() -> bool {
    env_var_any(&["LLM_PROVIDER", "HC_LLM_PROVIDER"])
        .map(|provider| provider.trim().eq_ignore_ascii_case("mock"))
        .unwrap_or(false)
}
