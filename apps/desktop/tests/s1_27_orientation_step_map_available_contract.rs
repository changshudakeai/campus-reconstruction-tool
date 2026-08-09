//! T36 正式回归（b）：步骤切换 navigate(1) 后 map_available 如实反映 WebView
//! 创建结果；页面 onerror / 5s SDK 超时（及 Rust 侧 10s 加载超时标记）→
//! 明确错误对话框，不静默；地图不可用后仍可用"方位角手动输入"完成朝向。
//!
//! - WebView 创建前失败（HTML 构建）如实上报 false 的确定性断言在
//!   `map_webview::tests::creation_failure_reports_map_unavailable_immediately`
//!   （非法密钥同步失败，无事件循环依赖）；本契约锁定状态通道到 UI 的链路：
//!   status=false → map_available=false、处理中清除、明确提示。
//! - 错误 IPC（SDK 失败/超时标记）→ 错误弹窗 + 地图不可用 + 手动输入兜底。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;
use std::sync::Arc;

#[test]
fn s1_27_orientation_step_map_available_and_error_contract() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-27.db");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接库"))
            .expect("正式注入器");
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
        .create_campus("验收校区")
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
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // ── 1. 打开方案并完成边界（朝向步骤前置条件）──
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 1);

    // ── 2. 步骤切换 navigate(1)：请求后先进入"地图加载中"状态 ──
    window.invoke_workspace_step_clicked(1);
    assert_eq!(window.get_workspace_active_step(), 1);
    assert!(
        window.get_workspace_map_loading(),
        "navigate(1) 后必须进入地图加载中状态（map_processing）"
    );
    assert_eq!(
        window.get_workspace_step_locked().row_data(2),
        Some(false),
        "边界确认后朝向步骤必须解锁"
    );

    // ── 3. 创建失败如实上报：status(false) → map_available=false、处理中清除 ──
    window.invoke_workspace_map_status_changed(false);
    assert!(
        !window.get_workspace_map_available(),
        "创建失败不得保持 map_available=true"
    );
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "状态回传后处理中必须清除"
    );
    assert!(
        !window.get_workspace_map_loading(),
        "状态回传后加载中必须清除"
    );
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("boundary.map_load_failed")),
        "地图创建失败必须留底明确提示"
    );

    // ── 4. 页面 onerror / 5s SDK 超时 → 明确错误对话框，不静默 ──
    window.invoke_workspace_map_ipc(r#"{"type":"error","message":"SDK 加载超时"}"#.into());
    assert!(
        window.get_error_dialog_visible(),
        "SDK 加载失败必须弹明确错误对话框"
    );
    assert!(
        window
            .get_error_dialog_body()
            .as_str()
            .contains("SDK 加载超时"),
        "错误正文必须透传页面错误：{}",
        window.get_error_dialog_body()
    );
    assert!(
        !window.get_workspace_map_available(),
        "错误后地图必须如实不可用"
    );
    window.invoke_error_dialog_dismissed();
    assert!(!window.get_error_dialog_visible());
    assert!(
        !window.get_workspace_map_available(),
        "失败后弹窗关闭不得自动重建为可用"
    );

    // ── 5. Rust 侧 10s 加载超时标记 → 本地化超时错误对话框 ──
    let timeout_payload = format!(
        r#"{{"type":"error","message":"{}"}}"#,
        desktop_shell::MAP_LOAD_TIMEOUT_MARKER
    );
    window.invoke_workspace_map_ipc(timeout_payload.into());
    assert!(
        window.get_error_dialog_visible(),
        "加载超时必须弹明确错误对话框"
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("map.load_timeout_body"),
        "超时标记必须本地化为明确超时文案"
    );
    window.invoke_error_dialog_dismissed();

    // ── 6. 地图失败后仍可用"方位角手动输入"完成朝向（兜底入口）──
    window.set_workspace_orientation_input_text("270".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(
        !window.get_confirm_dialog_visible(),
        "首次设定朝向直接保存，不弹重算确认"
    );
    assert!(window.get_workspace_orientation_is_determined());
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("270.0"),
        "手动输入角度必须生效：{}",
        window.get_workspace_orientation_angle_display()
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // ── 7. 显式重新进入步骤②会清除失败态、重新请求地图 ──
    window.invoke_workspace_step_clicked(0);
    window.invoke_workspace_step_clicked(1);
    assert_eq!(window.get_workspace_active_step(), 1);
    assert!(
        window.get_workspace_map_available(),
        "重新进入步骤后重新请求地图（待创建）"
    );
    assert!(
        window.get_workspace_map_loading(),
        "重新请求后回到地图加载中状态"
    );
}
