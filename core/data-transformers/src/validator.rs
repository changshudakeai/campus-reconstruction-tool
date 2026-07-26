//! 标签映射表校验：所有五大类都必须有规则，规则不得被静默丢弃。
//!
//! 校验项（缺一即拒）：
//! 1. 映射表非空；
//! 2. 每条规则的类别名必须是六类别之一（写错类别名 = 规则被静默丢弃，禁止）；
//! 3. 每条规则至少有一个标签模式（空规则等同静默丢弃）；
//! 4. 五大类（建筑/体育/水域/道路/植被）每类至少一条规则——避免
//!    "建筑类没有规则，所有 building=* 都进其他"的事故；
//!    "其他"类可以没有显式规则（引擎自动兜底）。

use std::collections::HashSet;

use shared_domain_types::CandidateCategory;

use crate::config::ClassifyConfig;
use crate::engine::parse_category;
use crate::error::TransformError;

/// 五大类（"其他"由引擎兜底，不强制显式规则）。
const REQUIRED_CATEGORIES: &[CandidateCategory] = &[
    CandidateCategory::Building,
    CandidateCategory::Sports,
    CandidateCategory::Water,
    CandidateCategory::Road,
    CandidateCategory::Vegetation,
];

/// 标签映射表校验器。
pub struct TagMappingValidator;

impl TagMappingValidator {
    /// 全量校验映射表，返回全部违规（空 = 通过）。
    pub fn validate(config: &ClassifyConfig) -> Result<(), Vec<TransformError>> {
        let mut errors = Vec::new();

        if config.rules.is_empty() {
            errors.push(TransformError::EmptyRuleSet);
        }

        let mut covered: HashSet<CandidateCategory> = HashSet::new();
        for entry in &config.rules {
            match parse_category(&entry.category) {
                Ok(category) => {
                    if entry.tags.is_empty() {
                        errors.push(TransformError::EmptyTagList(entry.category.clone()));
                    } else {
                        covered.insert(category);
                    }
                }
                Err(unknown) => errors.push(unknown),
            }
        }

        for required in REQUIRED_CATEGORIES {
            if !covered.contains(required) {
                errors.push(TransformError::MissingCategoryRules(
                    required.display_name().to_owned(),
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TagPattern, TagRuleEntry};

    fn entry(category: &str, pattern: &str) -> TagRuleEntry {
        TagRuleEntry::new(category, vec![TagPattern::new(pattern)])
    }

    fn five_category_config() -> ClassifyConfig {
        ClassifyConfig::new(vec![
            entry("建筑", "building=school"),
            entry("体育", "leisure=pitch"),
            entry("水域", "water=*"),
            entry("道路", "highway=*"),
            entry("植被", "natural=tree"),
        ])
    }

    #[test]
    fn complete_mapping_passes() {
        assert!(TagMappingValidator::validate(&five_category_config()).is_ok());
    }

    #[test]
    fn default_mapping_passes() {
        assert!(TagMappingValidator::validate(&ClassifyConfig::default_mapping()).is_ok());
    }

    #[test]
    fn empty_config_reports_all_missing_categories() {
        let errors = TagMappingValidator::validate(&ClassifyConfig::new(Vec::new())).unwrap_err();
        // 1 条 EmptyRuleSet + 5 条 MissingCategoryRules
        assert_eq!(errors.len(), 6);
        assert!(matches!(errors[0], TransformError::EmptyRuleSet));
    }

    #[test]
    fn missing_building_rules_is_reported() {
        let mut config = five_category_config();
        config.rules.retain(|rule| rule.category != "建筑");
        let errors = TagMappingValidator::validate(&config).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], TransformError::MissingCategoryRules(name) if name == "建筑"));
    }

    #[test]
    fn unknown_category_is_not_silently_dropped() {
        let mut config = five_category_config();
        config.rules.push(entry("建筑物", "building=hut"));
        let errors = TagMappingValidator::validate(&config).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], TransformError::UnknownCategory(name) if name == "建筑物"));
    }

    #[test]
    fn empty_tag_list_is_reported() {
        let mut config = five_category_config();
        config.rules.push(TagRuleEntry::new("其他", Vec::new()));
        let errors = TagMappingValidator::validate(&config).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], TransformError::EmptyTagList(name) if name == "其他"));
    }
}
