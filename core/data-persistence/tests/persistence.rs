//! B2 数据持久化——表级单元测试 + 完整采集流程集成测试
//!
//! 单元：三张表各自的 Insert/Select 回读正确性；
//! 集成：模拟一次完整采集（写入 → 断言 raw_observations 有记录 →
//! 增量刷新 → 封账批量写回 → 回收站生命周期）。

use data_persistence::{
    boundary_fingerprint, BoundaryRevalidationApi, CandidateDisplay, CandidateEligibility,
    CandidateProjectionDraft, CandidateProjectionsApi, CandidateRevalidationFact, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi, ReviewDecision,
    ReviewDecisionsApi, ReviewableValidation, TrashApi, TrashItem, LATEST_SCHEMA_VERSION,
};
use shared_domain_types::{Boundary, CandidateCategory, ReviewState};

fn sample_observation(plan_id: &str, entity_id: &str, name: &str) -> RawObservation {
    RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        entity_id,
        serde_json::json!({ "name": name, "building": "school", "height": 12 }),
        "overpass",
    )
}

// ── 表 1：raw_observations 单元测试 ─────────────────────────────

#[test]
fn raw_observation_insert_then_select_roundtrip() {
    let mut db = Database::open_in_memory().unwrap();
    let observation = sample_observation("plan-1", "way/100", "教学楼A");

    let written = db
        .write_raw_observations(std::slice::from_ref(&observation))
        .unwrap();
    assert_eq!(written, 1);

    let rows = db.list_raw_observations("plan-1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].entity_id, "way/100");
    assert_eq!(rows[0].entity_type, CandidateCategory::Building);
    assert_eq!(rows[0].data_source_tag, "overpass");
    assert_eq!(rows[0].source_data["name"], "教学楼A");
    assert_eq!(rows[0].digest, observation.digest);
}

#[test]
fn raw_observation_same_digest_is_not_rewritten() {
    let mut db = Database::open_in_memory().unwrap();
    let observation = sample_observation("plan-1", "way/100", "教学楼A");
    db.write_raw_observations(std::slice::from_ref(&observation))
        .unwrap();

    // 同一实体、同一内容再采一次：digest 相同，0 行受影响
    let rewritten = db
        .write_raw_observations(&[sample_observation("plan-1", "way/100", "教学楼A")])
        .unwrap();
    assert_eq!(rewritten, 0);
    assert_eq!(db.list_raw_observations("plan-1").unwrap().len(), 1);
}

#[test]
fn raw_observation_changed_content_refreshes_in_place() {
    let mut db = Database::open_in_memory().unwrap();
    db.write_raw_observations(&[sample_observation("plan-1", "way/100", "教学楼A")])
        .unwrap();
    let before = db
        .find_raw_observation("plan-1", CandidateCategory::Building, "way/100")
        .unwrap()
        .unwrap();

    // 内容变了（改名）：增量刷新，行数不变、digest 更新、created_at 不动
    let renamed = sample_observation("plan-1", "way/100", "教学楼A（翻修）");
    let refreshed = db
        .write_raw_observations(std::slice::from_ref(&renamed))
        .unwrap();
    assert_eq!(refreshed, 1);

    let rows = db.list_raw_observations("plan-1").unwrap();
    assert_eq!(rows.len(), 1, "数据粮仓：刷新是更新既有行，不叠加重复行");
    assert_eq!(rows[0].digest, renamed.digest);
    assert_ne!(rows[0].digest, before.digest);
    assert_eq!(rows[0].created_at, before.created_at);
}

// ── 表 2：review_decisions 单元测试 ─────────────────────────────

#[test]
fn review_decision_batch_write_then_select_roundtrip() {
    let mut db = Database::open_in_memory().unwrap();
    let (revision, candidate_ids) = publish_reviewables(
        &mut db,
        "plan-1",
        &[
            (CandidateCategory::Building, "way/100"),
            (CandidateCategory::Road, "way/200"),
            (CandidateCategory::Water, "way/300"),
        ],
    );
    let decisions = vec![
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_ids[0].clone(),
            ReviewState::Keep,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Road,
            candidate_ids[1].clone(),
            ReviewState::Remove,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Water,
            candidate_ids[2].clone(),
            ReviewState::Pending,
        ),
    ];

    db.batch_update_review_decisions_at_revision("plan-1", &revision, &decisions)
        .unwrap();

    let rows = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(rows.len(), 3);
    let (pending, keep, remove) = db.count_review_states("plan-1").unwrap();
    assert_eq!((pending, keep, remove), (1, 1, 1));
}

#[test]
fn review_decision_duplicate_candidate_rejects_the_whole_seal() {
    let mut db = Database::open_in_memory().unwrap();
    let (revision, candidate_ids) = publish_reviewables(
        &mut db,
        "plan-1",
        &[(CandidateCategory::Building, "way/100")],
    );
    let decisions = vec![
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_ids[0].clone(),
            ReviewState::Keep,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_ids[0].clone(),
            ReviewState::Remove,
        ),
    ];

    assert!(db
        .batch_update_review_decisions_at_revision("plan-1", &revision, &decisions)
        .is_err());
    assert!(db.list_review_decisions("plan-1").unwrap().is_empty());
}

#[test]
fn review_decision_category_changes_without_duplicating_candidate_identity() {
    let mut db = Database::open_in_memory().unwrap();
    let (first_revision, candidate_ids) =
        publish_reviewables(&mut db, "plan-1", &[(CandidateCategory::Building, "way/1")]);
    let candidate_id = candidate_ids[0].clone();
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &first_revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_id.clone(),
            ReviewState::Keep,
        )],
    )
    .unwrap();
    let (second_revision, second_ids) =
        publish_reviewables(&mut db, "plan-1", &[(CandidateCategory::Road, "way/1")]);
    assert_eq!(second_ids[0], candidate_id);
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &second_revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Road,
            candidate_id.clone(),
            ReviewState::Remove,
        )],
    )
    .unwrap();

    let rows = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].category, CandidateCategory::Road);
    assert_eq!(rows[0].candidate_id, candidate_id);
    assert_eq!(rows[0].review_state, ReviewState::Remove);
}

#[test]
fn review_decision_reseal_updates_state() {
    let mut db = Database::open_in_memory().unwrap();
    let (revision, candidate_ids) = publish_reviewables(
        &mut db,
        "plan-1",
        &[(CandidateCategory::Building, "way/100")],
    );
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_ids[0].clone(),
            ReviewState::Pending,
        )],
    )
    .unwrap();

    // 重新采集重新评审后的第二次封账：同主键状态更新
    db.batch_update_review_decisions_at_revision(
        "plan-1",
        &revision,
        &[ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            candidate_ids[0].clone(),
            ReviewState::Keep,
        )],
    )
    .unwrap();

    let rows = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].review_state.is_keep());
}

// ── 表 3：trash 单元测试 ────────────────────────────────────────

#[test]
fn trash_insert_then_list_roundtrip() {
    let mut db = Database::open_in_memory().unwrap();
    let item = TrashItem::new_plan("campus-1", "plan-1", Some("user".to_owned()));

    db.insert_to_trash(&item).unwrap();

    let listed = db.list_restorable_trash("campus-1").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, item.id);
    assert_eq!(listed[0].entity_type, "plan");
    assert_eq!(listed[0].entity_id, "plan-1");
    assert_eq!(listed[0].deleted_by.as_deref(), Some("user"));
}

#[test]
fn trash_restore_marks_item_and_removes_from_listing() {
    let mut db = Database::open_in_memory().unwrap();
    let item = TrashItem::new_plan("campus-1", "plan-1", None);
    db.insert_to_trash(&item).unwrap();

    let restored = db.restore_from_trash(&item.id).unwrap();
    assert!(restored.restored_at.is_some());
    assert!(db.list_restorable_trash("campus-1").unwrap().is_empty());

    // 已恢复的条目不能再恢复，也不能永久删除
    assert!(db.restore_from_trash(&item.id).is_err());
    assert!(db.permanently_delete(&item.id).is_err());
}

#[test]
fn trash_permanent_delete_requires_active_item() {
    let mut db = Database::open_in_memory().unwrap();
    let item = TrashItem::new_plan("campus-1", "plan-1", None);
    db.insert_to_trash(&item).unwrap();

    db.permanently_delete(&item.id).unwrap();
    assert!(db.list_restorable_trash("campus-1").unwrap().is_empty());
    // 二次永久删除被拒
    assert!(db.permanently_delete(&item.id).is_err());
    // 不存在的条目被拒
    assert!(db.permanently_delete("no-such-id").is_err());
}

#[test]
fn trash_purge_expired_only_touches_overdue_items() {
    let mut db = Database::open_in_memory().unwrap();
    // 新鲜条目：不该被清理
    let fresh = TrashItem::new_plan("campus-1", "plan-fresh", None);
    db.insert_to_trash(&fresh).unwrap();
    // 过期条目：deleted_at 拨回 31 天前
    let mut stale = TrashItem::new_plan("campus-1", "plan-stale", None);
    stale.deleted_at = chrono::Utc::now() - chrono::Duration::days(31);
    db.insert_to_trash(&stale).unwrap();

    let purged = db.purge_expired().unwrap();
    assert_eq!(purged, 1);

    let remaining = db.list_restorable_trash("campus-1").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].plan_id, "plan-fresh");
}

// ── 集成测试：模拟一次完整采集流程 ──────────────────────────────

#[test]
fn full_collection_flow_lands_in_raw_observations() {
    let mut db = Database::open_in_memory().unwrap();
    assert_eq!(db.schema_version().unwrap(), Some(LATEST_SCHEMA_VERSION));

    // 1. 模拟 F4 采集：一小批六类候选（缝 3——原始观测当场落库）
    let plan_id = "plan-integration";
    let batch = vec![
        sample_observation(plan_id, "way/1", "教学楼A"),
        sample_observation(plan_id, "way/2", "图书馆"),
        RawObservation::new(
            plan_id,
            CandidateCategory::Road,
            "way/3",
            serde_json::json!({ "highway": "footway" }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Water,
            "way/4",
            serde_json::json!({ "leisure": "swimming_pool", "location": "outdoor" }),
            "gaode",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Other,
            "way/5",
            serde_json::json!({ "railway": "rail", "usage": "industrial" }),
            "overpass",
        ),
    ];
    let written = db.write_raw_observations(&batch).unwrap();
    assert_eq!(written, 5, "采集批次应全部入粮仓");

    // 2. 断言 raw_observations 表中有 N 条记录
    let stored = db.list_raw_observations(plan_id).unwrap();
    assert_eq!(stored.len(), 5);
    let buildings = db
        .list_raw_observations_by_category(plan_id, CandidateCategory::Building)
        .unwrap();
    assert_eq!(buildings.len(), 2);

    // 3. lifecycle 从来源事实发布完整候选批次，再由 F5 带 revision 封账。
    let drafts: Vec<_> = stored
        .iter()
        .map(|obs| {
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new(&obs.data_source_tag, &obs.entity_id, "fixture"),
                obs.entity_type,
                CandidateDisplay::new(&obs.entity_id, vec![]),
                CandidateShape::point(serde_json::json!([116.4, 39.9])),
                ReviewableValidation::Retained,
            )
        })
        .collect();
    let revision = db
        .publish_candidate_batch(plan_id, "integration-boundary", &drafts)
        .unwrap()
        .batch
        .id;
    let projections = db.list_current_candidate_projections(plan_id).unwrap();
    let decisions: Vec<ReviewDecision> = projections
        .iter()
        .map(|projection| {
            let state = if projection.source_entity_id == "way/1" {
                ReviewState::Keep
            } else {
                ReviewState::Pending
            };
            ReviewDecision::new(
                plan_id,
                projection.category,
                projection.candidate_id.clone(),
                state,
            )
        })
        .collect();
    db.batch_update_review_decisions_at_revision(plan_id, &revision, &decisions)
        .unwrap();
    let (pending, keep, remove) = db.count_review_states(plan_id).unwrap();
    assert_eq!((pending, keep, remove), (4, 1, 0));

    // 4. 封账后数据粮仓原样保留（永不删除铁律）
    assert_eq!(db.list_raw_observations(plan_id).unwrap().len(), 5);

    // 5. 方案删除进回收站，粮仓依然不动
    let trash_item = TrashItem::new_plan("campus-1", plan_id, None);
    db.insert_to_trash(&trash_item).unwrap();
    assert_eq!(db.list_raw_observations(plan_id).unwrap().len(), 5);
}

#[test]
fn boundary_fingerprint_changes_with_coordinates() {
    let boundary = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.40, 39.90],
            [116.41, 39.90],
            [116.41, 39.91],
            [116.40, 39.90]
        ]]),
    };
    let same = Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.40, 39.90],
            [116.41, 39.90],
            [116.41, 39.91],
            [116.40, 39.90]
        ]]),
    };
    assert_eq!(boundary_fingerprint(&boundary), boundary_fingerprint(&same));
    let mut moved = boundary.clone();
    moved.coordinates = serde_json::json!([[
        [116.40, 39.90],
        [116.42, 39.90],
        [116.42, 39.91],
        [116.40, 39.90]
    ]]);
    assert_ne!(
        boundary_fingerprint(&boundary),
        boundary_fingerprint(&moved)
    );
}

#[test]
fn boundary_revalidation_applies_eligibility_voids_and_pending_atomically() {
    let mut db = Database::open_in_memory().unwrap();
    let plan = "plan-rv";
    let observation_a = RawObservation::new(
        plan,
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({
            "name": "A",
            "tags": {"building": "school"},
            "payload": {},
            "source_geometry": {"kind": "point", "coordinates": [116.40, 39.90]},
            "geometry_part_id": "point"
        }),
        "overpass",
    );
    let observation_b = RawObservation::new(
        plan,
        CandidateCategory::Building,
        "way/2",
        serde_json::json!({
            "name": "B",
            "tags": {"building": "school"},
            "payload": {},
            "source_geometry": {"kind": "point", "coordinates": [116.41, 39.91]},
            "geometry_part_id": "point"
        }),
        "overpass",
    );
    db.write_raw_observations(&[observation_a.clone(), observation_b.clone()])
        .unwrap();
    let first_revision = db
        .publish_candidate_batch(
            plan,
            "fp-1",
            &[
                CandidateProjectionDraft::reviewable(
                    CandidateSourceIdentity::new("overpass", "way/1", "point"),
                    CandidateCategory::Building,
                    CandidateDisplay::new("教学楼A", vec![]),
                    CandidateShape::point(serde_json::json!([116.40, 39.90])),
                    ReviewableValidation::Retained,
                ),
                CandidateProjectionDraft::reviewable(
                    CandidateSourceIdentity::new("overpass", "way/2", "point"),
                    CandidateCategory::Building,
                    CandidateDisplay::new("教学楼B", vec![]),
                    CandidateShape::point(serde_json::json!([116.41, 39.91])),
                    ReviewableValidation::Retained,
                ),
            ],
        )
        .unwrap()
        .batch
        .id;
    let current = db.list_current_candidate_projections(plan).unwrap();
    let cand_a = current
        .iter()
        .find(|projection| projection.source_entity_id == "way/1")
        .unwrap()
        .candidate_id
        .clone();
    let cand_b = current
        .iter()
        .find(|projection| projection.source_entity_id == "way/2")
        .unwrap()
        .candidate_id
        .clone();
    db.batch_update_review_decisions_at_revision(
        plan,
        &first_revision,
        &[
            ReviewDecision::new(
                plan,
                CandidateCategory::Building,
                cand_a.clone(),
                ReviewState::Keep,
            ),
            ReviewDecision::new(
                plan,
                CandidateCategory::Building,
                cand_b.clone(),
                ReviewState::Remove,
            ),
        ],
    )
    .unwrap();

    // 指纹随候选批次原子发布。
    assert_eq!(
        db.load_plan_collection_boundary(plan).unwrap().as_deref(),
        Some("fp-1")
    );

    // 第一次边界变化只让 cand-b 离开：Remove 进入历史；cand-a 的 Keep 仍有效。
    let first_change = db
        .publish_candidate_revalidation(
            plan,
            "fp-1b",
            &[
                CandidateRevalidationFact::reviewable(
                    cand_a.clone(),
                    CandidateShape::point(serde_json::json!([116.40, 39.90])),
                    ReviewableValidation::Retained,
                ),
                CandidateRevalidationFact::isolated_validated(
                    cand_b.clone(),
                    CandidateShape::point(serde_json::json!([116.41, 39.91])),
                    ReviewableValidation::Retained,
                    "outside_confirmed_plan_boundary",
                )
                .unwrap(),
            ],
        )
        .unwrap();
    assert_eq!(first_change.decisions_voided, 1);

    // 单事务发布新 revision：cand-a 隔离 + cand-b 恢复 Reviewable；两条旧决定
    // 都进入历史，当前 cand-b 回待定，指纹一并更新。
    let summary = db
        .publish_candidate_revalidation(
            plan,
            "fp-2",
            &[
                CandidateRevalidationFact::isolated_validated(
                    cand_a.clone(),
                    CandidateShape::point(serde_json::json!([116.40, 39.90])),
                    ReviewableValidation::Retained,
                    "outside_confirmed_plan_boundary",
                )
                .unwrap(),
                CandidateRevalidationFact::reviewable(
                    cand_b.clone(),
                    CandidateShape::point(serde_json::json!([116.41, 39.91])),
                    ReviewableValidation::Retained,
                ),
            ],
        )
        .unwrap();
    assert_eq!(summary.eligibility_updated, 2);
    assert_eq!(summary.decisions_voided, 1);
    assert_eq!(summary.decisions_reset_to_pending, 1);
    assert_eq!(
        db.load_plan_collection_boundary(plan).unwrap().as_deref(),
        Some("fp-2")
    );

    // cand-a 投影已隔离（资格更新落库）。
    let all = db.list_current_candidate_projections(plan).unwrap();
    let a = all.iter().find(|p| p.candidate_id == cand_a).unwrap();
    assert_eq!(a.eligibility(), CandidateEligibility::Isolated);
    assert_eq!(
        a.isolation_reason(),
        Some("outside_confirmed_plan_boundary")
    );

    // 决定：cand-a 被作废标注（保留记录 + 历史）；cand-b 回到待定；
    // 常规读取只看到未作废的 cand-b。
    let decisions = db.list_review_decisions(plan).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, cand_b);
    assert!(decisions[0].review_state.is_pending());
    let (pending, keep, remove) = db.count_review_states(plan).unwrap();
    assert_eq!((pending, keep, remove), (1, 0, 0));
    let voided = db.list_voided_review_decisions(plan).unwrap();
    assert_eq!(voided.len(), 1);
    assert_eq!(voided[0].candidate_id, cand_a);
    assert_eq!(voided[0].review_state, ReviewState::Keep);
    assert_eq!(
        voided[0].voided_reason.as_deref(),
        Some("candidate_became_isolated_after_boundary_change")
    );
    assert!(voided[0].voided_at.is_some());
    let history = db.list_review_decision_invalidations(plan).unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.iter().any(|entry| {
        entry.candidate_id == cand_a && entry.previous_state == ReviewState::Keep
    }));
    assert!(history.iter().any(|entry| {
        entry.candidate_id == cand_b && entry.previous_state == ReviewState::Remove
    }));
}

fn publish_reviewables(
    db: &mut Database,
    plan_id: &str,
    candidates: &[(CandidateCategory, &str)],
) -> (String, Vec<String>) {
    let observations: Vec<_> = candidates
        .iter()
        .map(|(category, entity_id)| {
            RawObservation::new(
                plan_id,
                *category,
                *entity_id,
                serde_json::json!({"entity": entity_id}),
                "overpass",
            )
        })
        .collect();
    db.write_raw_observations(&observations).unwrap();
    let drafts: Vec<_> = candidates
        .iter()
        .map(|(category, entity_id)| {
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new("overpass", *entity_id, "outer"),
                *category,
                CandidateDisplay::new(*entity_id, vec![]),
                CandidateShape::point(serde_json::json!([116.4, 39.9])),
                ReviewableValidation::Retained,
            )
        })
        .collect();
    let summary = db
        .publish_candidate_batch(plan_id, "test-boundary", &drafts)
        .unwrap();
    let current = db.list_current_candidate_projections(plan_id).unwrap();
    let ids = candidates
        .iter()
        .map(|(_, entity_id)| {
            current
                .iter()
                .find(|projection| projection.source_entity_id == *entity_id)
                .unwrap()
                .candidate_id
                .clone()
        })
        .collect();
    (summary.batch.id, ids)
}
