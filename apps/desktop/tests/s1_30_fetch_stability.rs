//! workspace-restore 工单验收（B 部分）：
//! - 快端点：典型情况 5 秒内完成（B.9）；
//! - 慢/挂端点：超过 15 秒有明确阶段反馈（进度/当前阶段），不是干等（B.9）；
//! - 失败给出明确原因（端点不可达/阶段名），不笼统提示"超时"（B.10）；
//! - 重启后边界已恢复时不重复校名解析/Overpass 查询（B.12）。

use data_acquisition::overpass::{
    BoundarySourceKind, CampusBoundaryResult, FetchProgress, FetchStage,
};
use data_persistence::CampusCrudApi;
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, BoundaryFetchSource, ShellDatabases,
    ShellPresenter, ViewModelInjector,
};

fn fast_boundary() -> CampusBoundaryResult {
    CampusBoundaryResult::AutoSelected {
        name: "快速端点校区".to_owned(),
        gcj02: vec![
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
            [121.40, 31.21],
        ],
        source: BoundarySourceKind::OverpassAmenity,
        candidate_count: 1,
    }
}

fn setup_app(
    database_path: &PathBuf,
    boundary_source: BoundaryFetchSource,
) -> (AppWindow, ApplicationRuntime, Arc<NotificationCenter>) {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(database_path).expect("打开数据库"),
        boundary_source,
    )
    .expect("创建注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首开设置");
    injector
        .settings_mut()
        .set_gaode_api_key("testapikey1234567890")
        .expect("写入测试密钥");
    injector
        .settings_mut()
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("写入测试安全密钥");
    injector
        .settings_mut()
        .set_default_export_location(
            std::env::temp_dir()
                .join("mc-rebuild-s1-30")
                .to_str()
                .expect("导出目录"),
        )
        .expect("设置导出目录");
    let runtime = assemble_application(&window, injector, Arc::clone(&center));
    (window, runtime, center)
}

/// 用独立注入器建一个校区 + 方案并记住校区（返回方案 ID）。
fn seed_plan(
    database_path: &PathBuf,
    campus_name: &str,
    plan_name: &str,
) -> shared_domain_types::PlanId {
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(database_path).expect("打开数据库"),
        Arc::new(|_, _, _, _| fast_boundary()),
    )
    .expect("创建注入器");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus(campus_name)
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, plan_name)
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记住校区");
    plan_id
}

fn pump_until(window: &AppWindow, condition: impl Fn(&AppWindow) -> bool + 'static) {
    let deadline = Instant::now() + Duration::from_secs(8);
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if condition(&window) || Instant::now() >= deadline {
                slint::quit_event_loop().expect("停止轮询");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行轮询");
    timer.stop();
}

#[test]
fn fetch_stability_fast_hang_failure_reason_and_restart_no_reresolve() {
    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-30.db");

    // ── 快端点：5 秒内完成（B.9）──────────────────────────
    let calls = Arc::new(AtomicUsize::new(0));
    let source_calls = Arc::clone(&calls);
    let fast_source: BoundaryFetchSource = Arc::new(move |_, _, _, _| {
        source_calls.fetch_add(1, Ordering::SeqCst);
        fast_boundary()
    });
    {
        let (window, _runtime, _center) = setup_app(&database_path, fast_source);
        let plan_id = seed_plan(&database_path, "快端点校区", "快端点方案");
        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        let started = Instant::now();
        window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
        pump_until(&window, |window| {
            window.get_workspace_boundary_point_count() == 4
        });
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "典型情况必须 5 秒内完成（B.9）：实际 {elapsed:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "快端点只允许一次请求（缓存吸收重复 map_ready）"
        );
    }

    // ── 慢/挂端点：>15s 有明确阶段反馈 + 失败原因明确（B.9/B.10）───────
    let hang_source: BoundaryFetchSource = Arc::new(|_, _, _, on_progress| {
        // 模拟"卡在第一个端点"：上报阶段 + 已耗时 16 秒（>15s 反馈线）
        on_progress(FetchProgress {
            stage: FetchStage::ByElementId,
            attempt: 1,
            total_attempts: 3,
            elapsed_secs: 16,
        });
        CampusBoundaryResult::Unreachable {
            message:
                "端点 https://overpass-api.de 不可达：连接超时；端点 https://overpass.kumi.systems 不可达：连接超时"
                    .to_owned(),
        }
    });
    {
        let (window, _runtime, center) = setup_app(&database_path, hang_source);
        let plan_id = seed_plan(&database_path, "慢端点校区", "慢端点方案");
        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());

        // >15s 阶段反馈：抽屉必须显示"当前阶段 + 已耗时"，不是干等
        pump_until(&window, |_window| {
            _window
                .get_workspace_boundary_fetch_status()
                .contains("按 ID 查询边界")
        });
        let status = window.get_workspace_boundary_fetch_status().to_string();
        assert!(
            status.contains("16 秒") && status.contains("第 1/3 个端点"),
            "超过 15 秒必须显示阶段与已耗时：{status}"
        );

        // 失败原因明确：点名"端点不可达"，不是笼统"超时"
        let center_probe = Arc::clone(&center);
        pump_until(&window, move |_window| {
            let records = center_probe.board_records();
            records
                .iter()
                .any(|record| record.notification().title.contains("边界自动获取失败"))
        });
        let board = center.board_records();
        let failure = board
            .iter()
            .find(|record| record.notification().title.contains("边界自动获取失败"))
            .expect("必须出现边界获取失败通知");
        let body = failure.notification().body.as_str();
        assert!(
            body.contains("端点 https://overpass-api.de 不可达：连接超时"),
            "失败原因必须点名端点与原因（B.10）：{body}"
        );
        assert!(
            !window.get_workspace_boundary_is_determined(),
            "失败后边界不得被伪造为已确认"
        );
    }

    // ── 重启不重复校名解析（边界已恢复时，B.12）────────────────
    let restore_calls = Arc::new(AtomicUsize::new(0));
    let restore_source_calls = Arc::clone(&restore_calls);
    let restore_source: BoundaryFetchSource = Arc::new(move |_, _, _, _| {
        restore_source_calls.fetch_add(1, Ordering::SeqCst);
        fast_boundary()
    });
    {
        // 第一段：确认边界（写检查点）
        let (window, _runtime, _center) = setup_app(&database_path, restore_source.clone());
        let plan_id = seed_plan(&database_path, "恢复校区", "恢复方案");
        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.41,31.20],[121.41,31.21],[121.40,31.21]]}"#
                .into(),
        );
        assert!(window.get_workspace_boundary_is_determined());
    }
    let calls_before_restart = restore_calls.load(Ordering::SeqCst);
    {
        // 第二段：重启后自动恢复 → map_ready 命中缓存，零校名解析
        let (window, _runtime, _center) = setup_app(&database_path, restore_source);
        assert!(
            window.get_workspace_boundary_is_determined(),
            "重启后边界必须已恢复"
        );
        window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
        pump_until(&window, |window| {
            window.get_workspace_boundary_point_count() == 4
        });
        assert_eq!(
            restore_calls.load(Ordering::SeqCst),
            calls_before_restart,
            "边界已恢复时重启后不得重复校名解析/Overpass 查询（B.12）"
        );
    }
    desktop_shell::shutdown();
}
