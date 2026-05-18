//! `llm-kit` 命令行：不传子命令时默认多轮聊天；另有 `providers` / `env` / `complete` / `six-layer-pipeline`。
//!
//! 启动时从当前工作目录**向上**查找第一个 `.env` 并加载（dotenvy：不覆盖已在环境里设置的变量）。
//! 与 llm-kit 库约定一致，支持 `LLM_PROVIDER`、`LLM_MODEL`、`LLM_API_KEY`、`LLM_BASE_URL` 等。

mod serve;

use std::io::{self, IsTerminal, Read};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::Ordering;

use clap::{Parser, Subcommand};
use cubecode_adapter::{TerminalAdapter, TerminalPoll};
use cubecode_contracts::{ControlEvent, SessionId, TurnContext, TurnId};
use cubecode_inbox::Inbox;
use cubecode_orchestrator::{
    cancel_active_turn, new_cancel_flag, run_full_turn, run_shutdown_turn, Orchestrator,
    SessionMetadata, SinkStyle, TurnRunner, USER_CANCELLED_MSG,
};
use cubecode_sink::emit_error_global;
use cubecode_step::{
    attach_memory_store, memory_store_from_config, MemoryChunk, MemoryConfig, MemoryStore,
    ToolRegistry, ENV_MEMORY_ENABLED, ENV_MEMORY_STORAGE,
};
use llm_kit::{
    default_model_from_env, default_provider_from_env, default_registry_from_env,
    provider_presets, ChatMessage, MessageRole, ModelRef, WireProtocol,
};

#[derive(Parser)]
#[command(
    name = "llm-kit",
    version,
    about = "llm-kit CLI：默认聊天；子命令 providers / env / complete / logs / serve / six-layer-pipeline",
    long_about = None,
    subcommand_required = false,
    arg_required_else_help = false
)]
struct Cli {
    /// 提高日志冗长度（`-v` / `-vv`）；未设时读 `RUST_LOG`
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// 禁用流式输出（默认对聊天 / complete 使用流式，逐块打印助手回复）
    #[arg(long, global = true)]
    no_stream: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 打印内置厂商预设（id、协议、默认 URL / 模型）
    Providers,
    /// 解析当前环境下的默认 provider / model（不打印密钥）
    Env,
    /// 发送一条 user 消息并打印助手回复
    Complete {
        /// User 消息；缺省则从 stdin 读入全文
        #[arg(short, long)]
        message: Option<String>,
    },
    /// 六层占位全流程：adapter → inbox → dispatch → orchestrator → step → sink（不调真实 LLM）
    SixLayerPipeline,
    /// 阻塞 HTTP 服务：`POST /rpc`（JSON-RPC）→ ① `HttpJsonAdapter` → ②～⑥（默认 ⑤ 占位）
    Serve {
        /// 监听地址（默认 `127.0.0.1:8787` 或 `CUBECODE_SERVE_ADDR`）
        #[arg(long, value_name = "ADDR")]
        bind: Option<String>,
        /// 使用真实 LLM（需配置 API Key）；默认仅 ⑤ 占位
        #[arg(long)]
        llm: bool,
    },
    /// 查看已落盘日志（类似 `docker logs`）；`-f` 先打已有内容再跟随，默认读 `latest.log`
    Logs {
        /// 会话 id（对应 `.cubecode/logs/{id}.log`）；省略或 `latest` 表示 latest.log
        #[arg(value_name = "SESSION")]
        session: Option<String>,
        /// 持续跟随新日志（`-f`）
        #[arg(short, long)]
        follow: bool,
        /// 只显示最后 N 行
        #[arg(long)]
        tail: Option<usize>,
    },
}

fn main() -> ExitCode {
    load_dotenv();
    let cli = Cli::parse();
    let skip_logging = matches!(cli.command, Some(Commands::Logs { .. }));
    if !skip_logging {
        init_logging(cli.verbose);
    }
    let stream = !cli.no_stream;
    let result = match cli.command {
        Some(cmd) => run(cmd, stream),
        None => run_default_chat(stream),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            emit_error_global(&e);
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Commands, stream: bool) -> Result<(), String> {
    match cmd {
        Commands::Providers => {
            println!("{:<12} {:<22} {:<24} {}", "id", "wire", "default_base_url", "balanced_model");
            for p in provider_presets() {
                println!(
                    "{:<12} {:<22} {:<24} {}",
                    p.id,
                    wire_label(p.wire),
                    p.default_base_url,
                    p.balanced_model
                );
            }
            Ok(())
        }
        Commands::Env => {
            let provider = default_provider_from_env();
            let model = default_model_from_env();
            let preset = llm_kit::provider_preset(&provider);
            println!("default_provider: {provider}");
            println!("default_model:    {model}");
            if let Some(p) = preset {
                println!("preset.display:     {}", p.display_name);
                println!("preset.wire:        {}", wire_label(p.wire));
                println!("preset.base_url:    {}", p.default_base_url);
            }
            let key_hint = if llm_kit::provider_api_key_from_env(&provider).is_some() {
                "set (value hidden)"
            } else {
                "not set"
            };
            println!("api_key:            {key_hint}");
            Ok(())
        }
        Commands::Complete { message } => {
            let user_text = match message {
                Some(m) if !m.trim().is_empty() => m,
                Some(_) => return Err("message is empty".to_owned()),
                None => read_stdin_to_string()?,
            };
            if user_text.trim().is_empty() {
                return Err("no user message: pass -m/--message or pipe stdin".to_owned());
            }

            one_shot_chat(&user_text, stream)?;
            Ok(())
        }
        Commands::SixLayerPipeline => run_six_layer_minimal(),
        Commands::Serve { bind, llm } => serve::run_serve(bind.as_deref(), llm, stream),
        Commands::Logs {
            session,
            follow,
            tail,
        } => cubecode_log::show_logs(cubecode_log::LogsOptions {
            session,
            follow,
            tail,
        }),
    }
}

/// ①→②→③→④→⑤→⑥ 占位串联（无网络、无真实 LLM）。
fn init_logging(verbose: u8) {
    if verbose > 0 {
        cubecode_log::init_cli(verbose);
    } else {
        cubecode_log::init_from_env();
    }
}

fn run_six_layer_minimal() -> Result<(), String> {
    tracing::info!(target: cubecode_log::CLI, "六层演示开始（⑤执行层为占位）");
    println!("=== 六层流水线演示（⑤执行层占位）===");
    let mut inbox = Inbox::with_capacity(cubecode_inbox::capacity_from_env());
    let session = SessionId::generate();
    let mut orchestrator = Orchestrator::new(session.clone());
    let ctx = TurnContext::new(session, TurnId::FIRST);
    let tools = ToolRegistry::from_env();
    run_full_turn(
        &ctx,
        &mut orchestrator,
        &mut inbox,
        "hello cubecode",
        &tools,
        TurnRunner::placeholder(),
        SinkStyle::Prefixed,
        None,
    )?;
    tracing::info!(target: cubecode_log::CLI, "六层演示结束");
    println!("=== 完成 ===");
    Ok(())
}

/// 不传子命令：终端上多轮对话；管道 stdin 则读入一次并回复。
fn run_default_chat(stream: bool) -> Result<(), String> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        let mode = if stream {
            "流式"
        } else {
            "非流式"
        };
        let stream_hint = if stream {
            "；加 `--no-stream` 可改为一次性输出"
        } else {
            ""
        };
        eprintln!(
            "多轮对话（{mode}{stream_hint}）。全链路诊断日志：`llm-kit logs -f`（终端默认不刷屏；调试可加 -v）。\
             退出：/exit 或 :q；取消进行中轮次：/cancel 或 Ctrl+C（空闲时 Ctrl+C 退出）"
        );
        let cancel_flag = new_cancel_flag();
        {
            let flag = cancel_flag.clone();
            ctrlc::set_handler(move || {
                flag.store(true, Ordering::SeqCst);
            })
            .map_err(|e| format!("无法注册 Ctrl+C 处理：{e}"))?;
        }
        let reader = stdin.lock();
        let provider = default_provider_from_env();
        let model = default_model_from_env();
        let model_ref = ModelRef::new(provider, model);
        let mut registry = default_registry_from_env();
        let memory_cfg = MemoryConfig::from_env();
        let memory_store = memory_store_from_config(&memory_cfg).map_err(|e| e.to_string())?;
        if let Some(store) = &memory_store {
            attach_memory_store(&mut registry, store.clone());
            tracing::info!(
                target: cubecode_log::CLI,
                top_k = memory_cfg.top_k,
                storage = memory_cfg.storage.as_str(),
                path = %memory_cfg.storage_path.display(),
                env = ENV_MEMORY_ENABLED,
                storage_env = ENV_MEMORY_STORAGE,
                "记忆召回已启用"
            );
        }
        let tools = ToolRegistry::from_env();
        let mut inbox = Inbox::with_capacity(cubecode_inbox::capacity_from_env());
        let mut messages: Vec<ChatMessage> = Vec::new();
        let session = SessionId::generate();
        let mut orchestrator = Orchestrator::new(session.clone());
        let mut session_meta = SessionMetadata::new(session.clone());
        tracing::info!(
            target: cubecode_log::CLI,
            session_id = %session,
            stream,
            "会话开始"
        );
        let mut terminal = TerminalAdapter::new(session.clone(), reader);

        loop {
            if cancel_flag.swap(false, Ordering::SeqCst) {
                if orchestrator.is_turn_active() {
                    cancel_active_turn(&mut orchestrator, &mut inbox);
                    eprintln!("已取消进行中的轮次。");
                } else {
                    eprintln!("退出对话。");
                    break;
                }
                continue;
            }

            match terminal.poll().map_err(|e| e.to_string())? {
                TerminalPoll::Idle => continue,
                TerminalPoll::Eof => break,
                TerminalPoll::Cancel => {
                    if cancel_active_turn(&mut orchestrator, &mut inbox) {
                        eprintln!("已取消进行中的轮次。");
                    } else {
                        eprintln!("当前没有进行中的轮次。");
                    }
                }
                TerminalPoll::Events(events) => {
                    for event in events {
                        match event {
                            ControlEvent::Shutdown { .. } => {
                                if orchestrator.is_turn_active() {
                                    eprintln!("仍有进行中的轮次，请先 /cancel 再退出。");
                                    continue;
                                }
                                let turn_ctx = TurnContext::new(
                                    session.clone(),
                                    terminal.next_turn_id(),
                                );
                                run_shutdown_turn(&turn_ctx, &mut orchestrator, &mut inbox)?;
                                return Ok(());
                            }
                            ControlEvent::UserTurn { turn_id, text, .. } => {
                                let turn_ctx = TurnContext::new(session.clone(), turn_id);
                                let input = text.as_str();
                                let transcript_before = messages.len();
                                messages.push(ChatMessage::new(MessageRole::User, &text));
                                let runner = TurnRunner::llm(
                                    &registry,
                                    model_ref.clone(),
                                    &mut messages,
                                    stream,
                                    &mut session_meta,
                                );
                                match run_full_turn(
                                    &turn_ctx,
                                    &mut orchestrator,
                                    &mut inbox,
                                    input,
                                    &tools,
                                    runner,
                                    SinkStyle::Assistant,
                                    Some(cancel_flag.as_ref()),
                                ) {
                                    Ok(finished) => {
                                        if let Some(store) = &memory_store {
                                            remember_turn_exchange(
                                                store.as_ref(),
                                                &session,
                                                turn_ctx.turn_id,
                                                input,
                                                finished.user_reply(),
                                            );
                                        }
                                    }
                                    Err(ref e) if e.contains(USER_CANCELLED_MSG) => {
                                        messages.truncate(transcript_before);
                                        cancel_flag.store(false, Ordering::SeqCst);
                                    }
                                    Err(_) => {
                                        // ⑥ 已在编排层 emit_error，继续下一轮
                                    }
                                }
                            }
                            ControlEvent::ToolResult { .. } => {
                                tracing::warn!(
                                    target: cubecode_log::CLI,
                                    "终端适配器不应直接产出 ToolResult，已忽略"
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    } else {
        let user_text = read_stdin_to_string()?;
        if user_text.trim().is_empty() {
            return Err("stdin 为空；终端下直接运行可无参进入对话".to_owned());
        }
        one_shot_chat(user_text.trim(), stream)?;
        Ok(())
    }
}

fn remember_turn_exchange(
    store: &dyn MemoryStore,
    session: &SessionId,
    turn_id: TurnId,
    user_text: &str,
    assistant_reply: Option<&str>,
) {
    if let Err(e) = store.remember(
        session,
        MemoryChunk {
            id: format!("u-{turn_id}"),
            content: user_text.to_owned(),
            source: Some("user".into()),
        },
    ) {
        tracing::warn!(target: "cubecode.cli", %e, "写入用户记忆失败");
    }
    if let Some(reply) = assistant_reply.filter(|s| !s.is_empty()) {
        if let Err(e) = store.remember(
            session,
            MemoryChunk {
                id: format!("a-{turn_id}"),
                content: reply.to_owned(),
                source: Some("assistant".into()),
            },
        ) {
            tracing::warn!(target: "cubecode.cli", %e, "写入助手记忆失败");
        }
    }
}

fn one_shot_chat(user_text: &str, stream: bool) -> Result<(), String> {
    let mut registry = default_registry_from_env();
    let memory_cfg = MemoryConfig::from_env();
    let memory_store = memory_store_from_config(&memory_cfg).map_err(|e| e.to_string())?;
    if let Some(store) = &memory_store {
        attach_memory_store(&mut registry, store.clone());
    }
    let provider = default_provider_from_env();
    let model = default_model_from_env();
    let model_ref = ModelRef::new(provider, model);
    let mut messages = vec![ChatMessage::new(MessageRole::User, user_text)];
    let mut inbox = Inbox::with_capacity(cubecode_inbox::capacity_from_env());
    let session = SessionId::generate();
    let mut orchestrator = Orchestrator::new(session.clone());
    let mut session_meta = SessionMetadata::new(session.clone());
    let turn_ctx = TurnContext::new(session.clone(), TurnId::FIRST);
    let tools = ToolRegistry::from_env();
    let runner = TurnRunner::llm(
        &registry,
        model_ref,
        &mut messages,
        stream,
        &mut session_meta,
    );
    let finished = run_full_turn(
        &turn_ctx,
        &mut orchestrator,
        &mut inbox,
        user_text,
        &tools,
        runner,
        SinkStyle::Assistant,
        None,
    )?;
    if let Some(store) = &memory_store {
        remember_turn_exchange(
            store.as_ref(),
            &session,
            turn_ctx.turn_id,
            user_text,
            finished.user_reply(),
        );
    }
    Ok(())
}

fn read_stdin_to_string() -> Result<String, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

fn wire_label(w: WireProtocol) -> &'static str {
    match w {
        WireProtocol::ChatCompletions => "chat_completions",
        WireProtocol::AnthropicMessages => "anthropic_messages",
    }
}

/// 自当前目录起向父目录查找 `.env`，找到则加载（已设置的同名环境变量不会被覆盖）。
fn load_dotenv() {
    let Ok(cwd) = std::env::current_dir() else {
        let _ = dotenvy::dotenv();
        return;
    };

    let mut dir: Option<&Path> = Some(cwd.as_path());
    for _ in 0..16 {
        let Some(d) = dir else {
            break;
        };
        let candidate = d.join(".env");
        if candidate.is_file() {
            let _ = dotenvy::from_path(&candidate);
            return;
        }
        dir = d.parent();
    }
}
