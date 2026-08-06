//! M5 契约测试：B7 通知中心与 F2 教程在桌面端的完整接线。
//!
//! - 通知中心（ADR-0021/0031）：公告栏页展示全部留底记录、错误记录带
//!   "导出故障资料"入口、铃铛未读角标随发布增减、点开即清零。
//! - 教程（ADR-0020）：跟练气泡首次出现、只教一次（关掉后不再出现）、
//!   "跳过全部引导"永不再出现、设置"重新查看教程"可逆（进度清零）。
//!
//! Slint 每进程只能初始化一次平台，故整个文件只有一个 `#[test]` 串行
//! 跑完两个场景（与 presentation_seams/ui_bindings 同一约定）。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{
    Notification, NotificationActionOutcome, NotificationCenter, OpaqueNotificationAction,
    PresenterRegistry,
};
use onboarding_tutorial::{OnboardingTutorial, TutorialStatus};
use shared_domain_types::CampusId;
use slint::Model;
use std::path::Path;
use std::sync::Arc;

/// 完成首设、建校区与两个方案，装配正式窗口；返回 (window, center, db_path)。
fn setup(db_path: &Path) -> (AppWindow, Arc<NotificationCenter>, CampusId, String, String) {
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let mut injector = ViewModelInjector::new(ShellDatabases::open(db_path).expect("正式连接组"))
        .expect("正式注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("契约校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_a = injector
        .projects_mut()
        .create_plan(&campus_id, "契约方案 A")
        .expect("创建方案 A")
        .to_string();
    let plan_b = injector
        .projects_mut()
        .create_plan(&campus_id, "契约方案 B")
        .expect("创建方案 B")
        .to_string();
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));
    (window, center, campus_id, plan_a, plan_b)
}

fn open_plan(window: &AppWindow, plan_id: &str) {
    window.invoke_plan_list_card_clicked(plan_id.into());
    assert_eq!(window.get_active_screen(), 4, "打开方案进入工作区");
}

fn back_to_plan_list(window: &AppWindow, campus_id: &CampusId) {
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 1, "切校区按钮回到校区选择");
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    assert_eq!(window.get_active_screen(), 2, "选中校区回到方案列表");
}

#[test]
fn m5_notification_center_and_tutorial_contract() {
    let directory = tempfile::tempdir().expect("临时目录");
    let db_path = directory.path().join("contract.db");
    let (window, center, _campus, _plan_a, _plan_b) = setup(&db_path);

    // ── 场景 1：通知中心（ADR-0021/0031）────────────────────────────
    // 要紧错误带不透明故障资料操作（ADR-0031：错误记录提供导出入口）
    center.publish_with_action(
        Notification::error("方案 1", "导出失败", "磁盘写入被拒绝"),
        OpaqueNotificationAction::new(|| {
            NotificationActionOutcome::succeeded(Notification::info(
                "应用",
                "故障资料已导出",
                "feature-owned-payload",
            ))
        }),
    );
    // 普通提示（warn 级触发 toast 浮层）
    center.publish(Notification::warn("应用", "设置已保存", "语言切换为中文"));

    assert_eq!(center.unread_count(), 2, "发布后未读数应为 2");
    assert_eq!(window.get_notice_unread_count(), 2, "铃铛角标应显示未读数");

    // 打开通知中心页
    window.invoke_notice_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 5, "进入通知中心页");
    assert_eq!(window.get_notice_board_model().row_count(), 2);

    let mut error_row = None;
    for index in 0..window.get_notice_board_model().row_count() {
        let row = window
            .get_notice_board_model()
            .row_data(index)
            .expect("行数据存在");
        if row.title.as_str() == "导出失败" {
            assert_eq!(row.importance.as_str(), "high", "要紧错误标为 high");
            assert!(row.has_diagnostic_action, "错误记录带故障资料入口");
            error_row = Some(row);
        }
    }
    assert!(error_row.is_some(), "错误记录必须出现在公告栏");

    assert_eq!(
        center.unread_count(),
        0,
        "点开公告栏后未读数清零（ADR-0021）"
    );
    assert_eq!(window.get_notice_unread_count(), 0, "角标同步清零");

    // toast 浮层由 warn 级发布点亮（Timer 自动消失）
    assert!(window.get_toast_visible(), "warn 级发布点亮 toast");
    assert_eq!(window.get_toast_title().as_str(), "设置已保存");

    // ── 场景 2：教程（ADR-0020）─────────────────────────────────────
    let campus_id = _campus;
    let plan_a = _plan_a;
    let plan_b = _plan_b;
    // 从通知中心回方案列表
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 1, "切校区按钮回到校区选择");
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    assert_eq!(window.get_active_screen(), 2, "选中校区回到方案列表");

    // 首次打开方案：步骤条跟练气泡出现（文案走 zh-CN）
    open_plan(&window, &plan_a);
    assert!(
        window.get_workspace_tutorial_visible(),
        "首次进入工作区显示步骤条气泡"
    );
    let bubble = window.get_workspace_tutorial_text();
    assert!(
        bubble.as_str().contains("五格"),
        "气泡文案必须来自 zh-CN 文本键：{bubble}"
    );
    assert!(!window.get_workspace_tutorial_skip_all_label().is_empty());

    // 规矩③只教一次：关掉后重进同一方案不再出现
    window.invoke_workspace_tutorial_dismiss_clicked();
    assert!(!window.get_workspace_tutorial_visible());
    back_to_plan_list(&window, &campus_id);
    open_plan(&window, &plan_a);
    assert!(
        !window.get_workspace_tutorial_visible(),
        "已看过的引导点不再出现（只教一次）"
    );

    // 规矩②跳过全部：永不再出现（第二个方案也安静）
    window.invoke_workspace_tutorial_skip_all_clicked();
    back_to_plan_list(&window, &campus_id);
    open_plan(&window, &plan_b);
    assert!(
        !window.get_workspace_tutorial_visible(),
        "跳过全部引导后所有气泡永不再出现"
    );
    let reloaded =
        OnboardingTutorial::load(&data_persistence::Database::open(&db_path).expect("重开引导库"))
            .expect("重新装载引导进度");
    assert_eq!(
        reloaded.status(),
        TutorialStatus::Completed,
        "跳过全部必须持久化为应用级状态"
    );

    // 规矩④重看：设置页“重新查看教程”清零进度，气泡恢复
    back_to_plan_list(&window, &campus_id);
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "进入设置页");
    window.invoke_replay_tutorial_clicked();
    let reloaded =
        OnboardingTutorial::load(&data_persistence::Database::open(&db_path).expect("重开引导库"))
            .expect("重新装载引导进度");
    assert_eq!(
        reloaded.status(),
        TutorialStatus::NotStarted,
        "重看教程必须清零 F2 进度"
    );

    back_to_plan_list(&window, &campus_id);
    open_plan(&window, &plan_b);
    assert!(
        window.get_workspace_tutorial_visible(),
        "重看教程后气泡恢复出现"
    );
}
