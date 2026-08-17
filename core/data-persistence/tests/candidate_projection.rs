//! B2 候选投影 lifecycle interface 测试：调用方只交来源/验证事实。

use data_persistence::{
    BoundaryRevalidationApi, CandidateDisplay, CandidateEligibility, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateRevalidationFact, CandidateShape, CandidateSourceIdentity,
    CandidateValidation, Database, RawObservation, RawObservationsApi, ReviewDecision,
    ReviewDecisionsApi, ReviewDraft, ReviewDraftApi, ReviewDraftEntry, ReviewableValidation,
};
use shared_domain_types::{CandidateCategory, ReviewState};

fn write_raw(db: &mut Database, entity_id: &str) {
    let raw = RawObservation::new(
        "plan-1",
        CandidateCategory::Building,
        entity_id,
        serde_json::json!({"source": "evidence", "entity": entity_id}),
        "overpass",
    );
    db.write_raw_observations(&[raw]).unwrap();
}

fn reviewable(source_tag: &str, entity_id: &str, part_id: &str) -> CandidateProjectionDraft {
    CandidateProjectionDraft::reviewable(
        CandidateSourceIdentity::new(source_tag, entity_id, part_id),
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
        ReviewableValidation::Retained,
    )
}

fn isolated(source_tag: &str, entity_id: &str, part_id: &str) -> CandidateProjectionDraft {
    CandidateProjectionDraft::isolated(
        CandidateSourceIdentity::new(source_tag, entity_id, part_id),
        CandidateCategory::Building,
        CandidateDisplay::new("第一教学楼", vec![]),
        CandidateShape::polygon(serde_json::json!([])),
        "self_intersecting",
    )
    .unwrap()
}

fn publish(
    db: &mut Database,
    fingerprint: &str,
    drafts: &[CandidateProjectionDraft],
) -> data_persistence::CandidateBatchSummary {
    db.publish_candidate_batch("plan-1", fingerprint, drafts)
        .expect("原子发布")
}

#[test]
fn published_batch_lists_only_reviewable_projections() {
    let mut db = Database::open_in_memory().expect("内存库");
    write_raw(&mut db, "way/1");
    write_raw(&mut db, "way/2");
    publish(
        &mut db,
        "boundary-1",
        &[
            reviewable("overpass", "way/1", "outer"),
            isolated("overpass", "way/2", "outer"),
        ],
    );
    let visible = db
        .list_reviewable_candidate_projections("plan-1")
        .expect("列出可评审候选");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].source_entity_id, "way/1");
}

#[test]
fn same_source_entity_id_from_different_sources_do_not_collide() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[
            reviewable("overpass", "way/1", "outer"),
            reviewable("overture", "way/1", "outer"),
        ],
    );
    assert_eq!(
        db.list_reviewable_candidate_projections("plan-1")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn first_seen_candidate_identity_is_deterministic_from_its_source_facts() {
    let mut first = Database::open_in_memory().unwrap();
    let mut second = Database::open_in_memory().unwrap();
    for db in [&mut first, &mut second] {
        write_raw(db, "way/1");
        publish(
            db,
            "boundary-1",
            &[reviewable("overpass", "way/1", "outer")],
        );
    }
    let first_id = &first.list_current_candidate_projections("plan-1").unwrap()[0].candidate_id;
    let second_id = &second.list_current_candidate_projections("plan-1").unwrap()[0].candidate_id;
    assert_eq!(first_id, second_id);
}

#[test]
fn geometry_parts_of_one_source_entity_do_not_collide() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[
            reviewable("overpass", "way/1", "outer"),
            reviewable("overpass", "way/1", "inner-1"),
        ],
    );
    assert_eq!(
        db.list_reviewable_candidate_projections("plan-1")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn duplicate_source_identity_rejects_the_whole_batch_and_keeps_current() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let current_id = db.list_reviewable_candidate_projections("plan-1").unwrap()[0]
        .candidate_id
        .clone();
    let duplicate = reviewable("overpass", "way/1", "outer");
    assert!(db
        .publish_candidate_batch("plan-1", "boundary-2", &[duplicate.clone(), duplicate])
        .is_err());
    assert_eq!(
        db.list_reviewable_candidate_projections("plan-1").unwrap()[0].candidate_id,
        current_id
    );
    assert_eq!(
        db.load_plan_collection_boundary("plan-1").unwrap(),
        Some("boundary-1".to_owned())
    );
}

#[test]
fn successful_collection_atomically_publishes_seen_and_missing_candidates() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/old");
    write_raw(&mut db, "way/new");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/old", "outer")],
    );
    publish(
        &mut db,
        "boundary-2",
        &[reviewable("overpass", "way/new", "outer")],
    );
    let current = db.list_current_candidate_projections("plan-1").unwrap();
    assert_eq!(current.len(), 2);
    assert!(current
        .iter()
        .find(|item| item.source_entity_id == "way/old")
        .unwrap()
        .missing_in_latest_batch());
    assert!(!current
        .iter()
        .find(|item| item.source_entity_id == "way/new")
        .unwrap()
        .missing_in_latest_batch());
}

#[test]
fn missing_candidate_is_carried_forward_with_a_missing_marker() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let candidate_id = db.list_current_candidate_projections("plan-1").unwrap()[0]
        .candidate_id
        .clone();
    publish(&mut db, "boundary-2", &[]);
    let carried = db
        .get_current_candidate_projection("plan-1", &candidate_id)
        .unwrap()
        .unwrap();
    assert!(carried.missing_in_latest_batch());
    assert_eq!(carried.eligibility(), CandidateEligibility::Reviewable);
}

#[test]
fn isolated_replacement_hides_an_old_reviewable_projection() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let candidate_id = db.list_current_candidate_projections("plan-1").unwrap()[0]
        .candidate_id
        .clone();
    publish(
        &mut db,
        "boundary-2",
        &[isolated("overpass", "way/1", "outer")],
    );
    assert!(db
        .list_reviewable_candidate_projections("plan-1")
        .unwrap()
        .is_empty());
    let current = db
        .get_current_candidate_projection("plan-1", &candidate_id)
        .unwrap()
        .unwrap();
    assert_eq!(current.eligibility(), CandidateEligibility::Isolated);
    assert_eq!(current.isolation_reason(), Some("self_intersecting"));
}

#[test]
fn raw_observation_survives_failed_candidate_publication() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    let duplicate = reviewable("overpass", "way/1", "outer");
    assert!(db
        .publish_candidate_batch("plan-1", "boundary-1", &[duplicate.clone(), duplicate])
        .is_err());
    assert_eq!(db.list_raw_observations("plan-1").unwrap().len(), 1);
}

#[test]
fn stable_candidate_id_reads_the_same_normalized_geometry_for_downstream_consumers() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    let line = CandidateProjectionDraft::reviewable(
        CandidateSourceIdentity::new("overpass", "way/1", "outer"),
        CandidateCategory::Building,
        CandidateDisplay::new("道路形候选", vec![]),
        CandidateShape::line_string(serde_json::json!([[121.4, 31.2], [121.5, 31.3]])),
        ReviewableValidation::Retained,
    );
    publish(&mut db, "boundary-1", &[line]);
    let projection = db
        .list_current_candidate_projections("plan-1")
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(projection.shape.kind, "line_string");
    assert_eq!(
        projection.shape.coordinates,
        serde_json::json!([[121.4, 31.2], [121.5, 31.3]])
    );
}

#[test]
fn reappearing_candidate_keeps_identity_but_requires_a_new_review_decision() {
    let mut db = Database::open_in_memory().expect("内存库");
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let stable_id = db.list_current_candidate_projections("plan-1").unwrap()[0]
        .candidate_id
        .clone();
    let first_revision = db
        .current_candidate_batch_revision("plan-1")
        .unwrap()
        .unwrap();
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &first_revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            stable_id.clone(),
            ReviewState::Keep,
        )],
    )
    .unwrap();
    db.save_review_draft(&ReviewDraft {
        plan_id: "plan-1".to_owned(),
        active_category: CandidateCategory::Building,
        entries: vec![ReviewDraftEntry {
            candidate_id: stable_id.clone(),
            review_state: ReviewState::Keep,
            selected: true,
        }],
    })
    .unwrap();
    publish(&mut db, "boundary-2", &[]);
    assert!(
        db.load_review_draft("plan-1").unwrap().is_none(),
        "候选消失时旧评审草稿也必须失效"
    );
    publish(
        &mut db,
        "boundary-3",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let current = db
        .list_current_candidate_projections("plan-1")
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(current.candidate_id, stable_id);
    assert!(!current.missing_in_latest_batch());
    assert_eq!(current.eligibility(), CandidateEligibility::Reviewable);
    let decisions = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(decisions.len(), 1, "重新出现后应有显式待定当前状态");
    assert!(decisions[0].review_state.is_pending());
    assert!(
        db.list_kept_candidate_projections("plan-1")
            .unwrap()
            .is_empty(),
        "用户未再次保留前，增强导出不得读到该候选"
    );
    let history = db.list_review_decision_invalidations("plan-1").unwrap();
    assert_eq!(history.len(), 1, "旧保留决定必须只作为作废历史保留");
    assert!(history[0].previous_state.is_keep());

    let reappeared_revision = db
        .current_candidate_batch_revision("plan-1")
        .unwrap()
        .unwrap();
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &reappeared_revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            stable_id,
            ReviewState::Keep,
        )],
    )
    .unwrap();
    assert_eq!(
        db.list_kept_candidate_projections("plan-1").unwrap().len(),
        1,
        "只有用户再次保留后，增强导出才读到该候选"
    );
}

#[test]
fn boundary_isolation_preserves_geometry_validation_history() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let projection = db.list_current_candidate_projections("plan-1").unwrap()[0].clone();
    db.publish_candidate_revalidation(
        "plan-1",
        "boundary-2",
        &[CandidateRevalidationFact::isolated_validated(
            projection.candidate_id.clone(),
            projection.shape.clone(),
            ReviewableValidation::Retained,
            "outside_confirmed_plan_boundary",
        )
        .unwrap()],
    )
    .unwrap();

    let isolated = db.list_current_candidate_projections("plan-1").unwrap()[0].clone();
    assert_eq!(isolated.validation(), CandidateValidation::Retained);
    assert_eq!(isolated.eligibility(), CandidateEligibility::Isolated);
    assert_eq!(
        isolated.isolation_reason(),
        Some("outside_confirmed_plan_boundary")
    );
    let revision = db
        .current_candidate_batch_revision("plan-1")
        .unwrap()
        .unwrap();
    let duplicate = db
        .publish_candidate_revalidation(
            "plan-1",
            "boundary-2",
            &[CandidateRevalidationFact::isolated_validated(
                isolated.candidate_id,
                isolated.shape,
                ReviewableValidation::Retained,
                "outside_confirmed_plan_boundary",
            )
            .unwrap()],
        )
        .unwrap();
    assert_eq!(
        duplicate,
        data_persistence::RevalidationWriteSummary::default()
    );
    assert_eq!(
        db.current_candidate_batch_revision("plan-1")
            .unwrap()
            .as_deref(),
        Some(revision.as_str()),
        "相同边界指纹不得制造无意义的新 revision"
    );
}

#[test]
fn stale_review_revision_cannot_restore_a_keep_after_collection_changes() {
    let mut db = Database::open_in_memory().unwrap();
    write_raw(&mut db, "way/1");
    publish(
        &mut db,
        "boundary-1",
        &[reviewable("overpass", "way/1", "outer")],
    );
    let stale_revision = db
        .current_candidate_batch_revision("plan-1")
        .unwrap()
        .unwrap();
    let candidate_id = db.list_current_candidate_projections("plan-1").unwrap()[0]
        .candidate_id
        .clone();
    publish(&mut db, "boundary-2", &[]);

    let error = db
        .batch_update_review_decisions_at_revision(
            "plan-1",
            &stale_revision,
            &[ReviewDecision::new(
                "plan-1",
                CandidateCategory::Building,
                candidate_id,
                ReviewState::Keep,
            )],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        data_persistence::Error::StaleCandidateProjectionRevision { .. }
    ));
    assert!(db
        .list_kept_candidate_projections("plan-1")
        .unwrap()
        .is_empty());
}
