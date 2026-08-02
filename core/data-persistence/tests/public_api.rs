//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、三组 trait 的关键行为可调用。

use data_persistence::{
    AppSettingKey, AppSettingsApi, CampusCrudApi, CandidateBatchStatus, CandidateDisplay,
    CandidateEligibility, CandidateProjection, CandidateProjectionsApi, CandidateShape,
    CandidateValidation, Database, Error, PlanCrudApi, RawObservation, RawObservationsApi,
    ReviewDecision, ReviewDecisionsApi, TrashApi, TrashItem, LATEST_SCHEMA_VERSION,
    TRASH_RETENTION_DAYS,
};
use shared_domain_types::{CandidateCategory, ReviewState};

#[test]
fn public_api_types_exist() {
    // ADR-0040：版本号升级到 6（候选投影展示属性）
    assert_eq!(LATEST_SCHEMA_VERSION, 6);
    assert_eq!(TRASH_RETENTION_DAYS, 30);

    // Database：打开即迁移到最新版本
    let mut db = Database::open_in_memory().expect("内存库可打开");
    assert_eq!(db.schema_version().unwrap(), Some(LATEST_SCHEMA_VERSION));
    assert!(format!("{db:?}").contains("Database"));

    // RawObservation：构造 + digest 计算
    let source = serde_json::json!({ "building": "school" });
    let digest = RawObservation::compute_digest(&source);
    assert_eq!(digest.len(), 64, "SHA256 十六进制指纹应为 64 字符");
    let observation = RawObservation::new(
        "plan-1",
        CandidateCategory::Building,
        "way/1",
        source,
        "overpass",
    );
    assert_eq!(observation.digest, digest);

    // RawObservationsApi trait
    assert_eq!(db.write_raw_observations(&[observation]).unwrap(), 1);
    assert_eq!(db.list_raw_observations("plan-1").unwrap().len(), 1);
    assert_eq!(
        db.list_raw_observations_by_category("plan-1", CandidateCategory::Building)
            .unwrap()
            .len(),
        1
    );
    assert!(db
        .find_raw_observation("plan-1", CandidateCategory::Building, "way/1")
        .unwrap()
        .is_some());

    // ADR-0040：候选投影只能在完整批次发布后对评审入口可见。
    let batch = db.prepare_candidate_batch("plan-1").unwrap();
    assert_eq!(batch.status, CandidateBatchStatus::Building);
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
        .unwrap();
    assert!(db
        .list_reviewable_candidate_projections("plan-1")
        .unwrap()
        .is_empty());
    db.publish_candidate_batch(&batch.id).unwrap();
    assert!(db
        .get_current_candidate_projection("plan-1", "overpass:way/1:outer")
        .unwrap()
        .is_some());
    assert_eq!(
        db.candidate_batch_summary(&batch.id)
            .unwrap()
            .reviewable_count,
        1
    );

    // ReviewDecision + ReviewDecisionsApi trait
    let decision = ReviewDecision::new(
        "plan-1",
        CandidateCategory::Building,
        "way/1",
        ReviewState::Keep,
    );
    db.batch_update_review_decisions(&[decision]).unwrap();
    assert_eq!(db.list_review_decisions("plan-1").unwrap().len(), 1);
    assert_eq!(db.count_review_states("plan-1").unwrap(), (0, 1, 0));

    // TrashItem + TrashApi trait
    let item = TrashItem::new_plan("campus-1", "plan-1", None);
    assert!(item.is_restorable(chrono_now()));
    db.insert_to_trash(&item).unwrap();
    assert_eq!(db.list_restorable_trash("campus-1").unwrap().len(), 1);
    let restored = db.restore_from_trash(&item.id).unwrap();
    assert!(restored.restored_at.is_some());
    assert_eq!(db.purge_expired().unwrap(), 0);

    // Error #[non_exhaustive]：带类型错误可匹配
    let err = db.permanently_delete(&item.id).unwrap_err();
    assert!(matches!(err, Error::TrashOperationRejected(_)));
    assert!(!err.to_string().is_empty());

    // CampusCrudApi / PlanCrudApi / AppSettingsApi（缝 2，T04）
    let campus = db.create_campus("测试大学").unwrap();
    assert_eq!(db.list_campuses().unwrap().len(), 1);
    assert!(db.find_campus_by_id(&campus.id).unwrap().is_some());

    let plan = db.create_plan(&campus.id, "方案 1").unwrap();
    assert_eq!(db.list_plans(&campus.id).unwrap().len(), 1);
    assert!(db.find_plan_by_id(&plan.id).unwrap().is_some());
    db.rename_plan(&plan.id, "方案 1 改").unwrap();
    db.touch_plan(&plan.id).unwrap();
    let dup = db.create_plan(&campus.id, "方案 1 改").unwrap_err();
    assert!(matches!(dup, Error::DuplicatePlanName(_)));

    // 校区地址（ADR-0006 最近使用记录展示）：带地址创建与读回
    let with_address = db
        .create_campus_with_anchor(
            "华东师范大学(普陀校区)",
            "B01",
            "中山北路3663号",
            121.406,
            31.228,
        )
        .unwrap();
    assert_eq!(with_address.address, "中山北路3663号");
    assert_eq!(
        db.find_campus_by_id(&with_address.id)
            .unwrap()
            .map(|c| c.address),
        Some("中山北路3663号".to_owned())
    );
    // 最近使用记录键（F1 持久化 JSON 校区 ID 列表，ADR-0006）
    db.set_setting(AppSettingKey::RecentCampuses, "[\"campus-1\",\"campus-2\"]")
        .unwrap();
    assert_eq!(
        db.get_setting(AppSettingKey::RecentCampuses)
            .unwrap()
            .as_deref(),
        Some("[\"campus-1\",\"campus-2\"]")
    );

    db.set_setting(AppSettingKey::LastUsedCampus, &campus.id)
        .unwrap();
    assert_eq!(
        db.get_setting(AppSettingKey::LastUsedCampus).unwrap(),
        Some(campus.id.clone())
    );

    // 回收站集成：删除进站 → 确认后永久删除（方案行一并清理）
    let trashed = db.delete_plan_to_trash(&campus.id, &plan.id).unwrap();
    assert!(db.list_plans(&campus.id).unwrap().is_empty());
    db.purge_plan_permanently(&trashed.id).unwrap();
    assert!(db.find_plan_by_id(&plan.id).unwrap().is_none());
    assert_eq!(db.purge_expired_plans().unwrap(), 0);

    // 清空回收站：一次性永久删除当前校区全部仍可恢复的方案，不影响其他校区
    let other_campus = db.create_campus("另一所大学").unwrap();
    let other_plan = db.create_plan(&other_campus.id, "他人方案").unwrap();
    let second = db.create_plan(&campus.id, "方案 2").unwrap();
    db.delete_plan_to_trash(&campus.id, &second.id).unwrap();
    db.delete_plan_to_trash(&other_campus.id, &other_plan.id)
        .unwrap();
    assert_eq!(db.purge_all_in_campus_trash(&campus.id).unwrap(), 1);
    assert!(db.list_restorable_trash(&campus.id).unwrap().is_empty());
    assert_eq!(
        db.list_restorable_trash(&other_campus.id).unwrap().len(),
        1,
        "其他校区回收站不受影响"
    );
    assert!(db.find_plan_by_id(&second.id).unwrap().is_none());
    db.purge_all_in_campus_trash(&other_campus.id).unwrap();
    assert!(db
        .list_restorable_trash(&other_campus.id)
        .unwrap()
        .is_empty());
}

/// 测试内取当前时刻（避免直接依赖 chrono 的版本细节）
fn chrono_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}
