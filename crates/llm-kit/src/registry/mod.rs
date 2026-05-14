//! 多后端注册与带重试的 generate / generate_stream；可选 [`crate::pipeline::Pipeline`]；另有节点驱动的 [`crate::flow`]（[`ProviderRegistry::generate_with_flow`]）。
//!
//! 挂 pipeline 时的调用顺序与语义见 [`crate::pipeline`] 模块文档（**单次** provider + 前后 `PipelineStage` 链）。

use std::collections::BTreeMap;
use std::thread;

use crate::core::error::LlmError;
use crate::core::provider::LlmProvider;
use crate::core::types::{GenerateRequest, GenerateResponse, ProviderInfo, StreamChunk};
use crate::pipeline::{Pipeline, PipelineContext};
use crate::runtime::retry::LlmRetryPolicy;

pub struct ProviderRegistry {
    providers: BTreeMap<String, Box<dyn LlmProvider>>,
    retry_policy: LlmRetryPolicy,
    pipeline: Option<Pipeline>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: BTreeMap::new(),
            retry_policy: LlmRetryPolicy::from_env(),
            pipeline: None,
        }
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置贯穿 generate / generate_stream 的流水线；`None` 或空 [`Pipeline`] 均不改变原有逻辑路径。
    pub fn set_pipeline(&mut self, pipeline: Option<Pipeline>) {
        self.pipeline = pipeline.filter(|p| !p.is_empty());
    }

    pub fn pipeline(&self) -> Option<&Pipeline> {
        self.pipeline.as_ref()
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
        if let Some(pipe) = self.pipeline.as_ref().filter(|p| !p.is_empty()) {
            let mut ctx = PipelineContext::new(request.clone());
            pipe.run_before(&mut ctx)?;
            let response = self.generate_core(&ctx.request)?;
            ctx.response = Some(response);
            pipe.run_after_generate(&mut ctx)?;
            return Ok(ctx.take_response());
        }
        self.generate_core(request)
    }

    pub fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        if let Some(pipe) = self.pipeline.as_ref().filter(|p| !p.is_empty()) {
            let mut ctx = PipelineContext::new(request.clone());
            pipe.run_before(&mut ctx)?;
            let response = self.generate_stream_core(&ctx.request, on_chunk)?;
            ctx.response = Some(response);
            pipe.run_after_stream(&mut ctx)?;
            return Ok(ctx.take_response());
        }
        self.generate_stream_core(request, on_chunk)
    }

    /// 按 [`crate::flow::FlowPipeline`] 执行节点链：**不在此 API 内**自动调用 `generate`；由节点通过 [`crate::flow::FlowContext::llm`] 自行调用。
    /// 结束后必须有节点写入 [`crate::flow::FlowContext::response`]，否则会 [`LlmError::InvalidRequest`]。
    pub fn generate_with_flow<'a>(
        &'a self,
        request: GenerateRequest,
        flow: &crate::flow::FlowPipeline,
    ) -> Result<GenerateResponse, LlmError> {
        let provider = self
            .provider(&request.model.provider)
            .ok_or_else(|| LlmError::ProviderNotFound(request.model.provider.clone()))?;
        let mut ctx = crate::flow::FlowContext::new(request, provider);
        flow.run(&mut ctx)?;
        ctx.into_response()
    }

    fn generate_core(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
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

    fn generate_stream_core(
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
