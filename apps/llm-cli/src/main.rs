//! `llm-kit` 命令行：不传子命令时默认多轮聊天；另有 `providers` / `env` / `complete` / `six-layer-pipeline`。
//!
//! 启动时从当前工作目录**向上**查找第一个 `.env` 并加载（dotenvy：不覆盖已在环境里设置的变量）。
//! 与 llm-kit 库约定一致，支持 `LLM_PROVIDER`、`LLM_MODEL`、`LLM_API_KEY`、`LLM_BASE_URL` 等。

use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use llm_kit::{
    default_model_from_env, default_provider_from_env, default_registry_from_env,
    provider_presets, ChatMessage, GenerateRequest, MessageRole, ModelRef, WireProtocol,
};

#[derive(Parser)]
#[command(
    name = "llm-kit",
    version,
    about = "llm-kit CLI：默认聊天；子命令 providers / env / complete / six-layer-pipeline",
    long_about = None,
    subcommand_required = false,
    arg_required_else_help = false
)]
struct Cli {
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
}

fn main() -> ExitCode {
    load_dotenv();
    let cli = Cli::parse();
    let result = match cli.command {
        Some(cmd) => run(cmd),
        None => run_default_chat(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Commands) -> Result<(), String> {
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

            one_shot_chat(&user_text)?;
            Ok(())
        }
        Commands::SixLayerPipeline => run_six_layer_minimal(),
    }
}

/// ①→②→③→④→⑤→⑥ 占位串联（无网络、无真实 LLM）。
fn run_six_layer_minimal() -> Result<(), String> {
    println!("=== six-layer pipeline (placeholder) ===");
    let mut inbox = cubecode_inbox::Inbox::new();
    println!("① adapter → ② inbox: enqueue user line");
    cubecode_adapter::enqueue_user_line(&mut inbox, "hello cubecode");
    println!("② inbox: pop one event");
    let ev = inbox
        .pop()
        .ok_or_else(|| "inbox unexpectedly empty".to_owned())?;
    println!("③ dispatch: route");
    let route = cubecode_dispatch::route(&ev);
    println!("④ orchestrator → ⑤ step → ⑥ sink");
    cubecode_orchestrator::run_minimal_pipeline(route, &ev)?;
    println!("=== done ===");
    Ok(())
}

/// 不传子命令：终端上多轮对话；管道 stdin 则读入一次并回复。
fn run_default_chat() -> Result<(), String> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        eprintln!("多轮对话（上下文会保留）。退出：/exit 或 :q 或 Ctrl+Z/Ctrl+D");
        let mut reader = stdin.lock();
        let mut line = String::new();
        let provider = default_provider_from_env();
        let model = default_model_from_env();
        let registry = default_registry_from_env();
        let mut messages: Vec<ChatMessage> = Vec::new();

        loop {
            eprint!("> ");
            io::stderr().flush().map_err(|e| e.to_string())?;
            line.clear();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                break;
            }
            let input = line.trim();
            if input.is_empty() {
                continue;
            }
            if input == "/exit" || input == ":q" {
                break;
            }

            messages.push(ChatMessage::new(MessageRole::User, input));
            let request = GenerateRequest::new(
                ModelRef::new(provider.clone(), model.clone()),
                messages.clone(),
            );
            let response = registry.generate(&request).map_err(|e| e.to_string())?;
            let content = response.message.content.clone();
            println!("\n{}\n", content.trim_end());
            messages.push(ChatMessage::new(MessageRole::Assistant, content));
        }
        Ok(())
    } else {
        let user_text = read_stdin_to_string()?;
        if user_text.trim().is_empty() {
            return Err("stdin 为空；终端下直接运行可无参进入对话".to_owned());
        }
        one_shot_chat(user_text.trim())?;
        Ok(())
    }
}

fn one_shot_chat(user_text: &str) -> Result<(), String> {
    let registry = default_registry_from_env();
    let provider = default_provider_from_env();
    let model = default_model_from_env();
    let request = GenerateRequest::new(
        ModelRef::new(provider, model),
        vec![ChatMessage::new(MessageRole::User, user_text)],
    );
    let response = registry.generate(&request).map_err(|e| e.to_string())?;
    println!("{}", response.message.content.trim_end());
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
