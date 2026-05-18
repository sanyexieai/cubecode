//! [`Adapter`] trait：统一「原始输入 → [`ControlEvent`]」出口（M6-1）。

use cubecode_contracts::ControlEvent;

use crate::error::AdapterError;

/// 输入源适配器：将终端、HTTP、IDE 等整理为语义事件，再由调用方写入 ②。
///
/// - 实现方负责在事件中填入正确的 `session_id` / `turn_id`。
/// - `poll_events` 宜为非阻塞或带超时的「拉取」；无新输入时返回空 `Vec`。
pub trait Adapter {
    /// 稳定标识（日志与配置）。
    fn id(&self) -> &'static str;

    /// 拉取当前可用的一批事件（可为空）。
    fn poll_events(&mut self) -> Result<Vec<ControlEvent>, AdapterError>;
}
