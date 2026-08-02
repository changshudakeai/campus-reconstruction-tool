//! B2 候选投影公开契约测试：只通过批次接口观察资格与可见性。

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
};
use shared_domain_types::CandidateCategory;

fn projection(id: &str, eligibility: CandidateEligibility) -> CandidateProjection {
    CandidateProjection::new(
        id,
        "plan-1",
        "raw-1",
        "overpass",
        "way/1",
        "outer",
        CandidateCategory::Building,
        CandidateDisplay::new(
            "第一教学楼",
            vec![("building".to_owned(), "school".to_owned())],
        ),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        CandidateValidation::Retained,
        eligibility,
    )
}

#[test]
fn published_batch_lists_only_reviewable_projections() {
    let mut db = Database::open_in_memory().expect("内存库");
    let batch = db.prepare_candidate_batch("plan-1").expect("准备批次");
    db.write_candidate_projections(
        &batch.id,
        &[
            projection("overpass:way/1:outer", CandidateEligibility::Reviewable),
            projection("overpass:way/2:outer", CandidateEligibility::Isolated),
        ],
    )
    .expect("写入投影");
    db.publish_candidate_batch(&batch.id).expect("完整发布");

    let visible = db
        .list_reviewable_candidate_projections("plan-1")
        .expect("列出可评审候选");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].candidate_id, "overpass:way/1:outer");
}

#[test]
fn same_source_entity_id_from_different_sources_do_not_collide() {
    let mut db = Database::open_in_memory().expect("内存库");
    let batch = db.prepare_candidate_batch("plan-1").unwrap();
    let overpass = projection("overpass:way/1:outer", CandidateEligibility::Reviewable);
    let mut overture = projection("overture:way/1:outer", CandidateEligibility::Reviewable);
    overture.data_source_tag = "overture".to_owned();
    db.write_candidate_projections(&batch.id, &[overpass, overture])
        .unwrap();
    db.publish_candidate_batch(&batch.id).unwrap();

    assert_eq!(
        db.list_reviewable_candidate_projections("plan-1")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn geometry_parts_of_one_source_entity_do_not_collide() {
    let mut db = Database::open_in_memory().expect("内存库");
    let batch = db.prepare_candidate_batch("plan-1").unwrap();
    let outer = projection("overpass:way/1:outer", CandidateEligibility::Reviewable);
    let mut inner = projection("overpass:way/1:inner-1", CandidateEligibility::Reviewable);
    inner.geometry_part_id = "inner-1".to_owned();
    db.write_candidate_projections(&batch.id, &[outer, inner])
        .unwrap();
    db.publish_candidate_batch(&batch.id).unwrap();

    assert_eq!(
        db.list_reviewable_candidate_projections("plan-1")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn failed_new_batch_write_keeps_the_previous_published_batch_current() {
    let mut db = Database::open_in_memory().expect("内存库");
    let old = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &old.id,
        &[projection("old", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&old.id).unwrap();

    let next = db.prepare_candidate_batch("plan-1").unwrap();
    let duplicate = projection("duplicate", CandidateEligibility::Reviewable);
    assert!(db
        .write_candidate_projections(&next.id, &[duplicate.clone(), duplicate])
        .is_err());

    let visible = db.list_reviewable_candidate_projections("plan-1").unwrap();
    assert_eq!(
        visible
            .iter()
            .map(|item| item.candidate_id.as_str())
            .collect::<Vec<_>>(),
        vec!["old"]
    );
}

#[test]
fn successful_new_batch_atomically_replaces_the_previous_current_batch() {
    let mut db = Database::open_in_memory().expect("内存库");
    let old = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &old.id,
        &[projection("old", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&old.id).unwrap();
    let next = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &next.id,
        &[projection("new", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&next.id).unwrap();

    let visible = db.list_reviewable_candidate_projections("plan-1").unwrap();
    assert_eq!(
        visible
            .iter()
            .map(|item| item.candidate_id.as_str())
            .collect::<Vec<_>>(),
        vec!["new"]
    );
}

#[test]
fn missing_candidate_is_carried_forward_with_a_missing_marker() {
    let mut db = Database::open_in_memory().expect("内存库");
    let old = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &old.id,
        &[projection("old", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&old.id).unwrap();
    let next = db.prepare_candidate_batch("plan-1").unwrap();
    db.carry_forward_missing_candidate_projections(&next.id)
        .unwrap();
    db.publish_candidate_batch(&next.id).unwrap();

    let carried = db
        .get_current_candidate_projection("plan-1", "old")
        .unwrap()
        .unwrap();
    assert!(carried.missing_in_latest_batch);
    assert_eq!(carried.eligibility, CandidateEligibility::Reviewable);
}

#[test]
fn isolated_replacement_hides_an_old_reviewable_projection() {
    let mut db = Database::open_in_memory().expect("内存库");
    let old = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &old.id,
        &[projection("same", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&old.id).unwrap();
    let next = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &next.id,
        &[projection("same", CandidateEligibility::Isolated).isolated_reason("self_intersecting")],
    )
    .unwrap();
    db.publish_candidate_batch(&next.id).unwrap();

    assert!(db
        .list_reviewable_candidate_projections("plan-1")
        .unwrap()
        .is_empty());
    assert_eq!(
        db.get_current_candidate_projection("plan-1", "same")
            .unwrap()
            .unwrap()
            .eligibility,
        CandidateEligibility::Isolated
    );
}

#[test]
fn raw_observation_survives_failed_and_replaced_candidate_batches() {
    let mut db = Database::open_in_memory().expect("内存库");
    let raw = RawObservation::new(
        "plan-1",
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({"source": "evidence"}),
        "overpass",
    );
    db.write_raw_observations(&[raw]).unwrap();
    let old = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &old.id,
        &[projection("old", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&old.id).unwrap();
    let failed = db.prepare_candidate_batch("plan-1").unwrap();
    let duplicate = projection("duplicate", CandidateEligibility::Reviewable);
    assert!(db
        .write_candidate_projections(&failed.id, &[duplicate.clone(), duplicate])
        .is_err());
    let replacement = db.prepare_candidate_batch("plan-1").unwrap();
    db.write_candidate_projections(
        &replacement.id,
        &[projection("new", CandidateEligibility::Reviewable)],
    )
    .unwrap();
    db.publish_candidate_batch(&replacement.id).unwrap();

    assert_eq!(db.list_raw_observations("plan-1").unwrap().len(), 1);
}

#[test]
fn stable_candidate_id_reads_the_same_normalized_geometry_for_downstream_consumers() {
    let mut db = Database::open_in_memory().expect("内存库");
    let batch = db.prepare_candidate_batch("plan-1").unwrap();
    let mut candidate = projection("geometry", CandidateEligibility::Reviewable);
    candidate.shape = CandidateShape::line_string(serde_json::json!([[121.4, 31.2], [121.5, 31.3]]));
    db.write_candidate_projections(&batch.id, &[candidate]).unwrap();
    db.publish_candidate_batch(&batch.id).unwrap();

    let projection = db.get_current_candidate_projection("plan-1", "geometry").unwrap().unwrap();
    assert_eq!(projection.shape.kind, "line_string");
    assert_eq!(projection.shape.coordinates, serde_json::json!([[121.4, 31.2], [121.5, 31.3]]));
}
