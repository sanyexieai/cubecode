//! ④ 会话范围 metadata：在进入 ⑤ 前写入，供 pipeline / 记忆召回消费（M5-3）。

use std::collections::BTreeMap;

use cubecode_contracts::{ControlEvent, SessionId, TurnContext};
use cubecode_step::{
    MemoryConfig, META_SESSION_ID, META_TURN_ID, META_USER_TEXT,
};

/// 当前 ③ 路由（写入 metadata，便于 pipeline 区分）。
pub const META_ROUTE: &str = "cubecode.route";
/// 工具回灌时的 `call_id`。
pub const META_TOOL_CALL_ID: &str = "cubecode.tool_call_id";

/// 单会话内跨轮次保留的 metadata（键值由 ④ 维护，⑤ 合并进 [`GenerateRequest`]）。
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    session_id: SessionId,
    values: BTreeMap<String, String>,
    /// 最近一次用户轮次正文（工具回灌时继续供记忆检索）。
    last_user_text: Option<String>,
    memory: MemoryConfig,
}

impl SessionMetadata {
    pub fn new(session_id: SessionId) -> Self {
        Self::with_memory_config(session_id, MemoryConfig::from_env())
    }

    pub fn with_memory_config(session_id: SessionId, memory: MemoryConfig) -> Self {
        let mut values = BTreeMap::new();
        values.insert(META_SESSION_ID.into(), session_id.as_str().into());
        memory.stamp_metadata(&mut values);
        Self {
            session_id,
            values,
            last_user_text: None,
            memory,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    /// 在调用 ⑤ 之前，按本轮 `TurnContext` 与事件刷新 metadata。
    pub fn prepare_for_step(&mut self, turn_ctx: &TurnContext, event: &ControlEvent) {
        self.values
            .insert(META_SESSION_ID.into(), turn_ctx.session_id.as_str().into());
        self.values
            .insert(META_TURN_ID.into(), turn_ctx.turn_id.to_string());

        match event {
            ControlEvent::UserTurn { text, .. } => {
                self.last_user_text = Some(text.clone());
                self.values.insert(META_USER_TEXT.into(), text.clone());
                self.values.insert(META_ROUTE.into(), "chat_turn".into());
                self.values.remove(META_TOOL_CALL_ID);
            }
            ControlEvent::ToolResult { call_id, .. } => {
                self.values.insert(META_ROUTE.into(), "tool_follow_up".into());
                self.values
                    .insert(META_TOOL_CALL_ID.into(), call_id.clone());
                if let Some(text) = &self.last_user_text {
                    self.values.insert(META_USER_TEXT.into(), text.clone());
                }
            }
            ControlEvent::Shutdown { .. } => {
                self.values.insert(META_ROUTE.into(), "exit".into());
            }
        }

        self.memory.stamp_metadata(&mut self.values);
        let user_text_bytes = self
            .values
            .get(META_USER_TEXT)
            .map(|s| s.len())
            .unwrap_or(0);

        tracing::info!(
            target: "cubecode.orchestrator",
            session_id = %turn_ctx.session_id,
            turn_id = %turn_ctx.turn_id,
            route = self.values.get(META_ROUTE).map(String::as_str),
            metadata_keys = self.values.len(),
            user_text_bytes,
            memory_enabled = self.memory.enabled,
            memory_top_k = self.memory.top_k,
            "④编排层：session 范围 metadata 已写入（进入 ⑤ 前）"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubecode_contracts::TurnId;

    #[test]
    fn prepare_user_turn_sets_route_and_text() {
        let session = SessionId::new("s-meta");
        let mut meta =
            SessionMetadata::with_memory_config(session.clone(), MemoryConfig::disabled());
        let ctx = TurnContext::new(session, TurnId::FIRST);
        let event = ControlEvent::user_turn(&ctx, "你好");
        meta.prepare_for_step(&ctx, &event);
        assert_eq!(meta.as_map().get(META_ROUTE).map(String::as_str), Some("chat_turn"));
        assert_eq!(meta.as_map().get(META_USER_TEXT).map(String::as_str), Some("你好"));
    }

    #[test]
    fn prepare_tool_result_keeps_user_text() {
        let session = SessionId::new("s-tool-meta");
        let mut meta =
            SessionMetadata::with_memory_config(session.clone(), MemoryConfig::disabled());
        let ctx = TurnContext::new(session, TurnId::FIRST);
        meta.prepare_for_step(&ctx, &ControlEvent::user_turn(&ctx, "读文件"));
        meta.prepare_for_step(
            &ctx,
            &ControlEvent::ToolResult {
                session_id: ctx.session_id.clone(),
                turn_id: TurnId::FIRST,
                call_id: "c1".into(),
                output: "body".into(),
            },
        );
        assert_eq!(
            meta.as_map().get(META_ROUTE).map(String::as_str),
            Some("tool_follow_up")
        );
        assert_eq!(meta.as_map().get(META_TOOL_CALL_ID).map(String::as_str), Some("c1"));
        assert_eq!(meta.as_map().get(META_USER_TEXT).map(String::as_str), Some("读文件"));
    }

    #[test]
    fn enabled_memory_stamps_top_k_in_metadata() {
        use cubecode_step::META_MEMORY_TOP_K;

        let session = SessionId::new("s-mem-cfg");
        let mut meta = SessionMetadata::with_memory_config(
            session.clone(),
            MemoryConfig {
                enabled: true,
                top_k: 7,
            },
        );
        let ctx = TurnContext::new(session, TurnId::FIRST);
        meta.prepare_for_step(&ctx, &ControlEvent::user_turn(&ctx, "hi"));
        assert_eq!(meta.as_map().get(META_MEMORY_TOP_K).map(String::as_str), Some("7"));
    }
}
