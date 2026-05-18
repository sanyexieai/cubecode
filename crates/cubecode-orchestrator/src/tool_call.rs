//! 从模型助手输出中解析 tool call（M4-4：固定 JSON 协议 + 常见包裹形式）。

use serde::Deserialize;

/// 解析成功的工具调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedToolCall {
    pub call_id: String,
    pub tool_name: String,
    /// 传给 [`cubecode_step::ToolRegistry::execute`] 的参数（常为 JSON 字符串）。
    pub arguments: String,
}

/// 从模型整段输出中提取工具调用；无法识别时返回 `None`（视为普通文本）。
pub fn parse_tool_call_from_model_output(text: &str) -> Option<ParsedToolCall> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(call) = try_parse_envelope(trimmed) {
        return Some(call);
    }
    for block in extract_json_fences(trimmed) {
        if let Some(call) = try_parse_envelope(&block) {
            return Some(call);
        }
    }
    if let Some(slice) = find_tool_call_json_slice(trimmed) {
        return try_parse_envelope(slice);
    }
    None
}

#[derive(Deserialize)]
struct ToolCallEnvelope {
    tool_call: RawToolCall,
}

#[derive(Deserialize)]
struct RawToolCall {
    id: String,
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

fn try_parse_envelope(json: &str) -> Option<ParsedToolCall> {
    let raw: ToolCallEnvelope = serde_json::from_str(json.trim()).ok()?;
    if raw.tool_call.id.trim().is_empty() || raw.tool_call.name.trim().is_empty() {
        return None;
    }
    let arguments = normalize_arguments(raw.tool_call.arguments);
    Some(ParsedToolCall {
        call_id: raw.tool_call.id,
        tool_name: raw.tool_call.name,
        arguments,
    })
}

fn normalize_arguments(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "{}".to_owned(),
        serde_json::Value::String(s) => s,
        other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_owned()),
    }
}

/// 提取 markdown ```json ... ``` 或 ``` ... ``` 代码块内容。
fn extract_json_fences(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut start_line = 0usize;
    let line_vec: Vec<&str> = text.lines().collect();
    for (i, line) in line_vec.iter().enumerate() {
        let t = line.trim();
        if !in_fence && t.starts_with("```") {
            in_fence = true;
            start_line = i + 1;
            continue;
        }
        if in_fence && t.starts_with("```") {
            if start_line < i {
                blocks.push(line_vec[start_line..i].join("\n"));
            }
            in_fence = false;
        }
    }
    blocks
}

/// 在混合文本中定位包含 `"tool_call"` 的 JSON 对象切片。
fn find_tool_call_json_slice(text: &str) -> Option<&str> {
    let key = "\"tool_call\"";
    let start = text.find(key)?;
    let obj_start = text[..start].rfind('{')?;
    let slice = &text[obj_start..];
    let end = matching_brace_end(slice)?;
    Some(&slice[..=end])
}

fn matching_brace_end(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in s.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_poc() {
        let json = r#"{"tool_call":{"id":"call-1","name":"read_file"}}"#;
        let call = parse_tool_call_from_model_output(json).expect("parse");
        assert_eq!(call.call_id, "call-1");
        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.arguments, "{}");
    }

    #[test]
    fn parse_with_object_arguments() {
        let json = r#"{"tool_call":{"id":"c2","name":"read_file","arguments":{"path":"README.md"}}}"#;
        let call = parse_tool_call_from_model_output(json).expect("parse");
        assert_eq!(call.tool_name, "read_file");
        assert!(call.arguments.contains("README.md"));
    }

    #[test]
    fn parse_from_markdown_fence() {
        let text = r#"好的，我来读文件。

```json
{"tool_call":{"id":"c3","name":"read_file","arguments":{"path":"Cargo.toml"}}}
```
"#;
        let call = parse_tool_call_from_model_output(text).expect("fence");
        assert_eq!(call.call_id, "c3");
    }

    #[test]
    fn parse_embedded_in_prose() {
        let text = r#"分析如下：{"tool_call":{"id":"c4","name":"read_file","arguments":"src/lib.rs"}} 请稍候。"#;
        let call = parse_tool_call_from_model_output(text).expect("embedded");
        assert_eq!(call.arguments, "src/lib.rs");
    }

    #[test]
    fn plain_text_returns_none() {
        assert!(parse_tool_call_from_model_output("你好，这是普通回复。").is_none());
    }
}
