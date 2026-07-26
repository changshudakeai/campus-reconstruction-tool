//! 标签映射表配置（tag_rules）——全工程集中定义的唯一一处（ADR-0009/0011）。
//!
//! 配置为 JSON 格式：`{ "version": "1.0", "rules": [{ "category": "建筑", "tags": [...] }] }`。
//! 标签模式三种写法：
//! - `"building=school"`：key=value 精确匹配；
//! - `"highway=*"`：key 通配（该 key 任意值都命中）；
//! - `"building=dorm*"`：value 前缀模糊匹配。
//!
//! 默认映射表内嵌于 `config/tag-rules.json`，与采集查询范围同源管理
//! （ADR-0012；查询范围本身由后续 F4 工单落实）。

use serde::{Deserialize, Serialize};

use crate::error::TransformError;

/// 内嵌的默认标签映射表（集中定义的唯一一处）。
const DEFAULT_TAG_RULES_JSON: &str = include_str!("../config/tag-rules.json");

/// 单条标签模式。
///
/// 语义见模块文档：`key=value` 精确、`key=*` 通配、`key=prefix*` 前缀。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TagPattern(String);

impl TagPattern {
    /// 从模式字符串创建（如 `"building=school"`、`"highway=*"`）。
    pub fn new(pattern: impl Into<String>) -> Self {
        Self(pattern.into())
    }

    /// 模式原文。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 检查是否命中给定的 key=value 标签。
    pub fn matches(&self, key: &str, value: &str) -> bool {
        match self.0.split_once('=') {
            Some((pattern_key, pattern_value)) => {
                if pattern_key != key {
                    return false;
                }
                if pattern_value == "*" {
                    return true;
                }
                if let Some(prefix) = pattern_value.strip_suffix('*') {
                    return value.starts_with(prefix);
                }
                pattern_value == value
            }
            // 无 '=' 的模式按 value 精确匹配（key 不限）
            None => self.0 == value,
        }
    }
}

/// 单个类别的规则条目。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRuleEntry {
    /// 目标类别（`CandidateCategory` 的中文显示名：建筑/道路/水域/植被/体育/其他）
    pub category: String,
    /// 命中即归入该类别的标签模式数组
    pub tags: Vec<TagPattern>,
}

impl TagRuleEntry {
    /// 创建规则条目。
    pub fn new(category: impl Into<String>, tags: Vec<TagPattern>) -> Self {
        Self {
            category: category.into(),
            tags,
        }
    }
}

/// 标签映射表配置（JSON 反序列化根）。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifyConfig {
    /// 配置格式版本（当前 "1.0"）
    pub version: String,
    /// 标签规则列表
    pub rules: Vec<TagRuleEntry>,
}

impl ClassifyConfig {
    /// 从规则列表创建配置。
    pub fn new(rules: Vec<TagRuleEntry>) -> Self {
        Self {
            version: "1.0".to_owned(),
            rules,
        }
    }

    /// 从 JSON 字符串解析配置。
    pub fn from_json(json: &str) -> Result<Self, TransformError> {
        serde_json::from_str(json)
            .map_err(|parse_error| TransformError::InvalidConfigJson(parse_error.to_string()))
    }

    /// 加载内嵌的默认标签映射表（ADR-0011 修订版：含泳池、围墙、校门等新规则）。
    pub fn default_mapping() -> Self {
        Self::from_json(DEFAULT_TAG_RULES_JSON)
            .expect("内嵌默认映射表必须是合法 JSON（由本 crate 测试保证）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pattern_matches_key_value() {
        let pattern = TagPattern::new("building=school");
        assert!(pattern.matches("building", "school"));
        assert!(!pattern.matches("building", "supermarket"));
        assert!(!pattern.matches("leisure", "school"));
    }

    #[test]
    fn key_wildcard_matches_any_value() {
        let pattern = TagPattern::new("highway=*");
        assert!(pattern.matches("highway", "footway"));
        assert!(pattern.matches("highway", "service"));
        assert!(!pattern.matches("railway", "rail"));
    }

    #[test]
    fn prefix_pattern_matches_value_prefix() {
        let pattern = TagPattern::new("building=dorm*");
        assert!(pattern.matches("building", "dormitory"));
        assert!(!pattern.matches("building", "school"));
    }

    #[test]
    fn default_mapping_parses() {
        let config = ClassifyConfig::default_mapping();
        assert_eq!(config.version, "1.0");
        assert!(!config.rules.is_empty());
    }
}
