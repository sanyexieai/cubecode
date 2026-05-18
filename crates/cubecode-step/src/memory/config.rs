//! 记忆配置（M5-4）：环境变量与默认值。

use std::collections::BTreeMap;

use super::types::DEFAULT_TOP_K;

/// 是否启用记忆召回。
pub const ENV_MEMORY_ENABLED: &str = "CUBECODE_MEMORY_ENABLED";
/// 召回条数上限（正整数）。
pub const ENV_MEMORY_TOP_K: &str = "CUBECODE_MEMORY_TOP_K";

/// 写入 [`GenerateRequest::metadata`]，供 pipeline 读取 top-k。
pub const META_MEMORY_TOP_K: &str = "cubecode.memory.top_k";

/// 记忆开关与召回条数（从环境变量加载）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub top_k: usize,
}

impl MemoryConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: parse_enabled_env(),
            top_k: memory_top_k_from_env(),
        }
    }

    /// 关闭记忆时的占位配置（测试或默认路径）。
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            top_k: DEFAULT_TOP_K,
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
    fn disabled_config_has_default_top_k() {
        let cfg = MemoryConfig::disabled();
        assert!(!cfg.enabled);
        assert_eq!(cfg.top_k, DEFAULT_TOP_K);
    }

    #[test]
    fn stamp_metadata_only_when_enabled() {
        let mut map = BTreeMap::new();
        MemoryConfig::disabled().stamp_metadata(&mut map);
        assert!(!map.contains_key(META_MEMORY_TOP_K));
        let enabled = MemoryConfig {
            enabled: true,
            top_k: 3,
        };
        enabled.stamp_metadata(&mut map);
        assert_eq!(map.get(META_MEMORY_TOP_K).map(String::as_str), Some("3"));
    }
}
