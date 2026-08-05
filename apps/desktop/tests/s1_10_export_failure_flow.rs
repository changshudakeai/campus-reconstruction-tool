//! M1 desktop failure acceptance: every F9 failure remains visible and leaves no fake pair.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::{ExportFileKind, ExportFileSystem, StdExportFileSystem};
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
    BackgroundPanicOnce,
    BlockManifest,
    ManifestPublish,
    ManifestPublishAndRestore,
    BackupCleanup,
}

#[derive(Clone)]
struct FailingFileSystem {
    mode: Arc<Mutex<FailureMode>>,
    standard: Arc<StdExportFileSystem>,
    panic_once: Arc<AtomicBool>,
    publish_failed: Arc<AtomicBool>,
    manifest_gate: Arc<(Mutex<bool>, Condvar)>,
}

impl FailingFileSystem {
    fn new() -> (Self, Arc<Mutex<FailureMode>>) {
        let mode = Arc::new(Mutex::new(FailureMode::None));
        (
            Self {
                mode: Arc::clone(&mode),
                standard: Arc::new(StdExportFileSystem),
                panic_once: Arc::new(AtomicBool::new(false)),
                publish_failed: Arc::new(AtomicBool::new(false)),
                manifest_gate: Arc::new((Mutex::new(false), Condvar::new())),
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

    fn wait_for_manifest_block(&self) {
        let (lock, signal) = &*self.manifest_gate;
        let mut started = lock.lock().expect("manifest block lock");
        while !*started {
            started = signal.wait(started).expect("manifest block wait");
        }
    }

    fn release_manifest_block(&self) {
        let (lock, signal) = &*self.manifest_gate;
        *lock.lock().expect("manifest block release lock") = true;
        signal.notify_one();
    }

    fn reset_manifest_block(&self) {
        let (lock, _) = &*self.manifest_gate;
        *lock.lock().expect("manifest block reset lock") = false;
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
            FailureMode::BackgroundPanicOnce
                if Self::is_staging(path, ".m1-manifest-")
                    && !self.panic_once.swap(true, Ordering::SeqCst) =>
            {
                panic!("injected one-shot F9 background panic")
            }
            FailureMode::BlockManifest if Self::is_staging(path, ".m1-manifest-") => {
                let (lock, signal) = &*self.manifest_gate;
                *lock.lock().expect("manifest block lock") = true;
                signal.notify_one();
                let mut released = lock.lock().expect("manifest block release lock");
                while !*released {
                    released = signal.wait(released).expect("manifest block wait");
                }
                self.standard.write(path, contents)
            }
            _ => self.standard.write(path, contents),
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mode = self.mode_for();
        let is_final_manifest = to
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".json") && !name.contains(".m1-"));
        let should_fail_publish = is_final_manifest
            && match mode {
                FailureMode::ManifestPublish => !self.publish_failed.swap(true, Ordering::SeqCst),
                FailureMode::ManifestPublishAndRestore => true,
                _ => false,
            };
        if is_final_manifest && should_fail_publish {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected manifest publication failure",
            ));
        }
        self.standard.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        if matches!(self.mode_for(), FailureMode::BackupCleanup)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("backup-"))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected backup cleanup failure",
            ));
        }
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

    fn start_and_wait_for_failure(&self, deadline: Duration, label: &str) {
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
            "{label}: a failed F9 operation must never be presented as success: title={} subtitle={}",
            self.window.get_workspace_placeholder_title(),
            self.window.get_workspace_placeholder_subtitle()
        );
        assert!(self.window.get_error_dialog_visible());
    }

    fn start_and_wait_for_success(&self, deadline: Duration) {
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
                    slint::quit_event_loop().expect("stop success export loop");
                }
            },
        );
        slint::run_event_loop_until_quit().expect("run success export loop");
        assert_eq!(
            self.window.get_operation_state(),
            OperationPresentationState::Succeeded,
            "successful F9 operation must be presented as success"
        );
    }

    fn wait_for_terminal(&self, deadline: Duration) {
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
                    slint::quit_event_loop().expect("stop terminal export loop");
                }
            },
        );
        slint::run_event_loop_until_quit().expect("run terminal export loop");
        assert_ne!(
            self.window.get_operation_state(),
            OperationPresentationState::Processing,
            "F9 operation must reach a terminal presentation state"
        );
        assert_eq!(
            self.window.get_operation_state(),
            OperationPresentationState::Succeeded,
            "duplicate Start must leave the original operation observable to success"
        );
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
    let file_system = Arc::new(file_system);
    let app = TestApp::new(
        Arc::clone(&file_system) as Arc<dyn ExportFileSystem>,
        Arc::clone(&mode),
    );

    // No boundary: Start fails synchronously at the F9 input boundary.
    app.set_mode(FailureMode::None);
    app.window.invoke_workspace_step_clicked(4);
    app.start_and_wait_for_failure(Duration::from_secs(5), "missing boundary");
    app.assert_localized_failure("error.export_boundary_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B18 generation failure: dimensions exceed the production i32 contract.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[0.0,39.9],[1000000000.0,39.9],[1000000000.0,39.91],[0.0,39.91]]}"#,
    );
    app.set_mode(FailureMode::None);
    app.start_and_wait_for_failure(Duration::from_secs(5), "generation");
    app.assert_localized_failure("error.export_generation_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B17 manifest staging failure: the pair is not published.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    app.set_mode(FailureMode::ManifestStaging);
    app.start_and_wait_for_failure(Duration::from_secs(5), "manifest staging");
    app.assert_localized_failure("error.export_manifest_write_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // B4 schematic staging failure: a staged manifest is cleaned up as well.
    app.set_mode(FailureMode::SchematicStaging);
    app.start_and_wait_for_failure(Duration::from_secs(5), "schematic staging");
    app.assert_localized_failure("error.export_schematic_write_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // Worker panic: a disconnected operation is a localized background failure.
    app.set_mode(FailureMode::BackgroundPanicOnce);
    app.start_and_wait_for_failure(Duration::from_secs(5), "worker panic");
    app.assert_localized_failure("error.export_background_failed");
    app.assert_no_artifact_pair();
    app.dismiss_error();

    // The worker guard clears active state, so the same confirmed boundary can retry.
    app.set_mode(FailureMode::None);
    app.start_and_wait_for_success(Duration::from_secs(5));
    assert!(app.schematic_path().is_file());
    assert!(app.manifest_path().is_file());

    // A publish failure restores the old pair and remains an ordinary artifact failure.
    let old_schematic = b"old schematic";
    let old_manifest = b"old manifest";
    std::fs::write(app.schematic_path(), old_schematic).unwrap();
    std::fs::write(app.manifest_path(), old_manifest).unwrap();
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    app.set_mode(FailureMode::ManifestPublish);
    app.start_and_wait_for_failure(Duration::from_secs(5), "publish");
    app.assert_localized_failure("error.export_artifact_write_failed");
    assert_eq!(std::fs::read(app.schematic_path()).unwrap(), old_schematic);
    assert_eq!(std::fs::read(app.manifest_path()).unwrap(), old_manifest);
    app.dismiss_error();

    // A restore failure is promoted to ArtifactRecovery and is never shown as a safe rollback.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    app.set_mode(FailureMode::ManifestPublishAndRestore);
    app.start_and_wait_for_failure(Duration::from_secs(5), "recovery");
    app.assert_localized_failure("error.export_recovery_failed");
    app.dismiss_error();

    // A cleanup failure keeps the newly published pair successful and exposes a warning fact.
    std::fs::write(app.schematic_path(), old_schematic).unwrap();
    std::fs::write(app.manifest_path(), old_manifest).unwrap();
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    app.set_mode(FailureMode::BackupCleanup);
    app.start_and_wait_for_success(Duration::from_secs(5));
    assert_ne!(std::fs::read(app.schematic_path()).unwrap(), old_schematic);
    assert_ne!(std::fs::read(app.manifest_path()).unwrap(), old_manifest);
    assert!(app
        .center
        .board_records()
        .into_iter()
        .any(|record| record.notification().body == app.l10n.t("export.cleanup_warning")));

    // Leaving the workspace expires the presentation result while the F9 worker
    // finishes in its plan-specific output directory; it must not repaint the
    // campus page or a later reopening of the plan.
    app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    file_system.reset_manifest_block();
    app.set_mode(FailureMode::BlockManifest);
    app.window.invoke_workspace_export_start_clicked();
    file_system.wait_for_manifest_block();
    app.window.invoke_switch_campus_toolbar_button_clicked();
    assert_ne!(
        app.window.get_active_screen(),
        4,
        "leaving the workspace must stop presenting the old export page"
    );
    assert!(!app.window.get_error_dialog_visible());
    file_system.release_manifest_block();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.manifest_path().is_file() {
        assert!(Instant::now() < deadline, "abandoned worker did not finish");
        std::thread::yield_now();
    }
    assert_ne!(app.window.get_active_screen(), 4);

    app.window
        .invoke_plan_list_card_clicked(app.plan_id.to_string().into());
    assert_eq!(app.window.get_active_screen(), 4);
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Ready,
        "the old result must not appear as success after reopening the plan"
    );

    // Drop the first app before exercising duplicate Start in a fresh output
    // directory; the terminal state is observed through the operation poll.
    drop(app);
    let duplicate_app = TestApp::new(
        Arc::clone(&file_system) as Arc<dyn ExportFileSystem>,
        Arc::clone(&mode),
    );
    duplicate_app.confirm_boundary(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#,
    );
    file_system.reset_manifest_block();
    duplicate_app.set_mode(FailureMode::BlockManifest);
    duplicate_app.window.invoke_workspace_export_start_clicked();
    file_system.wait_for_manifest_block();
    duplicate_app.window.invoke_workspace_export_start_clicked();
    assert!(duplicate_app.window.get_error_dialog_visible());
    duplicate_app.assert_localized_failure("error.export_failed");
    duplicate_app.dismiss_error();
    file_system.release_manifest_block();
    duplicate_app.wait_for_terminal(Duration::from_secs(5));
    assert!(duplicate_app.schematic_path().is_file());
    assert!(duplicate_app.manifest_path().is_file());
}
