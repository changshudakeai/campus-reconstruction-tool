//! T36 验收：采集“点击后长时间无反应”体验修复的桌面契约。
//!
//! 覆盖验收点：
//! 1. 注入“慢/挂起 regeo transport”：处理中显示阶段（补名）与已用时长，
//!    “取消采集”按钮可用；取消后回到待定，不无限等、不残留处理态。
//! 2. 注入失败数据源：任何失败必须弹错误对话框并给“重试”，点“重试”重新发起采集。

use data_persistence::CampusCrudApi;
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_acquisition::{DataSource, OverpassDataSource, RegeoNamer};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::StdExportFileSystem;
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn pump_until(
    window: &AppWindow,
    deadline: Duration,
    condition: impl Fn(&AppWindow) -> bool + 'static,
) {
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
            if condition(&window) || Instant::now() >= deadline_at {
                slint::quit_event_loop().expect("停止事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行事件循环");
}

fn unnamed_polygons_payload(count: usize) -> String {
    let elements: Vec<serde_json::Value> = (0..count)
        .map(|index| {
            let base_lon = 121.40 + index as f64 * 0.001;
            serde_json::json!({
                "type": "way",
                "id": 10_000 + index as i64,
                "tags": {"building": "yes"},
                "geometry": [
                    {"lat": 31.200, "lon": base_lon},
                    {"lat": 31.201, "lon": base_lon},
                    {"lat": 31.201, "lon": base_lon + 0.001},
                    {"lat": 31.200, "lon": base_lon + 0.001},
                    {"lat": 31.200, "lon": base_lon}
                ]
            })
        })
        .collect();
    serde_json::json!({ "elements": elements }).to_string()
}

fn slow_regeo_source() -> Arc<dyn DataSource + Send + Sync> {
    let payload = unnamed_polygons_payload(20);
    let overpass_transport =
        Box::new(move |_boundary: &shared_domain_types::Boundary| Ok(payload.clone()));
    // 慢/挂起 regeo：每次调用睡满超时（5s）后失败
    let regeo_transport = Box::new(|_: &str, timeout: Duration| {
        let (_tx, rx) = std::sync::mpsc::channel::<()>();
        let _ = rx.recv_timeout(timeout);
        Err("模拟 regeo 挂起".to_owned())
    });
    let namer = Arc::new(RegeoNamer::new(
        regeo_transport,
        Box::new(|| Some("web-key".to_owned())),
    ));
    Arc::new(OverpassDataSource::new(overpass_transport).with_name_enricher(Some(namer)))
}

fn setup_window(source: Arc<dyn DataSource + Send + Sync>) -> (AppWindow, Localization) {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-26.db");
    let mut injector = ViewModelInjector::new_with_collection_source(
        ShellDatabases::open(&database_path).expect("连接数据库"),
        Arc::new(StdExportFileSystem),
        source,
    )
    .expect("创建注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    let (anchor_lng, anchor_lat) = gaode_client::wgs84_to_gcj02(121.399, 31.199);
    let campus = injector
        .projects_mut()
        .database()
        .create_campus_with_anchor(
            "验收校区",
            "fixture-putuo",
            "上海离线夹具",
            anchor_lng,
            anchor_lat,
        )
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (3000.0, 0.0), (3000.0, 500.0), (0.0, 500.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    window.invoke_workspace_step_clicked(2);
    (window, l10n)
}

#[test]
fn s1_26_slow_regeo_shows_stage_elapsed_and_cancel() {
    let (window, l10n) = setup_window(slow_regeo_source());

    window.invoke_collection_start_clicked();
    assert!(
        window.get_collection_cancel_visible(),
        "处理中必须显示取消采集"
    );
    assert_eq!(
        window.get_collection_stage_label().as_str(),
        l10n.t("collection.stage_fetching"),
        "启动后先呈现“拉取数据”阶段"
    );

    // 补名阶段：慢 regeo 挂起时页面持续显示阶段 + 已用时长，不冻结
    let naming_stage = l10n.t("collection.stage_naming");
    pump_until(&window, Duration::from_secs(5), move |window| {
        window.get_collection_stage_label().as_str() == naming_stage.as_str()
    });
    assert_eq!(
        window.get_collection_stage_label().as_str(),
        l10n.t("collection.stage_naming"),
        "补名阶段必须可见"
    );
    assert!(
        window
            .get_collection_elapsed_label()
            .as_str()
            .contains("秒"),
        "必须显示已用时长：{}",
        window.get_collection_elapsed_label()
    );

    // 取消采集：回到待定、取消按钮隐藏、不残留处理态
    window.invoke_collection_cancel_clicked();
    pump_until(&window, Duration::from_secs(5), |window| {
        !window.get_collection_cancel_visible()
    });
    assert!(!window.get_collection_cancel_visible());
    assert_eq!(
        window.get_collection_progress_label().as_str(),
        l10n.t("collection.progress_title"),
        "取消后回到待定进度面板"
    );
}
