//! Pipeline 节点：在请求进入 provider 前写入 `metadata`（键名可配置）。

use llm_kit::{LlmError, PipelineContext, PipelineStage};

#[derive(Debug, Clone)]
pub struct MetadataStampStage {
    pub key: String,
    pub value: String,
}

impl MetadataStampStage {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl PipelineStage for MetadataStampStage {
    fn id(&self) -> &'static str {
        "metadata_stamp"
    }

    fn before_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        ctx.request
            .metadata
            .insert(self.key.clone(), self.value.clone());
        Ok(())
    }
}
