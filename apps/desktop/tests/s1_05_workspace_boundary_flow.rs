//! S1 工单 05 正式验收：方案工作区、步骤导航与边界流程全部经功能入口。
//!
//! 断言只观察用户可看到的页面、状态、通知与导航结果；边界闭合、有效性、
//! 重置与保存由工作区功能入口完成（B5），地图通道只转交原始动作（B3 IPC）。

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
fn s1_05_workspace_navigation_and_boundary_flow_through_functional_entry() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-05.db");
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
    let campus = injector
        .projects_mut()
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
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    assert_eq!(window.get_active_screen(), 2, "老用户着陆到方案列表");

    // ── 1. 打开方案：五个步骤顶部始终同时显示校区名与方案名 ──
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4, "单击方案卡片打开工作区");
    assert_eq!(window.get_workspace_campus_name().as_str(), "验收校区");
    assert_eq!(window.get_workspace_plan_name().as_str(), "验收方案");
    let context_label = window.get_workspace_context_label();
    assert!(
        context_label.as_str().contains("验收校区") && context_label.as_str().contains("验收方案"),
        "顶部上下文必须同时包含校区名与方案名：{context_label}"
    );
    assert!(
        window.get_workspace_tutorial_visible(),
        "首次进入显示步骤条气泡"
    );
    window.invoke_workspace_tutorial_dismiss_clicked();
    assert!(!window.get_workspace_tutorial_visible());

    // 步骤锁定状态由功能入口注入（S1 不自行判断）
    assert_eq!(window.get_workspace_step_locked().row_count(), 5);
    assert_eq!(window.get_workspace_step_locked().row_data(0), Some(false));
    assert_eq!(window.get_workspace_step_locked().row_data(1), Some(true));

    // ── 2. 步骤点击由功能入口返回决策：前跳上锁 → 条件不足，停留 ──
    window.invoke_workspace_step_clicked(2);
    assert_eq!(window.get_workspace_active_step(), 0, "未解锁步骤不得进入");
    assert!(!window.get_confirm_dialog_visible());

    // 缺少高德密钥进入边界 → 需要确认（前往设置）
    window.invoke_workspace_step_clicked(0);
    assert!(window.get_confirm_dialog_visible(), "缺密钥进入边界需确认");
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("settings.gaode_empty_key_title")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_active_screen(), 4, "取消后停留工作区");
    window.invoke_workspace_step_clicked(0);
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 3, "确认后前往设置页");

    // ── 3. 配置高德密钥后重新打开方案：加载地图立即显示处理中且不冻结 ──
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    assert_eq!(
        window.get_plan_list_model().row_count(),
        1,
        "已有正式数据仍可安全访问"
    );
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing,
        "需要加载地图时界面立即显示处理中状态"
    );
    // 地图加载中离开边界页 → 必须停留（功能入口判定）
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(
        window.get_active_screen(),
        4,
        "地图加载中离开边界页必须停留"
    );
    assert!(!window.get_confirm_dialog_visible());

    // ── 4. 高德地图故障只暂停地图相关操作 ──
    window.invoke_workspace_map_status_changed(false);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready
    );
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("boundary.map_load_failed")),
        "地图故障必须留底公告"
    );
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "地图故障后设置仍可访问");
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    assert_eq!(window.get_plan_list_model().row_count(), 1);
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    // ── 5. 边界闭合/有效性/重置/保存由功能入口完成（B5）──
    // 无效边界（共线 → 面积过小）：失败留在边界页，可继续修改
    window.invoke_workspace_boundary_canvas_clicked(0.0, 0.0);
    window.invoke_workspace_boundary_canvas_clicked(50.0, 50.0);
    window.invoke_workspace_boundary_canvas_clicked(100.0, 100.0);
    assert_eq!(window.get_workspace_boundary_point_count(), 3);
    window.invoke_workspace_boundary_confirm_clicked();
    assert!(window.get_error_dialog_visible(), "无效边界必须走错误弹窗");
    assert!(
        window
            .get_workspace_boundary_status()
            .as_str()
            .contains("已添加"),
        "失败后保留可继续修改的状态：{}",
        window.get_workspace_boundary_status()
    );
    assert_eq!(window.get_workspace_completed_steps(), 0);
    window.invoke_error_dialog_dismissed();

    // 重置
    window.invoke_workspace_boundary_reset_clicked();
    assert_eq!(window.get_workspace_boundary_point_count(), 0);
    assert!(!window.get_workspace_boundary_is_determined());

    // 合法边界（100m×100m 正方形）：闭合 + 保存 → 解锁下一步
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(
        window.get_workspace_boundary_status().as_str(),
        l10n.t("boundary.status_determined")
    );
    assert_eq!(window.get_workspace_completed_steps(), 1);
    assert_eq!(window.get_workspace_step_locked().row_data(1), Some(false));
    assert_eq!(
        window.get_workspace_step_locked().row_data(2),
        Some(false),
        "确认边界后采集与导出均可直接进入"
    );
    assert_eq!(
        window.get_workspace_step_completed().row_data(0),
        Some(true)
    );

    // 方案列表进度如实反映边界完成（数据结果不变）
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    let card = window.get_plan_list_model().row_data(0).expect("方案卡片");
    assert_eq!(
        card.progress_desc.as_str(),
        l10n.t("plan.progress_boundary_done")
    );
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());

    // ── 6. 地图通道只转交原始动作：confirm_boundary 由功能入口校验并保存 ──
    window.invoke_workspace_boundary_reset_clicked();
    let raw_confirm = r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#;
    window.invoke_workspace_map_ipc(raw_confirm.into());
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 1);

    // ── 7. 朝向门控：方位角提交后步骤③可进入 ──
    window.invoke_workspace_step_clicked(1);
    window.invoke_workspace_map_status_changed(false);
    window.set_workspace_orientation_mode("bearing-angle".into());
    window.set_workspace_orientation_input_text("90".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_workspace_orientation_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 2);
    window.invoke_workspace_step_clicked(2);
    assert_eq!(window.get_workspace_active_step(), 2, "允许进入采集步骤");
    assert_eq!(
        window.get_workspace_campus_name().as_str(),
        "验收校区",
        "步骤③顶部仍显示校区名"
    );
    assert_eq!(
        window.get_workspace_plan_name().as_str(),
        "验收方案",
        "步骤③顶部仍显示方案名"
    );

    // ── 8. 离开边界页由功能入口判定：未保存绘制需确认，确认后离开 ──
    window.invoke_workspace_step_clicked(0);
    window.invoke_workspace_map_status_changed(false);
    window.invoke_workspace_boundary_reset_clicked();
    window.invoke_workspace_boundary_canvas_clicked(10.0, 10.0);
    window.invoke_settings_toolbar_button_clicked();
    assert!(window.get_confirm_dialog_visible(), "未保存边界离开需确认");
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("workspace.leave_discard_title")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_active_screen(), 4, "取消后停留边界页");
    window.invoke_settings_toolbar_button_clicked();
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 3, "确认后离开边界页");
}
