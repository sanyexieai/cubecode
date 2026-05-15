//! 自请求到响应的可插拔流水线：由若干 [`PipelineStage`] 顺序组成，**无节点或 `None` 时与未接 pipeline 行为一致**。
//!
//! 扩展方式：在独立 crate 中 `impl PipelineStage for YourType`，用 [`PipelineBuilder`] 组装后交给 [`crate::registry::ProviderRegistry::set_pipeline`]。
//!
//! 约定：`before_generate` 按注册顺序执行；`after_generate` / `after_generate_stream` 按**相同顺序**执行（非洋葱逆序），便于推理。
//!
//! 若需要「每个节点自行持 [`LlmProvider`] 并可能多次调用模型」、且无中心 `generate`，请使用 [`crate::flow`] 与 [`ProviderRegistry::generate_with_flow`](crate::registry::ProviderRegistry::generate_with_flow)。
//!
//! [`crate::registry::ProviderRegistry`] 在挂有非空 pipeline 时，对单次 `generate` / `generate_stream` 的顺序为：
//!
//! 1. 克隆请求 → [`PipelineContext`]
//! 2. 按注册顺序依次调用各阶段的 [`PipelineStage::before_generate`]
//! 3. **恰好一次**对 [`crate::core::provider::LlmProvider`] 的调用（内部仍带重试）
//! 4. 将结果写入 `ctx.response`
//! 5. 按注册顺序依次调用各阶段的 [`PipelineStage::after_generate`]（流式为结束后 [`PipelineStage::after_generate_stream`]）
//!
//! 文本化数据流（箭头表示顺序，与一次 provider 调用的关系）：
//!
//! ```text
//! Request
//!   -> before[stage0] -> before[stage1] -> ... -> before[stageN]
//!   -> LlmProvider.generate(请求为 ctx.request)
//!   -> after[stage0] -> after[stage1] -> ... -> after[stageN]
//!   -> Response
//! ```
//!
//! **含义**：框架只提供「围绕**一次** provider 往返」的前后链；**没有**「每个节点自动对应一次独立 LLM 调用」的内置语义。
//!
//! # 典型业务节点如何落位
//!
//! | 概念（例：输入→记忆→意图→执行→输出） | 是否天然「一节点一框架钩子」 | 常见落位 |
//! |--------------------------------------|------------------------------|----------|
//! | 输入 | 是 | 初始 [`GenerateRequest`] / [`PipelineContext::request`] |
//! | 记忆召回 | 部分 | [`PipelineStage::before_generate`] 内改 `messages` / `metadata`，或节点内调检索；若召回本身还要一次 LLM，在**节点内部**自建调用，不是 Registry 的第二趟 |
//! | 意图分类 | 部分 | `before_generate` 内分类，结果写 `metadata`；**无**内置意图路由表 |
//! | 意图执行（改 prompt/模型后交主模型） | 可 | 仍在前序 `before_generate` 中改 `ctx.request`，由步骤 3 一次生成 |
//! | 意图执行（独立第二条 LLM 链） | 否（单条 pipeline） | 应用层多次 `generate`，或单节点内自建多轮调用 |
//! | 输出整形 / 落库 | 是 | [`PipelineStage::after_generate`] |
//!
//! # 与字面线性链 A→B→C→D→输出的差异
//!
//! 若业务期望「全程线性、且 D 在模型之后」，应把 **D 放在 `after_generate`**；若某步必须在模型之前，应放在 **`before_generate`**，而不是假设存在「模型后的第四种全局钩子」。
//!
//! # 多步编排（多轮 Registry 调用）
//!
//! 若每个节点都要**各自**走 `ProviderRegistry::generate` 级别的一次往返，请在 Registry **之上**编排（多次调用或组合 pipeline），或在一个「复合」[`PipelineStage`] 内部封装多轮逻辑；本模块**不**替代通用工作流引擎。

use std::sync::Arc;
use crate::core::error::LlmError;
use crate::core::types::{GenerateRequest, GenerateResponse};

/// 单次调用在节点间传递的可变上下文。
#[derive(Debug, Clone)]
pub struct PipelineContext {
    pub request: GenerateRequest,
    pub response: Option<GenerateResponse>,
}

impl PipelineContext {
    pub fn new(request: GenerateRequest) -> Self {
        Self {
            request,
            response: None,
        }
    }

    pub fn take_response(mut self) -> GenerateResponse {
        self.response
            .take()
            .expect("pipeline after_generate must leave response set")
    }
}

/// 一个流水线节点：默认钩子为空，只实现关心的阶段即可。
pub trait PipelineStage: Send + Sync {
    /// 稳定 id，用于日志 / 调试。
    fn id(&self) -> &'static str;

    /// 在调用 provider 之前；可改写 `ctx.request`。
    fn before_generate(&self, _ctx: &mut PipelineContext) -> Result<(), LlmError> {
        Ok(())
    }

    /// 在非流式 `generate` 得到响应之后；可改写 `ctx.response`。
    fn after_generate(&self, _ctx: &mut PipelineContext) -> Result<(), LlmError> {
        Ok(())
    }

    /// 在流式 `generate_stream` 完整结束之后调用；默认转调 [`PipelineStage::after_generate`]。
    fn after_generate_stream(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        self.after_generate(ctx)
    }
}

/// 有序阶段列表；[`Pipeline::is_empty`] 或 registry 未设置 pipeline 时不跑任何钩子。
#[derive(Clone, Default)]
pub struct Pipeline {
    stages: Vec<Arc<dyn PipelineStage>>,
}

impl Pipeline {
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::default()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub(crate) fn run_before(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        for stage in &self.stages {
            stage.before_generate(ctx)?;
        }
        Ok(())
    }

    pub(crate) fn run_after_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        for stage in &self.stages {
            stage.after_generate(ctx)?;
        }
        Ok(())
    }

    pub(crate) fn run_after_stream(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        for stage in &self.stages {
            stage.after_generate_stream(ctx)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct PipelineBuilder {
    stages: Vec<Arc<dyn PipelineStage>>,
}

impl PipelineBuilder {
    pub fn push<S: PipelineStage + 'static>(mut self, stage: S) -> Self {
        self.stages.push(Arc::new(stage));
        self
    }

    pub fn push_arc(mut self, stage: Arc<dyn PipelineStage>) -> Self {
        self.stages.push(stage);
        self
    }

    pub fn build(self) -> Pipeline {
        Pipeline {
            stages: self.stages,
        }
    }
}
