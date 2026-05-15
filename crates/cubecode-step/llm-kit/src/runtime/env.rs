use std::env;

use crate::providers;

pub fn default_provider_from_env() -> String {
    if let Some(provider) = env_var_any(&["LLM_PROVIDER", "HC_LLM_PROVIDER"]) {
        if !provider.trim().eq_ignore_ascii_case("mock") {
            return provider.trim().to_owned();
        }
    }

    if let Some(id) = providers::infer_provider_id_from_env_keys() {
        return id.to_owned();
    }

    providers::vendors::openai::ID.to_owned()
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
        .or_else(|| non_empty_env_source(providers::provider_api_key_var_name(provider_id)))
}

pub fn provider_base_url_env_source(provider_id: &str) -> Option<&'static str> {
    non_empty_env_source_any(&["LLM_BASE_URL", "HC_LLM_BASE_URL"])
        .or_else(|| non_empty_env_source(providers::provider_base_url_var_name(provider_id)))
}

pub fn provider_requires_api_key(provider_id: &str) -> bool {
    !provider_id.trim().eq_ignore_ascii_case("ollama")
}

pub fn default_base_url_for_provider(provider: &str) -> String {
    providers::provider_preset(provider)
        .map(|preset| preset.default_base_url.to_owned())
        .unwrap_or_else(|| providers::vendors::openai::PRESET.default_base_url.to_owned())
}

pub fn default_model_for_provider(provider: &str, model_type: &str) -> String {
    if let Some(preset) = providers::provider_preset(provider) {
        return match model_type {
            "fast" => preset.fast_model.to_owned(),
            "coding" => preset.coding_model.to_owned(),
            _ => preset.balanced_model.to_owned(),
        };
    }

    match model_type {
        "fast" => providers::vendors::openai::PRESET.fast_model.to_owned(),
        "coding" => providers::vendors::openai::PRESET.coding_model.to_owned(),
        _ => providers::vendors::openai::PRESET.balanced_model.to_owned(),
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
