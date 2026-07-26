//! 着陆流程集成测试（ADR-0006）
//!
//! 核心断言：上次使用的校区已被删除（库中查无此校区）时，
//! `landing_campus` 返回 `None`——调用方退回校区选择页，而不是报错。

use data_persistence::{CampusCrudApi, Database};
use global_settings::SettingsManager;
use shared_domain_types::CampusId;

#[test]
fn landing_returns_campus_after_remember() {
    let mut db = Database::open_in_memory().unwrap();
    let campus = db.create_campus("华东师范大学(普陀校区)").unwrap();
    let campus_id = CampusId::parse(&campus.id).unwrap();

    let mut manager = SettingsManager::new(db);
    manager.remember_campus(&campus_id).unwrap();

    let landed = manager.landing_campus().unwrap().expect("校区存在必着陆");
    assert_eq!(landed.id, campus_id);
    assert_eq!(landed.name, "华东师范大学(普陀校区)");
}

#[test]
fn landing_returns_none_when_campus_was_deleted() {
    let manager_db = Database::open_in_memory().unwrap();
    let mut manager = SettingsManager::new(manager_db);

    // 记住一个库中不存在的校区 ID，等价于"上次使用的校区已被删除"
    let deleted = CampusId::generate();
    manager.remember_campus(&deleted).unwrap();

    assert!(
        manager.landing_campus().unwrap().is_none(),
        "校区被删 → None（退回校区选择页，ADR-0006）"
    );
}

#[test]
fn landing_returns_none_when_never_remembered() {
    let manager = SettingsManager::new(Database::open_in_memory().unwrap());
    assert!(manager.landing_campus().unwrap().is_none());
}

#[test]
fn switching_campus_overwrites_previous_landing() {
    let mut db = Database::open_in_memory().unwrap();
    let first = db.create_campus("第一大学").unwrap();
    let second = db.create_campus("第二大学").unwrap();
    let first_id = CampusId::parse(&first.id).unwrap();
    let second_id = CampusId::parse(&second.id).unwrap();

    let mut manager = SettingsManager::new(db);
    manager.remember_campus(&first_id).unwrap();
    manager.remember_campus(&second_id).unwrap();

    let landed = manager.landing_campus().unwrap().unwrap();
    assert_eq!(landed.name, "第二大学", "切换校区后着陆到最新校区");
}
