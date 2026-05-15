//! Anthropic Messages API：`POST /v1/messages`、SSE 流式事件（`content_block_delta` 等）。
//! 与 Chat Completions 形态不同；见 [`crate::providers::preset::WireProtocol`]。

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

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicMessagesProvider {
    info: ProviderInfo,
    base_url: String,
    api_key: String,
    client: Client,
}

impl AnthropicMessagesProvider {
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

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

impl LlmProvider for AnthropicMessagesProvider {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        let (system, messages) = build_anthropic_messages(request)?;
        let max_tokens = anthropic_max_tokens(request);
        let body = AnthropicRequest {
            model: request.model.model.clone(),
            max_tokens,
            messages,
            system,
            temperature: request.temperature,
            stream: false,
        };

        let response = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
        let parsed: AnthropicResponse = serde_json::from_str(&response_text).map_err(|error| {
            LlmError::ProviderFailure(format!(
                "failed to decode anthropic response: {}; body: {}",
                error,
                compact_error_body(&response_text)
            ))
        })?;

        let text = anthropic_text_from_content(&parsed.content);
        let finish_reason = anthropic_stop_reason(parsed.stop_reason.as_deref());
        let usage = parsed.usage.map(|u| TokenUsage {
            input_tokens: u.input_tokens.unwrap_or_default(),
            output_tokens: u.output_tokens.unwrap_or_default(),
        });

        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: text,
                name: None,
            },
            finish_reason,
            usage,
            raw: serde_json::from_str(&response_text).ok(),
        })
    }

    fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        let (system, messages) = build_anthropic_messages(request)?;
        let max_tokens = anthropic_max_tokens(request);
        let body = AnthropicRequest {
            model: request.model.model.clone(),
            max_tokens,
            messages,
            system,
            temperature: request.temperature,
            stream: true,
        };

        let response = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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

        let mut accumulated = String::new();
        let mut finish_reason = FinishReason::Stop;
        let mut raw_events = Vec::new();
        let mut reader = BufReader::new(response);
        let mut line = String::new();
        let mut block: Vec<String> = Vec::new();

        loop {
            line.clear();
            let read = reader
                .read_line(&mut line)
                .map_err(|error| LlmError::ProviderFailure(error.to_string()))?;
            if read == 0 {
                if !block.is_empty() {
                    process_anthropic_sse_block(&block, on_chunk, &mut accumulated, &mut finish_reason, &mut raw_events)?;
                }
                break;
            }

            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                if !block.is_empty() {
                    process_anthropic_sse_block(&block, on_chunk, &mut accumulated, &mut finish_reason, &mut raw_events)?;
                    block.clear();
                }
                continue;
            }
            block.push(trimmed.to_owned());
        }

        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: accumulated,
                name: None,
            },
            finish_reason,
            usage: None,
            raw: Some(serde_json::Value::Array(raw_events)),
        })
    }
}

fn process_anthropic_sse_block(
    block: &[String],
    on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    accumulated: &mut String,
    finish_reason: &mut FinishReason,
    raw_events: &mut Vec<serde_json::Value>,
) -> Result<(), LlmError> {
    let mut data_json: Option<&str> = None;
    for entry in block {
        if let Some(rest) = entry.strip_prefix("data:") {
            data_json = Some(rest.trim());
        }
    }
    let Some(data) = data_json else {
        return Ok(());
    };
    let value: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    raw_events.push(value.clone());

    if value.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
        if let Some(delta) = value.get("delta") {
            if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                    accumulated.push_str(text);
                    on_chunk(StreamChunk {
                        delta: text.to_owned(),
                        finish_reason: None,
                    })?;
                }
            }
        }
    }

    if value.get("type").and_then(|t| t.as_str()) == Some("message_delta") {
        if let Some(delta) = value.get("delta") {
            if let Some(reason) = delta.get("stop_reason").and_then(|r| r.as_str()) {
                *finish_reason = anthropic_stop_reason(Some(reason));
            }
        }
    }

    if value.get("type").and_then(|t| t.as_str()) == Some("error") {
        let msg = value
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("anthropic stream error");
        return Err(LlmError::ProviderFailure(msg.to_owned()));
    }

    Ok(())
}

fn anthropic_text_from_content(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| {
            if b.block_type == "text" {
                b.text.clone()
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn anthropic_stop_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") | Some("pause_turn") | Some("stop_sequence") => FinishReason::ToolCall,
        Some("end_turn") | None => FinishReason::Stop,
        _ => FinishReason::Error,
    }
}

fn anthropic_max_tokens(request: &GenerateRequest) -> u32 {
    request.max_output_tokens.unwrap_or(4096).max(1)
}

fn build_anthropic_messages(
    request: &GenerateRequest,
) -> Result<(Option<String>, Vec<AnthropicMessage>), LlmError> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut out: Vec<AnthropicMessage> = Vec::new();

    for message in &request.messages {
        match message.role {
            MessageRole::System => {
                if !message.content.trim().is_empty() {
                    system_parts.push(message.content.clone());
                }
            }
            MessageRole::User => out.push(AnthropicMessage {
                role: "user".to_owned(),
                content: message.content.clone(),
            }),
            MessageRole::Assistant => out.push(AnthropicMessage {
                role: "assistant".to_owned(),
                content: message.content.clone(),
            }),
            MessageRole::Tool => {
                let name = message
                    .name
                    .as_deref()
                    .unwrap_or("tool");
                out.push(AnthropicMessage {
                    role: "user".to_owned(),
                    content: format!("[tool output:{name}]\n{}", message.content),
                });
            }
        }
    }

    if out.is_empty() {
        return Err(LlmError::InvalidRequest(
            "anthropic messages: need at least one user or assistant message".to_owned(),
        ));
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    Ok((system, out))
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
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
