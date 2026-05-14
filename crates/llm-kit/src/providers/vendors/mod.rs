//! 各厂商的默认端点、默认模型、环境变量名与 [`crate::providers::preset::WireProtocol`]。
//!
//! 具体 HTTP 编解码在 [`crate::providers::protocols`] 下的各协议子模块。

pub mod anthropic;
pub mod deepseek;
pub mod minimax;
pub mod openai;
