//! ② **Inbox**：进程内 FIFO 占位；后续可换有界 channel / 跨线程队列。

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
        self.q.push_back(event);
    }

    pub fn pop(&mut self) -> Option<ControlEvent> {
        self.q.pop_front()
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }
}
