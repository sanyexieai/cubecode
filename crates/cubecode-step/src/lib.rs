//! ⑤ **Step**：占位执行；**真实 LLM 调用**后续在此接入 `llm-kit`。
//!
//! 保留对 `llm-kit` 的依赖，避免占位阶段后遗忘接线。

use cubecode_contracts::ControlEvent;
use llm_kit as _;

/// 占位：不访问网络，只生成可读的摘要字符串。
pub fn placeholder_turn(event: &ControlEvent) -> Result<String, String> {
    match event {
        ControlEvent::UserLine(s) => Ok(format!(
            "(step placeholder) user bytes={}",
            s.len()
        )),
        ControlEvent::Shutdown => Err("shutdown".into()),
    }
}
