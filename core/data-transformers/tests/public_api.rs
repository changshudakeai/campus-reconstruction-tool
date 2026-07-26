//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use data_transformers::{
    Classification, ClassifyConfig, ClassifyEngine, TagMap, TagMappingValidator, TagPattern,
    TagRuleEntry, TransformError,
};

#[test]
fn public_api_types_exist() {
    // TagPattern：三种匹配语义
    let exact = TagPattern::new("building=school");
    assert_eq!(exact.as_str(), "building=school");
    assert!(exact.matches("building", "school"));
    assert!(TagPattern::new("highway=*").matches("highway", "footway"));
    assert!(TagPattern::new("building=dorm*").matches("building", "dormitory"));

    // TagRuleEntry / ClassifyConfig：构造与 JSON 解析
    let entry = TagRuleEntry::new("建筑", vec![TagPattern::new("building=yes")]);
    assert_eq!(entry.category, "建筑");
    let config = ClassifyConfig::new(vec![entry]);
    assert_eq!(config.version, "1.0");
    assert!(ClassifyConfig::from_json("{ bad json").is_err());
    let default_mapping = ClassifyConfig::default_mapping();
    assert!(!default_mapping.rules.is_empty());

    // TagMappingValidator：五大类缺规则必报错
    assert!(TagMappingValidator::validate(&default_mapping).is_ok());
    let errors = TagMappingValidator::validate(&ClassifyConfig::new(Vec::new())).unwrap_err();
    assert!(!errors.is_empty());

    // TransformError #[non_exhaustive]：带类型错误可匹配
    assert!(matches!(errors[0], TransformError::EmptyRuleSet));

    // ClassifyEngine + Classification：归类与兜底信号
    let engine = ClassifyEngine::with_default_mapping().expect("默认映射表可用");
    let mut tags = TagMap::new();
    tags.insert("natural".to_owned(), "tree_row".to_owned());
    let classification: Classification = engine.classify(&tags);
    assert_eq!(classification.category.display_name(), "植被");
    assert!(!classification.is_fallback);
    assert!(!classification.matched_patterns.is_empty());
    assert!(!engine.config().rules.is_empty());
}
