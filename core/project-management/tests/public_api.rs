//! F3 公开 API 快照测试（执法清单 2.5）
//!
//! 快照测试保证任何公开类型的增删显形于 PR diff；行为断言补充在
//! `public_api_types_exist`（用 B2 内存库验证关键行为可调用）。

use localization::Language;
use localization::Localization;

use data_persistence::{CampusCrudApi, Database};
use project_management::{
    CampusPlanSnapshot, CampusView, Error, PlanCardView, PlanContextView, PlanProgress,
    ProjectManager, RestoredPlan, TrashItemView, DUPLICATE_SUFFIX_KEY, RESTORE_NAME_TEMPLATE_KEY,
};
use shared_domain_types::{CampusId, PlanId};

#[test]
fn public_api_snapshot() {
    let rustdoc_json = rustdoc_json::Builder::default()
        .toolchain(public_api::MINIMUM_NIGHTLY_RUST_VERSION)
        .build()
        .unwrap();
    let api = public_api::Builder::from_rustdoc_json(rustdoc_json)
        .build()
        .unwrap();
    api.assert_eq_or_update("tests/snapshots/public-api.txt");
}

#[test]
fn public_api_types_exist() {
    // 断言项目自述的本地化文本键在 zh-CN 中可解析（ADR-0005）。
    let l10n = Localization::new(Language::ZhCn).expect("zh-CN.json 可加载");
    for key in [DUPLICATE_SUFFIX_KEY, RESTORE_NAME_TEMPLATE_KEY] {
        assert!(!l10n.t(key).is_empty(), "文本键 {key} 必须可解析");
    }

    // 常量：副本后缀与恢复名称模板文本键（文案外置，ADR-0005）
    assert_eq!(DUPLICATE_SUFFIX_KEY, "plan.duplicate_suffix");
    assert_eq!(RESTORE_NAME_TEMPLATE_KEY, "plan.restore_name_template");

    // ProjectManager：用 B2 内存库创建
    let db = Database::open_in_memory().expect("内存库可打开");
    let mut manager = ProjectManager::new(db);
    assert!(format!("{manager:?}").contains("ProjectManager"));

    // 校区建立经 B2 原语（T30：F3 不再提供 create_campus 业务入口）
    let campus: CampusView = manager
        .database()
        .create_campus("测试大学")
        .map(|campus| CampusView {
            id: campus.id,
            name: campus.name,
            address: campus.address,
            anchor_lng: campus.anchor_lng,
            anchor_lat: campus.anchor_lat,
        })
        .unwrap();
    assert_eq!(campus.name, "测试大学");
    assert_eq!(manager.list_campuses().unwrap().len(), 1);

    // 着陆流程（ADR-0006）：记住/读取上次使用的校区
    let campus_id = CampusId::parse(&campus.id).unwrap();
    assert!(manager.landing_campus().unwrap().is_none());
    manager.remember_campus(&campus_id).unwrap();
    assert_eq!(
        manager.landing_campus().unwrap().map(|c| c.id),
        Some(campus.id.clone())
    );

    // 方案轻创建（ADR-0010）与卡片三件套（ADR-0018）
    let plan_id: PlanId = manager.create_plan(&campus_id, "方案 1").unwrap();
    // 工作区上下文（S1-05）：方案名/校区名/锚点一次返回；不存在的方案返回 None
    let context: PlanContextView = manager.plan_context(&plan_id).unwrap().unwrap();
    assert_eq!(context.plan_id, plan_id.to_string());
    assert_eq!(context.plan_name, "方案 1");
    assert_eq!(context.campus_id, campus_id.to_string());
    assert_eq!(context.campus_name, "测试大学");
    assert!(context.anchor_lng.is_finite() && context.anchor_lat.is_finite());
    let missing = PlanId::parse("00000000-0000-4000-8000-000000000000").unwrap();
    assert!(manager.plan_context(&missing).unwrap().is_none());
    assert_eq!(
        manager.suggest_plan_name(&campus_id, "新方案").unwrap(),
        "新方案 1"
    );
    let cards: Vec<PlanCardView> = manager.list_plan_cards(&campus_id).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].name, "方案 1");
    assert_eq!(cards[0].progress, PlanProgress::BoundaryNotSet);
    assert_eq!(cards[0].progress.text_key(), "plan.boundary_not_set");
    assert!(!cards[0].last_modified_at.is_empty());
    let snapshot: CampusPlanSnapshot = manager.campus_plan_snapshot().unwrap();
    assert_eq!(snapshot.campuses.len(), 1);
    assert_eq!(snapshot.plans.len(), 1);

    // 改名 + 复制 + 删除进回收站
    manager.rename_plan(&plan_id, "方案 1 - 全景复刻").unwrap();
    let copy_id = manager.duplicate_plan(&plan_id, "副本").unwrap();
    assert_ne!(copy_id, plan_id);
    let trash: TrashItemView = manager.delete_plan(&campus_id, &copy_id).unwrap();
    assert_eq!(trash.plan_id, copy_id.to_string());

    // Error #[non_exhaustive]：带类型错误可匹配，同名冲突可判别
    let err: Error = manager
        .create_plan(&campus_id, "方案 1 - 全景复刻")
        .unwrap_err();
    assert!(err.is_duplicate_name());
    assert!(!err.to_string().is_empty());

    // 回收站查询 / 恢复 / 到期清理框架
    assert_eq!(manager.list_trash(&campus_id).unwrap().len(), 1);
    manager
        .restore_plan(&campus_id, &trash.trash_id, "（恢复 {n}）")
        .unwrap();
    assert_eq!(manager.purge_expired_trash().unwrap(), 0);

    // 回收站视图带方案名/校区名/剩余天数（ADR-0018）
    let trash2: TrashItemView = manager.delete_plan(&campus_id, &copy_id).unwrap();
    let items: Vec<TrashItemView> = manager.list_trash(&campus_id).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, trash2.name);
    assert!((0..=30).contains(&items[0].expires_in_days));
    // 恢复自动后缀（ADR-0018 §五）
    let restored: RestoredPlan = manager
        .restore_plan(&campus_id, &trash2.trash_id, "（恢复 {n}）")
        .unwrap();
    assert!(!restored.name.is_empty());

    // 清空回收站（确认后调用）：一次性清当前校区，不影响其他校区
    let other = manager.database().create_campus("另一所大学").unwrap();
    let other_id = CampusId::parse(&other.id).unwrap();
    let other_plan = manager.create_plan(&other_id, "他人方案").unwrap();
    let trash3 = manager.delete_plan(&campus_id, &plan_id).unwrap();
    let trash4 = manager.delete_plan(&campus_id, &copy_id).unwrap();
    manager.delete_plan(&other_id, &other_plan).unwrap();
    assert_eq!(manager.purge_all_trash_confirmed(&campus_id).unwrap(), 2);
    assert!(manager.list_trash(&campus_id).unwrap().is_empty());
    assert_eq!(manager.list_trash(&other_id).unwrap().len(), 1);
    assert!(manager.purge_plan_confirmed(&trash3.trash_id).is_err());
    assert!(manager.purge_plan_confirmed(&trash4.trash_id).is_err());
}
