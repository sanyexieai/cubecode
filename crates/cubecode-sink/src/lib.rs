//! ⑥ **输出层**：用户可见输出；诊断走 tracing。

/// 演示/占位：`[标签] 正文` 格式。
pub fn emit_line(label: &str, text: &str) {
    tracing::info!(
        target: "cubecode.sink",
        label,
        bytes = text.len(),
        "⑥输出层：写出（带标签）"
    );
    println!("[{label}] {text}");
}

/// 聊天：干净助手正文（仍记一条输出层日志）。
pub fn emit_assistant(text: &str) {
    tracing::info!(
        target: "cubecode.sink",
        bytes = text.len(),
        "⑥输出层：写出（助手回复）"
    );
    println!("\n{}\n", text.trim_end());
}
