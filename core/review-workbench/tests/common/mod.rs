#![allow(
    dead_code,
    reason = "shared fixtures are intentionally consumed by only some split integration tests"
)]
//! ReviewWorkbench 集成测试共享的私有候选夹具。
//!
//! Cargo 会把每个场景文件编译为独立测试目标，各目标只使用这里的一部分夹具。

use data_persistence::{
    CandidateDisplay, CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi, ReviewableValidation,
};
use review_workbench::CandidateKey;
use shared_domain_types::{CandidateCategory, PlanId};

/// 在内存库里种入原始观测与一批已发布的可评审候选：6 栋建筑 + 2 条道路 + 1 处水域。
pub(crate) fn fixture() -> (Database, PlanId) {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();

    let mut observations = Vec::new();
    for index in 0..6 {
        observations.push(RawObservation::new(
            &plan_key,
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼 {index}"), "building": "school" } }),
            "overpass",
        ));
    }
    for index in 0..2 {
        observations.push(RawObservation::new(
            &plan_key,
            CandidateCategory::Road,
            format!("way/r{index}"),
            serde_json::json!({ "tags": { "highway": "footway" } }),
            "overpass",
        ));
    }
    observations.push(RawObservation::new(
        &plan_key,
        CandidateCategory::Water,
        "way/w0",
        serde_json::json!({ "tags": { "name": "游泳池", "leisure": "swimming_pool" } }),
        "overpass",
    ));
    db.write_raw_observations(&observations)
        .expect("种子观测写入成功");
    let projections: Vec<_> = observations
        .iter()
        .map(|observation| {
            let display = match observation.entity_type {
                CandidateCategory::Building => CandidateDisplay::new(
                    observation.source_data["tags"]["name"]
                        .as_str()
                        .expect("建筑夹具名称")
                        .to_owned(),
                    vec![
                        ("building".to_owned(), "school".to_owned()),
                        (
                            "name".to_owned(),
                            observation.source_data["tags"]["name"]
                                .as_str()
                                .expect("建筑夹具名称")
                                .to_owned(),
                        ),
                    ],
                ),
                CandidateCategory::Road => CandidateDisplay::new(
                    &observation.entity_id,
                    vec![("highway".to_owned(), "footway".to_owned())],
                ),
                CandidateCategory::Water => CandidateDisplay::new(
                    "游泳池",
                    vec![
                        ("leisure".to_owned(), "swimming_pool".to_owned()),
                        ("name".to_owned(), "游泳池".to_owned()),
                    ],
                ),
                _ => unreachable!("夹具只含建筑、道路和水体"),
            };
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new(
                    &observation.data_source_tag,
                    &observation.entity_id,
                    "default",
                ),
                observation.entity_type,
                display,
                CandidateShape::polygon(serde_json::json!([
                    [121.4, 31.2],
                    [121.5, 31.2],
                    [121.4, 31.3],
                    [121.4, 31.2]
                ])),
                ReviewableValidation::Retained,
            )
        })
        .collect();
    db.publish_candidate_batch(&plan_key, "fixture-boundary", &projections)
        .expect("候选批次发布成功");
    (db, plan_id)
}

pub(crate) fn candidate_key(
    db: &Database,
    plan_id: &PlanId,
    source_entity_id: &str,
) -> CandidateKey {
    candidate_key_part(db, plan_id, source_entity_id, None)
}

pub(crate) fn candidate_key_part(
    db: &Database,
    plan_id: &PlanId,
    source_entity_id: &str,
    geometry_part_id: Option<&str>,
) -> CandidateKey {
    let projection = db
        .list_current_candidate_projections(&plan_id.to_string())
        .expect("读取当前候选投影")
        .into_iter()
        .find(|projection| {
            projection.source_entity_id == source_entity_id
                && geometry_part_id.is_none_or(|part| projection.geometry_part_id == part)
        })
        .unwrap_or_else(|| panic!("来源候选不存在：{source_entity_id}"));
    CandidateKey::new(projection.candidate_id)
}

pub(crate) fn building_key(db: &Database, plan_id: &PlanId, index: usize) -> CandidateKey {
    candidate_key(db, plan_id, &format!("way/b{index}"))
}

pub(crate) fn reviewable_projection(
    source_entity_id: &str,
    title: &str,
    category: CandidateCategory,
) -> CandidateProjectionDraft {
    CandidateProjectionDraft::reviewable(
        CandidateSourceIdentity::new("overpass", source_entity_id, "outer"),
        category,
        CandidateDisplay::new(
            title,
            vec![
                ("building".to_owned(), "school".to_owned()),
                ("name".to_owned(), title.to_owned()),
            ],
        ),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        ReviewableValidation::Retained,
    )
}

pub(crate) fn write_raw_observation(
    db: &mut Database,
    plan_id: &PlanId,
    source_entity_id: &str,
    title: &str,
    category: CandidateCategory,
) {
    db.write_raw_observations(&[RawObservation::new(
        plan_id.to_string(),
        category,
        source_entity_id,
        serde_json::json!({"tags": {"name": title}}),
        "overpass",
    )])
    .expect("写入候选来源原始观测");
}
