//! S1 工单 04 正式验收：校区搜索、最近记录、方案列表 CRUD 与回收站全部
//! 经方案管理入口完成（独立进程，Slint 平台单线程约束集中在单个 #[test]）。
//!
//! 断言只观察用户可看到的页面、模型、通知与导航结果，不读取功能模块内部状态；
//! 正式数据经 F1/F3 公开接口读写，S1 不直接打开数据库。

use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use slint::Model;
use std::sync::Arc;

#[test]
fn s1_04_campus_plan_trash_flow_through_plan_management_entry() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-04.db");
    let injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接组"))
            .expect("正式注入器");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // ── 1. 首跑完成 → 校区选择页；无记录时直接显示搜索框 ──
    window.set_wizard_acknowledged(true);
    window.invoke_wizard_continue_clicked();
    assert_eq!(window.get_active_screen(), 1, "首跑完成后进入校区选择页");
    assert_eq!(
        window.get_campus_select_model().row_count(),
        0,
        "没有最近记录时不显示列表，直接显示搜索框"
    );
    // 准备正式数据：两个带地址的校区（最近进入的排最前）
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    let first = settings
        .select_campus_with_anchor(
            "华东师范大学(普陀校区)",
            "B01",
            "中山北路3663号",
            121.406,
            31.228,
        )
        .expect("创建校区一");
    let second = settings
        .select_campus_with_anchor("复旦大学(邯郸校区)", "B02", "邯郸路220号", 121.505, 31.296)
        .expect("创建校区二");

    // 刷新校区选择页 → 最近使用记录（名称 + 地址，最近进入排最前）
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 1);
    assert_eq!(window.get_campus_select_model().row_count(), 2);
    let first_row = window
        .get_campus_select_model()
        .row_data(1)
        .expect("第一行校区");
    assert_eq!(first_row.name.as_str(), "华东师范大学(普陀校区)");
    assert_eq!(
        first_row.address.as_str(),
        "中山北路3663号",
        "最近记录卡片显示地址"
    );
    let second_row = window
        .get_campus_select_model()
        .row_data(0)
        .expect("最新校区行");
    assert_eq!(
        second_row.name.as_str(),
        "复旦大学(邯郸校区)",
        "最近进入的排最前"
    );

    // ── 2. 搜索只在点击"搜索"或按回车时开始 ──
    window.set_campus_search_text("华东师范".into());
    assert!(!window.get_campus_show_results(), "输入期间不自动搜索");
    assert_eq!(window.get_campus_search_results_model().row_count(), 0);
    window.invoke_campus_search_requested();
    assert!(window.get_campus_show_results(), "点击搜索后切换为搜索结果");
    assert_eq!(window.get_campus_search_results_model().row_count(), 1);
    let result = window
        .get_campus_search_results_model()
        .row_data(0)
        .expect("搜索结果");
    assert_eq!(result.name.as_str(), "华东师范大学(普陀校区)");
    assert_eq!(result.address.as_str(), "中山北路3663号");

    // ── 3. 选择重复校区 → 直接进入原校区方案页 + 通知事实 ──
    window.invoke_campus_select_campus_clicked(first.to_string().into());
    assert_eq!(window.get_active_screen(), 2, "重复校区直接进入方案列表");
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("campus.already_added")),
        "重复校区必须产生“该校区已添加，已为你切换”通知事实"
    );

    // ── 4. 最近记录小叉：立即移除、不弹确认、通知事实 ──
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_remove_recent_clicked(second.to_string().into());
    assert_eq!(
        window.get_campus_select_model().row_count(),
        1,
        "小叉立即移除该条最近记录"
    );
    assert!(
        !window.get_confirm_dialog_visible(),
        "移除最近记录不弹确认窗"
    );
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("campus.recent_removed")),
        "移除最近记录必须产生通知事实"
    );
    let settings_after =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    assert_eq!(
        settings_after
            .recent_campuses()
            .expect("读取最近记录")
            .len(),
        1,
        "移除必须持久化"
    );

    // 进入校区一的方案列表
    window.invoke_campus_select_campus_clicked(first.to_string().into());
    assert_eq!(window.get_active_screen(), 2);

    // ── 5. 创建 / 复制 / 改名 / 删除均由方案管理入口完成 ──
    window.invoke_plan_list_create_clicked();
    assert!(window.get_input_dialog_visible(), "新建方案弹出输入窗");
    assert_eq!(
        window.get_input_dialog_text().as_str(),
        "新方案 1",
        "预填不冲突默认名"
    );
    window.set_input_dialog_text("全景复刻方案".into());
    window.invoke_input_dialog_confirmed();
    assert!(!window.get_input_dialog_visible(), "创建成功后关闭输入窗");
    assert_eq!(window.get_plan_list_model().row_count(), 1);
    let plan_a = window.get_plan_list_model().row_data(0).expect("方案卡片");
    assert_eq!(plan_a.name.as_str(), "全景复刻方案");

    // 复制：自动加"副本"后缀
    window.invoke_plan_list_duplicate_clicked(plan_a.plan_id.clone());
    assert_eq!(window.get_plan_list_model().row_count(), 2);
    let plan_b = window.get_plan_list_model().row_data(0).expect("复制卡片");
    assert_eq!(plan_b.name.as_str(), "全景复刻方案 副本");

    // 改名：输入窗预填现名
    window.invoke_plan_list_rename_clicked(plan_b.plan_id.clone());
    assert!(window.get_input_dialog_visible());
    assert_eq!(window.get_input_dialog_text().as_str(), "全景复刻方案 副本");
    window.set_input_dialog_text("方案 B".into());
    window.invoke_input_dialog_confirmed();
    assert_eq!(window.get_plan_list_model().row_count(), 2);
    let renamed = window.get_plan_list_model().row_data(0).expect("改名卡片");
    assert_eq!(renamed.name.as_str(), "方案 B");

    // ── 8. 单个操作失败：方案页保持可用 + 正确错误 ──
    window.invoke_plan_list_rename_clicked(renamed.plan_id.clone());
    window.set_input_dialog_text("全景复刻方案".into());
    window.invoke_input_dialog_confirmed();
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("plan.duplicate_name")),
        "重名必须产生正确错误通知"
    );
    assert!(
        window.get_input_dialog_visible(),
        "失败后输入窗保持打开（草稿不丢）"
    );
    assert_eq!(
        window.get_plan_list_model().row_count(),
        2,
        "失败不影响列表"
    );
    window.invoke_input_dialog_cancelled();

    // 删除：先确认，取消不提交
    window.invoke_plan_list_delete_clicked(plan_a.plan_id.clone());
    assert!(window.get_confirm_dialog_visible(), "删除前必须确认");
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("dialog.delete_title")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_plan_list_model().row_count(), 2, "取消不提交");
    window.invoke_plan_list_delete_clicked(plan_a.plan_id.clone());
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(
        window.get_plan_list_model().row_count(),
        1,
        "确认后删除进回收站"
    );

    // ── 6. 回收站：恢复 / 永久删除 / 清空均停留并短暂提示 ──
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 6);
    assert_eq!(window.get_trash_page_model().row_count(), 1);
    let trash_item = window
        .get_trash_page_model()
        .row_data(0)
        .expect("回收站条目");
    assert_eq!(trash_item.name.as_str(), "全景复刻方案");
    assert_eq!(trash_item.campus_name.as_str(), "华东师范大学(普陀校区)");
    assert!(
        trash_item.expires_in.as_str().contains("剩余"),
        "显示剩余保留时间"
    );
    let trash_id = trash_item.trash_id.clone();

    // 恢复：停留在回收站 + “方案已恢复”
    window.invoke_trash_restore_clicked(trash_id.clone());
    assert_eq!(window.get_active_screen(), 6, "恢复后停留回收站");
    assert_eq!(window.get_trash_page_model().row_count(), 0);
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("trash.restored_title")),
        "恢复必须产生“方案已恢复”提示事实"
    );

    // 再次删除 → 永久删除：先确认，取消不提交
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(first.to_string().into());
    window.invoke_plan_list_delete_clicked(plan_a.plan_id.clone());
    window.invoke_confirm_dialog_confirmed();
    window.invoke_trash_toolbar_button_clicked();
    let trash2 = window
        .get_trash_page_model()
        .row_data(0)
        .expect("再次删除的条目");
    window.invoke_trash_purge_clicked(trash2.trash_id.clone());
    assert!(
        window.get_confirm_dialog_visible(),
        "立即永久删除需再次确认"
    );
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("trash.purge_confirm_title")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(
        window.get_trash_page_model().row_count(),
        1,
        "取消不永久删除"
    );
    window.invoke_trash_purge_clicked(trash2.trash_id.clone());
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 6, "永久删除后停留回收站");
    assert_eq!(window.get_trash_page_model().row_count(), 0);
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("trash.purged_title")),
        "永久删除必须产生提示事实"
    );

    // 清空回收站：先确认，取消不提交，确认后清空并提示
    window.invoke_switch_campus_toolbar_button_clicked();
    window.invoke_campus_select_campus_clicked(first.to_string().into());
    window.invoke_plan_list_create_clicked();
    window.set_input_dialog_text("待清空方案一".into());
    window.invoke_input_dialog_confirmed();
    window.invoke_plan_list_create_clicked();
    window.set_input_dialog_text("待清空方案二".into());
    window.invoke_input_dialog_confirmed();
    let p1 = window.get_plan_list_model().row_data(1).expect("方案一");
    let p2 = window.get_plan_list_model().row_data(0).expect("方案二");
    window.invoke_plan_list_delete_clicked(p1.plan_id.clone());
    window.invoke_confirm_dialog_confirmed();
    window.invoke_plan_list_delete_clicked(p2.plan_id.clone());
    window.invoke_confirm_dialog_confirmed();
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_trash_page_model().row_count(), 2);
    window.invoke_trash_purge_all_clicked();
    assert!(window.get_confirm_dialog_visible(), "清空回收站需确认");
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("trash.clear_confirm_title")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_trash_page_model().row_count(), 2, "取消不提交");
    window.invoke_trash_purge_all_clicked();
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 6, "清空后停留回收站");
    assert_eq!(window.get_trash_page_model().row_count(), 0);
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("trash.cleared_title")),
        "清空必须产生提示事实"
    );
}
