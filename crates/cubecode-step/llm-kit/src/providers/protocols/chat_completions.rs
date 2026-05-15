//! Chat Completions 传输层：请求 `POST …/chat/completions`、SSE 流式解析等。
//! 与具体厂商无关；MiniMax、DeepSeek 等只要实现同一套 JSON 形态即可复用。

use std::env;
use std::error::Error as _;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::core::error::LlmError;
use crate::core::provider::LlmProvider;
use crate::core::types::{
    ChatMessage, FinishReason, GenerateRequest, GenerateResponse, MessageRole, ProviderInfo,
    StreamChunk, TokenUsage,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    info: ProviderInfo,
    base_url: String,
    api_key: String,
    client: Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, LlmError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(LlmError::InvalidRequest("missing api key".to_owned()));
        }

        let client = Client::builder()
            .timeout(request_timeout_from_env())
            .build()
            .map_err(|error| LlmError::ProviderFailure(error.to_string()))?;

        Ok(Self {
            info: ProviderInfo {
                id: id.into(),
                display_name: display_name.into(),
                supports_chat: true,
                supports_streaming: true,
            },
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            client,
        })
    }
}

impl LlmProvider for OpenAiCompatibleProvider {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        let body = build_openai_chat_request(request, false);

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| LlmError::ProviderFailure(format_transport_error(&error)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_owned());
            return Err(LlmError::ProviderFailure(format!(
                "http {}: {}",
                status.as_u16(),
                text
            )));
        }

        let response_text = response
            .text()
            .map_err(|error| LlmError::ProviderFailure(format_transport_error(&error)))?;
        let raw: OpenAiChatResponse = serde_json::from_str(&response_text).map_err(|error| {
            LlmError::ProviderFailure(format!(
                "failed to decode chat response: {}; body: {}",
                error,
                compact_error_body(&response_text)
            ))
        })?;

        let choice = raw
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::ProviderFailure("missing choice".to_owned()))?;

        let message = choice.message;
        let name = message.name.clone();

        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage {
                role: parse_openai_role(&message.role),
                content: message_content_to_string(message),
                name,
            },
            finish_reason: parse_finish_reason(choice.finish_reason.as_deref()),
            usage: raw.usage.map(|usage| TokenUsage {
                input_tokens: usage.prompt_tokens.unwrap_or_default(),
                output_tokens: usage.completion_tokens.unwrap_or_default(),
            }),
            raw: raw.raw,
        })
    }

    fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        let body = build_openai_chat_request(request, true);
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| LlmError::ProviderFailure(format_transport_error(&error)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_owned());
            return Err(LlmError::ProviderFailure(format!(
                "http {}: {}",
                status.as_u16(),
                text
            )));
        }

        let mut assistant_role = MessageRole::Assistant;
        let mut accumulated = String::new();
        let mut finish_reason = FinishReason::Stop;
        let mut raw_chunks = Vec::new();
        let mut reader = BufReader::new(response);
        let mut line = String::new();

        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|error| LlmError::ProviderFailure(error.to_string()))?;
            if read == 0 {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Some(payload) = trimmed.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                break;
            }

            let chunk: OpenAiChatStreamChunk = serde_json::from_str(payload)
                .map_err(|error| LlmError::ProviderFailure(error.to_string()))?;
            raw_chunks.push(
                serde_json::to_value(&chunk)
                    .map_err(|error| LlmError::ProviderFailure(error.to_string()))?,
            );

            for choice in chunk.choices {
                if let Some(role) = choice.delta.role.as_deref() {
                    assistant_role = parse_openai_role(role);
                }
                if let Some(content) = choice.delta.content.map(content_to_string) {
                    accumulated.push_str(&content);
                    on_chunk(StreamChunk {
                        delta: content,
                        finish_reason: None,
                    })?;
                }
                if let Some(reason) = choice.finish_reason.as_deref() {
                    finish_reason = parse_finish_reason(Some(reason));
                }
            }
        }

        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage {
                role: assistant_role,
                content: accumulated,
                name: None,
            },
            finish_reason,
            usage: None,
            raw: Some(serde_json::Value::Array(raw_chunks)),
        })
    }
}

fn format_transport_error(error: &reqwest::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(current) = source {
        let message = current.to_string();
        if !message.is_empty() && !parts.iter().any(|part| part == &message) {
            parts.push(message);
        }
        source = current.source();
    }
    parts.join(": ")
}

fn request_timeout_from_env() -> Duration {
    env_u64_any(&["LLM_REQUEST_TIMEOUT_SECS", "HC_LLM_REQUEST_TIMEOUT_SECS"])
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(180))
}

fn env_u64_any(keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| env::var(key).ok()?.trim().parse().ok())
}

fn compact_error_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > 800 {
        let mut truncated = compact.chars().take(800).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        compact
    }
}

fn build_openai_chat_request(request: &GenerateRequest, stream: bool) -> OpenAiChatRequest {
    OpenAiChatRequest {
        model: request.model.model.clone(),
        messages: request
            .messages
            .iter()
            .map(|message| OpenAiMessage {
                role: openai_role(&message.role).to_owned(),
                content: Some(OpenAiMessageContent::Text(message.content.clone())),
                reasoning_content: None,
                name: message.name.clone(),
            })
            .collect(),
        temperature: request.temperature,
        max_tokens: request.max_output_tokens,
        stream,
    }
}

fn openai_role(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn parse_openai_role(role: &str) -> MessageRole {
    match role {
        "system" => MessageRole::System,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}

fn parse_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolCall,
        Some("stop") | None => FinishReason::Stop,
        _ => FinishReason::Error,
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OpenAiMessage {
    pub(crate) role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<OpenAiMessageContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_content: Option<OpenAiMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum OpenAiMessageContent {
    Text(String),
    Parts(Vec<OpenAiMessageContentPart>),
    Other(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OpenAiMessageContentPart {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

pub(crate) fn content_to_string(content: OpenAiMessageContent) -> String {
    match content {
        OpenAiMessageContent::Text(text) => text,
        OpenAiMessageContent::Parts(parts) => parts
            .into_iter()
            .filter_map(|part| part.text)
            .collect::<Vec<_>>()
            .join(""),
        OpenAiMessageContent::Other(value) => match value {
            serde_json::Value::String(text) => text,
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        },
    }
}

pub(crate) fn message_content_to_string(message: OpenAiMessage) -> String {
    let content = message.content.map(content_to_string).unwrap_or_default();
    if !content.trim().is_empty() {
        return content;
    }
    message
        .reasoning_content
        .map(content_to_string)
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChatResponse {
    pub(crate) choices: Vec<OpenAiChoice>,
    pub(crate) usage: Option<OpenAiUsage>,
    #[serde(flatten)]
    raw: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChatStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiStreamDelta {
    role: Option<String>,
    content: Option<OpenAiMessageContent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChoice {
    pub(crate) message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiUsage {
    pub(crate) prompt_tokens: Option<u32>,
    pub(crate) completion_tokens: Option<u32>,
}
