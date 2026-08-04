//! M1 regression: a confirmed MultiPolygon must reach F9 without reconstruction.

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
                slint::quit_event_loop().expect("stop multipolygon export loop");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run multipolygon export loop");
}

fn displayed_dimensions(subtitle: &str) -> [usize; 3] {
    let dimensions = subtitle
        .split_once("尺寸 ")
        .and_then(|(_, value)| value.split_once('）'))
        .map(|(value, _)| value)
        .expect("success subtitle must expose F9 dimensions");
    let values: Vec<_> = dimensions
        .split('×')
        .map(|value| value.parse::<usize>().expect("dimension must be numeric"))
        .collect();
    values
        .try_into()
        .expect("success subtitle must expose three dimensions")
}

#[test]
fn desktop_multipolygon_confirmation_reaches_f9_and_publishes_pair() {
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("m1-multipolygon.db");
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
        .create_campus("multipolygon campus")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "multipolygon plan")
        .expect("create plan");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","geometry":{"type":"MultiPolygon","coordinates":[[[[116.4000,39.9000],[116.4010,39.9000],[116.4010,39.9010],[116.4000,39.9010],[116.4000,39.9000]]],[[[116.4050,39.9050],[116.4060,39.9050],[116.4060,39.9060],[116.4050,39.9060],[116.4050,39.9050]]]]}}"#
            .into(),
    );
    assert!(
        window.get_workspace_boundary_is_determined(),
        "desktop must preserve the confirmed MultiPolygon contract"
    );
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until_terminal(&window);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    let schematic_path = export_dir.join(format!("{plan_id}.schem"));
    assert!(schematic_path.is_file());
    assert!(
        std::fs::metadata(&schematic_path)
            .expect("schematic metadata")
            .len()
            > 0
    );
    assert!(export_dir
        .join(format!("{plan_id}.foundation_manifest.json"))
        .is_file());
    let dimensions = displayed_dimensions(&window.get_workspace_placeholder_subtitle());
    assert!(dimensions[0] > 300 && dimensions[2] > 300);
}
