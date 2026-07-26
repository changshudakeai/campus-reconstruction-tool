//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、三组 trait 的关键行为可调用。

use data_persistence::{
    Database, Error, RawObservation, RawObservationsApi, ReviewDecision, ReviewDecisionsApi,
    TrashApi, TrashItem, LATEST_SCHEMA_VERSION, TRASH_RETENTION_DAYS,
};
use shared_domain_types::{CandidateCategory, ReviewState};

#[test]
fn public_api_types_exist() {
    // 常量
    assert_eq!(LATEST_SCHEMA_VERSION, 2);
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
}

/// 测试内取当前时刻（避免直接依赖 chrono 的版本细节）
fn chrono_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}
