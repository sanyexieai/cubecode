use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::paths::{format_missing_log, log_dir, resolve_log_file_for_read};

/// 类似 `docker logs` 的查看选项。
#[derive(Debug, Clone, Default)]
pub struct LogsOptions {
    /// 会话 id；省略或 `latest` 表示 `latest.log`
    pub session: Option<String>,
    /// 只打印最后 N 行
    pub tail: Option<usize>,
    /// 持续跟随追加（`-f`）
    pub follow: bool,
}

/// 将日志打印到 **stdout**（用户可见输出），不写 stderr。
pub fn show_logs(opts: LogsOptions) -> Result<(), String> {
    let path = resolve_log_file_for_read(opts.session.as_deref());
    if !path.is_file() {
        if opts.follow {
            eprintln!(
                "等待日志文件出现: {}（在另一终端运行 llm-kit 等命令；Ctrl+C 退出）",
                path.display()
            );
            wait_for_file(&path)?;
        } else {
            return Err(format_missing_log(&path));
        }
    }
    if path.metadata().map(|m| m.len() == 0).unwrap_or(true) && !opts.follow {
        // 空文件：tail/全量均无输出，视为成功
        return Ok(());
    }
    if opts.follow {
        eprintln!(
            "跟随 {}（Ctrl+C 退出）",
            path.strip_prefix(log_dir())
                .unwrap_or(&path)
                .display()
        );
        tail_then_follow(&path, opts.tail)?;
    } else if let Some(n) = opts.tail {
        print_last_lines(&path, n)?;
    } else {
        io::copy(
            &mut File::open(&path).map_err(|e| e.to_string())?,
            &mut io::stdout(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn wait_for_file(path: &Path) -> Result<(), String> {
    while !path.is_file() {
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn file_len(path: &Path) -> Result<u64, String> {
    Ok(File::open(path)
        .and_then(|f| f.metadata())
        .map(|m| m.len())
        .map_err(|e| e.to_string())?)
}

fn read_all_lines(path: &Path) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    reader.lines().collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn print_last_lines(path: &Path, n: usize) -> Result<(), String> {
    if n == 0 {
        return Ok(());
    }
    let lines = read_all_lines(path)?;
    let start = lines.len().saturating_sub(n);
    let mut out = io::stdout();
    for line in &lines[start..] {
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn tail_then_follow(path: &Path, tail: Option<usize>) -> Result<(), String> {
    match tail {
        Some(n) => print_last_lines(path, n)?,
        None => {
            let len = file_len(path)?;
            if len == 0 {
                eprintln!("（当前无历史日志，等待新写入；可在另一终端运行聊天或 six-layer-pipeline）");
            } else {
                print_entire_file(path)?;
            }
        }
    }
    follow_from_end(path)
}

fn print_entire_file(path: &Path) -> Result<(), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    io::copy(&mut file, &mut io::stdout()).map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// 按偏移轮询并重开文件，兼容 Windows 上其他进程追加写入（类似 `tail -f`）。
fn follow_from_end(path: &Path) -> Result<(), String> {
    let mut stdout = io::stdout();
    let mut offset = file_len(path)?;
    loop {
        if !path.is_file() {
            thread::sleep(Duration::from_millis(250));
            offset = 0;
            continue;
        }
        let len = file_len(path)?;
        if len > offset {
            let mut file = File::open(path).map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut reader = BufReader::new(file);
            let mut chunk = Vec::new();
            reader
                .read_to_end(&mut chunk)
                .map_err(|e| e.to_string())?;
            if !chunk.is_empty() {
                stdout.write_all(&chunk).map_err(|e| e.to_string())?;
                stdout.flush().map_err(|e| e.to_string())?;
            }
            offset = len;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::ensure_log_dir;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    #[test]
    fn tail_and_follow_prefix() {
        let dir = std::env::temp_dir().join(format!("cubecode-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.log");
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .open(&path)
                .unwrap();
            writeln!(f, "line1").unwrap();
            writeln!(f, "line2").unwrap();
            writeln!(f, "line3").unwrap();
        }
        let mut buf = Vec::new();
        // capture last 2 lines
        let lines = read_all_lines(&path).unwrap();
        let n = 2usize;
        let start = lines.len().saturating_sub(n);
        for line in &lines[start..] {
            buf.push(line.clone());
        }
        assert_eq!(buf, vec!["line2", "line3"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_log_dir_under_cwd() {
        let _ = ensure_log_dir();
    }
}
