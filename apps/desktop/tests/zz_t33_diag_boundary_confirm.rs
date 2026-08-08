//! T33 诊断（临时）：验证 WebView confirm_boundary IPC → Rust 边界确认路径。
use std::sync::Arc;

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;

#[test]
fn webview_confirm_boundary_ipc_confirms_boundary() {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("t33-diag.db");
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
        .database()
        .create_campus("诊断校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "诊断方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    assert_eq!(window.get_workspace_active_step(), 0);
    assert!(!window.get_workspace_boundary_is_determined());

    // 模拟 WebView “确认边界”按钮发送的 confirm_boundary IPC（GCJ-02 四点）。
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[121.4,31.2],[121.5,31.2],[121.5,31.3],[121.4,31.3]]}"#
            .into(),
    );

    assert!(
        window.get_workspace_boundary_is_determined(),
        "confirm_boundary IPC 后边界必须已确定"
    );
    assert_eq!(window.get_workspace_active_step(), 0);
    let locked: Vec<bool> = (0..5)
        .map(|i| window.get_workspace_step_locked().row_data(i).unwrap())
        .collect();
    assert!(
        locked.iter().all(|locked| !*locked),
        "确认边界后五步必须全部解锁：step_locked={locked:?} status={}",
        window.get_workspace_boundary_status()
    );
}
