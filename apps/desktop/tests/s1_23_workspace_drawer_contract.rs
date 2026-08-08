//! T34 定向契约测试：五步工作区"地图为主 + 左侧抽屉"（做法 A：地图让位）。
//!
//! 覆盖验收条目：
//! 1. 抽屉开合 → Slint 槽位上报的地图矩形跟随（x 右移/宽度收窄/恢复），
//!    且地图矩形与五步条、抽屉、左缘箭头互不相交（800×666/1000×666 逻辑）。
//! 2. 圈边界：地图 manual_point IPC（含 total）→ 抽屉 ① 点数/状态反馈；
//!    撤销/清空回传同步点数。
//! 3. 定朝向：抽屉 ② 手动角度输入覆盖已有朝向 → F5 重算确认（取消不落库/
//!    确认生效）；"确认两点朝向"按钮提交地图两点草稿。
//! 4. 旧 osm_elements 路径惰性（无 convertAndDraw 残留调用，不崩溃）。
//! 5. 弹窗遮挡统一机制的守卫与按模式恢复由 `map_webview` 单元测试覆盖
//!    （真实 WebView 创建依赖环境，不在本契约中断言 is_visible）。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;
use slint::Model;
use std::sync::Arc;

const DRAWER_WIDTH: f32 = 300.0;
const DRAWER_GAP: f32 = 12.0;
const SLOT_LEFT: f32 = 20.0;
const SLOT_TOP: f32 = 128.0;
const SLOT_RIGHT_MARGIN: f32 = 16.0;

fn logical_size(window: &AppWindow) -> (f32, f32) {
    let scale = window.window().scale_factor().max(0.001);
    let size = window.window().size();
    (size.width as f32 / scale, size.height as f32 / scale)
}

fn assert_close(actual: f32, expected: f32, message: &str) {
    assert!(
        (actual - expected).abs() < 0.5,
        "{message}：期望 {expected}，实际 {actual}"
    );
}

#[test]
fn s1_23_drawer_toggle_moves_map_slot_without_intersections() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-23.db");
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

    // 显式设定验收窗口尺寸（800×666 逻辑；软件后端下 set_size 直接生效）
    window
        .window()
        .set_size(slint::LogicalSize::new(800.0, 666.0));
    let (width, _height) = logical_size(&window);
    assert_close(width, 800.0, "800 逻辑宽生效");

    // ── 1. 初始收起：地图槽位紧贴左缘（20px 箭头条之后），不与五步条相交 ──
    assert!(!window.get_workspace_drawer_open(), "抽屉默认收起");
    assert_close(window.get_workspace_map_slot_x(), SLOT_LEFT, "收起时槽位 x");
    assert_close(window.get_workspace_map_slot_y(), SLOT_TOP, "槽位 y");
    let closed_width = window.get_workspace_map_slot_width();
    assert!(
        closed_width > 0.0,
        "800 宽下收起槽位宽必须 > 0：{closed_width}"
    );
    assert_close(
        window.get_workspace_map_slot_height(),
        (666.0 - SLOT_TOP - SLOT_RIGHT_MARGIN).max(0.0),
        "槽位高",
    );
    assert!(
        window.get_workspace_map_slot_y() >= 128.0,
        "地图矩形不得与五步条（y 64..128）相交"
    );

    // ── 2. 展开抽屉：地图右移让位（做法 A），宽度收窄，与抽屉互不相交 ──
    window.invoke_workspace_drawer_toggle_clicked();
    assert!(window.get_workspace_drawer_open(), "抽屉展开");
    let open_x = window.get_workspace_map_slot_x();
    assert_close(
        open_x,
        SLOT_LEFT + DRAWER_WIDTH + DRAWER_GAP,
        "展开时槽位 x（地图右移让位）",
    );
    let open_width = window.get_workspace_map_slot_width();
    assert_close(
        open_width,
        (closed_width - DRAWER_WIDTH - DRAWER_GAP).max(0.0),
        "展开时槽位宽 = 收起宽 − 抽屉让位",
    );
    assert!(open_width < closed_width, "抽屉展开后地图必须收窄让位");
    // 抽屉占 20..(20+300)，地图从 332 开始，间隙 12px，互不相交
    assert!(
        open_x >= SLOT_LEFT + DRAWER_WIDTH + DRAWER_GAP,
        "地图与左侧抽屉不得相交"
    );

    // ── 3. 窗口改为 1000×666 逻辑：槽位按新宽度跟随 ──
    window
        .window()
        .set_size(slint::LogicalSize::new(1000.0, 666.0));
    let (width1000, height1000) = logical_size(&window);
    assert_close(width1000, 1000.0, "1000 逻辑宽生效");
    assert_close(height1000, 666.0, "666 逻辑高生效");
    let open_width_1000 = window.get_workspace_map_slot_width();
    assert_close(
        open_width_1000,
        (open_width + 200.0).max(0.0),
        "1000 宽下展开槽位宽 = 800 宽 + 200",
    );
    assert_close(
        window.get_workspace_map_slot_height(),
        (666.0 - SLOT_TOP - SLOT_RIGHT_MARGIN).max(0.0),
        "666 高下槽位高",
    );

    // ── 4. 收回抽屉：地图恢复原宽 ──
    window.invoke_workspace_drawer_toggle_clicked();
    assert!(!window.get_workspace_drawer_open(), "抽屉收起");
    assert_close(
        window.get_workspace_map_slot_x(),
        SLOT_LEFT,
        "收起后槽位 x 恢复",
    );
    assert_close(
        window.get_workspace_map_slot_width(),
        (1000.0 - SLOT_LEFT - SLOT_RIGHT_MARGIN).max(0.0),
        "收起后槽位宽恢复",
    );

    // ── 5. 边界抽屉 ①：地图 manual_point 回传 → 点数/状态反馈 ──
    window.invoke_workspace_map_ipc(
        r#"{"type":"manual_point","point":[116.40,39.90],"total":4}"#.into(),
    );
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        4,
        "点数跟随 total"
    );
    assert!(
        window.get_workspace_boundary_points_label().contains("4"),
        "点数文案包含总数：{}",
        window.get_workspace_boundary_points_label()
    );
    assert!(
        window.get_workspace_boundary_status().contains("已添加 4"),
        "状态反馈点数：{}",
        window.get_workspace_boundary_status()
    );
    window.invoke_workspace_map_ipc(r#"{"type":"manual_cancel"}"#.into());
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        3,
        "撤销同步点数"
    );
    window.invoke_workspace_map_ipc(r#"{"type":"manual_clear"}"#.into());
    assert_eq!(
        window.get_workspace_boundary_point_count(),
        0,
        "清空复位点数"
    );

    // ── 6. 完成边界（经地图 IPC，确定性路径）──
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
            .into(),
    );
    assert!(window.get_workspace_boundary_is_determined());

    // ── 7. 朝向抽屉 ②：手动角度覆盖 → F5 重算确认；取消不落库、确认生效 ──
    window.invoke_workspace_step_clicked(1);
    assert_eq!(window.get_workspace_active_step(), 1);
    window.invoke_workspace_map_status_changed(true);
    window.set_workspace_orientation_input_text("90".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(
        window.get_workspace_orientation_is_determined(),
        "首次设定直接保存"
    );
    assert!(window
        .get_workspace_orientation_angle_display()
        .as_str()
        .contains("90.0"));

    // 手动覆盖已有朝向：必须弹重算确认（沿用 F5 规则）
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
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "取消后仍为旧朝向"
    );
    window.set_workspace_orientation_input_text("180".into());
    window.invoke_workspace_orientation_submit_clicked();
    window.invoke_confirm_dialog_confirmed();
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("180.0"),
        "确认后新朝向生效"
    );

    // ── 8. "确认两点朝向"按钮：提交地图两点草稿（覆盖仍走重算确认）──
    let two_points =
        r#"{"type":"orientation_points","points":[[116.3975,39.9160],[116.3985,39.9160]]}"#;
    window.invoke_workspace_map_ipc(two_points.into());
    assert_eq!(window.get_workspace_orientation_points().row_count(), 2);
    window.invoke_workspace_orientation_confirm_two_points_clicked();
    assert!(
        window.get_confirm_dialog_visible(),
        "两点确认覆盖已有朝向仍需重算确认"
    );
    window.invoke_confirm_dialog_confirmed();
    assert!(
        window
            .get_workspace_orientation_angle_display()
            .as_str()
            .contains("90.0"),
        "两点朝向生效"
    );

    // ── 9. 旧 osm_elements 路径惰性：不崩溃、不改状态（无 convertAndDraw 残留）──
    let before = window.get_workspace_orientation_angle_display().to_string();
    window.invoke_workspace_map_ipc(r#"{"type":"osm_elements","elements":[]}"#.into());
    assert_eq!(
        window.get_workspace_orientation_angle_display().as_str(),
        before,
        "旧 osm_elements 载荷不得改变工作区状态"
    );
    assert_eq!(window.get_active_screen(), 4);
}
