//! 分类准确性集成测试（在 trait 边界只测外部行为）：
//! 夹具驱动的 12 组典型标签组合（含优先级冲突与兜底场景），
//! 外加工单点名的单元断言。

use data_transformers::{ClassifyEngine, TagMap};
use shared_domain_types::CandidateCategory;

/// 夹具文件（tests/fixtures/campus-objects.json）。
const FIXTURES_JSON: &str = include_str!("fixtures/campus-objects.json");

#[derive(serde::Deserialize)]
struct FixtureFile {
    cases: Vec<FixtureCase>,
}

#[derive(serde::Deserialize)]
struct FixtureCase {
    name: String,
    tags: TagMap,
    expected: String,
    fallback: bool,
}

fn engine() -> ClassifyEngine {
    ClassifyEngine::with_default_mapping().expect("默认映射表必须通过校验")
}

fn tags(pairs: &[(&str, &str)]) -> TagMap {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// 夹具全量回归：12 组典型校园对象，逐条断言类别与兜底信号。
#[test]
fn campus_object_fixtures_classify_as_expected() {
    let fixtures: FixtureFile = serde_json::from_str(FIXTURES_JSON).expect("夹具必须是合法 JSON");
    assert!(fixtures.cases.len() >= 10, "夹具至少覆盖 10 组标签组合");

    let engine = engine();
    for case in &fixtures.cases {
        let classification = engine.classify(&case.tags);
        assert_eq!(
            classification.category.display_name(),
            case.expected,
            "夹具 [{}] 归类错误",
            case.name
        );
        assert_eq!(
            classification.is_fallback, case.fallback,
            "夹具 [{}] 兜底信号错误",
            case.name
        );
    }
}

/// 工单点名断言：building=supermarket + leisure=pitch → 体育。
/// （supermarket 不在建筑家族规则内，leisure=pitch 命中体育。）
#[test]
fn supermarket_with_pitch_is_sports() {
    let classification =
        engine().classify(&tags(&[("building", "supermarket"), ("leisure", "pitch")]));
    assert_eq!(classification.category, CandidateCategory::Sports);
    assert!(!classification.is_fallback);
}

/// 优先级冲突：同一对象命中建筑与体育两类时按 建筑 > 体育 取最高（ADR-0011）。
#[test]
fn priority_conflict_resolves_to_building() {
    let classification = engine().classify(&tags(&[("building", "yes"), ("leisure", "pitch")]));
    assert_eq!(classification.category, CandidateCategory::Building);
    assert!(
        classification.matched_patterns.len() >= 2,
        "两类规则都应命中，冲突由优先级裁决而非丢弃"
    );
}

/// 兜底逻辑：未知标签归"其他"，带明确信号，不静默丢弃。
#[test]
fn unknown_tags_fall_back_to_other_with_signal() {
    let classification = engine().classify(&tags(&[("mystery_key", "mystery_value")]));
    assert_eq!(classification.category, CandidateCategory::Other);
    assert!(
        classification.is_fallback,
        "兜底必须带明确信号（评审队列可见）"
    );
    assert!(classification.matched_patterns.is_empty());
}

/// 空标签集同样兜底进"其他"（对象不会消失）。
#[test]
fn empty_tags_fall_back_to_other() {
    let classification = engine().classify(&TagMap::new());
    assert_eq!(classification.category, CandidateCategory::Other);
    assert!(classification.is_fallback);
}
