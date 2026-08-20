//! 边界顶点编辑 + 工作现场恢复（验收第 7 条）：
//! - OSM 自动获取原始 4 点边界 → 顶点编辑插入 1 点成 5 点 → 确认；
//! - 边界入库：检查点中保存的是编辑后的 5 点环（非 OSM 原始 4 点）；
//! - 重启（全新注入器 + 同一数据库 + 计数 fake 源）→ 自动恢复上次打开方案，
//!   恢复为"调整后的版本"（点数 5、已确认、导出尺寸与会话一一致）；
//! - map_ready 后仍不重新抓取 OSM（fake 源调用数 = 0）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_acquisition::overpass::{BoundarySourceKind, CampusBoundaryResult};
use data_persistence::{CampusCrudApi, Database, WorkspaceStateApi};
use desktop_shell::{
    assemble_application, AppWindow, BoundaryFetchSource, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn original_square() -> CampusBoundaryResult {
    CampusBoundaryResult::AutoSelected {
        name: "OSM 原始边界校区".to_owned(),
        gcj02: vec![
            [116.40, 39.90],
            [116.41, 39.90],
            [116.41, 39.91],
            [116.40, 39.91],
        ],
        source: BoundarySourceKind::OverpassAmenity,
        candidate_count: 1,
    }
}

/// 顶点编辑后的 5 点环（在边 1→2 中点插入 1 点）
fn edited_ring() -> Vec<[f64; 2]> {
    vec![
        [116.40, 39.90],
        [116.41, 39.90],
        [116.41, 39.905],
        [116.41, 39.91],
        [116.40, 39.91],
    ]
}

fn edited_ring_json() -> String {
    serde_json::to_string(&edited_ring()).expect("edited ring json")
}

fn edited_confirm_ipc() -> String {
    format!(
        r#"{{"type":"confirm_boundary","coords":{}}}"#,
        edited_ring_json()
    )
}

fn edited_update_ipc() -> String {
    format!(
        r#"{{"type":"boundary_update","coords":{}}}"#,
        edited_ring_json()
    )
}

fn pump_until_point_count(window: &AppWindow, expected: i32) {
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
            if window.get_workspace_boundary_point_count() == expected || Instant::now() >= deadline
            {
                slint::quit_event_loop().expect("stop point-count pump");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run point-count pump");
    assert_eq!(window.get_workspace_boundary_point_count(), expected);
}

fn pump_for(duration: Duration) {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::SingleShot, duration, || {
        slint::quit_event_loop().expect("stop bounded pump");
    });
    slint::run_event_loop_until_quit().expect("run bounded pump");
}

fn pump_until_terminal(window: &AppWindow) -> OperationPresentationState {
    let deadline = Instant::now() + Duration::from_secs(30);
    let weak = window.as_weak();
    let terminal = Arc::new(std::sync::Mutex::new(
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
                slint::quit_event_loop().expect("stop export pump");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run export pump");
    let value = *terminal.lock().expect("terminal state lock");
    value
}

fn export_dimensions(
    window: &AppWindow,
    export_dir: &std::path::Path,
    plan_id: &str,
) -> [usize; 3] {
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        pump_until_terminal(window),
        OperationPresentationState::Succeeded,
        "编辑后的边界必须能够导出"
    );
    sponge_export::inspect_schematic(&export_dir.join(format!("{plan_id}.schem")))
        .expect("导出 .schem 必须存在且可读")
        .dimensions
}

#[test]
fn s1_31_edited_boundary_persists_and_restores_without_refetch() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("vertex-edit-persist.db");
    let export_first = directory.path().join("exports-first");
    let export_after = directory.path().join("exports-after-restart");

    // ── 第一段会话：OSM 自动获取 4 点 → 顶点编辑成 5 点 → 确认 → 导出 ──
    let (plan_id, first_dimensions) = {
        let calls = Arc::new(AtomicUsize::new(0));
        let source_calls = Arc::clone(&calls);
        let source: BoundaryFetchSource = Arc::new(move |_, _, _, _| {
            source_calls.fetch_add(1, Ordering::SeqCst);
            original_square()
        });

        desktop_shell::set_webview_creation_probe(true);
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));
        let mut injector = ViewModelInjector::new_with_boundary_source(
            ShellDatabases::open(&database_path).expect("open databases"),
            source,
        )
        .expect("construct first injector");
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
            .set_default_export_location(export_first.to_str().expect("export dir"))
            .expect("set export location");
        let campus = injector
            .projects_mut()
            .database()
            .create_campus("顶点持久化测试校区")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "顶点持久化测试方案")
            .expect("create plan")
            .to_string();
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        window.invoke_plan_list_card_clicked(plan_id.clone().into());
        window.invoke_workspace_tutorial_dismiss_clicked();
        window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
        pump_until_point_count(&window, 4);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "第一段会话必须恰好抓取一次 OSM 原始边界"
        );

        // 顶点编辑：插入 1 点成 5 点并确认（走现有 confirm_boundary 链路）
        window.invoke_workspace_map_ipc(edited_update_ipc().into());
        assert_eq!(window.get_workspace_boundary_point_count(), 5);
        window.invoke_workspace_map_ipc(edited_confirm_ipc().into());
        assert!(window.get_workspace_boundary_is_determined());
        assert_eq!(window.get_workspace_boundary_point_count(), 5);

        // 边界入库：检查点保存的是编辑后的 5 点环（非 OSM 原始 4 点）
        let database =
            Database::open(&database_path).expect("reopen database to inspect checkpoint");
        let state = database
            .load_plan_workspace_state(&plan_id)
            .expect("读取检查点")
            .expect("确认后必须落库检查点");
        assert!(state.boundary_confirmed);
        assert_eq!(
            state.boundary_gcj02,
            edited_ring(),
            "入库必须为编辑后的顶点"
        );
        drop(database);

        // 编辑后边界导出的基准尺寸
        let dimensions = export_dimensions(&window, &export_first, &plan_id);
        window.invoke_workspace_step_clicked(0);
        pump_for(Duration::from_millis(50));
        (plan_id, dimensions)
    };

    // ── 第二段会话：重启（同一数据库 + 全新注入器 + 计数 fake 源）──
    let restore_calls = Arc::new(AtomicUsize::new(0));
    let source_calls = Arc::clone(&restore_calls);
    let source: BoundaryFetchSource = Arc::new(move |_, _, _, _| {
        source_calls.fetch_add(1, Ordering::SeqCst);
        original_square()
    });
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(&database_path).expect("reopen databases"),
        source,
    )
    .expect("construct second injector");
    injector
        .settings_mut()
        .set_default_export_location(export_after.to_str().expect("export dir after restart"))
        .expect("set export location after restart");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // 启动即自动恢复上次打开方案，边界为"调整后的版本"（5 点已确认）
    assert_eq!(
        window.get_workspace_plan_name().as_str(),
        "顶点持久化测试方案",
        "重启后必须自动恢复上次打开方案"
    );
    assert!(
        window.get_workspace_boundary_is_determined(),
        "重启后必须恢复已确认的编辑后边界"
    );
    pump_until_point_count(&window, 5);

    // map_ready 不得重新抓取 OSM 原始边界（命中恢复缓存，源调用数保持 0）
    window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(150));
    assert_eq!(
        restore_calls.load(Ordering::SeqCst),
        0,
        "重启后恢复编辑后边界，不得重新抓取 OSM 原始边界"
    );
    assert_eq!(window.get_workspace_boundary_point_count(), 5);
    assert!(window.get_workspace_boundary_is_determined());

    // 恢复的边界可直接导出，且导出尺寸与会话一的编辑后边界一致
    let restored_dimensions = export_dimensions(&window, &export_after, &plan_id);
    assert_eq!(
        restored_dimensions, first_dimensions,
        "重启后恢复的边界必须是编辑后的版本（导出尺寸一致），而非 OSM 原始边界"
    );
    assert!(
        export_after.join(format!("{plan_id}.schem")).is_file(),
        "恢复后导出必须产出 .schem"
    );
}
