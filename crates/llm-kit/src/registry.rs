use std::collections::BTreeMap;
use std::thread;

use crate::error::LlmError;
use crate::provider::LlmProvider;
use crate::retry::LlmRetryPolicy;
use crate::types::{GenerateRequest, GenerateResponse, ProviderInfo, StreamChunk};

pub struct ProviderRegistry {
    providers: BTreeMap<String, Box<dyn LlmProvider>>,
    retry_policy: LlmRetryPolicy,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: BTreeMap::new(),
            retry_policy: LlmRetryPolicy::from_env(),
        }
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_retry_policy(&mut self, policy: LlmRetryPolicy) {
        self.retry_policy = policy;
    }

    pub fn retry_policy(&self) -> LlmRetryPolicy {
        self.retry_policy
    }

    pub fn register<P>(&mut self, provider: P)
    where
        P: LlmProvider + 'static,
    {
        let id = provider.info().id;
        self.providers.insert(id, Box::new(provider));
    }

    pub fn provider(&self, id: &str) -> Option<&dyn LlmProvider> {
        self.providers.get(id).map(|provider| provider.as_ref())
    }

    pub fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        let provider = self
            .provider(&request.model.provider)
            .ok_or_else(|| LlmError::ProviderNotFound(request.model.provider.clone()))?;
        let policy = self.retry_policy;
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            match provider.generate(request) {
                Ok(response) => return Ok(response),
                Err(error) if policy.should_retry(attempt, &error) => {
                    let delay = policy.backoff_for_attempt(attempt);
                    policy.log_retry(attempt, &error, delay);
                    thread::sleep(delay);
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        let provider = self
            .provider(&request.model.provider)
            .ok_or_else(|| LlmError::ProviderNotFound(request.model.provider.clone()))?;
        let policy = self.retry_policy;
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let mut saw_chunk = false;
            let mut wrapped = |chunk: StreamChunk| -> Result<(), LlmError> {
                saw_chunk = true;
                on_chunk(chunk)
            };
            match provider.generate_stream(request, &mut wrapped) {
                Ok(response) => return Ok(response),
                Err(error) if !saw_chunk && policy.should_retry(attempt, &error) => {
                    let delay = policy.backoff_for_attempt(attempt);
                    policy.log_retry(attempt, &error, delay);
                    thread::sleep(delay);
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn provider_infos(&self) -> Vec<ProviderInfo> {
        self.providers
            .values()
            .map(|provider| provider.info())
            .collect()
    }
}
