//! 会话与轮次标识（日志、事件协议共用）。

use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 一次 CLI / 连接级会话。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// 使用已有字符串作为会话 id（测试、外部 Adapter 注入）。
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// 生成新会话 id（`sess-{unix_ms}`）。
    pub fn generate() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Self(format!("sess-{ms}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 会话内单调递增的轮次（从 1 开始）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(u32);

impl TurnId {
    pub const FIRST: Self = Self(1);

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl Display for TurnId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 单轮流水线上下文（贯穿 ①～⑥ 日志字段）。
#[derive(Debug, Clone)]
pub struct TurnContext {
    pub session_id: SessionId,
    pub turn_id: TurnId,
}

impl TurnContext {
    pub fn new(session_id: SessionId, turn_id: TurnId) -> Self {
        Self {
            session_id,
            turn_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_non_empty() {
        let s = SessionId::generate();
        assert!(s.as_str().starts_with("sess-"));
    }

    #[test]
    fn turn_id_advances() {
        assert_eq!(TurnId::FIRST.get(), 1);
        assert_eq!(TurnId::FIRST.next().get(), 2);
    }
}
