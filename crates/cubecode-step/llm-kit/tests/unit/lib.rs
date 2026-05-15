use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct RetryOnceProvider {
    calls: Arc<AtomicUsize>,
}

impl LlmProvider for RetryOnceProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "retry-once".to_owned(),
            display_name: "Retry Once".to_owned(),
            supports_chat: true,
            supports_streaming: true,
        }
    }

    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Err(LlmError::ProviderFailure("http 503: overloaded".to_owned()));
        }

        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, "ok"),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        })
    }
}

struct RetryStreamOnceProvider {
    calls: Arc<AtomicUsize>,
}

impl LlmProvider for RetryStreamOnceProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "retry-stream-once".to_owned(),
            display_name: "Retry Stream Once".to_owned(),
            supports_chat: true,
            supports_streaming: true,
        }
    }

    fn generate(&self, _request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        unreachable!("streaming test should not call non-streaming method")
    }

    fn generate_stream(
        &self,
        request: &GenerateRequest,
        on_chunk: &mut dyn FnMut(StreamChunk) -> Result<(), LlmError>,
    ) -> Result<GenerateResponse, LlmError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Err(LlmError::ProviderFailure("http 429: rate limit".to_owned()));
        }

        on_chunk(StreamChunk {
            delta: "ok".to_owned(),
            finish_reason: None,
        })?;
        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, "ok"),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        })
    }
}

#[test]
fn registry_returns_error_for_missing_provider() {
    let registry = ProviderRegistry::new();
    let request = GenerateRequest::new(
        ModelRef::new("openai", "gpt-4.1-mini"),
        vec![ChatMessage::new(MessageRole::User, "route this")],
    );

    let error = registry
        .generate(&request)
        .expect_err("missing provider should fail");
    assert!(matches!(error, LlmError::ProviderNotFound(provider) if provider == "openai"));
}

#[test]
fn registry_retries_retryable_generate_failures() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::new();
    registry.set_retry_policy(LlmRetryPolicy {
        max_attempts: 2,
        base_delay_ms: 0,
        log_retries: false,
    });
    registry.register(RetryOnceProvider {
        calls: calls.clone(),
    });
    let request = GenerateRequest::new(
        ModelRef::new("retry-once", "mock"),
        vec![ChatMessage::new(MessageRole::User, "hello")],
    );

    let response = registry.generate(&request).expect("retry should succeed");
    assert_eq!(response.message.content, "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn registry_retries_stream_failures_before_first_chunk() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::new();
    registry.set_retry_policy(LlmRetryPolicy {
        max_attempts: 2,
        base_delay_ms: 0,
        log_retries: false,
    });
    registry.register(RetryStreamOnceProvider {
        calls: calls.clone(),
    });
    let request = GenerateRequest::new(
        ModelRef::new("retry-stream-once", "mock"),
        vec![ChatMessage::new(MessageRole::User, "hello")],
    );
    let mut output = String::new();

    let response = registry
        .generate_stream(&request, &mut |chunk| {
            output.push_str(&chunk.delta);
            Ok(())
        })
        .expect("retry should succeed");

    assert_eq!(response.message.content, "ok");
    assert_eq!(output, "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn registry_stops_after_configured_retry_attempts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::new();
    registry.set_retry_policy(LlmRetryPolicy {
        max_attempts: 1,
        base_delay_ms: 0,
        log_retries: false,
    });
    registry.register(RetryOnceProvider {
        calls: calls.clone(),
    });
    let request = GenerateRequest::new(
        ModelRef::new("retry-once", "mock"),
        vec![ChatMessage::new(MessageRole::User, "hello")],
    );

    let error = registry
        .generate(&request)
        .expect_err("single attempt should return first error");

    assert!(matches!(error, LlmError::ProviderFailure(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn retry_policy_backoff_is_exponential() {
    let policy = LlmRetryPolicy {
        max_attempts: 3,
        base_delay_ms: 100,
        log_retries: false,
    };

    assert_eq!(
        policy.backoff_for_attempt(1),
        std::time::Duration::from_millis(100)
    );
    assert_eq!(
        policy.backoff_for_attempt(2),
        std::time::Duration::from_millis(200)
    );
}

#[test]
fn sanitizer_removes_structural_hidden_assistant_markup() {
    let sanitized = sanitize_assistant_text(
        r#"visible
<think>hidden reasoning</think>
$SKILL tool.frontend
<minimax:tool_call>
<parameter name="command">run</parameter>
</minimax:tool_call>
done"#,
    );

    assert!(sanitized.contains("visible"));
    assert!(sanitized.contains("done"));
    assert!(!sanitized.contains("hidden reasoning"));
    assert!(!sanitized.contains("$SKILL"));
    assert!(!sanitized.contains("tool_call"));
    assert!(!sanitized.contains("parameter"));
}

#[test]
fn deepseek_provider_preset_uses_openai_compatible_endpoint() {
    let preset = provider_preset("deepseek").expect("deepseek preset should exist");

    assert_eq!(preset.default_base_url, "https://api.deepseek.com");
    assert_eq!(preset.balanced_model, "deepseek-v4-flash");
    assert_eq!(preset.coding_model, "deepseek-v4-pro");
    assert_eq!(preset.wire, WireProtocol::ChatCompletions);
    assert_eq!(provider_api_key_var_name("deepseek"), "DEEPSEEK_API_KEY");
    assert_eq!(provider_base_url_var_name("deepseek"), "DEEPSEEK_BASE_URL");
}

#[test]
fn anthropic_provider_preset_uses_messages_api() {
    let preset = provider_preset("anthropic").expect("anthropic preset should exist");

    assert_eq!(preset.default_base_url, "https://api.anthropic.com");
    assert_eq!(preset.wire, WireProtocol::AnthropicMessages);
    assert_eq!(provider_api_key_var_name("anthropic"), "ANTHROPIC_API_KEY");
    assert_eq!(provider_base_url_var_name("anthropic"), "ANTHROPIC_BASE_URL");
}

#[test]
fn openai_compatible_response_accepts_extra_content_shapes() {
    let response: OpenAiChatResponse = serde_json::from_str(
        r#"{
          "id": "chatcmpl-test",
          "choices": [{
            "message": {
              "role": "assistant",
              "content": [
                {"type": "text", "text": "hello"},
                {"type": "text", "text": " world"}
              ],
              "reasoning_content": "hidden"
            },
            "finish_reason": "stop"
          }],
          "usage": {"total_tokens": 12}
        }"#,
    )
    .expect("response should decode");
    let choice = response.choices.into_iter().next().unwrap();

    assert_eq!(
        choice.message.content.map(content_to_string).unwrap(),
        "hello world"
    );
    assert_eq!(response.usage.unwrap().prompt_tokens.unwrap_or_default(), 0);
}

#[test]
fn openai_compatible_response_falls_back_to_reasoning_content() {
    let response: OpenAiChatResponse = serde_json::from_str(
        r#"{
          "choices": [{
            "message": {
              "role": "assistant",
              "content": null,
              "reasoning_content": "reasoned answer"
            },
            "finish_reason": "stop"
          }]
        }"#,
    )
    .expect("response should decode");
    let choice = response.choices.into_iter().next().unwrap();

    assert_eq!(message_content_to_string(choice.message), "reasoned answer");
}

struct MetadataEchoProvider;

impl LlmProvider for MetadataEchoProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "echo-meta".to_owned(),
            display_name: "Echo metadata".to_owned(),
            supports_chat: true,
            supports_streaming: false,
        }
    }

    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        let v = request
            .metadata
            .get("x-pipeline")
            .cloned()
            .unwrap_or_else(|| "none".to_owned());
        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, v),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        })
    }
}

struct StampPipelineNode;

impl PipelineStage for StampPipelineNode {
    fn id(&self) -> &'static str {
        "stamp"
    }

    fn before_generate(&self, ctx: &mut PipelineContext) -> Result<(), LlmError> {
        ctx.request
            .metadata
            .insert("x-pipeline".into(), "stamped".into());
        Ok(())
    }
}

#[test]
fn pipeline_before_generate_can_mutate_request_for_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(MetadataEchoProvider);
    registry.set_pipeline(Some(
        Pipeline::builder().push(StampPipelineNode).build(),
    ));

    let request = GenerateRequest::new(
        ModelRef::new("echo-meta", "m"),
        vec![ChatMessage::new(MessageRole::User, "hi")],
    );
    let out = registry.generate(&request).expect("generate");
    assert_eq!(out.message.content, "stamped");
}

#[test]
fn pipeline_with_no_stages_behaves_like_no_pipeline() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::new();
    registry.set_retry_policy(LlmRetryPolicy {
        max_attempts: 2,
        base_delay_ms: 0,
        log_retries: false,
    });
    registry.set_pipeline(Some(Pipeline::builder().build()));
    registry.register(RetryOnceProvider {
        calls: calls.clone(),
    });
    let request = GenerateRequest::new(
        ModelRef::new("retry-once", "mock"),
        vec![ChatMessage::new(MessageRole::User, "hello")],
    );

    let response = registry.generate(&request).expect("retry should succeed");
    assert_eq!(response.message.content, "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

struct OkFlowProvider;

impl LlmProvider for OkFlowProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "flow-ok".to_owned(),
            display_name: "Flow Ok".to_owned(),
            supports_chat: true,
            supports_streaming: false,
        }
    }

    fn generate(&self, request: &GenerateRequest) -> Result<GenerateResponse, LlmError> {
        Ok(GenerateResponse {
            model: request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, "from-llm"),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        })
    }
}

struct PassThroughLlmFlowNode;

impl FlowNode for PassThroughLlmFlowNode {
    fn id(&self) -> &'static str {
        "pass_llm"
    }

    fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
        let response = ctx.llm.generate(&ctx.request)?;
        ctx.response = Some(response);
        Ok(())
    }
}

#[test]
fn flow_node_calls_llm_via_context() {
    let mut registry = ProviderRegistry::new();
    registry.register(OkFlowProvider);
    let flow = FlowPipeline::builder().push(PassThroughLlmFlowNode).build();
    let request = GenerateRequest::new(
        ModelRef::new("flow-ok", "any"),
        vec![ChatMessage::new(MessageRole::User, "hi")],
    );
    let out = registry
        .generate_with_flow(request, &flow)
        .expect("flow should succeed");
    assert_eq!(out.message.content, "from-llm");
}

struct FanOutFlowNode;

impl FlowNode for FanOutFlowNode {
    fn id(&self) -> &'static str {
        "fanout"
    }

    fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
        let a = ctx.llm.generate(&ctx.request)?;
        ctx.branch_outputs.insert("a".into(), a);
        let b = ctx.llm.generate(&ctx.request)?;
        ctx.branch_outputs.insert("b".into(), b);
        Ok(())
    }
}

struct MergeBranchesFlowNode;

impl FlowNode for MergeBranchesFlowNode {
    fn id(&self) -> &'static str {
        "merge"
    }

    fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
        let a = ctx.branch_outputs.get("a").cloned();
        let b = ctx.branch_outputs.get("b").cloned();
        let (Some(ra), Some(rb)) = (a, b) else {
            return Err(LlmError::InvalidRequest("missing branch_outputs".into()));
        };
        let merged = format!("{}|{}", ra.message.content, rb.message.content);
        ctx.response = Some(GenerateResponse {
            model: ctx.request.model.clone(),
            message: ChatMessage::new(MessageRole::Assistant, merged),
            finish_reason: FinishReason::Stop,
            usage: None,
            raw: None,
        });
        Ok(())
    }
}

#[test]
fn flow_branch_outputs_one_to_many_then_merge() {
    let mut registry = ProviderRegistry::new();
    registry.register(OkFlowProvider);
    let flow = FlowPipeline::builder()
        .push(FanOutFlowNode)
        .push(MergeBranchesFlowNode)
        .build();
    let request = GenerateRequest::new(
        ModelRef::new("flow-ok", "any"),
        vec![ChatMessage::new(MessageRole::User, "hi")],
    );
    let out = registry
        .generate_with_flow(request, &flow)
        .expect("merge branches");
    assert_eq!(out.message.content, "from-llm|from-llm");
}

#[test]
fn flow_empty_pipeline_errors_without_response() {
    let mut registry = ProviderRegistry::new();
    registry.register(OkFlowProvider);
    let flow = FlowPipeline::builder().build();
    let request = GenerateRequest::new(
        ModelRef::new("flow-ok", "any"),
        vec![ChatMessage::new(MessageRole::User, "hi")],
    );
    let err = registry
        .generate_with_flow(request, &flow)
        .expect_err("need response");
    assert!(matches!(err, LlmError::InvalidRequest(_)));
}

#[test]
fn flow_default_output_node_fills_when_none() {
    struct NoopFlowNode;

    impl FlowNode for NoopFlowNode {
        fn id(&self) -> &'static str {
            "noop"
        }

        fn run(&self, _ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
            Ok(())
        }
    }

    /// 与 `llm-node-output-default` 同语义；测试内联以避免 path 依赖下双份 `llm-kit` trait 不兼容。
    struct InlineDefaultOutput;

    impl FlowNode for InlineDefaultOutput {
        fn id(&self) -> &'static str {
            "default_output"
        }

        fn run(&self, ctx: &mut FlowContext<'_>) -> Result<(), LlmError> {
            if ctx.response.is_some() {
                return Ok(());
            }
            ctx.response = Some(GenerateResponse {
                model: ctx.request.model.clone(),
                message: ChatMessage::new(MessageRole::Assistant, "fallback-inline"),
                finish_reason: FinishReason::Stop,
                usage: None,
                raw: None,
            });
            Ok(())
        }
    }

    let mut registry = ProviderRegistry::new();
    registry.register(OkFlowProvider);
    let flow = FlowPipeline::builder()
        .push(NoopFlowNode)
        .push(InlineDefaultOutput)
        .build();
    let request = GenerateRequest::new(
        ModelRef::new("flow-ok", "any"),
        vec![ChatMessage::new(MessageRole::User, "hi")],
    );
    let out = registry
        .generate_with_flow(request, &flow)
        .expect("default output");
    assert_eq!(out.message.content, "fallback-inline");
}
