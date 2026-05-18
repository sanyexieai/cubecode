//! 阻塞式 HTTP `serve`（M6-4）：`POST /rpc` → [`HttpJsonAdapter`] → ② → ④ 占位/可选 LLM。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cubecode_adapter::{Adapter, HttpJsonAdapter};
use cubecode_contracts::{ControlEvent, SessionId, TurnContext};
use cubecode_inbox::Inbox;
use cubecode_orchestrator::{
    run_full_turn, run_shutdown_turn, Orchestrator, SessionMetadata, SinkStyle, TurnRunner,
};
use cubecode_step::ToolRegistry;
use llm_kit::{ModelRef, ProviderRegistry};
use llm_kit::{ChatMessage, MessageRole};

const DEFAULT_BIND: &str = "127.0.0.1:8787";

/// 启动阻塞 HTTP 服务（单线程顺序处理连接；`Ctrl+C` 退出）。
pub fn run_serve(bind: Option<&str>, use_llm: bool, llm_stream: bool) -> Result<(), String> {
    let addr = bind
        .map(str::to_owned)
        .or_else(|| std::env::var("CUBECODE_SERVE_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_BIND.to_owned());

    let listener = TcpListener::bind(&addr).map_err(|e| format!("绑定 {addr} 失败：{e}"))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
            .map_err(|e| format!("无法注册 Ctrl+C：{e}"))?;
    }

    eprintln!(
        "cubecode serve 监听 http://{addr}（POST /rpc JSON-RPC；GET /health；⑤ {}）",
        if use_llm { "真实 LLM" } else { "占位" }
    );
    eprintln!("示例：curl -s -X POST http://{addr}/rpc -H \"Content-Type: application/json\" -d '{{\"method\":\"cubecode.echo_turn\",\"params\":{{\"text\":\"hi\"}},\"id\":1}}'");

    tracing::info!(
        target: cubecode_log::CLI,
        %addr,
        use_llm,
        stream = llm_stream,
        "serve 已启动"
    );

    for incoming in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            eprintln!("serve 已停止。");
            break;
        }
        match incoming {
            Ok(conn) => {
                if let Err(e) = handle_connection(conn, use_llm, llm_stream) {
                    tracing::warn!(target: cubecode_log::CLI, error = %e, "连接处理失败");
                }
            }
            Err(e) => {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                return Err(format!("accept 失败：{e}"));
            }
        }
    }
    Ok(())
}

fn handle_connection(mut conn: TcpStream, use_llm: bool, stream: bool) -> Result<(), String> {
    conn.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    conn.set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;

    let request = read_http_request(&mut conn)?;
    let (status, body) = dispatch_http(&request, use_llm, stream)?;
    write_http_response(&mut conn, status, "application/json", body.as_bytes())?;
    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
    body: String,
}

fn read_http_request(conn: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    loop {
        let n = conn.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if header_end.is_none() {
            header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
        }
        if let Some(end) = header_end {
            let headers = String::from_utf8_lossy(&buf[..end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    if k.eq_ignore_ascii_case("content-length") {
                        v.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if buf.len() >= end + content_length {
                break;
            }
        }
        if buf.len() > 1024 * 1024 {
            return Err("请求体过大".into());
        }
    }
    let header_end = header_end.ok_or("无效的 HTTP 请求")?;
    let header_text = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = header_text.lines();
    let request_line = lines.next().ok_or("缺少请求行")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    let body_bytes = &buf[header_end..];
    let body = String::from_utf8(body_bytes.to_vec()).map_err(|e| e.to_string())?;
    Ok(HttpRequest { method, path, body })
}

fn dispatch_http(req: &HttpRequest, use_llm: bool, stream: bool) -> Result<(u16, String), String> {
    if req.method.eq_ignore_ascii_case("GET") && (req.path == "/health" || req.path == "/") {
        let body = if req.path == "/health" {
            r#"{"ok":true,"adapter":"http_json"}"#.to_owned()
        } else {
            r#"{"service":"cubecode-serve","rpc":"POST /rpc","health":"GET /health"}"#.to_owned()
        };
        return Ok((200, body));
    }
    if req.method.eq_ignore_ascii_case("POST") && req.path == "/rpc" {
        return handle_rpc(&req.body, use_llm, stream);
    }
    Ok((
        404,
        r#"{"error":"not_found","hint":"POST /rpc 或 GET /health"}"#.to_owned(),
    ))
}

fn handle_rpc(body: &str, use_llm: bool, llm_stream: bool) -> Result<(u16, String), String> {
    let session = SessionId::generate();
    let mut http = HttpJsonAdapter::new(session.clone());
    let resp_json = http
        .handle_request_body(body)
        .map_err(|e| e.to_string())?;
    let events = http.poll_events().map_err(|e| e.to_string())?;

    if events.is_empty() {
        return Ok((200, resp_json));
    }

    let mut inbox = Inbox::with_capacity(cubecode_inbox::capacity_from_env());
    let mut orchestrator = Orchestrator::new(session.clone());
    let mut session_meta = SessionMetadata::new(session.clone());
    let tools = ToolRegistry::from_env();

    let mut llm_setup = if use_llm {
        Some(LlmServeSetup::from_env()?)
    } else {
        None
    };

    for event in events {
        match event {
            ControlEvent::UserTurn {
                session_id,
                turn_id,
                text,
            } => {
                let ctx = TurnContext::new(session_id, turn_id);
                let runner = if let Some(setup) = &mut llm_setup {
                    setup.runner_for(&text, llm_stream, &mut session_meta)
                } else {
                    TurnRunner::placeholder()
                };
                run_full_turn(
                    &ctx,
                    &mut orchestrator,
                    &mut inbox,
                    text,
                    &tools,
                    runner,
                    SinkStyle::Prefixed,
                    None,
                )?;
            }
            ControlEvent::Shutdown { session_id } => {
                let ctx = TurnContext::new(session_id, http.next_turn_id());
                run_shutdown_turn(&ctx, &mut orchestrator, &mut inbox)?;
            }
            ControlEvent::ToolResult { .. } => {
                tracing::warn!(target: cubecode_log::CLI, "serve：忽略 ToolResult 入队事件");
            }
        }
    }

    Ok((200, resp_json))
}

struct LlmServeSetup {
    registry: ProviderRegistry,
    model_ref: ModelRef,
    messages: Vec<ChatMessage>,
}

impl LlmServeSetup {
    fn from_env() -> Result<Self, String> {
        use llm_kit::{default_model_from_env, default_provider_from_env, default_registry_from_env};
        Ok(Self {
            registry: default_registry_from_env(),
            model_ref: ModelRef::new(default_provider_from_env(), default_model_from_env()),
            messages: Vec::new(),
        })
    }

    fn runner_for<'a>(
        &'a mut self,
        text: &str,
        stream: bool,
        session_meta: &'a mut SessionMetadata,
    ) -> TurnRunner<'a> {
        self.messages.clear();
        self.messages
            .push(ChatMessage::new(MessageRole::User, text.to_owned()));
        TurnRunner::llm(
            &self.registry,
            self.model_ref.clone(),
            &mut self.messages,
            stream,
            session_meta,
        )
    }
}

fn write_http_response(
    conn: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    conn.write_all(header.as_bytes())
        .and_then(|_| conn.write_all(body))
        .and_then(|_| conn.flush())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_health_and_rpc() {
        let health = HttpRequest {
            method: "GET".into(),
            path: "/health".into(),
            body: String::new(),
        };
        let (status, body) = dispatch_http(&health, false, false).expect("health");
        assert_eq!(status, 200);
        assert!(body.contains("ok"));

        let rpc = HttpRequest {
            method: "POST".into(),
            path: "/rpc".into(),
            body: r#"{"method":"cubecode.echo_turn","params":{"text":"x"},"id":1}"#.into(),
        };
        let (status, body) = dispatch_http(&rpc, false, false).expect("rpc");
        assert_eq!(status, 200);
        assert!(body.contains("echo"));
    }
}
