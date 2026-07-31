//! F3 集成测试：真实调用 B2 内存库（不用 mock），覆盖工单验收点
//!
//! - 新建方案 → 同名冲突被拦截（ADR-0010）
//! - 删除方案 → 回收站中存在且 deleted_at 不为空（ADR-0018）
//! - 卡片三件套与最近修改倒序、复制加"副本"后缀、恢复冲突

use data_persistence::{Database, TrashApi};
use project_management::{PlanProgress, ProjectManager};
use shared_domain_types::CampusId;

/// 建一个带校区的 manager，返回 (manager, campus_id)
fn manager_with_campus() -> (ProjectManager, CampusId) {
    let db = Database::open_in_memory().expect("内存库可打开");
    let mut manager = ProjectManager::new(db);
    let campus = manager.create_campus("测试大学").unwrap();
    let campus_id = CampusId::parse(&campus.id).unwrap();
    (manager, campus_id)
}

#[test]
fn create_plan_rejects_duplicate_name_in_same_campus() {
    let (mut manager, campus_id) = manager_with_campus();
    manager.create_plan(&campus_id, "方案 1").unwrap();

    // 同校区同名 → 带类型冲突错误
    let err = manager.create_plan(&campus_id, "方案 1").unwrap_err();
    assert!(err.is_duplicate_name(), "同名冲突必须被拦截：{err}");

    // 跨校区可重名（ADR-0010 后果条）
    let other = manager.create_campus("另一所大学").unwrap();
    let other_id = CampusId::parse(&other.id).unwrap();
    assert!(manager.create_plan(&other_id, "方案 1").is_ok());
}

#[test]
fn rename_plan_rejects_duplicate_and_allows_same_name() {
    let (mut manager, campus_id) = manager_with_campus();
    let first = manager.create_plan(&campus_id, "方案 1").unwrap();
    manager.create_plan(&campus_id, "方案 2").unwrap();

    let err = manager.rename_plan(&first, "方案 2").unwrap_err();
    assert!(err.is_duplicate_name());

    // 改成新名字成功
    manager.rename_plan(&first, "方案 1 - 全景复刻").unwrap();
    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert!(cards.iter().any(|c| c.name == "方案 1 - 全景复刻"));
}

#[test]
fn plan_cards_show_three_pieces_and_sort_by_recent() {
    let (mut manager, campus_id) = manager_with_campus();
    let first = manager.create_plan(&campus_id, "方案 1").unwrap();
    manager.create_plan(&campus_id, "方案 2").unwrap();

    // 改名会刷新"方案 1"的最后修改时间 → 它排到最前
    manager.rename_plan(&first, "方案 1 改").unwrap();

    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert_eq!(cards.len(), 2);
    // 三件套：名称 / 进度描述 / 最后修改时间
    assert_eq!(cards[0].name, "方案 1 改");
    assert_eq!(cards[0].progress, PlanProgress::BoundaryNotSet);
    assert_eq!(cards[0].progress.text_key(), "plan.boundary_not_set");
    assert!(!cards[0].last_modified_at.is_empty());
    // 最近修改倒序
    assert!(cards[0].last_modified_at >= cards[1].last_modified_at);
}

#[test]
fn duplicate_plan_appends_suffix_and_resolves_collisions() {
    let (mut manager, campus_id) = manager_with_campus();
    let source = manager.create_plan(&campus_id, "方案 1").unwrap();

    manager.duplicate_plan(&source, "副本").unwrap();
    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert!(cards.iter().any(|c| c.name == "方案 1 副本"));

    // 再复制一次：后缀撞名时自动追加序号
    manager.duplicate_plan(&source, "副本").unwrap();
    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert!(cards.iter().any(|c| c.name == "方案 1 副本 2"));
}

#[test]
fn delete_plan_lands_in_trash_with_deleted_at() {
    let (mut manager, campus_id) = manager_with_campus();
    let plan_id = manager.create_plan(&campus_id, "方案 1").unwrap();

    let trash = manager.delete_plan(&campus_id, &plan_id).unwrap();

    // 它在 trash 表中存在且 deleted_at 不为空（用 B2 TrashApi 直接对账）
    let items = manager
        .database_mut()
        .list_restorable_trash(&campus_id.to_string())
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, trash.trash_id);
    assert_eq!(items[0].plan_id, plan_id.to_string());
    assert!(!trash.deleted_at.is_empty(), "deleted_at 必须有值");

    // 删除后不再出现在方案列表
    assert!(manager.list_plan_cards(&campus_id).unwrap().is_empty());

    // 重复删除同一方案被拒
    assert!(manager.delete_plan(&campus_id, &plan_id).is_err());
}

#[test]
fn restore_plan_returns_it_to_list() {
    let (mut manager, campus_id) = manager_with_campus();
    let plan_id = manager.create_plan(&campus_id, "方案 1").unwrap();
    let trash = manager.delete_plan(&campus_id, &plan_id).unwrap();

    manager
        .restore_plan(&campus_id, &trash.trash_id, "（恢复 {n}）")
        .unwrap();
    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].name, "方案 1");
    assert!(manager.list_trash(&campus_id).unwrap().is_empty());
}

#[test]
fn restore_conflict_auto_renames_with_suffix() {
    let (mut manager, campus_id) = manager_with_campus();
    let plan_id = manager.create_plan(&campus_id, "方案 1").unwrap();
    let trash = manager.delete_plan(&campus_id, &plan_id).unwrap();

    // 同名与"（恢复 1）"都被占用 → 恢复不阻断，自动依次使用"（恢复 2）"
    manager.create_plan(&campus_id, "方案 1").unwrap();
    manager.create_plan(&campus_id, "方案 1（恢复 1）").unwrap();
    let restored = manager
        .restore_plan(&campus_id, &trash.trash_id, "（恢复 {n}）")
        .unwrap();
    assert_eq!(restored.name, "方案 1（恢复 2）");
    let cards = manager.list_plan_cards(&campus_id).unwrap();
    assert!(cards.iter().any(|card| card.name == "方案 1"));
    assert!(cards.iter().any(|card| card.name == "方案 1（恢复 1）"));
    assert!(cards.iter().any(|card| card.name == "方案 1（恢复 2）"));
}

#[test]
fn purge_confirmed_removes_from_trash_for_good() {
    let (mut manager, campus_id) = manager_with_campus();
    let plan_id = manager.create_plan(&campus_id, "方案 1").unwrap();
    let trash = manager.delete_plan(&campus_id, &plan_id).unwrap();

    // 确认后永久删除（确认窗由 UI 层负责弹出）
    manager.purge_plan_confirmed(&trash.trash_id).unwrap();
    assert!(manager.list_trash(&campus_id).unwrap().is_empty());
    // 永久删除后不可恢复
    assert!(manager
        .restore_plan(&campus_id, &trash.trash_id, "（恢复 {n}）")
        .is_err());
}

#[test]
fn search_campuses_matches_name_case_insensitively() {
    let (mut manager, _) = manager_with_campus();
    manager.create_campus("华东师范大学(普陀校区)").unwrap();
    let results = manager.search_campuses("华东师范").unwrap();
    assert!(
        results.iter().any(|c| c.name.contains("华东师范大学")),
        "按连续名称片段搜索"
    );
    assert!(manager.search_campuses("不存在的学校").unwrap().is_empty());
    assert!(
        manager.search_campuses("   ").unwrap().is_empty(),
        "空关键词不搜索"
    );
}
#[test]
fn suggest_plan_name_skips_existing_names() {
    let (mut manager, campus_id) = manager_with_campus();
    manager.create_plan(&campus_id, "新方案 1").unwrap();
    manager.create_plan(&campus_id, "新方案 2").unwrap();
    let suggested = manager.suggest_plan_name(&campus_id, "新方案").unwrap();
    assert_eq!(suggested, "新方案 3");
}

#[test]
fn trash_list_views_carry_name_campus_and_remaining_days() {
    let (mut manager, campus_id) = manager_with_campus();
    let plan_id = manager.create_plan(&campus_id, "待恢复方案").unwrap();
    manager.delete_plan(&campus_id, &plan_id).unwrap();

    let items = manager.list_trash(&campus_id).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "待恢复方案");
    assert_eq!(items[0].campus_name, "测试大学");
    assert!(
        (0..=30).contains(&items[0].expires_in_days),
        "剩余保留天数应在 0～30"
    );
    assert!(!items[0].deleted_at.is_empty());
}

#[test]
fn purge_all_trash_confirmed_clears_only_current_campus() {
    let (mut manager, campus_id) = manager_with_campus();
    let other = manager.create_campus("另一所大学").unwrap();
    let other_id = CampusId::parse(&other.id).unwrap();
    let p1 = manager.create_plan(&campus_id, "方案 1").unwrap();
    let p2 = manager.create_plan(&campus_id, "方案 2").unwrap();
    let other_plan = manager.create_plan(&other_id, "他人方案").unwrap();
    manager.delete_plan(&campus_id, &p1).unwrap();
    manager.delete_plan(&campus_id, &p2).unwrap();
    manager.delete_plan(&other_id, &other_plan).unwrap();

    assert_eq!(manager.purge_all_trash_confirmed(&campus_id).unwrap(), 2);
    assert!(manager.list_trash(&campus_id).unwrap().is_empty());
    assert_eq!(
        manager.list_trash(&other_id).unwrap().len(),
        1,
        "其他校区不受影响"
    );
}

#[test]
fn landing_returns_none_when_campus_deleted_or_unset() {
    let db = Database::open_in_memory().expect("内存库可打开");
    let mut manager = ProjectManager::new(db);

    // 未设置过 → None（首次流程走 ADR-0004）
    assert!(manager.landing_campus().unwrap().is_none());

    // 指向不存在的校区 → None（退回校区选择页，ADR-0006）
    let ghost = CampusId::generate();
    manager.remember_campus(&ghost).unwrap();
    assert!(manager.landing_campus().unwrap().is_none());
}

#[test]
fn campus_plan_snapshot_is_one_complete_feature_result() {
    let (mut manager, campus_id) = manager_with_campus();
    manager.create_plan(&campus_id, "方案 1").unwrap();
    manager.remember_campus(&campus_id).unwrap();

    let snapshot = manager.campus_plan_snapshot().expect("完整校区方案结果");
    assert_eq!(snapshot.campuses.len(), 1);
    assert_eq!(snapshot.landing_campus.expect("着陆校区").name, "测试大学");
    assert_eq!(snapshot.plans.len(), 1);
}
