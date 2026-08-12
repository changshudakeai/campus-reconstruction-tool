//! 工作区返回方案列表回归：空闲时五步均能返回当前校区方案列表；运行中操作
//! 复用离开确认，并在确认/取消两条路径保持 ADR-0042 §6 的过期语义。

use data_persistence::CampusCrudApi;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::{ExportFileKind, ExportFileSystem, StdExportFileSystem};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::{ComponentHandle, Model};

/// 后台导出被阻断在 manifest staging：测试可以在 worker 运行期间做离开操作。
#[derive(Clone)]
struct BlockingFileSystem {
    standard: Arc<StdExportFileSystem>,
    manifest_gate: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingFileSystem {
    fn new() -> Self {
        Self {
            standard: Arc::new(StdExportFileSystem),
            manifest_gate: Arc::new((Mutex::new(false), Condvar::new())),
        }
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
        *lock.lock().expect("manifest release lock") = true;
        signal.notify_one();
    }

    fn is_manifest_staging(path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".m1-manifest-"))
    }
}

impl ExportFileSystem for BlockingFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.standard.create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if Self::is_manifest_staging(path) {
            let (lock, signal) = &*self.manifest_gate;
            *lock.lock().expect("manifest block lock") = true;
            signal.notify_one();
            let mut released = lock.lock().expect("manifest release lock");
            while !*released {
                released = signal.wait(released).expect("manifest release wait");
            }
        }
        self.standard.write(path, contents)
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
    _runtime: ApplicationRuntime,
    plan_id: String,
    export_dir: PathBuf,
}

impl TestApp {
    fn new(file_system: Arc<BlockingFileSystem>) -> Self {
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("m1-leave-confirm.db");
        let export_dir = directory.path().join("exports");
        let mut injector = ViewModelInjector::new_with_export_file_system(
            ShellDatabases::open(&database_path).expect("open databases"),
            Arc::clone(&file_system) as Arc<dyn ExportFileSystem>,
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
            .database()
            .create_campus("leave confirm campus")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "leave confirm plan")
            .expect("create plan");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        Self {
            _directory: directory,
            window,
            _runtime,
            plan_id: plan_id.to_string(),
            export_dir,
        }
    }

    fn confirm_boundary(&self) {
        self.window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
                .into(),
        );
        assert!(self.window.get_workspace_boundary_is_determined());
    }

    fn schematic_path(&self) -> PathBuf {
        self.export_dir.join(format!("{}.schem", self.plan_id))
    }

    fn manifest_path(&self) -> PathBuf {
        self.export_dir
            .join(format!("{}.foundation_manifest.json", self.plan_id))
    }
}

fn assert_return_to_plan_list_works_from_all_five_steps_and_preserves_saved_plan_state() {
    for step in 0..5 {
        let file_system = Arc::new(BlockingFileSystem::new());
        let app = TestApp::new(file_system);

        app.confirm_boundary();
        app.window
            .set_workspace_orientation_input_text("135".into());
        app.window.invoke_workspace_orientation_submit_clicked();
        assert!(app.window.get_workspace_orientation_is_determined());

        // 五步共享同一个标题区入口；这里注入功能入口已决定的当前步骤，
        // 返回动作本身仍从真实 Slint callback 贯穿到最终 Screen。
        app.window.set_workspace_active_step(step);
        assert_eq!(app.window.get_workspace_active_step(), step);
        assert_eq!(
            app.window.get_workspace_back_to_plan_list_label().as_str(),
            "← 返回方案列表"
        );

        app.window.invoke_workspace_back_to_plan_list_clicked();

        assert_eq!(
            app.window.get_active_screen(),
            2,
            "步骤 {step} 必须返回方案列表"
        );
        assert_eq!(app.window.get_plan_list_model().row_count(), 1);

        // 再次打开同一方案：已确认边界和已保存朝向仍在，返回不能清空方案状态。
        app.window
            .invoke_plan_list_card_clicked(app.plan_id.clone().into());
        assert_eq!(app.window.get_active_screen(), 4);
        assert!(app.window.get_workspace_boundary_is_determined());
        assert!(app.window.get_workspace_orientation_is_determined());
        assert_eq!(app.window.get_workspace_orientation_angle(), 135.0);
    }
}

/// 运行事件循环直到 `condition` 为真或超时；返回是否在超时前满足。
fn pump_until(
    window: &AppWindow,
    condition: impl Fn(&AppWindow) -> bool + 'static,
    deadline: Duration,
) -> bool {
    let deadline_at = Instant::now() + deadline;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let met = std::sync::Arc::new(std::sync::Mutex::new(false));
    let met_flag = Arc::clone(&met);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                slint::quit_event_loop().expect("stop pump");
                return;
            };
            if condition(&window) || Instant::now() >= deadline_at {
                *met_flag.lock().expect("met flag") = condition(&window);
                slint::quit_event_loop().expect("stop pump");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run pump loop");
    let met_value = *met.lock().expect("met flag");
    met_value
}

fn wait_for_file(path: &Path, deadline: Duration) {
    let deadline_at = Instant::now() + deadline;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline_at,
            "worker did not finish: {}",
            path.display()
        );
        std::thread::yield_now();
    }
}

#[test]
fn leave_confirmation_paths_expire_or_preserve_background_export() {
    assert_return_to_plan_list_works_from_all_five_steps_and_preserves_saved_plan_state();

    // ── 阶段一：运行中确认离开 → 返回方案列表并过期后台导出 ──
    let file_system = Arc::new(BlockingFileSystem::new());
    let app = TestApp::new(Arc::clone(&file_system));

    app.confirm_boundary();
    app.window.invoke_workspace_step_clicked(4);
    app.window.invoke_workspace_export_start_clicked();
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Processing
    );
    file_system.wait_for_manifest_block();

    app.window.invoke_workspace_back_to_plan_list_clicked();
    assert!(
        app.window.get_confirm_dialog_visible(),
        "存在运行中操作时返回方案列表必须请求确认"
    );
    assert_eq!(app.window.get_active_screen(), 4);
    assert_eq!(app.window.get_workspace_active_step(), 4);

    // 确认离开：过期当前 generation、停轮询并进入当前校区方案列表。
    app.window.invoke_confirm_dialog_confirmed();
    assert_eq!(app.window.get_active_screen(), 2, "确认后必须进入方案列表");

    file_system.release_manifest_block();
    // CI 冷缓存/负载下 worker 可能超过 5 秒才落盘；用 30 秒宽松兜底。
    wait_for_file(&app.manifest_path(), Duration::from_secs(30));
    // 给任何仍存活的轮询一个机会把旧结果画回工作区；修复后不会发生。
    let stayed_away = pump_until(
        &app.window,
        |window| window.get_active_screen() == 4,
        Duration::from_millis(400),
    );
    assert!(
        !stayed_away,
        "旧 worker 结果不得把用户拉回工作区（确认后离开必须过期结果）"
    );
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Ready,
        "旧结果不得呈现为成功"
    );
    assert!(!app.window.get_error_dialog_visible());
    drop(app);

    // ── 阶段二：取消离开 → 停留原步骤、后台导出继续成功 ──
    let file_system = Arc::new(BlockingFileSystem::new());
    let app = TestApp::new(Arc::clone(&file_system));

    app.confirm_boundary();
    app.window.invoke_workspace_step_clicked(4);
    app.window.invoke_workspace_export_start_clicked();
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Processing
    );
    file_system.wait_for_manifest_block();

    app.window.invoke_workspace_back_to_plan_list_clicked();
    assert!(app.window.get_confirm_dialog_visible());

    // 取消：停留工作区原步骤，后台导出继续有效。
    app.window.invoke_confirm_dialog_cancelled();
    assert_eq!(app.window.get_active_screen(), 4);
    assert_eq!(app.window.get_workspace_active_step(), 4);
    let still_processing = pump_until(
        &app.window,
        |window| window.get_operation_state() == OperationPresentationState::Processing,
        Duration::from_secs(2),
    );
    assert!(still_processing, "取消离开不得中断进行中的导出");

    file_system.release_manifest_block();
    let succeeded = pump_until(
        &app.window,
        |window| {
            window.get_operation_state() == OperationPresentationState::Succeeded
                || window.get_operation_state() == OperationPresentationState::Failed
        },
        Duration::from_secs(30),
    );
    assert!(
        succeeded,
        "停留时后台导出必须到达终态；当前状态：{:?}",
        app.window.get_operation_state()
    );
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Succeeded,
        "取消离开后当前工作区仍是有效交付上下文"
    );
    assert_eq!(app.window.get_active_screen(), 4);
    assert!(app.schematic_path().is_file());
    assert!(app.manifest_path().is_file());
}
