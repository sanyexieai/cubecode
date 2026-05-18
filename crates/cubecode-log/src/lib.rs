//! Cubecode 进程级日志初始化（[`tracing-subscriber`]），与各层 [`tracing`] 埋点配合。
//!
//! - 在 **二进制入口**（如 `apps/llm-cli`）调用 [`init_from_env`] 或 [`init_cli`] **一次**。
//! - 日志同时写入 **stderr** 与 **`.cubecode/logs/`**（[`latest.log`] + 当前会话 `{session}.log`）。
//! - 用 [`show_logs`] / CLI `logs` 子命令查看，语义类似 `docker logs`。

mod layers;
mod paths;
mod tail;

pub use layers::{ADAPTER as LAYER_ADAPTER, CLI as LAYER_CLI, DISPATCH as LAYER_DISPATCH, INBOX as LAYER_INBOX, ORCHESTRATOR as LAYER_ORCHESTRATOR, SINK as LAYER_SINK, STEP as LAYER_STEP};

pub use paths::{
    current_session_marker, ensure_log_dir, latest_log_path, list_session_log_names, log_dir,
    new_session_id, read_current_session, resolve_log_file, resolve_log_file_for_read,
    session_log_path, write_current_session, ENV_LOG_DIR, ENV_SESSION, LATEST_LOG,
};
pub use tail::{show_logs, LogsOptions};

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// tracing target：①适配层
pub const ADAPTER: &str = "cubecode.adapter";
/// tracing target：②收件箱
pub const INBOX: &str = "cubecode.inbox";
/// tracing target：③调度层
pub const DISPATCH: &str = "cubecode.dispatch";
/// tracing target：④编排层
pub const ORCHESTRATOR: &str = "cubecode.orchestrator";
/// tracing target：⑤执行层
pub const STEP: &str = "cubecode.step";
/// tracing target：⑥输出层
pub const SINK: &str = "cubecode.sink";
/// tracing target：入口
pub const CLI: &str = "cubecode.cli";

static SESSION_ID: OnceLock<String> = OnceLock::new();

/// 当前进程写入的 session id（初始化成功后可用）。
pub fn active_session_id() -> Option<&'static str> {
    SESSION_ID.get().map(String::as_str)
}

fn default_filter() -> EnvFilter {
    EnvFilter::new("info,cubecode=info,llm_kit=warn")
}

/// 在加载 `.env` **之后**调用：优先 `RUST_LOG`，否则 [`default_filter`].
pub fn init_from_env() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter());
    init_with_filter(filter);
}

/// 按 CLI 冗长度初始化；若已设置 `RUST_LOG` 则仍以环境变量为准。
pub fn init_cli(verbosity: u8) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = match verbosity {
            0 => "info",
            1 => "debug",
            _ => "trace",
        };
        EnvFilter::new(format!("{level},cubecode={level},llm_kit={level}"))
    });
    init_with_filter(filter);
}

fn init_with_filter(filter: EnvFilter) {
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(io::stderr);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer);

    match open_log_files() {
        Ok((latest, session)) => {
            let session_id = SESSION_ID.get().map(String::as_str).unwrap_or("").to_owned();
            let latest_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(latest);
            let session_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(session);
            let _ = registry.with(latest_layer).with(session_layer).try_init();
            tracing::info!(
                target: CLI,
                session = %session_id,
                dir = %log_dir().display(),
                "文件日志已启用"
            );
        }
        Err(e) => {
            eprintln!("cubecode-log: 无法启用文件日志: {e}");
            let _ = registry.try_init();
        }
    }
}

/// 打开 `latest.log` 与当前 session 文件（追加写）。
fn open_log_files() -> io::Result<(SharedLogFile, SharedLogFile)> {
    let session = SESSION_ID.get_or_init(new_session_id).clone();
    let _ = write_current_session(&session);

    let latest_path = latest_log_path();
    let session_path = session_log_path(&session);

    Ok((
        SharedLogFile::open(&latest_path)?,
        SharedLogFile::open(&session_path)?,
    ))
}

#[derive(Clone)]
struct SharedLogFile(Arc<Mutex<std::fs::File>>);

impl SharedLogFile {
    fn open(path: &std::path::Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self(Arc::new(Mutex::new(file))))
    }
}

struct SharedLogWriter(Arc<Mutex<std::fs::File>>);

impl Write for SharedLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| io::Error::other("log file mutex poisoned"))?;
        let n = guard.write(buf)?;
        guard.flush()?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log file mutex poisoned"))?
            .flush()
    }
}

impl<'a> MakeWriter<'a> for SharedLogFile {
    type Writer = SharedLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedLogWriter(Arc::clone(&self.0))
    }
}

/// 供测试或二次初始化前判断（`try_init` 仅允许成功一次）。
pub fn is_initialized() -> bool {
    tracing::dispatcher::has_been_set()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_parses() {
        let _ = default_filter();
    }

    #[test]
    fn resolve_latest_and_session_paths() {
        let p = resolve_log_file(None);
        assert!(p.ends_with(LATEST_LOG));
        let p = resolve_log_file(Some("abc"));
        assert!(p.to_string_lossy().contains("abc.log"));
    }

    #[test]
    fn missing_log_hint_lists_commands() {
        use crate::paths::format_missing_log;

        let dir = std::env::temp_dir().join(format!("cubecode-log-hint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var(ENV_LOG_DIR, dir.to_string_lossy().as_ref());
        let msg = format_missing_log(&dir.join("nope.log"));
        assert!(msg.contains("six-layer-pipeline"));
        std::env::remove_var(ENV_LOG_DIR);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
