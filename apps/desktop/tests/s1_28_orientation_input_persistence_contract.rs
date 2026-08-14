//! T40 回归（a）：朝向角度输入框的呈现持久性——键入后任意呈现（map_status、
//! orientation_points IPC、步骤切换）都不得重置文本；输入值只活在窗口，
//! 提交时才读取；程序性清空只发生在"重置/清除/切换方案"显式入口。
//!
//! T40 回归（b）：步骤② WebView 创建失败 → 明确错误提示 + 地图如实不可用，
//! 仍可退回"方位角手动输入"完成朝向；切换方案是输入框显式清空入口。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::Model;
use std::sync::Arc;

const TWO_POINTS_IPC: &str =
    r#"{"type":"orientation_points","points":[[116.3975,39.9160],[116.3985,39.9160]]}"#;

struct Harness {
    window: AppWindow,
    center: Arc<NotificationCenter>,
    l10n: Localization,
    campus_id: String,
    plan_a: String,
    plan_b: String,
}

fn setup() -> Harness {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-28.db");
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
    let plan_a = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    let plan_b = injector
        .projects_mut()
        .create_plan(&campus_id, "切换方案")
        .expect("创建方案 B");
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
    Harness {
        window,
        center,
        l10n,
        campus_id: campus_id.to_string(),
        plan_a: plan_a.to_string(),
        plan_b: plan_b.to_string(),
    }
}

fn open_plan_boundary_and_step_2(harness: &Harness, plan_id: &str) {
    harness
        .window
        .invoke_plan_list_card_clicked(plan_id.to_string().into());
    harness.window.invoke_workspace_tutorial_dismiss_clicked();
    harness.window.invoke_workspace_map_status_changed(true);
    harness.window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
            .into(),
    );
    assert!(harness.window.get_workspace_boundary_is_determined());
    harness.window.invoke_workspace_step_clicked(1);
    assert_eq!(harness.window.get_workspace_active_step(), 1);
}

#[test]
fn s1_28_orientation_input_persistence_fallback_and_clear_contract() {
    // 单一 AppWindow（winit 事件循环不可在同一进程重建）：一个流程锁死
    // 键入后任意呈现不重置、重置清空、创建失败后手动输入兜底、切方案清空。
    let harness = setup();
    let window = &harness.window;
    open_plan_boundary_and_step_2(&harness, &harness.plan_a);

    // 键入后任意呈现不得重置文本。
    window.set_workspace_orientation_input_text("123".into());
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "123",
        "键入值必须保留在输入框"
    );

    // 呈现 1：map_status（地图可用/不可用状态回传都会触发完整呈现）。
    window.invoke_workspace_map_status_changed(true);
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "123",
        "map_status 呈现不得重置输入框"
    );
    window.invoke_workspace_map_status_changed(false);
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "123",
        "地图不可用回传也不得重置输入框"
    );

    // 呈现 2：orientation_points IPC（两点回填抽屉角度/参考线）。
    window.invoke_workspace_map_ipc(TWO_POINTS_IPC.into());
    assert_eq!(
        window.get_workspace_orientation_points().row_count(),
        2,
        "两点必须回填到抽屉"
    );
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        format!("{:.1}", window.get_workspace_orientation_angle()),
        "orientation_points 必须把计算角度回填进输入框"
    );

    // 呈现 3：步骤切换（切到边界步再切回朝向步）。
    let filled_angle = format!("{:.1}", window.get_workspace_orientation_angle());
    window.invoke_workspace_step_clicked(0);
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        filled_angle.as_str(),
        "切到边界步不得重置输入框"
    );
    window.invoke_workspace_step_clicked(1);
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        filled_angle.as_str(),
        "切回朝向步不得重置输入框"
    );

    // 显式清空入口：抽屉"重置"。
    window.invoke_workspace_orientation_reset_clicked();
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "",
        "重置是输入框的显式清空入口"
    );

    // 步骤② WebView 创建失败 → 明确提示 + 地图如实不可用，不呈现无声空白地图。
    window.invoke_workspace_map_status_changed(false);
    assert!(
        !window.get_workspace_map_available(),
        "创建失败必须如实上报地图不可用"
    );
    assert!(
        harness
            .center
            .board_snapshot()
            .iter()
            .any(|record| record.body == harness.l10n.t("boundary.map_load_failed")),
        "创建失败必须留底明确提示"
    );

    // 仍可退回"方位角手动输入"完成朝向（不依赖地图）。
    window.set_workspace_orientation_input_text("270".into());
    window.invoke_workspace_orientation_submit_clicked();
    assert!(
        !window.get_confirm_dialog_visible(),
        "首次设定朝向直接保存，不弹重算确认"
    );
    assert!(window.get_workspace_orientation_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // 切换方案是输入框显式清空入口。
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(harness.campus_id.clone().into());
    assert_eq!(window.get_plan_list_model().row_count(), 2);
    window.invoke_plan_list_card_clicked(harness.plan_b.clone().into());
    assert_eq!(window.get_active_screen(), 4);
    assert_eq!(
        window.get_workspace_orientation_input_text().as_str(),
        "",
        "切换方案必须显式清空朝向输入框"
    );
}
