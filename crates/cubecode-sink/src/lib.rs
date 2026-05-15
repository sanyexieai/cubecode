//! ⑥ **Sink**：占位输出（stdout）；后续可换 outbox / 日志 / UI。

/// 将一行结果写到标准输出（演示用）。
pub fn emit_line(label: &str, text: &str) {
    println!("[{label}] {text}");
}
