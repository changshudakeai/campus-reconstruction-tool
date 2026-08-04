//! M1 desktop failure acceptance: every F9 failure remains visible and leaves no fake pair.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_console::{ExportFileKind, ExportFileSystem, StdExportFileSystem};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, PlanId};
use slint::ComponentHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureMode {
    None,
    ManifestStaging,
    SchematicStaging,
    BackgroundPanic,
}

#[derive(Clone)]
struct FailingFileSystem {
    mode: Arc<Mutex<FailureMode>>,
    standard: Arc<StdExportFileSystem>,
}

impl FailingFileSystem {
    fn new() -> (Self, Arc<Mutex<FailureMode>>) {
        let mode = Arc::new(Mutex::new(FailureMode::None));
        (
            Self {
                mode: Arc::clone(&mode),
                standard: Arc::new(StdExportFileSystem),
            },
            mode,
        )
    }

    fn mode_for(&self) -> FailureMode {
        *self.mode.lock().expect("failure mode lock")
    }

    fn is_staging(path: &Path, suffix: &str) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(suffix))
    }
}

impl ExportFileSystem for FailingFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.standard.create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        match self.mode_for() {
            FailureMode::ManifestStaging if Self::is_staging(path, ".m1-manifest-") => {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected manifest staging failure",
                ))
            }
            FailureMode::SchematicStaging if Self::is_staging(path, ".m1-schem-") => {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected schematic staging failure",
                ))
            }
            FailureMode::BackgroundPanic if Self::is_staging(path, ".m1-manifest-") => {
                panic!("injected F9 background panic")
            }
            _ => self.standard.write(path, contents),
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.standard.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.standard.remove_file(path)
    }

    fn kind(&self, path: &Path) -> io::Result<Option<ExportFileKind>> {
        self.standard.kind(path)
    }
}

struct TestApp {
    _directory: tempfile::TempDir,
    window: AppWindow,
    center: Arc<NotificationCenter>,
    _runtime: ApplicationRuntime,
    l10n: Localization,
    plan_id: PlanId,
    export_dir: PathBuf,
    mode: Arc<Mutex<FailureMode>>,
}

impl TestApp {
    fn new(file_system: Arc<dyn ExportFileSystem>, mode: Arc<Mutex<FailureMode>>) -> Self {
        let l10n = Localization::new(Language::ZhCn).expect("load zh-CN resources");
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("m1-failure.db");
        let export_dir = directory.path().join("exports");
        let mut injector = ViewModelInjector::new_with_export_file_system(
            ShellDatabases::open(&database_path).expect("open databases"),
            file_system,
        )
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
            .create_campus("M1 failure campus")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "M1 failure plan")
            .expect("create plan");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let runtime = assemble_application(&window, injector, Arc::clone(&center));

        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        assert_eq!(window.get_active_screen(), 4);

        Self {
            _directory: directory,
            window,
            center,
            _runtime: runtime,
            l10n,
            plan_id,
            export_dir,
            mode,
        }
    }

    fn set_mode(&self, mode: FailureMode) {
        *self.mode.lock().expect("failure mode lock") = mode;
    }

    fn schematic_path(&self) -> PathBuf {
        self.export_dir.join(format!("{}.schem", self.plan_id))
    }

    fn manifest_path(&self) -> PathBuf {
        self.export_dir
            .join(format!("{}.foundation_manifest.json", self.plan_id))
    }

    fn assert_no_artifact_pair(&self) {
        assert!(
            !self.schematic_path().exists(),
            "no schematic may be published"
        );
        assert!(
            !self.manifest_path().exists(),
            "no manifest may be published"
        );
    }

    fn dismiss_error(&self) {
        if self.window.get_error_dialog_visible() {
            self.window.invoke_error_dialog_dismissed();
        }
    }

    fn confirm_boundary(&self, coords: &str) {
        self.window.invoke_workspace_boundary_reset_clicked();
        self.window.invoke_workspace_map_ipc(coords.into());
        assert!(self.window.get_workspace_boundary_is_determined());
        self.window.invoke_workspace_step_clicked(4);
    }

    fn start_and_wait_for_failure(&self, deadline: Duration) {
        self.window.invoke_workspace_export_start_clicked();
        let deadline_at = Instant::now() + deadline;
        let weak = self.window.as_weak();
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
                    slint::quit_event_loop().expect("stop failure export loop");
                }
            },
        );
        slint::run_event_loop_until_quit().expect("run failure export loop");
        assert_eq!(
            self.window.get_operation_state(),
            OperationPresentationState::Failed,
            "a failed F9 operation must never be presented as success"
        );
        assert!(self.window.get_error_dialog_visible());
    }

    fn assert_localized_failure(&self, category_key: &str) {
        let category = self.l10n.t(category_key);
        let expected_body = self
            .l10n
            .t_with_array("export.failure_user_message", &[&category]);
        assert_eq!(self.window.get_error_dialog_body().as_str(), expected_body);
        let record = self
            .center
            .board_records()
            .into_iter()
            .find(|record| record.notification().body == expected_body)
            .expect("F9 failure must be published to the notification board");
        assert!(
            record.has_diagnostic_action(),
            "F9 failure must expose raw detail through the diagnostic seam"
        );
    }
}

#[test]
fn desktop_export_failures_are_localized_async_and_pair_safe() {
    let (file_system, mode) = FailingFileSystem::new();
    let app = TestApp::new(Arc::new(file_system), Arc::clone(&mode));

    // No boundary: Start fails synchronously at the F9 input boundary.
    app.set_mode(FailureMode::None);
    app.window.invoke_workspace_step_clicked(4);
    app.start_and_wait_for_failure(Duration::from_secs(5));
    app.assert_localized_failure("error.export_boundary_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B18 generation failure: dimensions exceed the production i32 contract.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[0.0,39.9],[1000000000.0,39.9],[1000000000.0,39.91],[0.0,39.91]]}"#,
    );
    app.set_mode(FailureMode::None);
    app.start_and_wait_for_failure(Duration::from_secs(5));
    app.assert_localized_failure("error.export_generation_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B17 manifest staging failure: the pair is not published.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    app.set_mode(FailureMode::ManifestStaging);
    app.start_and_wait_for_failure(Duration::from_secs(5));
    app.assert_localized_failure("error.export_manifest_write_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B4 schematic staging failure: a staged manifest is cleaned up as well.
    app.set_mode(FailureMode::SchematicStaging);
    app.start_and_wait_for_failure(Duration::from_secs(5));
    app.assert_localized_failure("error.export_schematic_write_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // Worker panic: a disconnected operation is a localized background failure.
    app.set_mode(FailureMode::BackgroundPanic);
    app.start_and_wait_for_failure(Duration::from_secs(5));
    app.assert_localized_failure("error.export_background_failed");
    app.assert_no_artifact_pair();
}
