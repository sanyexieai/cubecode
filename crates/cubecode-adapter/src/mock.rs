//! 测试用 [`Adapter`] 实现。

use cubecode_contracts::ControlEvent;

use crate::adapter::Adapter;
use crate::error::AdapterError;

/// 从预置队列依次 `poll_events` 吐出事件（单元测试 / 占位 HTTP）。
#[derive(Debug, Default)]
pub struct MockAdapter {
    pending: Vec<ControlEvent>,
}

impl MockAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_events(events: impl IntoIterator<Item = ControlEvent>) -> Self {
        Self {
            pending: events.into_iter().collect(),
        }
    }

    pub fn push(&mut self, event: ControlEvent) {
        self.pending.push(event);
    }
}

impl Adapter for MockAdapter {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn poll_events(&mut self) -> Result<Vec<ControlEvent>, AdapterError> {
        Ok(std::mem::take(&mut self.pending))
    }
}
