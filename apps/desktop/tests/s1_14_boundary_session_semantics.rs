//! M1 acceptance: the confirmed boundary is a current-session promise only.
//!
//! ADR-0007 把边界归属定义为方案级数据，但 M1 尚未落地方案边界持久化
//! （plans 表无边界字段）。本测试证明：应用重启后已确认边界不会静默恢复，
//! 也不会伪造边界——导出入口明确拒绝（error.export_boundary_failed）。

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
use slint::ComponentHandle;

/// 轮询直到导出到达终态（Succeeded/Failed），不再依赖固定短超时。
///
/// CI（windows-latest 冷缓存/负载）下导出可能超过 5 秒；这里以 30 秒作为宽松
/// 兜底。超时（仍为 Processing）时返回当前状态，由调用方在断言信息中输出，
/// 便于 CI 定位。
fn pump_until_terminal(window: &AppWindow) -> OperationPresentationState {
    let deadline = Instant::now() + Duration::from_secs(30);
    let weak = window.as_weak();
    let terminal = std::sync::Arc::new(std::sync::Mutex::new(
        OperationPresentationState::Processing,
    ));
    let terminal_flag = Arc::clone(&terminal);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(20),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let state = window.get_operation_state();
            if state != OperationPresentationState::Processing || Instant::now() >= deadline {
                *terminal_flag.lock().expect("terminal state lock") = state;
                slint::quit_event_loop().expect("stop session semantics export loop");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run session semantics export loop");
    let terminal_value = *terminal.lock().expect("terminal state lock");
    terminal_value
}

#[test]
fn confirmed_boundary_is_session_only_and_restart_refuses_without_it() {
    let l10n = Localization::new(Language::ZhCn).expect("load zh-CN resources");
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("m1-session.db");
    let export_dir = directory.path().join("exports");

    // ── 第一段会话：确认边界并导出成功 ──
    let plan_id = {
        let window = AppWindow::new().expect("create first AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));
        let mut injector =
            ViewModelInjector::new(ShellDatabases::open(&database_path).expect("open databases"))
                .expect("construct injector");
        injector
            .settings_mut()
            .complete_first_run(&FirstRunSetup {
                language: "zh-CN".into(),
                minecraft_version: "26.1.2".into(),
                acknowledged: true,
            })
            .expect("complete first run");
        injector
            .settings_mut()
            .set_default_export_location(export_dir.to_str().expect("temporary path"))
            .expect("set export directory");
        let campus = injector
            .projects_mut()
            .database()
            .create_campus("session semantics campus")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "session semantics plan")
            .expect("create plan");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
                .into(),
        );
        assert!(
            window.get_workspace_boundary_is_determined(),
            "第一段会话内确认边界必须立即可见"
        );
        window.invoke_workspace_step_clicked(4);
        window.invoke_workspace_export_start_clicked();
        assert_eq!(
            window.get_operation_state(),
            OperationPresentationState::Processing
        );
        let terminal = pump_until_terminal(&window);
        assert_eq!(
            terminal,
            OperationPresentationState::Succeeded,
            "第一段会话导出必须成功；等待超时或异常状态：{terminal:?}"
        );
        assert!(export_dir.join(format!("{plan_id}.schem")).is_file());
        plan_id.to_string()
    };

    // ── 第二段会话：同一数据库、全新注入器（export-flow 快照为空）──
    let export_dir_after_restart = directory.path().join("exports-after-restart");
    let window = AppWindow::new().expect("create second AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("reopen databases"))
            .expect("construct second injector");
    injector
        .settings_mut()
        .set_default_export_location(export_dir_after_restart.to_str().expect("temporary path"))
        .expect("set export directory after restart");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.clone().into());
    assert!(
        !window.get_workspace_boundary_is_determined(),
        "重启后不得静默恢复已确认边界，也不得伪造边界"
    );

    // 无边界时导出必须明确拒绝，而不是静默成功或伪造边界。
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    let terminal = pump_until_terminal(&window);
    assert_eq!(
        terminal,
        OperationPresentationState::Failed,
        "重启后没有已确认边界时导出必须明确失败；当前状态：{terminal:?}"
    );
    assert!(window.get_error_dialog_visible());
    let expected_body = l10n.t_with_array(
        "export.failure_user_message",
        &[&l10n.t("error.export_boundary_failed")],
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        expected_body,
        "缺失边界必须呈现为边界失败（不产生成功产物）"
    );
    assert!(
        !export_dir_after_restart
            .join(format!("{plan_id}.schem"))
            .is_file(),
        "拒绝导出时不得产生成功产物"
    );
}
