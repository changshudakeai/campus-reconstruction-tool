//! M1 regression: reopening a plan must select its latest F9-owned boundary.

use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn pump_until_terminal(window: &AppWindow) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if window.get_operation_state() != OperationPresentationState::Processing
                || Instant::now() >= deadline
            {
                slint::quit_event_loop().expect("stop plan switch export loop");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run plan switch export loop");
}

#[test]
fn switching_and_reopening_a_plan_exports_its_latest_confirmed_boundary() {
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("m1-plan-switch.db");
    let export_dir = directory.path().join("exports");
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
        .create_campus("plan switch campus")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_a = injector
        .projects_mut()
        .create_plan(&campus_id, "plan A")
        .expect("create plan A");
    let plan_b = injector
        .projects_mut()
        .create_plan(&campus_id, "plan B")
        .expect("create plan B");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_a.to_string().into());
    for (x, y) in [(0.0, 0.0), (320.0, 0.0), (320.0, 160.0), (0.0, 160.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    assert!(window.get_workspace_boundary_is_determined());

    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    window.invoke_plan_list_card_clicked(plan_b.to_string().into());
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    window.invoke_plan_list_card_clicked(plan_a.to_string().into());
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until_terminal(&window);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded,
        "重新打开方案 A 后，F9 必须获得 A 的最新确认边界"
    );
    assert!(export_dir.join(format!("{plan_a}.schem")).is_file());
    assert!(export_dir
        .join(format!("{plan_a}.foundation_manifest.json"))
        .is_file());
}
