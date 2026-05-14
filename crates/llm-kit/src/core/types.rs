use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
        }
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenerateRequest {
    pub model: ModelRef,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub metadata: BTreeMap<String, String>,
}

impl GenerateRequest {
    pub fn new(model: ModelRef, messages: Vec<ChatMessage>) -> Self {
        Self {
            model,
            messages,
            temperature: None,
            max_output_tokens: None,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenerateResponse {
    pub model: ModelRef,
    pub message: ChatMessage,
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamChunk {
    pub delta: String,
    pub finish_reason: Option<FinishReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCall,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub supports_chat: bool,
    pub supports_streaming: bool,
}
