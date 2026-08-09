//! T35 P0 崩溃修复契约（a）：无效边界确认 → B7 错误弹窗可见、边界保持
//! 未确认、程序不退出（T19B-5B 破坏性验收项：画自相交多边形 → 弹窗报错
//! 而非崩溃）。
//!
//! 崩溃根因（修复前）：抽屉"确认边界"经 wry IPC 回调（WebView2 COM 栈）
//! 进入工作区入口；自相交校验失败发布 error 通知 → `ShellPresenter::present`
//! → `map_webview::hide()` 在 IPC 回调栈内同步 drop WebView → WebView2 COM
//! 重入崩溃（WER 0xc0000005 combase.dll / 0xc0000409）。
//!
//! 修复后：`hide()` 是唯一延迟销毁入口（`invoke_from_event_loop` 下一拍
//! drop），弹窗路径不再在回调栈内销毁 WebView。本契约经公开 Slint seam
//! 注入自相交蝴蝶结边界，断言错误弹窗可见、边界保持未确认、程序继续存活
//! （不退出），关闭弹窗后仍可继续操作并成功确认有效边界。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use notification_center::{NotificationCenter, NotificationLevel, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;
use std::sync::Arc;

/// 自相交蝴蝶结（GCJ-02）：边 (0)-(1) 与边 (2)-(3) 在中点交叉。
const BOWTIE_CONFIRM: &str = r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.50,31.30],[121.40,31.30],[121.50,31.20]]}"#;

/// 有效矩形（GCJ-02）。
const VALID_CONFIRM: &str = r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.50,31.20],[121.50,31.30],[121.40,31.30]]}"#;

#[test]
fn s1_24_self_intersecting_boundary_shows_error_modal_and_does_not_exit() {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-24.db");
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

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    assert_eq!(window.get_active_screen(), 4);
    assert_eq!(window.get_workspace_active_step(), 0);

    // ── 1. 自相交蝴蝶结确认：必须走错误弹窗而非崩溃 ──
    window.invoke_workspace_map_ipc(BOWTIE_CONFIRM.into());

    assert!(
        window.get_error_dialog_visible(),
        "自相交边界必须弹出 B7 错误弹窗（修复前此处会崩溃退出）"
    );
    assert!(
        !window.get_workspace_boundary_is_determined(),
        "无效边界不得进入已确认状态"
    );
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Failed,
        "无效边界确认必须呈现失败状态"
    );
    let body = window.get_error_dialog_body().to_string();
    assert!(
        body.contains("自相交"),
        "错误弹窗必须透传 B5 校验事实（自相交），实际正文：{body}"
    );
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|n| n.level == NotificationLevel::Error && n.body.contains("自相交")),
        "校验失败必须按 ADR-0021 经 B7 留底进公告栏（禁止静默吞掉）"
    );

    // ── 2. 程序未退出：继续操作——关闭弹窗后状态保持、可重试 ──
    window.invoke_error_dialog_dismissed();
    assert!(
        !window.get_error_dialog_visible(),
        "点'知道了'后错误弹窗必须关闭"
    );
    assert!(
        !window.get_workspace_boundary_is_determined(),
        "关闭错误弹窗不改变边界未确认状态"
    );

    // ── 3. 同一会话内有效边界确认仍成功（T33 保留路径）──
    window.invoke_workspace_map_ipc(VALID_CONFIRM.into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "关闭错误弹窗后有效边界必须可确认（程序未因崩溃而退出）"
    );
    let locked: Vec<bool> = (0..5)
        .map(|i| window.get_workspace_step_locked().row_data(i).unwrap())
        .collect();
    assert!(
        locked.iter().all(|locked| !*locked),
        "确认边界后五步必须全部解锁：step_locked={locked:?}",
    );
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "有效边界确认后回到就绪状态"
    );
}
