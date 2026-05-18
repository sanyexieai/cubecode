//! ④ **编排层**：编排 ⑤ 执行层 → ⑥ 输出层；[`run_full_turn`] 串联 ①～⑥。

mod flow;

use cubecode_contracts::{ControlEvent, RouteHint};
use cubecode_sink::{emit_assistant, emit_line};
use cubecode_step::{llm_turn, placeholder_turn, LlmStepContext};

pub use flow::{run_full_turn, run_shutdown_turn};

/// ⑤ 执行后端。
pub enum StepBackend<'a> {
    Placeholder,
    Llm(LlmStepContext<'a>),
}

/// ⑥ 输出样式。
pub enum SinkStyle {
    /// `[label] …`（演示）
    Prefixed,
    /// 聊天正文
    Assistant,
}

/// **④ → ⑤ → ⑥**（③ 路由结果传入）。
pub fn run_pipeline(
    route: RouteHint,
    event: &ControlEvent,
    backend: StepBackend<'_>,
    sink: SinkStyle,
) -> Result<Option<String>, String> {
    tracing::info!(
        target: "cubecode.orchestrator",
        ?route,
        "④编排层：进入"
    );
    let result = match route {
        RouteHint::Exit => {
            tracing::info!(
                target: "cubecode.orchestrator",
                "④编排层 → ⑥输出层（退出，跳过⑤执行层）"
            );
            emit_line("输出层", "编排层：退出路由（未调用执行层）");
            None
        }
        RouteHint::UserTurn => {
            tracing::info!(
                target: "cubecode.orchestrator",
                "④编排层 → ⑤执行层"
            );
            let body = match backend {
                StepBackend::Placeholder => placeholder_turn(event)?,
                StepBackend::Llm(ctx) => llm_turn(&ctx, event)?,
            };
            tracing::info!(
                target: "cubecode.orchestrator",
                out_bytes = body.len(),
                "④编排层 → ⑥输出层"
            );
            match sink {
                SinkStyle::Prefixed => emit_line("助手", &body),
                SinkStyle::Assistant => emit_assistant(&body),
            }
            Some(body)
        }
    };
    tracing::info!(target: "cubecode.orchestrator", "④编排层：离开");
    Ok(result)
}

/// 占位闭环（兼容旧调用）：**④ → ⑤ → ⑥**。
pub fn run_minimal_pipeline(route: RouteHint, event: &ControlEvent) -> Result<(), String> {
    run_pipeline(
        route,
        event,
        StepBackend::Placeholder,
        SinkStyle::Prefixed,
    )
    .map(|_| ())
}
