//! 记忆配置（M5-4）：环境变量与默认值。

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::store::default_storage_root;
use super::types::DEFAULT_TOP_K;

/// 是否启用记忆召回。
pub const ENV_MEMORY_ENABLED: &str = "CUBECODE_MEMORY_ENABLED";
/// 召回条数上限（正整数）。
pub const ENV_MEMORY_TOP_K: &str = "CUBECODE_MEMORY_TOP_K";
/// 存储模式：`memory` | `md`/`markdown` | `sqlite`/`db`/`database`。
pub const ENV_MEMORY_STORAGE: &str = "CUBECODE_MEMORY_STORAGE";
/// 存储根目录（Markdown 会话文件、SQLite `memory.db`）。
pub const ENV_MEMORY_PATH: &str = "CUBECODE_MEMORY_PATH";

/// 写入 [`GenerateRequest::metadata`]，供 pipeline 读取 top-k。
pub const META_MEMORY_TOP_K: &str = "cubecode.memory.top_k";

/// 记忆持久化方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryStorageMode {
    /// 进程内 `HashMap`（默认）。
    #[default]
    Memory,
    /// 每会话一个 `.md` 文件。
    Markdown,
    /// SQLite `memory.db`。
    Sqlite,
}

impl MemoryStorageMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "memory" | "in_memory" | "ram" => Some(Self::Memory),
            "md" | "markdown" | "file" => Some(Self::Markdown),
            "sqlite" | "db" | "database" => Some(Self::Sqlite),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Markdown => "markdown",
            Self::Sqlite => "sqlite",
        }
    }
}

/// 记忆开关、召回条数与存储模式（从环境变量加载）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub top_k: usize,
    pub storage: MemoryStorageMode,
    pub storage_path: PathBuf,
}

impl MemoryConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: parse_enabled_env(),
            top_k: memory_top_k_from_env(),
            storage: memory_storage_mode_from_env(),
            storage_path: memory_storage_path_from_env(),
        }
    }

    /// 关闭记忆时的占位配置（测试或默认路径）。
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            top_k: DEFAULT_TOP_K,
            storage: MemoryStorageMode::Memory,
            storage_path: default_storage_root(),
        }
    }

    /// 将 top-k 写入 metadata（仅当启用时）。
    pub fn stamp_metadata(&self, values: &mut BTreeMap<String, String>) {
        if self.enabled {
            values.insert(META_MEMORY_TOP_K.into(), self.top_k.to_string());
        } else {
            values.remove(META_MEMORY_TOP_K);
        }
    }
}

/// 是否启用记忆（`CUBECODE_MEMORY_ENABLED=1|true|yes|on`）。
pub fn memory_enabled_from_env() -> bool {
    MemoryConfig::from_env().enabled
}

/// 从 `CUBECODE_MEMORY_TOP_K` 读取召回条数；无效或未设置时用 [`DEFAULT_TOP_K`]。
pub fn memory_top_k_from_env() -> usize {
    std::env::var(ENV_MEMORY_TOP_K)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|&k| k > 0)
        .unwrap_or(DEFAULT_TOP_K)
}

pub fn memory_storage_mode_from_env() -> MemoryStorageMode {
    std::env::var(ENV_MEMORY_STORAGE)
        .ok()
        .and_then(|v| MemoryStorageMode::parse(&v))
        .unwrap_or_default()
}

pub fn memory_storage_path_from_env() -> PathBuf {
    std::env::var(ENV_MEMORY_PATH)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_storage_root)
}

fn parse_enabled_env() -> bool {
    std::env::var(ENV_MEMORY_ENABLED)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_storage_modes() {
        assert_eq!(
            MemoryStorageMode::parse("markdown"),
            Some(MemoryStorageMode::Markdown)
        );
        assert_eq!(
            MemoryStorageMode::parse("sqlite"),
            Some(MemoryStorageMode::Sqlite)
        );
        assert_eq!(MemoryStorageMode::parse("db"), Some(MemoryStorageMode::Sqlite));
    }
}
