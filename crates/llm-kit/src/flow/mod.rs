//! **节点驱动**编排：无「主 LLM」单次调用；[`FlowContext::llm`] 作为共享实例传入每个 [`FlowNode`]，由节点自行决定是否调用 `generate` / `generate_stream`。
//!
//! 与 [`crate::pipeline`] 的区别：`pipeline` 是「一次 `LlmProvider::generate` + 前后钩子」；`flow` 是「仅按顺序跑节点」，**框架内不再自动调用 provider**。
//!
//! 典型用法：用 [`ProviderRegistry::generate_with_flow`] 解析 `request.model.provider` 得到 `llm` 引用，再执行 [`FlowPipeline`]；输出由某个节点写入 [`FlowContext::response`]，或链尾挂载工作区 crate **`llm-node-output-default`** 作为兜底。

use std::sync::Arc;

use crate::core::error::LlmError;
use crate::core::provider::LlmProvider;
use crate::core::types::{GenerateRequest, GenerateResponse};

/// 在节点链之间传递的状态；[`FlowContext::llm`] 与 `request.model.provider` 对应的后端实例一致。
pub struct FlowContext<'a> {
    pub request: GenerateRequest,
    pub response: Option<GenerateResponse>,
    pub llm: &'a dyn LlmProvider,
}

impl<'a> FlowContext<'a> {
    pub fn new(request: GenerateRequest, llm: &'a dyn LlmProvider) -> Self {
        Self {
            request,
            response: None,
            llm,
        }
    }

    /// 节点链结束后取出响应；若仍为 `None` 则返回 [`LlmError::InvalidRequest`]。
    pub fn into_response(self) -> Result<GenerateResponse, LlmError> {
        self.response.ok_or_else(|| {
            LlmError::InvalidRequest(
                "flow finished without response; add an output node or llm-node-output-default"
                    .to_owned(),
            )
        })
    }
}

/// 单步节点：内部可任意调用 [`LlmProvider::generate`] / [`LlmProvider::generate_stream`]，或只改 `request` / `response`。
pub trait FlowNode: Send + Sync {
    fn id(&self) -> &'static str;

    fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError>;
}

/// 有序节点列表。
#[derive(Clone, Default)]
pub struct FlowPipeline {
    nodes: Vec<Arc<dyn FlowNode>>,
}

impl FlowPipeline {
    pub fn builder() -> FlowPipelineBuilder {
        FlowPipelineBuilder::default()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 按注册顺序依次执行每个节点。
    pub fn run<'a>(&self, ctx: &mut FlowContext<'a>) -> Result<(), LlmError> {
        for node in &self.nodes {
            node.run(ctx)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct FlowPipelineBuilder {
    nodes: Vec<Arc<dyn FlowNode>>,
}

impl FlowPipelineBuilder {
    pub fn push<N: FlowNode + 'static>(mut self, node: N) -> Self {
        self.nodes.push(Arc::new(node));
        self
    }

    pub fn push_arc(mut self, node: Arc<dyn FlowNode>) -> Self {
        self.nodes.push(node);
        self
    }

    pub fn build(self) -> FlowPipeline {
        FlowPipeline {
            nodes: self.nodes,
        }
    }
}
