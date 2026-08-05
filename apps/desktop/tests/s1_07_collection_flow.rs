//! S1-07 验收：采集入口在 F4 执行前呈现六类对象的待定状态。

use std::sync::Arc;

use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;

#[test]
fn collection_page_starts_with_six_pending_categories() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-07.db");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("连接数据库"))
            .expect("创建注入器");
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
        .create_campus("验收校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    window.invoke_workspace_step_clicked(1);
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_orientation","points":[[116.3975,39.9160],[116.3985,39.9160]]}"#.into(),
    );

    window.invoke_workspace_step_clicked(2);

    assert_eq!(window.get_workspace_active_step(), 2);
    assert_eq!(window.get_collection_category_labels().row_count(), 6);
    assert_eq!(window.get_collection_category_statuses().row_count(), 6);
    for index in 0..6 {
        assert_eq!(
            window.get_collection_category_statuses().row_data(index),
            Some(l10n.t("common.pending").into()),
            "第 {index} 类对象初始状态必须为待定"
        );
    }
    assert_eq!(
        window.get_collection_category_skip_label().as_str(),
        l10n.t("collection.skippable")
    );
    assert_eq!(
        window.get_collection_report_entry_label().as_str(),
        l10n.t("audit.report_entry")
    );

    // 点击采集必须立刻把页面切换到处理中；网络结果仍未回传时窗口保持可用。
    window.invoke_collection_start_clicked();
    assert_eq!(
        window.get_collection_progress_label().as_str(),
        l10n.t("collection.progress_fetching"),
        "耗时采集启动后先呈现处理中状态，不能冻结窗口"
    );

    // 空结果会产生六个空类别疑点，但 B7 只能合并呈现为一扇错误模态窗。
    window.invoke_workspace_map_ipc(
        r#"{"type":"collection_response","request_id":1,"payload":"{\"status\":\"1\",\"info\":\"OK\",\"pois\":[]}"}"#
            .into(),
    );
    assert!(
        window.get_error_dialog_visible(),
        "有疑点必须经 B7 弹窗呈现"
    );
    assert_eq!(
        window.get_error_dialog_body().lines().count(),
        6,
        "所有覆盖率疑点必须合并到同一扇窗口"
    );
    assert!(
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0'),
        "采集完成后应呈现对象总数"
    );
    assert_eq!(window.get_workspace_completed_steps(), 3);
    assert!(!window.get_workspace_step_locked().row_data(3).unwrap());
}
