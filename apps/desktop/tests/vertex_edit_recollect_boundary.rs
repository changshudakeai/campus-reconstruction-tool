//! 回归（边界顶点编辑工单）：编辑边界 → 确认 → 再次采集。
//!
//! 用户报告：已有采集结果的方案上手动编辑校区边界并确认后，再次点击“采集”
//! 必失败并提示网络/连接类错误。本测试用生产同款
//! `boundary_bbox + campus_objects_query + OverpassDataSource` 传输（离线罐头
//! 空响应，不发起真实网络请求）走真实 UI seam：采集成功 → 编辑（boundary_update）
//! → 确认（confirm_boundary）→ 再采集。断言：
//! 1. 第二次采集收到的是编辑后的 5 点边界（不是旧边界）；
//! 2. 该边界能正常派生查询包围盒并生成采集查询；
//! 3. 采集成功完成，且呈现给用户的错误弹窗不包含“数据源不可达 / 检查网络 /
//!    检查地图连接”类泛化提示。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use data_acquisition::overpass::{boundary_bbox, campus_objects_query};
use data_acquisition::{DataSource, OverpassDataSource};
use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::StdExportFileSystem;
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId};
use slint::ComponentHandle;

fn initial_confirm() -> &'static str {
    r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
}

fn edited_update() -> &'static str {
    r#"{"type":"boundary_update","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.905],[116.41,39.91],[116.40,39.91]]}"#
}

fn edited_confirm() -> &'static str {
    r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.905],[116.41,39.91],[116.40,39.91]]}"#
}

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
                slint::quit_event_loop().expect("停止回归事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行回归事件循环");
}

fn network_flavored_body(body: &str) -> bool {
    body.contains("数据源不可达")
        || body.contains("检查网络")
        || body.contains("检查地图连接")
        || body.contains("网络")
}

#[test]
fn vertex_edit_confirm_then_recollect_uses_edited_boundary_and_succeeds() {
    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("vertex-edit-recollect.db");
    let captured: Arc<Mutex<Vec<Boundary>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_transport = Arc::clone(&captured);

    // 生产等价传输：boundary_bbox → campus_objects_query → 罐头空 Overpass 响应。
    // 与 production_collection_source 的传输实现一一对应，不发起真实网络请求。
    let canned: Arc<dyn DataSource + Send + Sync> = Arc::new(OverpassDataSource::new(Box::new(
        move |boundary: &Boundary| {
            captured_transport
                .lock()
                .expect("capture lock")
                .push(boundary.clone());
            let bbox = boundary_bbox(boundary, 0.01)
                .ok_or_else(|| "边界坐标无法计算查询包围盒".to_owned())?;
            campus_objects_query(bbox)
                .map_err(|error| format!("集中标签规则无法生成采集查询：{error}"))?;
            Ok(r#"{"elements":[]}"#.to_owned())
        },
    )));

    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_collection_source(
        ShellDatabases::open(&database_path).expect("连接数据库"),
        Arc::new(StdExportFileSystem),
        canned,
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
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("回归校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "回归方案")
        .expect("创建方案")
        .to_string();
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // 1. 打开方案 → 确认初始 4 点边界。
    window.invoke_plan_list_card_clicked(plan_id.clone().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_ipc(initial_confirm().into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "首次边界必须确认成功"
    );

    // 2. 第一次采集（已有采集结果的前提）。
    window.invoke_workspace_step_clicked(2);
    window.invoke_collection_start_clicked();
    pump_until(&window, Duration::from_secs(15), |window| {
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0')
    });
    assert!(
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0'),
        "首次采集必须成功完成"
    );
    // 覆盖体检疑点窗是采集成功的正常呈现，关闭后继续。
    if window.get_error_dialog_visible() {
        window.invoke_error_dialog_dismissed();
    }

    // 3. 回到边界步：模拟拖动/加号/删除后的编辑（boundary_update 同步点数），
    //    再经 confirm_boundary 确认编辑后的 5 点边界。
    window.invoke_workspace_step_clicked(0);
    window.invoke_workspace_map_ipc(edited_update().into());
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        5,
        "编辑后抽屉点数必须同步为 5"
    );
    window.invoke_workspace_map_ipc(edited_confirm().into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "编辑后的边界必须确认成功"
    );

    // 4. 再次采集：必须成功完成，且不得出现网络/连接类泛化提示。
    window.invoke_workspace_step_clicked(2);
    window.invoke_collection_start_clicked();
    pump_until(&window, Duration::from_secs(15), |window| {
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0')
    });
    assert!(
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0'),
        "编辑后再次采集必须成功完成"
    );
    let body = window.get_error_dialog_body();
    assert!(
        !network_flavored_body(body.as_str()),
        "不得出现无意义的网络/连接泛化提示，实际弹窗：{body}"
    );

    // 5. 证据：第二次采集收到的必须是编辑后的 5 点边界，且可派生正常包围盒。
    let captured = captured.lock().expect("capture lock");
    assert_eq!(captured.len(), 2, "应恰好发生两次采集");
    let second = captured.last().expect("第二次采集边界");
    assert_eq!(second.r#type, "Polygon");
    let coords = second.coordinates.as_array().expect("Polygon 坐标数组");
    assert_eq!(coords.len(), 1, "单环 Polygon");
    let ring = coords[0].as_array().expect("外环");
    assert_eq!(ring.len(), 5, "编辑后边界必须为 5 点");
    let bbox = boundary_bbox(second, 0.01).expect("编辑后边界必须能派生包围盒");
    let (south, west, north, east) = bbox;
    assert!((south - 39.89).abs() < 1e-9);
    assert!((west - 116.39).abs() < 1e-9);
    assert!((north - 39.92).abs() < 1e-6);
    assert!((east - 116.42).abs() < 1e-9);
}
