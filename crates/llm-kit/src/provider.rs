use crate::error::LlmError;
use crate::types::{GenerateRequest, GenerateResponse, ProviderInfo, StreamChunk};

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
