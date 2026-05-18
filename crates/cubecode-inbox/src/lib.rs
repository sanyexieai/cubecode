//! ② **收件箱**：进程内 FIFO 队列。

use std::collections::VecDeque;

use cubecode_contracts::ControlEvent;

pub struct Inbox {
    q: VecDeque<ControlEvent>,
}

impl Inbox {
    pub fn new() -> Self {
        Self {
            q: VecDeque::new(),
        }
    }

    pub fn push(&mut self, event: ControlEvent) {
        tracing::info!(
            target: "cubecode.inbox",
            len_after = self.q.len() + 1,
            ?event,
            "②收件箱：入队"
        );
        self.q.push_back(event);
    }

    pub fn pop(&mut self) -> Option<ControlEvent> {
        let event = self.q.pop_front();
        tracing::info!(
            target: "cubecode.inbox",
            len_after = self.q.len(),
            popped = event.is_some(),
            ?event,
            "②收件箱：出队"
        );
        event
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }
}
