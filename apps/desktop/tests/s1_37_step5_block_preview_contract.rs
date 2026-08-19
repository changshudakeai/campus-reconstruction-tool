//! T52 S1 契约验收：第五步 3D 方块预览按需生成、与导出衔接、不改变
//! 第一/二/三/四步地图现场（ADR-0045）。
//!
//! 仓库惯例：一个测试二进制只创建一次 winit 事件循环，因此本文件合并为
//! 一个 `#[test]`，按顺序覆盖三个验收场景。

use std::sync::Arc;
use std::time::{Duration, Instant};

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, current_page_kind_name, preview_payload, set_webview_creation_probe,
    AppWindow, ApplicationRuntime, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::{ComponentHandle, Model};

/// 重复泵送事件循环直到条件满足或超时（后台线程结果经事件循环回调到达）。
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
                slint::quit_event_loop().expect("停止测试事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行测试事件循环");
}

/// 组装带已确认边界与导出目录的正式应用（与 s1_08 同构）。
fn assemble_with_confirmed_boundary(
    directory: &tempfile::TempDir,
) -> (
    ApplicationRuntime,
    AppWindow,
    Arc<NotificationCenter>,
    String,
) {
    set_webview_creation_probe(true);
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let database_path = directory.path().join("t52-s1.db");
    let export_dir = directory.path().join("exports");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接组"))
            .expect("正式注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("临时路径有效"))
        .expect("设置临时导出目录");
    // 测试密钥：使边界/朝向步骤的地图现场可恢复（T52 只改第五步呈现）。
    injector
        .settings_mut()
        .set_gaode_api_key("testapikey1234567890")
        .expect("设置测试密钥");
    injector
        .settings_mut()
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("设置测试安全码");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("T52 校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "T52 边界直出")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    let raw_confirm = r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#;
    window.invoke_workspace_map_ipc(raw_confirm.into());
    assert!(window.get_workspace_boundary_is_determined());
    (runtime, window, center, plan_id.to_string())
}

#[test]
fn s1_37_step5_block_preview_contract() {
    let directory = tempfile::tempdir().expect("临时目录");
    let (_runtime, window, _center, plan_id) = assemble_with_confirmed_boundary(&directory);

    // ── 1. 进入第五步不自动生成；点击按钮后才生成 ──
    window.invoke_workspace_step_clicked(4);
    assert_eq!(window.get_workspace_active_step(), 4);
    assert_eq!(
        current_page_kind_name(),
        Some("block_preview"),
        "第五步必须显示 3D 预览页"
    );
    assert!(preview_payload().is_none(), "进入第五步不得自动生成预览");
    assert_eq!(window.get_workspace_export_preview_status().as_str(), "");
    assert!(!window.get_workspace_export_preview_has_content());
    assert_eq!(
        window
            .get_workspace_export_preview_candidate_titles()
            .row_count(),
        0,
        "边界直出（无保留候选）时抽屉不显示候选卡片"
    );
    // 无候选时点击定位：入口存在且不产生副作用（不崩、不生成）。
    window.invoke_workspace_export_preview_locate_clicked(0);
    assert!(
        !window.get_workspace_export_preview_has_content(),
        "无候选定位不得触发预览生成"
    );

    window.invoke_workspace_export_preview_generate_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing,
        "生成预览必须后台执行，不冻结界面"
    );
    pump_until(&window, Duration::from_secs(30), |window| {
        window.get_workspace_export_preview_has_content()
    });

    let payload = preview_payload().expect("生成完成后必须有预览负载");
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("合法 JSON");
    assert!(
        parsed["palette"]
            .as_array()
            .expect("调色板数组")
            .iter()
            .any(|block| block == "minecraft:stone_bricks"),
        "平整场地预览必须使用与导出同源的 stone_bricks"
    );
    let status = window.get_workspace_export_preview_status().to_string();
    assert!(
        status.contains("方块"),
        "已生成状态文案应包含方块数：{status}"
    );
    assert_eq!(current_page_kind_name(), Some("block_preview"));

    // ── 2. 导出衔接：完成后同区域继续显示预览 + 结果状态 ──
    let payload_before_export = payload;
    window.invoke_workspace_export_start_clicked();
    pump_until(&window, Duration::from_secs(30), |window| {
        window.get_operation_state() == OperationPresentationState::Succeeded
    });
    assert_eq!(
        window.get_workspace_placeholder_title().as_str(),
        Localization::new(Language::ZhCn)
            .expect("zh-CN")
            .t("export.done"),
        "导出完成后必须显示导出结果状态"
    );
    assert_eq!(
        current_page_kind_name(),
        Some("block_preview"),
        "导出完成后同区域必须继续显示 3D 预览"
    );
    assert!(
        window.get_workspace_export_preview_has_content(),
        "导出不得清掉已生成的预览内容"
    );
    assert_eq!(
        preview_payload(),
        Some(payload_before_export),
        "导出流程不得改写预览负载"
    );
    assert!(directory
        .path()
        .join("exports")
        .join(format!("{plan_id}.schem"))
        .is_file());

    // ── 3. 其他步骤地图现场不变；回到第五步预览负载保留 ──
    window.invoke_workspace_step_clicked(0);
    assert_eq!(
        current_page_kind_name(),
        Some("boundary"),
        "步骤①必须恢复边界地图页"
    );
    window.invoke_workspace_step_clicked(1);
    assert_eq!(
        current_page_kind_name(),
        Some("orientation"),
        "步骤②必须恢复朝向地图页"
    );
    window.invoke_workspace_step_clicked(2);
    assert_eq!(
        current_page_kind_name(),
        Some("boundary"),
        "步骤③必须恢复边界让位页"
    );
    window.invoke_workspace_step_clicked(4);
    assert_eq!(current_page_kind_name(), Some("block_preview"));
    assert!(
        preview_payload().is_some(),
        "同方案内预览负载保留，不重新生成"
    );
}
