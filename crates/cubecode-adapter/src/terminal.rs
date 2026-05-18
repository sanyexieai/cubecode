//! 终端 stdin 适配器（M6-2）：读行、元命令、产出 [`ControlEvent`]。

use std::io::{self, BufRead, Write};

use cubecode_contracts::{ControlEvent, SessionId, TurnContext, TurnId};

use crate::adapter::Adapter;
use crate::error::AdapterError;

/// 终端读一行后的语义（元命令在进 ② 前由 ① 区分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalPoll {
    /// 空行，继续等待输入。
    Idle,
    /// stdin 已关闭（EOF）。
    Eof,
    /// `/cancel`：由宿主处理编排层取消，不入队。
    Cancel,
    /// 可入队的一条或多条语义事件。
    Events(Vec<ControlEvent>),
}

/// 从 [`BufRead`]（通常为 stdin）读取用户输入并解析元命令。
pub struct TerminalAdapter<R> {
    reader: R,
    session_id: SessionId,
    next_turn_id: TurnId,
    line_buf: String,
    /// 是否在每次 `poll` 前打印 `> `（测试可关闭）。
    prompt_enabled: bool,
}

impl<R: BufRead> TerminalAdapter<R> {
    pub fn new(session_id: SessionId, reader: R) -> Self {
        Self {
            reader,
            session_id,
            next_turn_id: TurnId::FIRST,
            line_buf: String::new(),
            prompt_enabled: true,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// 下一笔用户轮次将使用的 `turn_id`（`/exit` 关闭流程与旧 CLI 行为一致）。
    pub fn next_turn_id(&self) -> TurnId {
        self.next_turn_id
    }

    pub fn set_prompt_enabled(&mut self, enabled: bool) {
        self.prompt_enabled = enabled;
    }

    /// 读一行并解析；阻塞直到用户输入或 EOF。
    pub fn poll(&mut self) -> Result<TerminalPoll, AdapterError> {
        if self.prompt_enabled {
            eprint!("> ");
            io::stderr()
                .flush()
                .map_err(|e| AdapterError::Io(e.to_string()))?;
        }
        self.line_buf.clear();
        let n = self
            .reader
            .read_line(&mut self.line_buf)
            .map_err(|e| AdapterError::Io(e.to_string()))?;
        if n == 0 {
            tracing::debug!(target: "cubecode.adapter", "终端适配器：stdin EOF");
            return Ok(TerminalPoll::Eof);
        }
        let input = self.line_buf.trim();
        if input.is_empty() {
            return Ok(TerminalPoll::Idle);
        }
        if input == "/cancel" {
            tracing::info!(
                target: "cubecode.adapter",
                session_id = %self.session_id,
                "终端适配器：元命令 /cancel"
            );
            return Ok(TerminalPoll::Cancel);
        }
        if input == "/exit" || input == ":q" {
            tracing::info!(
                target: "cubecode.adapter",
                session_id = %self.session_id,
                "终端适配器：元命令退出"
            );
            return Ok(TerminalPoll::Events(vec![ControlEvent::shutdown(
                self.session_id.clone(),
            )]));
        }
        let turn_id = self.next_turn_id;
        self.next_turn_id = turn_id.next();
        let ctx = TurnContext::new(self.session_id.clone(), turn_id);
        tracing::info!(
            target: "cubecode.adapter",
            session_id = %self.session_id,
            turn_id = %turn_id,
            bytes = input.len(),
            "终端适配器：用户输入"
        );
        Ok(TerminalPoll::Events(vec![ControlEvent::user_turn(
            &ctx, input.to_owned(),
        )]))
    }
}

impl<R: BufRead> Adapter for TerminalAdapter<R> {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn poll_events(&mut self) -> Result<Vec<ControlEvent>, AdapterError> {
        match self.poll()? {
            TerminalPoll::Events(events) => Ok(events),
            TerminalPoll::Idle | TerminalPoll::Cancel | TerminalPoll::Eof => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn adapter_with(input: &str) -> TerminalAdapter<Cursor<&[u8]>> {
        let mut a = TerminalAdapter::new(SessionId::new("s-term"), Cursor::new(input.as_bytes()));
        a.set_prompt_enabled(false);
        a
    }

    #[test]
    fn idle_on_empty_line() {
        let mut a = adapter_with("\n");
        assert_eq!(a.poll().expect("poll"), TerminalPoll::Idle);
    }

    #[test]
    fn user_turn_assigns_turn_id() {
        let mut a = adapter_with("hello\n");
        let TerminalPoll::Events(events) = a.poll().expect("poll") else {
            panic!("expected events");
        };
        assert_eq!(events.len(), 1);
        match &events[0] {
            ControlEvent::UserTurn {
                turn_id,
                text,
                ..
            } => {
                assert_eq!(*turn_id, TurnId::FIRST);
                assert_eq!(text, "hello");
            }
            _ => panic!("expected user turn"),
        }
        assert_eq!(a.next_turn_id(), TurnId::FIRST.next());
    }

    #[test]
    fn exit_emits_shutdown() {
        let mut a = adapter_with("/exit\n");
        let TerminalPoll::Events(events) = a.poll().expect("poll") else {
            panic!("expected shutdown");
        };
        assert!(matches!(events[0], ControlEvent::Shutdown { .. }));
        assert_eq!(a.next_turn_id(), TurnId::FIRST);
    }

    #[test]
    fn cancel_is_not_an_event() {
        let mut a = adapter_with("/cancel\n");
        assert_eq!(a.poll().expect("poll"), TerminalPoll::Cancel);
        assert!(a.poll_events().expect("trait").is_empty());
    }

    #[test]
    fn adapter_trait_maps_user_line() {
        let mut a = adapter_with("hi\n");
        let events = a.poll_events().expect("poll_events");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ControlEvent::UserTurn { .. }));
    }
}
