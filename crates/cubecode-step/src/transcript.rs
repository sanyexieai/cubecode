//! 单轮对话 transcript：工具调用与结果写入消息列表（M4-6）。

use llm_kit::{ChatMessage, MessageRole};

/// 将模型返回的工具请求（原始助手正文）记入 transcript。
pub fn append_assistant_tool_request(messages: &mut Vec<ChatMessage>, assistant_content: &str) {
    messages.push(ChatMessage::new(
        MessageRole::Assistant,
        assistant_content.trim(),
    ));
}

/// 将工具执行结果记入 transcript（`name` 为 `call_id`，供部分协议关联）。
pub fn append_tool_result(
    messages: &mut Vec<ChatMessage>,
    call_id: &str,
    output: impl Into<String>,
) {
    messages.push(
        ChatMessage::new(MessageRole::Tool, output.into()).named(call_id),
    );
}

/// 将最终助手正文记入 transcript。
pub fn append_assistant_reply(messages: &mut Vec<ChatMessage>, content: &str) {
    messages.push(ChatMessage::new(MessageRole::Assistant, content));
}

/// 工具一轮：助手 tool 请求 + 工具输出。
pub fn append_tool_exchange(
    messages: &mut Vec<ChatMessage>,
    assistant_content: &str,
    call_id: &str,
    output: impl Into<String>,
) {
    append_assistant_tool_request(messages, assistant_content);
    append_tool_result(messages, call_id, output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_exchange_extends_messages() {
        let mut messages = vec![ChatMessage::new(MessageRole::User, "hi")];
        append_tool_exchange(
            &mut messages,
            r#"{"tool_call":{"id":"c1","name":"read_file"}}"#,
            "c1",
            "FILE_BODY",
        );
        assert_eq!(messages.len(), 3);
        assert!(matches!(messages[1].role, MessageRole::Assistant));
        assert!(matches!(messages[2].role, MessageRole::Tool));
        assert_eq!(messages[2].content, "FILE_BODY");
        assert_eq!(messages[2].name.as_deref(), Some("c1"));
    }
}
