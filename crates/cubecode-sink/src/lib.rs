//! ⑥ **输出层**：用户可见输出；诊断走 tracing。

use std::io::{self, Write};

use cubecode_contracts::TurnContext;

/// 用户可见输出的抽象（终端、IDE、HTTP 等实现此 trait）。
pub trait Sink {
    fn emit_line(&self, turn_ctx: &TurnContext, label: &str, text: &str);
    fn emit_assistant(&self, turn_ctx: &TurnContext, text: &str);
    /// 流式写出一块文本（不换行；由实现方 `flush`）。
    fn emit_chunk(&self, turn_ctx: &TurnContext, delta: &str);
    /// 助手流式回复开始前（默认无操作）。
    fn begin_assistant_stream(&self, _turn_ctx: &TurnContext) {}
    /// 助手流式回复结束后（默认无操作）。
    fn end_assistant_stream(&self, _turn_ctx: &TurnContext) {}
    /// 本轮用户可见错误（实现方通常写 stderr）。
    fn emit_error(&self, turn_ctx: &TurnContext, message: &str);
}

/// 终端 stdout 实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSink;

impl Sink for TerminalSink {
    fn emit_line(&self, turn_ctx: &TurnContext, label: &str, text: &str) {
        tracing::info!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            label,
            bytes = text.len(),
            "⑥输出层：写出（带标签）"
        );
        let _ = writeln!(io::stdout(), "[{label}] {text}");
        flush_stdout();
    }

    fn emit_assistant(&self, turn_ctx: &TurnContext, text: &str) {
        tracing::info!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            bytes = text.len(),
            "⑥输出层：写出（助手回复）"
        );
        let _ = writeln!(io::stdout(), "\n{}\n", text.trim_end());
        flush_stdout();
    }

    fn begin_assistant_stream(&self, turn_ctx: &TurnContext) {
        tracing::info!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            "⑥输出层：流式写出开始"
        );
        let _ = write!(io::stdout(), "\n");
        flush_stdout();
    }

    fn emit_chunk(&self, turn_ctx: &TurnContext, delta: &str) {
        if delta.is_empty() {
            return;
        }
        tracing::debug!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            bytes = delta.len(),
            "⑥输出层：流式块"
        );
        let _ = write!(io::stdout(), "{delta}");
        flush_stdout();
    }

    fn end_assistant_stream(&self, turn_ctx: &TurnContext) {
        tracing::info!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            "⑥输出层：流式写出结束"
        );
        let _ = writeln!(io::stdout());
        flush_stdout();
    }

    fn emit_error(&self, turn_ctx: &TurnContext, message: &str) {
        tracing::warn!(
            target: "cubecode.sink",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            error = %message,
            "⑥输出层：错误"
        );
        write_error_stderr(message);
    }
}

/// 进程内默认终端 sink。
pub const TERMINAL: TerminalSink = TerminalSink;

fn flush_stdout() {
    let _ = io::stdout().flush();
}

fn write_error_stderr(message: &str) {
    let _ = writeln!(io::stderr(), "\n错误: {message}\n");
    let _ = io::stderr().flush();
}

/// 演示/占位：`[标签] 正文` 格式。
pub fn emit_line(turn_ctx: &TurnContext, label: &str, text: &str) {
    TERMINAL.emit_line(turn_ctx, label, text);
}

/// 聊天：干净助手正文（仍记一条输出层日志）。
pub fn emit_assistant(turn_ctx: &TurnContext, text: &str) {
    TERMINAL.emit_assistant(turn_ctx, text);
}

/// 流式块写出（不换行，立即 flush）。
pub fn emit_chunk(turn_ctx: &TurnContext, delta: &str) {
    TERMINAL.emit_chunk(turn_ctx, delta);
}

/// 助手流式回复开始（打印前导换行）。
pub fn begin_assistant_stream(turn_ctx: &TurnContext) {
    TERMINAL.begin_assistant_stream(turn_ctx);
}

/// 助手流式回复结束（补尾换行）。
pub fn end_assistant_stream(turn_ctx: &TurnContext) {
    TERMINAL.end_assistant_stream(turn_ctx);
}

/// 本轮错误：stderr 展示 + `tracing::warn`（带 `session_id` / `turn_id`）。
pub fn emit_error(turn_ctx: &TurnContext, message: &str) {
    TERMINAL.emit_error(turn_ctx, message);
}

/// 无轮次上下文的全局错误（CLI 启动失败等）。
pub fn emit_error_global(message: &str) {
    tracing::warn!(
        target: "cubecode.sink",
        error = %message,
        "⑥输出层：全局错误"
    );
    write_error_stderr(message);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use cubecode_contracts::{SessionId, TurnId};

    #[derive(Default)]
    struct RecordingSink {
        lines: Mutex<Vec<String>>,
        assistants: Mutex<Vec<String>>,
        chunks: Mutex<String>,
        errors: Mutex<Vec<String>>,
        stream_started: Mutex<bool>,
        stream_ended: Mutex<bool>,
    }

    impl Sink for RecordingSink {
        fn emit_line(&self, _: &TurnContext, label: &str, text: &str) {
            self.lines
                .lock()
                .unwrap()
                .push(format!("[{label}] {text}"));
        }

        fn emit_assistant(&self, _: &TurnContext, text: &str) {
            self.assistants.lock().unwrap().push(text.to_owned());
        }

        fn begin_assistant_stream(&self, _: &TurnContext) {
            *self.stream_started.lock().unwrap() = true;
        }

        fn emit_chunk(&self, _: &TurnContext, delta: &str) {
            self.chunks.lock().unwrap().push_str(delta);
        }

        fn end_assistant_stream(&self, _: &TurnContext) {
            *self.stream_ended.lock().unwrap() = true;
        }

        fn emit_error(&self, _: &TurnContext, message: &str) {
            self.errors.lock().unwrap().push(message.to_owned());
        }
    }

    fn test_ctx() -> TurnContext {
        TurnContext::new(SessionId::new("sess-sink-test"), TurnId::FIRST)
    }

    #[test]
    fn emit_chunk_skips_empty_delta() {
        let sink = RecordingSink::default();
        let ctx = test_ctx();
        sink.emit_chunk(&ctx, "");
        assert!(sink.chunks.lock().unwrap().is_empty());
        sink.emit_chunk(&ctx, "Hi");
        assert_eq!(sink.chunks.lock().unwrap().as_str(), "Hi");
    }

    #[test]
    fn assistant_stream_lifecycle_records_chunks() {
        let sink = RecordingSink::default();
        let ctx = test_ctx();
        sink.begin_assistant_stream(&ctx);
        sink.emit_chunk(&ctx, "你");
        sink.emit_chunk(&ctx, "好");
        sink.end_assistant_stream(&ctx);
        assert!(*sink.stream_started.lock().unwrap());
        assert_eq!(sink.chunks.lock().unwrap().as_str(), "你好");
        assert!(*sink.stream_ended.lock().unwrap());
    }

    #[test]
    fn emit_error_records_message() {
        let sink = RecordingSink::default();
        let ctx = test_ctx();
        sink.emit_error(&ctx, "模型调用失败");
        assert_eq!(
            sink.errors.lock().unwrap().as_slice(),
            &["模型调用失败"]
        );
    }

    #[test]
    fn free_functions_delegate_to_terminal_trait() {
        let ctx = test_ctx();
        emit_line(&ctx, "测试", "正文");
        emit_assistant(&ctx, "完整回复");
        begin_assistant_stream(&ctx);
        emit_chunk(&ctx, "块");
        end_assistant_stream(&ctx);
    }
}
