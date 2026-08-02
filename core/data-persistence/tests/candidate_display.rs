//! ADR-0040 候选展示属性持久化验收。

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database,
};
use shared_domain_types::CandidateCategory;

#[test]
fn published_projection_preserves_display_title_and_tags() {
    let mut db = Database::open_in_memory().expect("内存库");
    let batch = db.prepare_candidate_batch("plan-1").expect("准备批次");
    let projection = CandidateProjection::new(
        "overpass:way/1:outer",
        "plan-1",
        "raw-1",
        "overpass",
        "way/1",
        "outer",
        CandidateCategory::Building,
        CandidateDisplay::new(
            "第一教学楼",
            vec![
                ("building".to_owned(), "school".to_owned()),
                ("name".to_owned(), "第一教学楼".to_owned()),
            ],
        ),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    );
    db.write_candidate_projections(&batch.id, &[projection])
        .expect("写入投影");
    db.publish_candidate_batch(&batch.id).expect("发布批次");

    let loaded = db
        .get_current_candidate_projection("plan-1", "overpass:way/1:outer")
        .expect("读取投影")
        .expect("投影存在");
    assert_eq!(loaded.display.title, "第一教学楼");
    assert_eq!(
        loaded.display.tags,
        vec![
            ("building".to_owned(), "school".to_owned()),
            ("name".to_owned(), "第一教学楼".to_owned()),
        ]
    );
}
