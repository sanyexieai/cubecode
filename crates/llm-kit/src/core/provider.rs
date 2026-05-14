use super::error::LlmError;
use super::types::{GenerateRequest, GenerateResponse, ProviderInfo, StreamChunk};

/// 应用层使用的「模型后端」抽象：与具体 HTTP 线格式无关。
///
/// 各协议实现见 [`crate::providers::protocols`]，并通常同时实现 [`crate::providers::ProtocolBinding`]（仅为 [`LlmProvider`] 的标记超 trait）。
pub trait LlmProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError>;
    fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        let response = self.generate(request)?;
        if !response.message.content.is_empty() {
            on_chunk(StreamChunk {
                delta: response.message.content.clone(),
                finish_reason: Some(response.finish_reason.clone()),
            })?;
        }
        Ok(response)
    }
}
