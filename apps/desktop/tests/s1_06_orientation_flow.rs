//! S1 工单 06 正式验收：朝向流程完整迁移到功能入口。
//!
//! 断言只观察用户可见的页面、状态、通知与导航结果：地图两点参考线只经
//! 通用地图 IPC 转交，角度计算在 F5 完成；确认/取消/重置/保存后的页面
//! 状态由功能入口完整返回；保存或计算失败时正式状态保持不变并显示明确
//! 错误；地图故障只暂停地图相关操作，不丢失已保存边界与方案数据。

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
fn s1_06_orientation_flow_through_functional_entry() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-06.db");
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
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // ── 1. 打开方案并完成边界（朝向步骤的前置条件）──
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    assert!(window.get_workspace_boundary_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 1);

    // ── 2. 地图两点参考线：IPC 转交 → F5 计算 → 路径/箭头/角度回填 ──
    window.invoke_workspace_step_clicked(1);
    assert_eq!(window.get_workspace_active_step(), 1);
    window.invoke_workspace_map_status_changed(true);
    let two_points =
        r#"{"type":"orientation_points","points":[[116.3975,39.9160],[116.3985,39.9160]]}"#;
    window.invoke_workspace_map_ipc(two_points.into());
    assert_eq!(
        window.get_workspace_orientation_points().row_count(),
        2,
        "两点必须回填到页面"
    );
    assert!(
        !window.get_workspace_orientation_path_commands().is_empty(),
        "参考线路径必须回填"
    );
    assert!(
        !window.get_workspace_orientation_arrow_commands().is_empty(),
        "方向箭头必须回填"
    );
    assert!(window.get_workspace_orientation_angle() >= 0.0);
    assert_eq!(
        window.get_workspace_orientation_status().as_str(),
        l10n.t("orientation.status_calculated")
    );
    assert!(
        !window.get_workspace_orientation_is_determined(),
        "未确认前不得保存"
    );
    assert_eq!(
        window.get_workspace_completed_steps(),
        1,
        "未保存不改变边界直出资格"
    );
    assert_eq!(window.get_workspace_step_locked().row_data(2), Some(false));

    // ── 3. 地图确认朝向：首次设定直接保存（无重算确认）──
    let confirm_two_points =
        r#"{"type":"confirm_orientation","points":[[116.3975,39.9160],[116.3985,39.9160]]}"#;
    window.invoke_workspace_map_ipc(confirm_two_points.into());
    assert!(!window.get_confirm_dialog_visible(), "首次设定不弹重算确认");
    assert!(window.get_workspace_orientation_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 2);
    assert_eq!(window.get_workspace_step_locked().row_data(2), Some(false));
    assert_eq!(
        window.get_workspace_step_completed().row_data(1),
        Some(true)
    );
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "角度显示由入口回填"
    );

    // ── 4. 修改已有朝向：返回影响说明与确认请求，取消不落库 ──
    window.invoke_workspace_orientation_mode_changed("bearing-angle".into());
    window.set_workspace_orientation_input_text("180".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_confirm_dialog_visible(), "覆盖既有朝向必须确认");
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("orientation.recalc_title")
    );
    assert!(
        window
            .get_confirm_dialog_body()
            .as_str()
            .contains(&l10n.t("collection.orientation_recalc_notice")),
        "确认窗必须包含重算影响说明"
    );
    assert!(
        window.get_workspace_orientation_is_determined(),
        "确认前正式状态保持已确认"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2, "确认前步数不变");
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "确认前仍显示旧朝向"
    );
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    assert_eq!(window.get_workspace_completed_steps(), 2, "取消后不落库");
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "取消后仍为旧朝向"
    );
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "180",
        "取消后保留页面临时输入"
    );

    // 再次提交并确认：新朝向生效
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_confirm_dialog_visible());
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    assert_eq!(window.get_workspace_completed_steps(), 2);
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("180.0"),
        "确认后应用新朝向"
    );

    // ── 5. 保存/计算失败：正式状态保持不变并显示明确错误 ──
    window.set_workspace_orientation_input_text("abc".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_error_dialog_visible(), "无效数字必须错误弹窗");
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("orientation.error_invalid_angle")
    );
    assert!(
        window.get_workspace_orientation_is_determined(),
        "失败后正式状态不变"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);
    window.invoke_error_dialog_dismissed();

    window.set_workspace_orientation_input_text("NaN".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_error_dialog_visible(), "非有限角度必须错误弹窗");
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("orientation.error_angle_out_of_range")
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);
    assert!(
        window.get_workspace_orientation_is_determined(),
        "非有限角度失败后正式状态不变"
    );
    window.invoke_error_dialog_dismissed();

    // 两点重合：计算失败只影响朝向操作
    window.invoke_workspace_orientation_mode_changed("two-points".into());
    let coincident =
        r#"{"type":"orientation_points","points":[[116.3975,39.9160],[116.3975,39.9160]]}"#;
    window.invoke_workspace_map_ipc(coincident.into());
    assert!(window.get_error_dialog_visible(), "重合两点必须错误弹窗");
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("orientation.error_coincident_points")
    );
    assert_eq!(
        window.get_workspace_orientation_points().row_count(),
        0,
        "失败后不得回填两点"
    );
    assert!(
        window.get_workspace_orientation_is_determined(),
        "计算失败不影响已保存朝向"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);
    window.invoke_error_dialog_dismissed();

    // ── 6. 地图“清除重来”只清草稿，不清已保存的正式状态 ──
    window.invoke_workspace_map_ipc(two_points.into());
    assert_eq!(window.get_workspace_orientation_points().row_count(), 2);
    window.invoke_workspace_map_ipc(r#"{"type":"orientation_clear"}"#.into());
    assert_eq!(
        window.get_workspace_orientation_points().row_count(),
        0,
        "清除草稿点"
    );
    assert!(
        window.get_workspace_orientation_is_determined(),
        "清除草稿不得清除已保存朝向"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2, "步数不得回退");
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("180.0"),
        "已保存角度仍在"
    );

    // ── 6b. 地图两点覆盖已有朝向：确认窗确认后生效，取消不落库 ──
    window.invoke_workspace_map_ipc(two_points.into());
    assert_eq!(window.get_workspace_orientation_points().row_count(), 2);
    window.invoke_workspace_map_ipc(confirm_two_points.into());
    assert!(
        window.get_confirm_dialog_visible(),
        "地图确认覆盖既有朝向必须弹重算确认窗"
    );
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("180.0"),
        "取消后仍为旧朝向"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);

    window.invoke_workspace_map_ipc(confirm_two_points.into());
    assert!(
        window.get_confirm_dialog_visible(),
        "再次确认覆盖仍须弹重算确认窗"
    );
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "确认后地图两点新朝向生效"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // ── 7. 地图故障只暂停地图相关操作：已保存数据不丢失 ──
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
    assert!(
        window.get_workspace_boundary_is_determined(),
        "地图故障不丢已保存边界"
    );
    assert_eq!(
        window.get_workspace_completed_steps(),
        2,
        "地图故障不丢已保存朝向"
    );
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "地图故障后设置仍可访问");
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(campus_id.to_string().into());
    assert_eq!(
        window.get_plan_list_model().row_count(),
        1,
        "方案列表仍可访问"
    );
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    // 地图故障下方位角手动输入仍可保存（不依赖地图）
    window.invoke_workspace_step_clicked(1);
    window.invoke_workspace_orientation_mode_changed("bearing-angle".into());
    window.set_workspace_orientation_input_text("270".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(window.get_confirm_dialog_visible(), "覆盖已有朝向仍须确认");
    window.invoke_confirm_dialog_confirmed();
    assert!(window
        .get_workspace_orientation_angle_display()
        .as_str()
        .contains("270.0"));
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // 越界输入按 F5 已确认语义归一化（400° → 40°），仍走覆盖确认
    window.set_workspace_orientation_input_text("400".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(
        window.get_confirm_dialog_visible(),
        "归一化后覆盖已有朝向仍须确认"
    );
    window.invoke_confirm_dialog_confirmed();
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("40.0"),
        "400° 按 F5 语义归一化为 40°"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // ── 8. 重置：页面状态完整返回，正式状态清除并重新锁住下一步 ──
    window.invoke_workspace_orientation_reset_clicked();
    assert_eq!(window.get_workspace_orientation_points().row_count(), 0);
    assert_eq!(window.get_workspace_orientation_angle(), -1.0);
    assert!(!window.get_workspace_orientation_is_determined());
    assert_eq!(
        window.get_workspace_orientation_angle_display().as_str(),
        ""
    );
    assert_eq!(window.get_workspace_orientation_input_text().as_str(), "");
    assert_eq!(
        window.get_workspace_completed_steps(),
        1,
        "重置后朝向未完成"
    );
    assert_eq!(
        window.get_workspace_step_locked().row_data(2),
        Some(false),
        "朝向重置不应重新锁住边界直出"
    );
}
