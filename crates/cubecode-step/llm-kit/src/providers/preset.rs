/// 与厂商无关的「线路协议」：决定用哪套 HTTP/JSON 形态发请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireProtocol {
    /// `POST …/chat/completions`（Bearer 等，OpenAI 兼容形）。
    ChatCompletions,
    /// Anthropic Messages API（`x-api-key` + `anthropic-version`）。
    AnthropicMessages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub balanced_model: &'static str,
    pub fast_model: &'static str,
    pub coding_model: &'static str,
    pub wire: WireProtocol,
}
