use std::path::{Path, PathBuf};

/// 环境变量：日志目录，未设置则用当前工作目录下的 `.cubecode/logs`。
pub const ENV_LOG_DIR: &str = "CUBECODE_LOG_DIR";
/// 环境变量：本次运行的会话 id（对应 `{id}.log`）。
pub const ENV_SESSION: &str = "CUBECODE_SESSION";
/// 环境变量：设为 `1`/`true` 时 tracing 同时写入 stderr（默认仅写文件）。
pub const ENV_LOG_STDERR: &str = "CUBECODE_LOG_STDERR";

pub const LATEST_LOG: &str = "latest.log";
pub const CURRENT_SESSION_FILE: &str = ".current_session";

/// 解析日志根目录（会 `canonicalize` 失败时仍返回原路径）。
pub fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(ENV_LOG_DIR) {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".cubecode")
        .join("logs")
}

pub fn latest_log_path() -> PathBuf {
    log_dir().join(LATEST_LOG)
}

pub fn session_log_path(session: &str) -> PathBuf {
    log_dir().join(format!("{session}.log"))
}

/// 将 CLI / 用户输入解析为日志文件路径（写入侧）；`None` 或 `"latest"` → [`latest_log_path`].
pub fn resolve_log_file(session: Option<&str>) -> PathBuf {
    match session {
        None | Some("latest") => latest_log_path(),
        Some(id) if !id.trim().is_empty() => session_log_path(id.trim()),
        Some(_) => latest_log_path(),
    }
}

/// 读取侧：未指定 session 时优先 `latest.log`，否则回退到 [`.current_session`] 对应文件。
pub fn resolve_log_file_for_read(session: Option<&str>) -> PathBuf {
    match session {
        Some(id) if !id.trim().is_empty() && id.trim() != "latest" => {
            session_log_path(id.trim())
        }
        _ => {
            let latest = latest_log_path();
            if latest.is_file() {
                return latest;
            }
            if let Some(sid) = read_current_session() {
                let session_path = session_log_path(&sid);
                if session_path.is_file() {
                    return session_path;
                }
            }
            latest
        }
    }
}

/// 日志目录下已有的 `*.log` 文件名（不含扩展名），按修改时间新→旧。
pub fn list_session_log_names() -> Vec<String> {
    let dir = log_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, std::time::SystemTime)> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "log")
        })
        .filter_map(|e| {
            let name = e.path().file_stem()?.to_string_lossy().into_owned();
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((name, modified))
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    entries.into_iter().map(|(n, _)| n).collect()
}

pub fn current_session_marker() -> PathBuf {
    log_dir().join(CURRENT_SESSION_FILE)
}

/// 本次进程使用的 session id（`CUBECODE_SESSION` 或 `run-{ms}`）。
pub fn new_session_id() -> String {
    std::env::var(ENV_SESSION).unwrap_or_else(|_| {
        format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
    })
}

pub fn ensure_log_dir() -> std::io::Result<PathBuf> {
    let dir = log_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn write_current_session(session: &str) -> std::io::Result<()> {
    let dir = ensure_log_dir()?;
    std::fs::write(dir.join(CURRENT_SESSION_FILE), session.as_bytes())
}

pub fn read_current_session() -> Option<String> {
    let path = current_session_marker();
    let s = std::fs::read_to_string(path).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn format_missing_log(path: &Path) -> String {
    let mut msg = format!("找不到日志文件: {}\n", path.display());
    let names = list_session_log_names();
    if names.is_empty() {
        msg.push_str("还没有任何落盘日志。请先运行会初始化日志的命令，例如：\n");
        msg.push_str("  cargo run -p llm-cli -- six-layer-pipeline\n");
        msg.push_str("  cargo run -p llm-cli -- -v complete -m hello\n");
        msg.push('\n');
    } else {
        msg.push_str("已有日志（可用 `llm-kit logs <SESSION> --tail N`）：\n");
        for name in &names {
            msg.push_str("  ");
            msg.push_str(name);
            msg.push('\n');
        }
        if let Some(current) = read_current_session() {
            msg.push_str(&format!("最近会话 id: {current}\n"));
        }
    }
    msg.push_str(&format!(
        "日志目录: {}（可用环境变量 {} 修改）",
        log_dir().display(),
        ENV_LOG_DIR
    ));
    msg
}
