//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//! 同时执行 B6 国际化验收：本 crate 产出的全部文本键在
//! zh-CN.json 中逐条可解析（ADR-0005，文案外置）。

use data_acquisition::{
    category_text_key, text_keys, AcquisitionError, AcquisitionPipeline, CategoryProgress,
    CollectionProgressView, DataSource, DiffEntry, DiffKind, GaodeDataSource, RawEntity,
    RefreshDiff, ALL_CATEGORIES,
};
use data_transformers::TagMap;
use localization::{Language, Localization};
use shared_domain_types::{Boundary, CandidateCategory};

#[test]
fn public_api_types_exist() {
    // RawEntity：原始对象与粮仓 source_data 组装
    let mut tags = TagMap::new();
    tags.insert("building".to_owned(), "school".to_owned());
    let entity = RawEntity::new("E01", "教学楼", tags, serde_json::json!({"id": "E01"}));
    let source_data = entity.to_source_data();
    assert_eq!(source_data["tags"]["building"], "school");

    // DataSource trait 对象安全（ADR-0013 可插拔缝）
    let gaode = GaodeDataSource::new(Box::new(|_| Err("离线".to_owned())));
    let dyn_source: &dyn DataSource = &gaode;
    assert_eq!(dyn_source.source_tag(), GaodeDataSource::SOURCE_TAG);
    let err = dyn_source
        .fetch_raw_entities(&Boundary::empty())
        .unwrap_err();
    assert!(matches!(err, AcquisitionError::SourceUnreachable { .. }));

    // AcquisitionPipeline：默认映射表构造 + 引擎只读访问
    let pipeline = AcquisitionPipeline::new().expect("默认映射表可用");
    assert!(!pipeline.engine().config().rules.is_empty());
    let rebuilt = AcquisitionPipeline::with_engine(pipeline.engine().clone());
    assert!(!rebuilt.engine().config().rules.is_empty());

    // RefreshDiff / DiffEntry / DiffKind：增量刷新检测框架
    let diff = RefreshDiff::new(vec![DiffEntry {
        category: CandidateCategory::Building,
        entity_id: "E01".to_owned(),
        kind: DiffKind::Added,
    }]);
    assert_eq!(diff.added_count(), 1);
    assert!(diff.has_changes());
    assert_eq!(diff.summary_key(), text_keys::DIFF_SUMMARY);

    // 进度视图占位：六类别逐行 + 文本键
    let fetching = CollectionProgressView::fetching();
    assert_eq!(fetching.categories.len(), ALL_CATEGORIES.len());
    let first: &CategoryProgress = &fetching.categories[0];
    assert_eq!(first.label_key, category_text_key(first.category));
}

/// B6 国际化验收：本 crate 产出的全部文本键在 zh-CN.json 中逐条可解析
/// （`t()` 查不到键时原样返回键名——据此断言解析结果不等于键名）。
#[test]
fn every_emitted_text_key_resolves_in_zh_cn() {
    let l10n = Localization::new(Language::ZhCn).expect("zh-CN.json 可加载");
    let mut keys = vec![
        text_keys::PROGRESS_TITLE,
        text_keys::PROGRESS_FETCHING,
        text_keys::PROGRESS_DONE,
        text_keys::DIFF_ADDED,
        text_keys::DIFF_UPDATED,
        text_keys::DIFF_UNCHANGED,
        text_keys::DIFF_SUMMARY,
        text_keys::SOURCE_GAODE,
    ];
    keys.extend(ALL_CATEGORIES.map(category_text_key));

    for key in keys {
        assert_ne!(l10n.t(key), key, "文本键 {key} 必须在 zh-CN.json 中有条目");
    }

    // 带变量文案用占位符插值（ADR-0005，禁止拼接组句）
    let done = l10n.t_with_args(text_keys::PROGRESS_DONE, serde_json::json!({ "count": 42 }));
    assert!(done.contains("42"));
    let summary = l10n.t_with_args(
        text_keys::DIFF_SUMMARY,
        serde_json::json!({ "added": 1, "updated": 2, "unchanged": 3 }),
    );
    assert!(summary.contains('1') && summary.contains('2') && summary.contains('3'));
}
