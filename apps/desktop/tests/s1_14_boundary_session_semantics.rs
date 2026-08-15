//! M1 + workspace-restore acceptance: the confirmed boundary is a per-plan
//! safety checkpoint and is restored after restart（含意外退出）。
//!
//! ADR-0007 把边界归属定义为方案级数据；workspace-restore 工单把已确认
//! 边界/朝向/步骤在状态变更点落库。本测试证明：确认边界 → 重启 → 恢复
//! "已确认"可直接导出；未确认边界时导出仍明确拒绝，不伪造边界。

use data_acquisition::overpass::{BoundarySourceKind, CampusBoundaryResult};
use data_persistence::CampusCrudApi;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, BoundaryFetchSource,
    OperationPresentationState, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn pump_for(duration: Duration) {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::SingleShot, duration, || {
        slint::quit_event_loop().expect("stop bounded boundary-session pump");
    });
    slint::run_event_loop_until_quit().expect("run bounded boundary-session pump");
}

fn canned_boundary() -> CampusBoundaryResult {
    CampusBoundaryResult::AutoSelected {
        name: "session cached campus".to_owned(),
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

fn canned_boundary_with_points(point_count: usize) -> CampusBoundaryResult {
    let mut gcj02 = vec![
        [116.40, 39.90],
        [116.41, 39.90],
        [116.41, 39.91],
        [116.40, 39.91],
        [116.395, 39.905],
    ];
    gcj02.truncate(point_count);
    CampusBoundaryResult::AutoSelected {
        name: format!("session cached campus {point_count}"),
        gcj02,
        source: BoundarySourceKind::OverpassAmenity,
        candidate_count: 1,
    }
}

fn pump_until_calls(calls: &Arc<AtomicUsize>, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while calls.load(Ordering::SeqCst) < expected && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(
        calls.load(Ordering::SeqCst) >= expected,
        "fake boundary source calls did not reach {expected}"
    );
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
                slint::quit_event_loop().expect("stop boundary point-count pump");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run boundary point-count pump");
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        expected,
        "plan={}, operation={:?}",
        window.get_workspace_plan_name(),
        window.get_operation_state()
    );
}

fn pump_until_ready(window: &AppWindow) {
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
                slint::quit_event_loop().expect("stop boundary ready-state pump");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run boundary ready-state pump");
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready
    );
}

struct TwoPlanWorkspace {
    window: AppWindow,
    _runtime: ApplicationRuntime,
    _directory: tempfile::TempDir,
    export_dir: PathBuf,
    plan_a: String,
    plan_b: String,
}

fn boundary_cache_scenarios() {
    let calls = Arc::new(AtomicUsize::new(0));
    let source_calls = Arc::clone(&calls);
    let (concurrent_release_tx, concurrent_release_rx) = std::sync::mpsc::channel::<()>();
    let concurrent_release_rx = Arc::new(std::sync::Mutex::new(concurrent_release_rx));
    let source_concurrent_release = Arc::clone(&concurrent_release_rx);
    let (stale_release_tx, stale_release_rx) = std::sync::mpsc::channel::<()>();
    let stale_release_rx = Arc::new(std::sync::Mutex::new(stale_release_rx));
    let source_stale_release = Arc::clone(&stale_release_rx);
    let (failure_release_tx, failure_release_rx) = std::sync::mpsc::channel::<()>();
    let failure_release_rx = Arc::new(std::sync::Mutex::new(failure_release_rx));
    let source_failure_release = Arc::clone(&failure_release_rx);
    let (refresh_release_tx, refresh_release_rx) = std::sync::mpsc::channel::<()>();
    let refresh_release_rx = Arc::new(std::sync::Mutex::new(refresh_release_rx));
    let source_refresh_release = Arc::clone(&refresh_release_rx);
    let (rapid_switch_a_release_tx, rapid_switch_a_release_rx) = std::sync::mpsc::channel::<()>();
    let rapid_switch_a_release_rx = Arc::new(std::sync::Mutex::new(rapid_switch_a_release_rx));
    let source_rapid_switch_a_release = Arc::clone(&rapid_switch_a_release_rx);
    let source: BoundaryFetchSource = Arc::new(move |_, _, _, _progress| {
        let call = source_calls.fetch_add(1, Ordering::SeqCst) + 1;
        match call {
            1 => canned_boundary(),
            3 | 6 => {
                if call == 6 {
                    source_concurrent_release
                        .lock()
                        .expect("lock concurrent release")
                        .recv()
                        .expect("release concurrent request");
                }
                canned_boundary_with_points(4)
            }
            2 | 5 | 8 => canned_boundary_with_points(5),
            9 => canned_boundary_with_points(4),
            10 => canned_boundary_with_points(5),
            11 => {
                source_refresh_release
                    .lock()
                    .expect("lock refresh release")
                    .recv()
                    .expect("release refresh request");
                canned_boundary_with_points(4)
            }
            12 => {
                source_rapid_switch_a_release
                    .lock()
                    .expect("lock rapid-switch A release")
                    .recv()
                    .expect("release rapid-switch A request");
                canned_boundary_with_points(4)
            }
            13 => canned_boundary_with_points(5),
            4 => {
                source_failure_release
                    .lock()
                    .expect("lock failure release")
                    .recv()
                    .expect("release failed request");
                CampusBoundaryResult::Unreachable {
                    message: "canned outage".to_owned(),
                }
            }
            7 => {
                source_stale_release
                    .lock()
                    .expect("lock stale release")
                    .recv()
                    .expect("release stale request");
                canned_boundary_with_points(3)
            }
            other => panic!("unexpected fake boundary call {other}"),
        }
    });
    let app = two_plan_workspace(source);

    // 首次实际请求 = 1；确认后跨五步多次往返仍 = 1。
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    app.window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
            .into(),
    );
    for _ in 0..3 {
        for step in [1, 2, 3, 4, 0] {
            app.window.invoke_workspace_step_clicked(step);
            app.window
                .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
        }
    }
    pump_for(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 方案 B 有自己的首次请求；返回 A 命中 A 缓存；清除 A 后 A 才请求第 2 次。
    app.window
        .invoke_plan_list_card_clicked(app.plan_b.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 2);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 5);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(app.window.get_workspace_boundary_point_count(), 4);
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 3);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    // 失败不写成功缓存，可重试；第二次成功后后续 map_ready 命中缓存。
    app.window
        .invoke_plan_list_card_clicked(app.plan_b.clone().into());
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 4);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    failure_release_tx.send(()).expect("release failed request");
    pump_until_ready(&app.window);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 5);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 5);
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 5);

    // 请求中重复 map_ready 只调用一次 fake 源。
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 6);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    concurrent_release_tx
        .send(())
        .expect("release concurrent request");
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);
    assert_eq!(calls.load(Ordering::SeqCst), 6);

    // 先让 B 缓存失效，再让 A 的请求悬挂；切到 B 后必须启动 B 的新请求，
    // A 的迟到结果随后到达也不能覆盖 B。
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 7);
    app.window
        .invoke_plan_list_card_clicked(app.plan_b.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 8);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 5);
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    stale_release_tx.send(()).expect("release stale A request");
    pump_for(Duration::from_millis(100));
    assert_eq!(app.window.get_workspace_boundary_point_count(), 5);

    // 用户明确刷新才允许 B 的下一次请求；普通 map_ready 已由缓存吸收。
    app.window.invoke_workspace_boundary_refresh_clicked();
    pump_until_calls(&calls, 9);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);
    assert_eq!(calls.load(Ordering::SeqCst), 9);

    // 已确认边界刷新后，新候选必须回到可编辑/可确认状态；确认后正式导出
    // 输入必须更新为新边界，而不是继续使用刷新前的旧边界。
    app.window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
            .into(),
    );
    assert!(app.window.get_workspace_boundary_is_determined());
    let old_dimensions = export_dimensions(&app);
    app.window.invoke_workspace_step_clicked(0);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    app.window.invoke_workspace_boundary_refresh_clicked();
    assert!(
        !app.window.get_workspace_boundary_is_determined(),
        "刷新已确认边界后必须呈现待确认候选，而不是继续锁定旧边界"
    );
    pump_until_calls(&calls, 10);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 5);
    assert!(!app.window.get_workspace_boundary_is_determined());
    app.window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91],[116.395,39.905]]}"#
            .into(),
    );
    assert!(app.window.get_workspace_boundary_is_determined());
    let new_dimensions = export_dimensions(&app);
    assert_ne!(
        new_dimensions, old_dimensions,
        "确认刷新候选后，正式导出边界必须替换旧边界"
    );

    // 同一方案的刷新已经在途时，连续点击刷新不得另起 OSM 请求。
    app.window.invoke_workspace_step_clicked(0);
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    app.window.invoke_workspace_boundary_refresh_clicked();
    pump_until_calls(&calls, 11);
    app.window.invoke_workspace_boundary_refresh_clicked();
    pump_for(Duration::from_millis(100));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        11,
        "同键刷新在途时重复点击必须复用原请求"
    );
    refresh_release_tx
        .send(())
        .expect("release refresh request");
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);

    // A 的请求仍在途时快速 A→B→A，返回 A 必须重新接管原请求，不能再访问 OSM。
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 12);
    app.window
        .invoke_plan_list_card_clicked(app.plan_b.clone().into());
    app.window.invoke_workspace_boundary_reset_clicked();
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_calls(&calls, 13);
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_for(Duration::from_millis(100));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        13,
        "A→B→A 快速返回必须复用 A 的原在途请求"
    );

    // B 的结果只在 B 活跃时呈现；A 迟到不能覆盖 B，返回 A 后才恢复 A。
    app.window
        .invoke_plan_list_card_clicked(app.plan_b.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 5);
    rapid_switch_a_release_tx
        .send(())
        .expect("release rapid-switch A request");
    pump_for(Duration::from_millis(100));
    assert_eq!(
        app.window.get_workspace_boundary_point_count(),
        5,
        "A 的迟到结果不得覆盖当前活跃的 B"
    );
    app.window
        .invoke_plan_list_card_clicked(app.plan_a.clone().into());
    app.window
        .invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    pump_until_point_count(&app.window, 4);
}

fn export_dimensions(app: &TwoPlanWorkspace) -> [usize; 3] {
    app.window.invoke_workspace_step_clicked(4);
    app.window.invoke_workspace_export_start_clicked();
    assert_eq!(
        pump_until_terminal(&app.window),
        OperationPresentationState::Succeeded
    );
    sponge_export::inspect_schematic(&app.export_dir.join(format!("{}.schem", app.plan_b)))
        .expect("exported refreshed boundary schematic")
        .dimensions
}

fn two_plan_workspace(boundary_source: BoundaryFetchSource) -> TwoPlanWorkspace {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("two-plan-workspace-cache.db");
    let export_dir = directory.path().join("exports");
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(&database_path).expect("open databases"),
        boundary_source,
    )
    .expect("construct injector with fake boundary source");
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
        .set_gaode_api_key("testapikey1234567890")
        .expect("save fake API key");
    injector
        .settings_mut()
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("save fake security key");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("temporary export path"))
        .expect("set export directory");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("two plan cached campus")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_a = injector
        .projects_mut()
        .create_plan(&campus_id, "cached plan A")
        .expect("create plan A")
        .to_string();
    let plan_b = injector
        .projects_mut()
        .create_plan(&campus_id, "cached plan B")
        .expect("create plan B")
        .to_string();
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let runtime = assemble_application(&window, injector, Arc::clone(&center));
    TwoPlanWorkspace {
        window,
        _runtime: runtime,
        _directory: directory,
        export_dir,
        plan_a,
        plan_b,
    }
}

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

fn confirmed_boundary_persists_and_restarts_ready_for_export() {
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

    // 第二段启动即自动恢复上次打开方案（A.1）；再点一次方案卡片保持幂等。
    window.invoke_plan_list_card_clicked(plan_id.clone().into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "重启后必须恢复上次确认的边界（状态“边界已确认”，A.2）"
    );

    // 恢复后无需重新抓取即可直接导出（A.2）。
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    let terminal = pump_until_terminal(&window);
    assert_eq!(
        terminal,
        OperationPresentationState::Succeeded,
        "重启后恢复的已确认边界必须可直接导出；当前状态：{terminal:?}"
    );
    assert!(
        export_dir_after_restart
            .join(format!("{plan_id}.schem"))
            .is_file(),
        "恢复后导出必须产出 .schem"
    );
}

#[test]
fn workspace_boundary_session_semantics() {
    boundary_cache_scenarios();
    confirmed_boundary_persists_and_restarts_ready_for_export();
}
