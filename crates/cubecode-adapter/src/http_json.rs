//! JSON-RPC / HTTP 占位适配器（M6-3）：解析 POST JSON 正文 → [`ControlEvent`]，响应回显事件。

use cubecode_contracts::{ControlEvent, SessionId, TurnContext, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapter::Adapter;
use crate::error::AdapterError;

/// JSON-RPC 2.0 请求（占位，仅 `cubecode.*` 方法）。
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// 回显结果：将本次产出的语义事件序列化返回给调用方（便于 IDE/HTTP 联调）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcEchoResult {
    pub adapter: &'static str,
    pub events: Vec<ControlEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct JsonRpcResponse<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a JsonRpcEchoResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcErrorObj>,
}

#[derive(Debug, Clone, Serialize)]
struct JsonRpcErrorObj {
    code: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
struct UserTurnParams {
    text: String,
}

/// HTTP POST 正文 → 语义事件；`poll_events` 吐出；JSON 响应回显事件列表。
#[derive(Debug)]
pub struct HttpJsonAdapter {
    session_id: SessionId,
    next_turn_id: TurnId,
    pending: Vec<ControlEvent>,
}

impl HttpJsonAdapter {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            next_turn_id: TurnId::FIRST,
            pending: Vec::new(),
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn next_turn_id(&self) -> TurnId {
        self.next_turn_id
    }

    /// 解析 JSON-RPC 请求体，生成待 `poll_events` 取出的事件，并返回 JSON-RPC 响应正文（回显）。
    pub fn handle_request_body(&mut self, body: &str) -> Result<String, AdapterError> {
        let request: JsonRpcRequest = serde_json::from_str(body)
            .map_err(|e| AdapterError::InvalidRequest(format!("JSON 解析失败：{e}")))?;
        if let Some(v) = &request.jsonrpc {
            if v != "2.0" {
                return Ok(encode_error_response(
                    request.id,
                    -32600,
                    format!("不支持的 jsonrpc 版本：{v}"),
                ));
            }
        }
        match request.method.as_str() {
            "cubecode.user_turn" | "cubecode.echo_turn" => {
                let params: UserTurnParams = serde_json::from_value(request.params).map_err(|e| {
                    AdapterError::InvalidRequest(format!("user_turn params：{e}"))
                })?;
                let turn_id = self.next_turn_id;
                self.next_turn_id = turn_id.next();
                let ctx = TurnContext::new(self.session_id.clone(), turn_id);
                let text = if request.method == "cubecode.echo_turn" {
                    format!("[echo] {}", params.text)
                } else {
                    params.text
                };
                let event = ControlEvent::user_turn(&ctx, text);
                self.pending.push(event.clone());
                tracing::info!(
                    target: "cubecode.adapter",
                    session_id = %self.session_id,
                    turn_id = %turn_id,
                    method = %request.method,
                    pending = self.pending.len(),
                    "HTTP/JSON 适配器：已受理用户轮次"
                );
                encode_ok_response(
                    request.id,
                    JsonRpcEchoResult {
                        adapter: "http_json",
                        events: vec![event],
                    },
                )
            }
            "cubecode.shutdown" => {
                let event = ControlEvent::shutdown(self.session_id.clone());
                self.pending.push(event.clone());
                tracing::info!(
                    target: "cubecode.adapter",
                    session_id = %self.session_id,
                    "HTTP/JSON 适配器：已受理关闭"
                );
                encode_ok_response(
                    request.id,
                    JsonRpcEchoResult {
                        adapter: "http_json",
                        events: vec![event],
                    },
                )
            }
            other => Ok(encode_error_response(
                request.id,
                -32601,
                format!("未知方法：{other}"),
            )),
        }
    }
}

impl Adapter for HttpJsonAdapter {
    fn id(&self) -> &'static str {
        "http_json"
    }

    fn poll_events(&mut self) -> Result<Vec<ControlEvent>, AdapterError> {
        Ok(std::mem::take(&mut self.pending))
    }
}

fn encode_ok_response(id: Option<Value>, result: JsonRpcEchoResult) -> Result<String, AdapterError> {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(&result),
        error: None,
    };
    serde_json::to_string(&resp).map_err(|e| AdapterError::InvalidRequest(e.to_string()))
}

fn encode_error_response(id: Option<Value>, code: i32, message: impl Into<String>) -> String {
    let resp = JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcErrorObj {
            code,
            message: message.into(),
        }),
    };
    serde_json::to_string(&resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"内部序列化失败"}}"#.into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{drain_adapter, MockAdapter};
    use cubecode_inbox::Inbox;

    #[test]
    fn user_turn_echo_response_and_poll() {
        let session = SessionId::new("s-http");
        let mut adapter = HttpJsonAdapter::new(session.clone());
        let body = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "cubecode.user_turn",
            "params": { "text": "你好" }
        }"#;
        let resp = adapter.handle_request_body(body).expect("handle");
        assert!(resp.contains("user_turn"));
        assert!(resp.contains("你好"));

        let events = adapter.poll_events().expect("poll");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ControlEvent::UserTurn { .. }));
        assert!(adapter.poll_events().expect("empty").is_empty());
    }

    #[test]
    fn echo_turn_prefixes_text() {
        let session = SessionId::new("s-echo");
        let mut adapter = HttpJsonAdapter::new(session);
        let resp = adapter
            .handle_request_body(
                r#"{"method":"cubecode.echo_turn","params":{"text":"ping"},"id":"a"}"#,
            )
            .expect("handle");
        assert!(resp.contains("[echo] ping"));
        let events = adapter.poll_events().expect("poll");
        match &events[0] {
            ControlEvent::UserTurn { text, .. } => assert_eq!(text, "[echo] ping"),
            _ => panic!("expected user turn"),
        }
    }

    #[test]
    fn shutdown_enqueued() {
        let session = SessionId::new("s-shutdown");
        let mut adapter = HttpJsonAdapter::new(session);
        adapter
            .handle_request_body(r#"{"method":"cubecode.shutdown","id":2}"#)
            .expect("handle");
        let events = adapter.poll_events().expect("poll");
        assert!(matches!(events[0], ControlEvent::Shutdown { .. }));
    }

    #[test]
    fn unknown_method_returns_json_rpc_error() {
        let mut adapter = HttpJsonAdapter::new(SessionId::new("s-err"));
        let resp = adapter
            .handle_request_body(r#"{"method":"foo.bar","id":3}"#)
            .expect("handle");
        assert!(resp.contains("未知方法"));
        assert!(adapter.poll_events().expect("poll").is_empty());
    }

    #[test]
    fn http_and_mock_adapters_share_inbox() {
        let session = SessionId::new("s-share");
        let ctx = TurnContext::new(session.clone(), TurnId::FIRST);
        let mut inbox = Inbox::with_capacity(8);

        let mut http = HttpJsonAdapter::new(session.clone());
        http.handle_request_body(
            r#"{"method":"cubecode.user_turn","params":{"text":"via-http"},"id":1}"#,
        )
        .expect("http");
        assert_eq!(drain_adapter(&mut http, &mut inbox).expect("drain http"), 1);

        let mut mock = MockAdapter::with_events([ControlEvent::user_turn(&ctx, "via-mock")]);
        assert_eq!(drain_adapter(&mut mock, &mut inbox).expect("drain mock"), 1);

        assert_eq!(inbox.len(), 2);
        let first = inbox.pop().expect("first");
        let second = inbox.pop().expect("second");
        assert_eq!(first.user_text(), Some("via-http"));
        assert_eq!(second.user_text(), Some("via-mock"));
    }
}
