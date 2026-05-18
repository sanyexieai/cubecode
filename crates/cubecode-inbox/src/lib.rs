//! ② **收件箱**：有界 FIFO 队列；满时 [`Inbox::try_push`] 返回背压错误。
//!
//! 生命周期：[`Inbox::clear`] 清空全部待处理事件；[`Inbox::cancel_session`] 仅移除指定会话的事件。

use std::collections::VecDeque;

use cubecode_contracts::{ControlEvent, SessionId};

/// 队列已满，拒绝入队。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxFull {
    pub capacity: usize,
}

impl std::fmt::Display for InboxFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "收件箱已满（容量 {}）", self.capacity)
    }
}

impl std::error::Error for InboxFull {}

pub struct Inbox {
    q: VecDeque<ControlEvent>,
    capacity: usize,
}

impl Inbox {
    /// 默认容量（可用环境变量 `CUBECODE_INBOX_CAPACITY` 在入口覆盖）。
    pub const DEFAULT_CAPACITY: usize = 256;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            q: VecDeque::with_capacity(capacity.min(4096)),
            capacity,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn remaining(&self) -> usize {
        self.capacity.saturating_sub(self.q.len())
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.q.len() >= self.capacity
    }

    /// 入队；满时返回 [`InboxFull`]（背压）。
    pub fn try_push(&mut self, event: ControlEvent) -> Result<(), InboxFull> {
        if self.is_full() {
            tracing::warn!(
                target: "cubecode.inbox",
                capacity = self.capacity,
                len = self.q.len(),
                ?event,
                "②收件箱：队列已满，拒绝入队"
            );
            return Err(InboxFull {
                capacity: self.capacity,
            });
        }
        tracing::info!(
            target: "cubecode.inbox",
            len_after = self.q.len() + 1,
            capacity = self.capacity,
            ?event,
            "②收件箱：入队"
        );
        self.q.push_back(event);
        Ok(())
    }

    pub fn pop(&mut self) -> Option<ControlEvent> {
        let event = self.q.pop_front();
        tracing::debug!(
            target: "cubecode.inbox",
            len_after = self.q.len(),
            capacity = self.capacity,
            popped = event.is_some(),
            "②收件箱：出队（详情见上层 flow 日志）"
        );
        event
    }

    /// 清空队列；返回被丢弃的事件数。
    pub fn clear(&mut self) -> usize {
        let removed = self.q.len();
        if removed > 0 {
            self.q.clear();
            tracing::warn!(
                target: "cubecode.inbox",
                removed,
                capacity = self.capacity,
                "②收件箱：已清空全部待处理事件"
            );
        }
        removed
    }

    /// 移除指定会话的待处理事件（保留其他会话）；返回移除数量。
    pub fn cancel_session(&mut self, session_id: &SessionId) -> usize {
        let before = self.q.len();
        self.q.retain(|e| e.session_id() != session_id);
        let removed = before.saturating_sub(self.q.len());
        if removed > 0 {
            tracing::warn!(
                target: "cubecode.inbox",
                %session_id,
                removed,
                len_after = self.q.len(),
                capacity = self.capacity,
                "②收件箱：已取消会话的待处理事件"
            );
        }
        removed
    }
}

/// 从环境变量读取容量；无效或未设置时用 [`Inbox::DEFAULT_CAPACITY`]。
pub fn capacity_from_env() -> usize {
    std::env::var("CUBECODE_INBOX_CAPACITY")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(Inbox::DEFAULT_CAPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::{SessionId, TurnContext, TurnId};

    #[test]
    fn try_push_respects_capacity() {
        let mut inbox = Inbox::with_capacity(2);
        let ctx = TurnContext::new(SessionId::generate(), TurnId::FIRST);
        assert!(inbox.try_push(ControlEvent::user_turn(&ctx, "a")).is_ok());
        assert!(inbox.try_push(ControlEvent::user_turn(&ctx, "b")).is_ok());
        let err = inbox
            .try_push(ControlEvent::user_turn(&ctx, "c"))
            .expect_err("full");
        assert_eq!(err.capacity, 2);
        assert_eq!(inbox.len(), 2);
    }

    #[test]
    fn pop_after_full() {
        let mut inbox = Inbox::with_capacity(1);
        let ctx = TurnContext::new(SessionId::generate(), TurnId::FIRST);
        inbox.try_push(ControlEvent::user_turn(&ctx, "a")).unwrap();
        assert!(inbox.try_push(ControlEvent::user_turn(&ctx, "b")).is_err());
        assert!(inbox.pop().is_some());
        assert!(inbox.try_push(ControlEvent::user_turn(&ctx, "b")).is_ok());
    }

    #[test]
    fn cancel_session_removes_only_matching() {
        let mut inbox = Inbox::with_capacity(8);
        let s1 = SessionId::new("sess-test-a");
        let s2 = SessionId::new("sess-test-b");
        let c1 = TurnContext::new(s1.clone(), TurnId::FIRST);
        let c2 = TurnContext::new(s2.clone(), TurnId::FIRST);
        inbox.try_push(ControlEvent::user_turn(&c1, "a")).unwrap();
        inbox.try_push(ControlEvent::user_turn(&c2, "b")).unwrap();
        inbox.try_push(ControlEvent::shutdown(s1.clone())).unwrap();
        assert_eq!(inbox.cancel_session(&s1), 2);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox.pop().unwrap().session_id(), &s2);
    }

    #[test]
    fn clear_empties_queue() {
        let mut inbox = Inbox::with_capacity(4);
        let ctx = TurnContext::new(SessionId::generate(), TurnId::FIRST);
        inbox.try_push(ControlEvent::user_turn(&ctx, "x")).unwrap();
        assert_eq!(inbox.clear(), 1);
        assert!(inbox.is_empty());
    }
}
