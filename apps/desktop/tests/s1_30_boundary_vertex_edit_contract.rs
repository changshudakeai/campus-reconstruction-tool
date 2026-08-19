//! 壳层顶点编辑契约（边界顶点编辑工单）：
//! - 抽屉"删除选中点"按钮：无选中禁用；vertex_selected 后启用；
//!   vertex_deselected 后禁用（未选中时删除不可用）；
//! - delete_vertex_rejected → 明确错误弹窗"至少需要 3 个点才能闭合边界"，
//!   边界点数与状态不变（<3 点保护不破坏边界）；
//! - 编辑后的顶点经现有 confirm_boundary 链路确认（boundary_update 同步
//!   点数、confirm_boundary 使用编辑后的坐标），导出成功；
//! - 无效形状（自相交蝴蝶结）→ 错误弹窗 + 不确认 + 程序存活（T35 语义），
//!   关闭后可恢复有效确认。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_acquisition::overpass::{BoundarySourceKind, CampusBoundaryResult};
use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;

fn canned_boundary() -> CampusBoundaryResult {
    CampusBoundaryResult::AutoSelected {
        name: "顶点编辑测试校区".to_owned(),
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

fn edited_boundary() -> &'static str {
    r#"{"type":"boundary_update","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.905],[116.41,39.91],[116.40,39.91]]}"#
}

fn edited_confirm() -> &'static str {
    r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.905],[116.41,39.91],[116.40,39.91]]}"#
}

fn bowtie_confirm() -> &'static str {
    r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.91],[116.41,39.90],[116.40,39.91]]}"#
}

struct VertexEditWorkspace {
    window: AppWindow,
    _runtime: desktop_shell::ApplicationRuntime,
    _directory: tempfile::TempDir,
    export_dir: PathBuf,
    plan_id: String,
}

fn build_workspace() -> VertexEditWorkspace {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("vertex-edit-contract.db");
    let export_dir = directory.path().join("exports");
    desktop_shell::set_webview_creation_probe(true);
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(&database_path).expect("open databases"),
        Arc::new(|_, _, _, _| canned_boundary()),
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
        .set_default_export_location(export_dir.to_str().expect("temporary export path"))
        .expect("set export directory");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("顶点编辑测试校区")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "顶点编辑测试方案")
        .expect("create plan")
        .to_string();
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));
    VertexEditWorkspace {
        window,
        _runtime,
        _directory: directory,
        export_dir,
        plan_id,
    }
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

#[test]
fn s1_30_vertex_edit_drawer_and_confirm_contract() {
    let l10n = Localization::new(Language::ZhCn).expect("load zh-CN resources");
    let app = build_workspace();
    let window = &app.window;

    window.invoke_plan_list_card_clicked(app.plan_id.clone().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());

    // 自动获取 4 点边界后：无选中 → 删除按钮禁用。
    pump_until_point_count(window, 4);
    assert!(
        !window.get_workspace_boundary_delete_selected_enabled(),
        "未选中任何点时删除按钮必须禁用"
    );

    // vertex_selected → 删除按钮启用；vertex_deselected → 禁用。
    window.invoke_workspace_map_ipc(r#"{"type":"vertex_selected","index":1,"count":4}"#.into());
    assert!(window.get_workspace_boundary_delete_selected_enabled());
    window.invoke_workspace_map_ipc(r#"{"type":"vertex_deselected"}"#.into());
    assert!(!window.get_workspace_boundary_delete_selected_enabled());

    // 删除被拒绝（剩余点数 < 3）：明确提示，点数与状态不变。
    window.invoke_workspace_map_ipc(
        r#"{"type":"delete_vertex_rejected","reason":"too_few_points"}"#.into(),
    );
    assert!(window.get_error_dialog_visible());
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("boundary.error_too_few_after_delete"),
        "拒绝删除必须呈现明确提示"
    );
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        4,
        "拒绝删除不得改变边界点数"
    );
    window.invoke_error_dialog_dismissed();
    assert!(!window.get_error_dialog_visible());

    // 编辑后的顶点经 boundary_update 同步点数，confirm_boundary 使用编辑后的
    // 5 点坐标 → 边界确认并可导出。
    window.invoke_workspace_map_ipc(edited_boundary().into());
    assert_eq!(window.get_workspace_boundary_point_count(), 5);
    window.invoke_workspace_map_ipc(edited_confirm().into());
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(window.get_workspace_boundary_point_count(), 5);
    assert!(
        !window.get_workspace_boundary_edited_since_confirmed(),
        "确认后不得处于'编辑后待重新确认'状态（确认按钮置灰）"
    );

    // 已确认边界被再次编辑（拖拽顶点）→ 确认按钮重新可用（编辑后待重新确认）。
    window.invoke_workspace_map_ipc(edited_boundary().into());
    assert!(
        window.get_workspace_boundary_edited_since_confirmed(),
        "已确认边界再次编辑后必须进入'编辑后待重新确认'状态，确认按钮重新可用"
    );
    // 重新确认：仍走现有 confirm_boundary 链路，确认后回到已确认态。
    window.invoke_workspace_map_ipc(edited_confirm().into());
    assert!(window.get_workspace_boundary_is_determined());
    assert!(
        !window.get_workspace_boundary_edited_since_confirmed(),
        "重新确认后不得再处于'编辑后待重新确认'状态"
    );

    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        pump_until_terminal(window),
        OperationPresentationState::Succeeded,
        "编辑后的边界必须能够导出"
    );
    assert!(app
        .export_dir
        .join(format!("{}.schem", app.plan_id))
        .is_file());

    // 无效形状（自相交蝴蝶结）：错误弹窗 + 不确认 + 程序存活（T35 语义）。
    window.invoke_workspace_step_clicked(0);
    window.invoke_workspace_map_ipc(bowtie_confirm().into());
    assert!(window.get_error_dialog_visible(), "自相交必须弹错");
    assert!(
        window.get_error_dialog_body().as_str().contains("自相交"),
        "错误信息必须说明自相交：{}",
        window.get_error_dialog_body()
    );
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        5,
        "无效形状不得替换已确认的编辑后边界"
    );
    window.invoke_error_dialog_dismissed();
    assert!(!window.get_error_dialog_visible(), "程序必须存活可关闭弹窗");

    // 关闭弹窗后可恢复有效确认（T35 语义保持）。
    window.invoke_workspace_map_ipc(edited_confirm().into());
    assert!(window.get_workspace_boundary_is_determined());
}
