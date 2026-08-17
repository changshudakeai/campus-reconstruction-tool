//! ADR-0040 候选展示属性的新装持久化验收。

use data_persistence::{
    CandidateDisplay, CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi, ReviewableValidation,
    LATEST_SCHEMA_VERSION,
};
use shared_domain_types::CandidateCategory;

#[test]
fn fresh_database_preserves_caller_provided_display_across_publish_and_reopen() {
    let directory = tempfile::tempdir().expect("临时目录");
    let path = directory.path().join("candidate-display.sqlite3");
    let source_data = serde_json::json!({
        "unprocessed_source_payload": true,
        "tags": {"source": "overpass"}
    });

    {
        let mut database = Database::open(&path).expect("创建新数据库");
        assert_eq!(
            database.schema_version().unwrap(),
            Some(LATEST_SCHEMA_VERSION)
        );
        let observation = RawObservation::new(
            "plan-1",
            CandidateCategory::Building,
            "way/1",
            source_data.clone(),
            "overpass",
        );
        database
            .write_raw_observations(&[observation])
            .expect("写入生产原始观测");
        let display = CandidateDisplay::new(
            "第一教学楼",
            vec![
                ("accessible".to_owned(), "true".to_owned()),
                ("building".to_owned(), "school".to_owned()),
                ("heated".to_owned(), "false".to_owned()),
                ("height".to_owned(), "24.5".to_owned()),
                ("levels".to_owned(), "6".to_owned()),
                ("name".to_owned(), "不会覆盖顶层名称".to_owned()),
                ("name".to_owned(), "第一教学楼".to_owned()),
            ],
        );
        let projections = [
            projection("outer", display.clone()),
            projection("inner", display),
        ];
        database
            .publish_candidate_batch("plan-1", "fixture-boundary", &projections)
            .expect("发布批次");
    }

    let database = Database::open(&path).expect("重开新装数据库");
    let loaded = database
        .list_reviewable_candidate_projections("plan-1")
        .expect("读取已发布投影");
    assert_eq!(
        loaded
            .iter()
            .map(|candidate| candidate.geometry_part_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["inner", "outer"])
    );
    for candidate in loaded {
        assert_eq!(candidate.source_entity_id, "way/1");
        assert_eq!(candidate.display.title, "第一教学楼");
        assert_eq!(
            candidate.display.tags,
            vec![
                ("accessible".to_owned(), "true".to_owned()),
                ("building".to_owned(), "school".to_owned()),
                ("heated".to_owned(), "false".to_owned()),
                ("height".to_owned(), "24.5".to_owned()),
                ("levels".to_owned(), "6".to_owned()),
                ("name".to_owned(), "不会覆盖顶层名称".to_owned()),
                ("name".to_owned(), "第一教学楼".to_owned()),
            ]
        );
    }
}

fn projection(geometry_part_id: &str, display: CandidateDisplay) -> CandidateProjectionDraft {
    CandidateProjectionDraft::reviewable(
        CandidateSourceIdentity::new("overpass", "way/1", geometry_part_id),
        CandidateCategory::Building,
        display,
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        ReviewableValidation::Retained,
    )
}
