use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider not found: {0}")]
    ProviderNotFound(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("provider failure: {0}")]
    ProviderFailure(String),
}
