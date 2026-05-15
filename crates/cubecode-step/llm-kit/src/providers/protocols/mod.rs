//! 线路协议实现目录：每种协议单独文件，统一走 [`crate::core::provider::LlmProvider`]。
//!
//! Rust 没有类继承；「协议」层用 [`ProtocolBinding`] 标记（超 trait 仅为 [`crate::core::provider::LlmProvider`]），
//! 具体请求/流式解析在各子模块中实现。

use crate::core::provider::LlmProvider;

pub mod anthropic_messages;
pub mod chat_completions;

pub use anthropic_messages::AnthropicMessagesProvider;
pub use chat_completions::OpenAiCompatibleProvider;

/// 标记一种**独立 HTTP/JSON 线格式**的协议实现；与 [`crate::core::provider::LlmProvider`] 语义相同，便于分类与后续加横切能力。
///
/// 新增协议：实现 `LlmProvider` + 在本模块末尾 `impl ProtocolBinding for YourType`。
pub trait ProtocolBinding: LlmProvider {}

impl ProtocolBinding for OpenAiCompatibleProvider {}

impl ProtocolBinding for AnthropicMessagesProvider {}
