//! 归类引擎：原始对象标签 → 六类别之一。
//!
//! 逻辑：逐条查映射表 → 多类命中按 B1 优先级取最高（建筑 > 体育 > 水域 >
//! 道路 > 植被 > 其他，ADR-0011）→ 未命中归"其他"并带明确兜底信号
//! （`is_fallback = true`），不猜测、不静默丢弃。

use std::collections::BTreeMap;

use shared_domain_types::CandidateCategory;

use crate::config::{ClassifyConfig, TagPattern};
use crate::error::TransformError;
use crate::validator::TagMappingValidator;

/// 原始观测对象的标签集（key → value）。
///
/// B13 只依赖 B1 共享类型，不依赖 B12 数据源适配器（ADR-0017）；
/// F4 从 B12 拿到原始对象后，把其标签以此形式递给本引擎（窗口契约缝 3）。
pub type TagMap = BTreeMap<String, String>;

/// 归类结果。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// 归入的类别（六类别之一）
    pub category: CandidateCategory,
    /// 兜底信号：true = 未命中任何映射规则，按纪律进"其他"（评审队列可见）
    pub is_fallback: bool,
    /// 命中的模式原文（诊断与评审可见性用；兜底时为空）
    pub matched_patterns: Vec<String>,
}

/// 归类引擎。
///
/// 构造时校验映射表（校验不过不开工，杜绝静默丢弃）；
/// 归类本身是纯内存查表，不做任何 IO。
#[derive(Debug, Clone)]
pub struct ClassifyEngine {
    config: ClassifyConfig,
    /// 预解析的 (类别, 模式) 规则表
    rules: Vec<(CandidateCategory, TagPattern)>,
}

impl ClassifyEngine {
    /// 用给定映射表创建引擎。
    ///
    /// 配置先过 [`TagMappingValidator`] 全量校验，任何一条不过即拒绝创建
    /// （返回第一条错误）。
    pub fn new(config: ClassifyConfig) -> Result<Self, TransformError> {
        if let Err(mut errors) = TagMappingValidator::validate(&config) {
            return Err(errors.remove(0));
        }
        let mut rules = Vec::new();
        for entry in &config.rules {
            let category = parse_category_from_key(&entry.category_tkey)?;
            for pattern in &entry.tags {
                rules.push((category, pattern.clone()));
            }
        }
        Ok(Self { config, rules })
    }

    /// 用内嵌的默认映射表创建引擎。
    pub fn with_default_mapping() -> Result<Self, TransformError> {
        Self::new(ClassifyConfig::default_mapping())
    }

    /// 归类单个原始对象的标签集。
    ///
    /// 多标签命中多个类别时按优先级取最高；一个都不命中时归"其他"
    /// 并置 `is_fallback = true`（禁止静默丢弃）。
    pub fn classify(&self, tags: &TagMap) -> Classification {
        let mut best: Option<CandidateCategory> = None;
        let mut matched_patterns = Vec::new();

        for (key, value) in tags {
            for (category, pattern) in &self.rules {
                if pattern.matches(key, value) {
                    matched_patterns.push(pattern.as_str().to_owned());
                    best = Some(match best {
                        Some(current) if current.priority() >= category.priority() => current,
                        _ => *category,
                    });
                }
            }
        }

        match best {
            Some(category) => Classification {
                category,
                is_fallback: false,
                matched_patterns,
            },
            // "其他"类兜底：不猜测、不丢弃，带明确信号进评审队列
            None => Classification {
                category: CandidateCategory::Other,
                is_fallback: true,
                matched_patterns: Vec::new(),
            },
        }
    }

    /// 当前使用的映射表配置。
    pub fn config(&self) -> &ClassifyConfig {
        &self.config
    }
}

/// 从文本键解析类别（如"collection.category_building" → Building）。
pub(crate) fn parse_category_from_key(tkey: &str) -> Result<CandidateCategory, TransformError> {
    match tkey {
        "collection.category_building" => Ok(CandidateCategory::Building),
        "collection.category_road" => Ok(CandidateCategory::Road),
        "collection.category_water" => Ok(CandidateCategory::Water),
        "collection.category_vegetation" => Ok(CandidateCategory::Vegetation),
        "collection.category_sports" => Ok(CandidateCategory::Sports),
        "collection.category_other" => Ok(CandidateCategory::Other),
        unknown => Err(TransformError::UnknownCategoryKey(unknown.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_rejected() {
        let error = ClassifyEngine::new(ClassifyConfig::new(Vec::new())).unwrap_err();
        assert!(matches!(error, TransformError::EmptyRuleSet));
    }

    #[test]
    fn default_mapping_builds_engine() {
        let engine = ClassifyEngine::with_default_mapping().expect("默认映射表必须可用");
        assert!(!engine.config().rules.is_empty());
    }

    #[test]
    fn unknown_category_key_is_rejected() {
        let config = ClassifyConfig::new(vec![crate::config::TagRuleEntry::new(
            "collection.category_unknown",
            vec![TagPattern::new("building=yes")],
        )]);
        let error = ClassifyEngine::new(config).unwrap_err();
        assert!(matches!(error, TransformError::UnknownCategoryKey(key) if key == "collection.category_unknown"));
    }
}
