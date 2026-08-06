//! M1 S1 接缝验收：边界确认后一次开始意图直达 F9 完整导出入口。

use data_persistence::CampusCrudApi;
use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::{ComponentHandle, Model};

fn pump_until_succeeded(window: &AppWindow, deadline: Duration) {
    let deadline_at = Instant::now() + deadline;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if window.get_operation_state() == OperationPresentationState::Succeeded
                || Instant::now() >= deadline_at
            {
                slint::quit_event_loop().expect("停止导出验收事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行导出验收事件循环");
}

#[test]
fn confirmed_boundary_unlocks_direct_export_without_orientation_or_collection() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("m1-s1.db");
    let export_dir = directory.path().join("exports");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接组"))
            .expect("正式注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("临时路径有效"))
        .expect("设置临时导出目录");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("M1 校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "M1 边界直出")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    // 直接送入已确认的地图边界；不进入朝向、采集或评审页面。
    let raw_confirm = r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#;
    window.invoke_workspace_map_ipc(raw_confirm.into());
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(window.get_workspace_step_locked().row_data(4), Some(false));

    // 进入导出步骤只呈现确认；一次按钮意图由 F9 完整入口执行整条链。
    window.invoke_workspace_step_clicked(4);
    assert_eq!(window.get_workspace_active_step(), 4);
    assert_eq!(
        window.get_workspace_placeholder_title().as_str(),
        l10n.t("export.confirm_title")
    );
    window.invoke_workspace_export_start_clicked();

    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing,
        "Start 必须先返回处理中，不能在 Slint 回调线程同步完成导出"
    );
    pump_until_succeeded(&window, Duration::from_secs(30));
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    assert_eq!(
        window.get_workspace_placeholder_title().as_str(),
        l10n.t("export.done")
    );
    assert!(export_dir.join(format!("{plan_id}.schem")).is_file());
    assert!(export_dir
        .join(format!("{plan_id}.foundation_manifest.json"))
        .is_file());
}
