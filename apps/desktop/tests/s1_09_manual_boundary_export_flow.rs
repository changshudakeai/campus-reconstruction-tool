//! M1 regression: manual boundary confirmation must feed F9 with the latest boundary.

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

fn pump_until_terminal(window: &AppWindow, deadline: Duration) {
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
            if window.get_operation_state() != OperationPresentationState::Processing
                || Instant::now() >= deadline_at
            {
                slint::quit_event_loop().expect("stop manual export loop");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run manual export loop");
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
fn manual_canvas_boundary_exports_latest_confirmed_revision() {
    let l10n = Localization::new(Language::ZhCn).expect("load zh-CN resources");
    desktop_shell::set_webview_creation_probe(true);
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("m1-manual.db");
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
        .database()
        .create_campus("manual regression campus")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "manual regression plan")
        .expect("create plan");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.4000,39.9000],[116.4014,39.9000],[116.4014,39.9004],[116.4000,39.9004]]}"#.into(),
    );
    assert!(window.get_workspace_boundary_is_determined());
    window.invoke_workspace_step_clicked(4);
    assert_eq!(
        window.get_workspace_placeholder_title().as_str(),
        l10n.t("export.confirm_title")
    );
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing,
        "Start returns Processing before export completion"
    );
    pump_until_terminal(&window, Duration::from_secs(30));
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    let first_dimensions = displayed_dimensions(&window.get_workspace_placeholder_subtitle());

    let schematic_path = export_dir.join(format!("{plan_id}.schem"));
    let manifest_path = export_dir.join(format!("{plan_id}.foundation_manifest.json"));
    assert!(schematic_path.is_file());
    assert!(manifest_path.is_file());

    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.4000,39.9000],[116.4038,39.9000],[116.4038,39.9015],[116.4000,39.9015]]}"#.into(),
    );
    assert!(window.get_workspace_boundary_is_determined());
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until_terminal(&window, Duration::from_secs(30));
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    let corrected_dimensions = displayed_dimensions(&window.get_workspace_placeholder_subtitle());
    assert!(
        corrected_dimensions[0] > first_dimensions[0]
            && corrected_dimensions[2] > first_dimensions[2],
        "修正边界后的 F9 结果必须反映最新几何尺寸，而不是旧快照"
    );

    assert!(schematic_path.is_file());
    assert!(
        std::fs::metadata(&schematic_path)
            .expect("schematic metadata")
            .len()
            > 0,
        "成功尺寸必须来自实际写入的 .schem 产物"
    );
    assert!(manifest_path.is_file());
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).expect("manifest must be readable"),
    )
    .expect("manifest must be valid JSON");
    assert_eq!(manifest["minecraftVersion"], "26.1.2");
    assert_eq!(manifest["orientation"]["degree"], 0.0);
    assert_eq!(manifest["orientation"]["source"], "map_north");
    assert_eq!(manifest["candidateFacts"]["candidateProjectionCount"], 0);
}
