//! B2 数据持久化——表级单元测试 + 完整采集流程集成测试
//!
//! 单元：三张表各自的 Insert/Select 回读正确性；
//! 集成：模拟一次完整采集（写入 → 断言 raw_observations 有记录 →
//! 增量刷新 → 封账批量写回 → 回收站生命周期）。

use data_persistence::{
    Database, RawObservation, RawObservationsApi, ReviewDecision, ReviewDecisionsApi, TrashApi,
    TrashItem, LATEST_SCHEMA_VERSION,
};
use shared_domain_types::{CandidateCategory, ReviewState};

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
    let decisions = vec![
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            "way/100",
            ReviewState::Keep,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Road,
            "way/200",
            ReviewState::Remove,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Water,
            "way/300",
            ReviewState::Pending,
        ),
    ];

    db.batch_update_review_decisions(&decisions).unwrap();

    let rows = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(rows.len(), 3);
    let (pending, keep, remove) = db.count_review_states("plan-1").unwrap();
    assert_eq!((pending, keep, remove), (1, 1, 1));
}

#[test]
fn review_decision_same_key_in_one_batch_last_write_wins() {
    let mut db = Database::open_in_memory().unwrap();
    // 同主键两条在同一批：后写覆盖前写（UPSERT），整批在同一事务内提交
    let decisions = vec![
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            "way/100",
            ReviewState::Keep,
        ),
        ReviewDecision::new(
            "plan-1",
            CandidateCategory::Building,
            "way/100",
            ReviewState::Remove,
        ),
    ];

    db.batch_update_review_decisions(&decisions).unwrap();
    let rows = db.list_review_decisions("plan-1").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].review_state, ReviewState::Remove);
}

#[test]
fn review_decision_reseal_updates_state() {
    let mut db = Database::open_in_memory().unwrap();
    db.batch_update_review_decisions(&[ReviewDecision::new(
        "plan-1",
        CandidateCategory::Building,
        "way/100",
        ReviewState::Pending,
    )])
    .unwrap();

    // 重新采集重新评审后的第二次封账：同主键状态更新
    db.batch_update_review_decisions(&[ReviewDecision::new(
        "plan-1",
        CandidateCategory::Building,
        "way/100",
        ReviewState::Keep,
    )])
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

    // 3. 模拟 F5 封账：一条保留、其余维持待定（缝 4——一次性批量写回）
    let decisions: Vec<ReviewDecision> = stored
        .iter()
        .map(|obs| {
            let state = if obs.entity_id == "way/1" {
                ReviewState::Keep
            } else {
                ReviewState::Pending
            };
            ReviewDecision::new(plan_id, obs.entity_type, obs.entity_id.clone(), state)
        })
        .collect();
    db.batch_update_review_decisions(&decisions).unwrap();
    let (pending, keep, remove) = db.count_review_states(plan_id).unwrap();
    assert_eq!((pending, keep, remove), (4, 1, 0));

    // 4. 封账后数据粮仓原样保留（永不删除铁律）
    assert_eq!(db.list_raw_observations(plan_id).unwrap().len(), 5);

    // 5. 方案删除进回收站，粮仓依然不动
    let trash_item = TrashItem::new_plan("campus-1", plan_id, None);
    db.insert_to_trash(&trash_item).unwrap();
    assert_eq!(db.list_raw_observations(plan_id).unwrap().len(), 5);
}
