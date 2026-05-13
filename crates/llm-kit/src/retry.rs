use std::env;
use std::time::Duration;

use crate::error::LlmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmRetryPolicy {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
    pub log_retries: bool,
}

impl Default for LlmRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 250,
            log_retries: false,
        }
    }
}

impl LlmRetryPolicy {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            max_attempts: env_usize_any(&["LLM_RETRY_MAX_ATTEMPTS", "HC_LLM_RETRY_MAX_ATTEMPTS"])
                .filter(|value| *value > 0)
                .unwrap_or(defaults.max_attempts),
            base_delay_ms: env_u64_any(&["LLM_RETRY_BASE_DELAY_MS", "HC_LLM_RETRY_BASE_DELAY_MS"])
                .unwrap_or(defaults.base_delay_ms),
            log_retries: env_bool_any(&["LLM_RETRY_LOG", "HC_LLM_RETRY_LOG"])
                .unwrap_or(defaults.log_retries),
        }
    }

    pub fn should_retry(&self, attempt: usize, error: &LlmError) -> bool {
        attempt < self.max_attempts && is_retryable_llm_error(error)
    }

    pub fn backoff_for_attempt(&self, attempt: usize) -> Duration {
        let multiplier = 1u64
            .checked_shl(attempt.saturating_sub(1).min(63) as u32)
            .unwrap_or(u64::MAX);
        Duration::from_millis(self.base_delay_ms.saturating_mul(multiplier))
    }

    pub fn log_retry(&self, attempt: usize, error: &LlmError, delay: Duration) {
        if !self.log_retries {
            return;
        }
        tracing::warn!(
            attempt,
            max_attempts = self.max_attempts,
            error = %error,
            retry_delay_ms = delay.as_millis(),
            "llm retry scheduled"
        );
    }
}

pub fn is_retryable_llm_error(error: &LlmError) -> bool {
    match error {
        LlmError::ProviderFailure(message) => is_retryable_provider_failure_message(message),
        LlmError::ProviderNotFound(_) | LlmError::InvalidRequest(_) => false,
    }
}

pub fn is_retryable_provider_failure_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "http 408", "http 409", "http 425", "http 429", "http 500", "http 502", "http 503",
        "http 504", "http 529",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || lower.contains("overloaded_error")
        || lower.contains("rate limit")
        || lower.contains("please retry")
        || lower.contains("retry later")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("temporary failure")
}

fn env_usize(key: &str) -> Option<usize> {
    env::var(key).ok()?.trim().parse().ok()
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok()?.trim().parse().ok()
}

fn env_bool(key: &str) -> Option<bool> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_usize_any(keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| env_usize(key))
}

fn env_u64_any(keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| env_u64(key))
}

fn env_bool_any(keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| env_bool(key))
}
