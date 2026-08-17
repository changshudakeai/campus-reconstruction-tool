//! F5 评审工作台的会话恢复、候选批次版本与封账集成测试。

mod common;

use common::{building_key, candidate_key, fixture, reviewable_projection, write_raw_observation};
use data_persistence::{CandidateProjectionsApi, Database, ReviewDecisionsApi};
use review_workbench::{CandidateKey, Error, ReviewWorkbench, StateChange};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

#[test]
fn stable_candidate_id_roundtrips_through_session_seal_and_reload() {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    write_raw_observation(
        &mut db,
        &plan_id,
        "way/1",
        "第一教学楼",
        CandidateCategory::Building,
    );
    db.publish_candidate_batch(
        &plan_id.to_string(),
        "fixture-boundary",
        &[reviewable_projection(
            "way/1",
            "第一教学楼",
            CandidateCategory::Building,
        )],
    )
    .expect("发布候选批次");
    let key = candidate_key(&db, &plan_id, "way/1");
    let stored = db
        .get_current_candidate_projection(&plan_id.to_string(), &key.candidate_id)
        .expect("读取当前投影")
        .expect("当前投影存在");
    assert_eq!(stored.candidate_id, key.candidate_id);
    assert_eq!(stored.source_entity_id, "way/1");
    assert_ne!(stored.candidate_id, stored.source_entity_id);

    let mut workbench = ReviewWorkbench::load(&db, &plan_id).expect("加载候选");
    workbench
        .submit(StateChange::single(key.clone(), ReviewState::Keep))
        .expect("按 candidate_id 修改状态");
    workbench
        .toggle_selected(&key)
        .expect("按 candidate_id 勾选");

    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stable-candidate-session.json");
    workbench.save_session(&session_path).expect("保存会话");
    let mut resumed = ReviewWorkbench::load(&db, &plan_id).expect("重新加载候选");
    resumed.restore_session(&session_path).expect("恢复会话");
    assert_eq!(resumed.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(resumed.selected_count(), 1);

    resumed.seal(&mut db).expect("按 candidate_id 封账");
    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, key.candidate_id);

    let reloaded = ReviewWorkbench::load(&db, &plan_id).expect("封账后重新加载");
    assert_eq!(reloaded.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(
        reloaded.state_of(&CandidateKey::new("way/1")),
        None,
        "source_entity_id 不能冒充稳定 candidate_id"
    );
}

#[test]
fn category_change_keeps_identity_but_rejects_stale_session_revision() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    write_raw_observation(
        &mut db,
        &plan_id,
        "way/1",
        "第一教学楼",
        CandidateCategory::Building,
    );
    db.publish_candidate_batch(
        &plan_id.to_string(),
        "boundary-1",
        &[reviewable_projection(
            "way/1",
            "第一教学楼",
            CandidateCategory::Building,
        )],
    )
    .unwrap();
    let key = candidate_key(&db, &plan_id, "way/1");
    let candidate_id = key.candidate_id.clone();

    let mut first_review = ReviewWorkbench::load(&db, &plan_id).unwrap();
    first_review
        .submit(StateChange::single(key.clone(), ReviewState::Keep))
        .unwrap();
    first_review.toggle_selected(&key).unwrap();
    first_review.highlight(&key).unwrap();
    let session_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("candidate-category-change-session.json");
    first_review.save_session(&session_path).unwrap();
    first_review.seal(&mut db).unwrap();

    write_raw_observation(
        &mut db,
        &plan_id,
        "way/1",
        "校园主路",
        CandidateCategory::Road,
    );
    db.publish_candidate_batch(
        &plan_id.to_string(),
        "boundary-2",
        &[reviewable_projection(
            "way/1",
            "校园主路",
            CandidateCategory::Road,
        )],
    )
    .unwrap();

    let mut second_review = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Keep));
    assert!(matches!(
        second_review.restore_session(&session_path),
        Err(Error::SessionRevisionMismatch { .. })
    ));
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(second_review.selected_count(), 0);
    assert_eq!(second_review.active_category(), CandidateCategory::Road);
    second_review.highlight(&key).unwrap();
    assert_eq!(second_review.highlighted(), Some(&key));
    let highlighted = second_review.view().map_objects;
    assert_eq!(highlighted.len(), 1);
    assert_eq!(highlighted[0].candidate_id, candidate_id);
    assert_eq!(highlighted[0].category, CandidateCategory::Road);
    assert!(highlighted[0].highlighted);
    second_review
        .submit(StateChange::single(key.clone(), ReviewState::Remove))
        .unwrap();
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Remove));
    second_review.seal(&mut db).unwrap();

    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, candidate_id);
    assert_eq!(decisions[0].category, CandidateCategory::Road);
    assert_eq!(decisions[0].review_state, ReviewState::Remove);

    let reloaded = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(reloaded.candidate_count(), 1);
    assert_eq!(reloaded.state_of(&key), Some(ReviewState::Remove));
}

#[test]
fn version_two_session_without_projection_revision_is_rejected() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    write_raw_observation(
        &mut db,
        &plan_id,
        "way/v2",
        "current-building",
        CandidateCategory::Building,
    );
    db.publish_candidate_batch(
        &plan_id.to_string(),
        "fixture-boundary",
        &[reviewable_projection(
            "way/v2",
            "current-building",
            CandidateCategory::Building,
        )],
    )
    .unwrap();
    let candidate_id = candidate_key(&db, &plan_id, "way/v2").candidate_id;

    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-two-session.json");
    let version_two = serde_json::json!({
        "version": 2,
        "plan_id": plan_id.to_string(),
        "active_category": "Road",
        "entries": [{
            "category": "Road",
            "candidate_id": candidate_id.clone(),
            "state": "keep",
            "selected": true
        }]
    });
    std::fs::write(
        &session_path,
        serde_json::to_vec_pretty(&version_two).unwrap(),
    )
    .unwrap();

    let key = CandidateKey::new(candidate_id);
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert!(matches!(
        workbench.restore_session(&session_path),
        Err(Error::SessionRevisionMismatch { .. })
    ));

    assert_eq!(workbench.state_of(&key), Some(ReviewState::Pending));
    assert_eq!(workbench.selected_count(), 0);
    assert_eq!(workbench.active_category(), CandidateCategory::Building);
    assert_eq!(
        workbench.view().map_objects[0].category,
        CandidateCategory::Building
    );
}

#[test]
fn pause_and_resume_roundtrip_via_temp_file() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Keep,
        ))
        .unwrap();
    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 1),
            ReviewState::Remove,
        ))
        .unwrap();
    workbench
        .toggle_selected(&building_key(&db, &plan_id, 2))
        .unwrap();
    workbench.set_active_category(CandidateCategory::Road);

    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("review-session.json");
    workbench.save_session(&session_path).unwrap();

    let mut resumed = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(
        resumed.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Pending)
    );
    resumed.restore_session(&session_path).unwrap();

    assert_eq!(
        resumed.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Keep)
    );
    assert_eq!(
        resumed.state_of(&building_key(&db, &plan_id, 1)),
        Some(ReviewState::Remove)
    );
    assert_eq!(resumed.selected_count(), 1);
    assert_eq!(resumed.active_category(), CandidateCategory::Road);

    let (other_db, other_plan) = fixture();
    let mut other = ReviewWorkbench::load(&other_db, &other_plan).unwrap();
    assert!(matches!(
        other.restore_session(&session_path),
        Err(Error::SessionPlanMismatch { .. })
    ));
}

#[test]
fn seal_batch_writes_back_and_freezes_review() {
    let (mut db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    workbench
        .submit(StateChange::batch(
            vec![
                building_key(&db, &plan_id, 0),
                building_key(&db, &plan_id, 1),
            ],
            ReviewState::Keep,
        ))
        .unwrap();
    workbench
        .submit(StateChange::single(
            candidate_key(&db, &plan_id, "way/r0"),
            ReviewState::Remove,
        ))
        .unwrap();

    let summary = workbench.export_summary();
    assert_eq!(summary.keep_total, 2);
    assert_eq!(summary.pending_count, 6);
    assert_eq!(summary.remove_count, 1);
    assert_eq!(
        summary.keep_by_category,
        vec![(CandidateCategory::Building, 2)]
    );

    let sealed_summary = workbench.seal(&mut db).unwrap();
    assert_eq!(sealed_summary.keep_total, 2);
    assert!(workbench.is_sealed());
    assert!(workbench.view().sealed);

    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 9);
    let (pending, keep, remove) = db.count_review_states(&plan_id.to_string()).unwrap();
    assert_eq!((pending, keep, remove), (6, 2, 1));

    assert!(matches!(
        workbench.submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Remove
        )),
        Err(Error::AlreadySealed)
    ));
    assert!(matches!(workbench.seal(&mut db), Err(Error::AlreadySealed)));

    let reloaded = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(
        reloaded.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Keep)
    );
}
