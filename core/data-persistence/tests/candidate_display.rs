//! ADR-0040 候选展示属性的新装持久化验收。

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
    LATEST_SCHEMA_VERSION,
};
use shared_domain_types::CandidateCategory;

#[test]
fn fresh_database_preserves_production_display_across_publish_and_reopen() {
    let directory = tempfile::tempdir().expect("临时目录");
    let path = directory.path().join("candidate-display.sqlite3");
    let source_data = serde_json::json!({
        "name": "第一教学楼",
        "height": 24.5,
        "accessible": true,
        "ignored": ["not", "a", "scalar"],
        "tags": {
            "name": "不会覆盖顶层名称",
            "building": "school",
            "levels": 6,
            "heated": false,
            "ignored": {"nested": "object"}
        }
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
        let raw_observation_id = observation.id.clone();
        database
            .write_raw_observations(&[observation])
            .expect("写入生产原始观测");
        let batch = database
            .prepare_candidate_batch("plan-1")
            .expect("准备批次");
        let display = CandidateDisplay::from_source_data(&source_data, "way/1");
        let projections = [
            projection(
                "overpass:way/1:outer",
                &raw_observation_id,
                "outer",
                display.clone(),
            ),
            projection(
                "overpass:way/1:inner",
                &raw_observation_id,
                "inner",
                display,
            ),
        ];
        database
            .write_candidate_projections(&batch.id, &projections)
            .expect("写入几何分片投影");
        database
            .publish_candidate_batch(&batch.id)
            .expect("发布批次");
    }

    let database = Database::open(&path).expect("重开新装数据库");
    let loaded = database
        .list_reviewable_candidate_projections("plan-1")
        .expect("读取已发布投影");
    assert_eq!(
        loaded
            .iter()
            .map(|candidate| (&candidate.candidate_id, &candidate.geometry_part_id))
            .collect::<Vec<_>>(),
        vec![
            (&"overpass:way/1:inner".to_owned(), &"inner".to_owned()),
            (&"overpass:way/1:outer".to_owned(), &"outer".to_owned()),
        ]
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

#[test]
fn display_title_falls_back_to_nested_name_then_source_entity_id() {
    assert_eq!(
        CandidateDisplay::from_source_data(
            &serde_json::json!({"tags": {"name": "游泳池"}}),
            "way/2",
        )
        .title,
        "游泳池"
    );
    assert_eq!(
        CandidateDisplay::from_source_data(
            &serde_json::json!({"tags": {"sport": "soccer"}}),
            "way/3",
        )
        .title,
        "way/3"
    );
}

fn projection(
    candidate_id: &str,
    raw_observation_id: &str,
    geometry_part_id: &str,
    display: CandidateDisplay,
) -> CandidateProjection {
    CandidateProjection::new(
        candidate_id,
        "plan-1",
        raw_observation_id,
        "overpass",
        "way/1",
        geometry_part_id,
        CandidateCategory::Building,
        display,
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    )
}
