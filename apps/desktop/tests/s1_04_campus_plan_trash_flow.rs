//! S1 工单 04 正式验收：校区在线搜索（D-3）、最近记录、方案列表 CRUD 与
//! 回收站全部经方案管理入口完成（独立进程，Slint 平台单线程约束集中在单个
//! #[test]）。
//!
//! D-3：校区搜索走高德在线链路（B3 解析 + 可注入传输），点选 → 详情确认 →
//! F1 建/选校区 → 直接进入方案列表；重复点选只切换不重复建；搜索失败可
//! 重试/取消、不建校区、不能绕过。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_persistence::{CampusCrudApi, Database};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::StdExportFileSystem;
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use slint::{ComponentHandle, Model};

/// 罐头校区搜索传输：按关键词返回固定高德响应；"失败大学"首次失败、重试成功。
type CannedTransport = Box<dyn Fn(&str, &str, &str) -> Result<String, String> + Send + Sync>;

fn canned_search_transport() -> (Arc<AtomicUsize>, CannedTransport) {
    let failures = Arc::new(AtomicUsize::new(0));
    let failures_for_closure = Arc::clone(&failures);
    let transport = Box::new(
        move |_api_key: &str, _security_key: &str, query: &str| -> Result<String, String> {
            match query {
            "上海交通大学" => Ok(r#"{"status":"1","info":"OK","pois":[
                {"id":"POI-SJTU","name":"上海交通大学","address":"闵行区东川路800号","location":{"lng":121.44,"lat":31.03},"typecode":"141201"}
            ]}"#
            .to_owned()),
            "华东师范" => Ok(r#"{"status":"1","info":"OK","pois":[
                {"id":"B01","name":"华东师范大学(普陀校区)","address":"中山北路3663号","location":[121.406,31.228],"typecode":"141201"}
            ]}"#
            .to_owned()),
            "失败大学" => {
                let attempt = failures_for_closure.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    Err("网络超时".to_owned())
                } else {
                    Ok(r#"{"status":"1","info":"OK","pois":[
                        {"id":"POI-RETRY","name":"重试大学","address":"重试路1号","location":"121.5,31.1","typecode":"141201"}
                    ]}"#
                    .to_owned())
                }
            }
            _ => Ok(r#"{"status":"1","info":"OK","pois":[]}"#.to_owned()),
        }
        },
    );
    (failures, transport)
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
                slint::quit_event_loop().expect("停止事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行事件循环");
}

fn logical_size(window: &AppWindow) -> (f32, f32) {
    let scale = window.window().scale_factor().max(0.001);
    let size = window.window().size();
    (size.width as f32 / scale, size.height as f32 / scale)
}

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
    let (failure_count, transport) = canned_search_transport();
    let mut injector = ViewModelInjector::new_with_campus_search_transport(
        ShellDatabases::open(&database_path).expect("正式连接组"),
        Arc::new(StdExportFileSystem),
        transport,
    )
    .expect("正式注入器");
    injector
        .settings_mut()
        .set_gaode_api_key("0123456789abcdef01234567")
        .expect("保存 API key");
    injector
        .settings_mut()
        .set_gaode_security_key("fedcba9876543210fedcba98")
        .expect("保存安全密钥");
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

    // ── 1b. 校区选择页工具栏矩阵：设置/通知可见，回收站/切换校区隐藏 ──
    assert!(
        window.get_settings_toolbar_button_visible(),
        "校区选择页设置入口必须可见（用户需要在此改 Key）"
    );
    assert!(
        window.get_notice_toolbar_button_visible(),
        "校区选择页通知入口可显示"
    );
    assert!(
        !window.get_trash_toolbar_button_visible(),
        "尚未选定校区时回收站入口必须隐藏"
    );
    assert!(
        !window.get_switch_campus_toolbar_button_visible(),
        "尚未选定校区时切换校区入口必须隐藏"
    );

    // ── 1c. 两档窗口搜索行矩形断言：限宽居中、不越界、输入框可读 ──
    for (width, height) in [(800.0, 666.0), (1000.0, 666.0)] {
        window
            .window()
            .set_size(slint::LogicalSize::new(width, height));
        let (w, h) = logical_size(&window);
        assert!((w - width).abs() < 0.5, "{width} 逻辑宽生效");
        let x = window.get_campus_search_row_x();
        let y = window.get_campus_search_row_y();
        let row_w = window.get_campus_search_row_width();
        let row_h = window.get_campus_search_row_height();
        let input_w = window.get_campus_search_input_width();
        assert!(
            row_w > 0.0 && row_w <= 560.5,
            "搜索行最大宽 560：实际 {row_w}（窗口 {width}）"
        );
        assert!(
            input_w >= 300.0,
            "搜索输入框可读：实际宽 {input_w}（窗口 {width}）"
        );
        assert!(
            x >= 0.0 && x + row_w <= w + 0.5,
            "搜索行水平不越界：{x}..{}（窗口 {w}）",
            x + row_w
        );
        assert!(
            y >= 56.0 && y + row_h <= h + 0.5,
            "搜索行垂直不越界：{y}..{}（窗口 {h}）",
            y + row_h
        );
    }

    // ── 2. 高德在线搜索：只在点击"搜索"或按回车时开始 ──
    window.set_campus_search_text("上海交通大学".into());
    assert!(!window.get_campus_show_results(), "输入期间不自动搜索");
    assert_eq!(window.get_campus_search_results_model().row_count(), 0);
    window.invoke_campus_search_requested();
    assert!(
        window
            .get_campus_search_status()
            .as_str()
            .contains("正在搜索"),
        "搜索中必须呈现处理状态"
    );
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_campus_show_results()
            && window.get_campus_search_results_model().row_count() == 1
            && !window
                .get_campus_search_status()
                .as_str()
                .contains("正在搜索")
    });
    assert_eq!(window.get_campus_search_results_model().row_count(), 1);
    let result = window
        .get_campus_search_results_model()
        .row_data(0)
        .expect("搜索结果");
    assert_eq!(result.name.as_str(), "上海交通大学");
    assert_eq!(result.id.as_str(), "POI-SJTU", "候选行 id 为高德 POI 标识");
    assert_eq!(result.address.as_str(), "闵行区东川路800号");

    // ── 3. 点选搜索候选 → 详情确认 → 建/选校区 → 直接进入方案列表 ──
    window.invoke_campus_select_campus_clicked("POI-SJTU".into());
    assert!(
        window.get_confirm_dialog_visible(),
        "点选候选必须弹详情确认窗"
    );
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("campus.confirm_add_title")
    );
    assert!(
        window
            .get_confirm_dialog_body()
            .as_str()
            .contains("上海交通大学"),
        "确认窗展示所选学校名称"
    );
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 2, "确认后直接进入方案列表");
    assert!(
        window.get_switch_campus_toolbar_button_visible(),
        "选定校区后切换校区入口恢复可见"
    );
    assert!(
        window.get_trash_toolbar_button_visible(),
        "选定校区后回收站入口恢复可见"
    );

    let campuses = Database::open(&database_path)
        .expect("重开设置库")
        .list_campuses()
        .expect("列出校区");
    assert_eq!(campuses.len(), 1, "新建真实学校校区");
    assert_eq!(campuses[0].name, "上海交通大学");

    // ── 4. 重复点选同一学校：只切换、不重复建（ADR-0008 第 6 条）──
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 1);
    assert_eq!(
        window.get_campus_select_model().row_count(),
        1,
        "新建校区进入最近记录"
    );
    window.set_campus_search_text("华东师范".into());
    window.invoke_campus_search_requested();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_campus_show_results()
            && window.get_campus_search_results_model().row_count() == 1
    });
    // 先建立华东师范大学（B01），再点选同一 POI → 切换通知、校区数不变
    let mut settings_prep =
        SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    settings_prep
        .select_campus_by_poi_id(
            "华东师范大学(普陀校区)",
            "B01",
            "中山北路3663号",
            121.406,
            31.228,
        )
        .expect("预建华东师大");
    drop(settings_prep);
    window.invoke_campus_select_campus_clicked("B01".into());
    assert!(window.get_confirm_dialog_visible());
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 2, "重复校区直接进入方案列表");
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("campus.already_added")),
        "重复校区必须产生“该校区已添加，已为你切换”通知事实"
    );
    assert_eq!(
        Database::open(&database_path)
            .expect("重开设置库")
            .list_campuses()
            .expect("列出校区")
            .len(),
        2,
        "重复点选不得重复建校区"
    );

    // ── 5. 搜索失败：重试/取消，不建校区、不能绕过 ──
    window.invoke_switch_campus_toolbar_button_clicked();
    window.set_campus_search_text("失败大学".into());
    window.invoke_campus_search_requested();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_confirm_dialog_visible()
    });
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("campus.search_failed_title"),
        "失败必须弹“暂时无法搜索”弹窗"
    );
    assert_eq!(
        window.get_confirm_dialog_confirm_label().as_str(),
        l10n.t("campus.retry_button")
    );
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_active_screen(), 1, "取消后停留校区选择页");
    assert_eq!(
        Database::open(&database_path)
            .expect("重开设置库")
            .list_campuses()
            .expect("列出校区")
            .len(),
        2,
        "取消不创建校区"
    );

    // 重试：同关键词再次搜索成功
    assert_eq!(failure_count.load(Ordering::SeqCst), 1);
    window.invoke_campus_search_requested();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_campus_show_results()
            && window.get_campus_search_results_model().row_count() == 1
    });
    let retried = window
        .get_campus_search_results_model()
        .row_data(0)
        .expect("重试结果");
    assert_eq!(retried.name.as_str(), "重试大学");

    // ── 6. 最近记录小叉：立即移除、不弹确认、通知事实 ──
    window.invoke_switch_campus_toolbar_button_clicked();
    let recents = window.get_campus_select_model();
    assert_eq!(recents.row_count(), 2);
    let first_recent = recents.row_data(0).expect("最近记录");
    window.invoke_campus_select_remove_recent_clicked(first_recent.id.clone());
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
    let settings_after_remove =
        SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    assert_eq!(
        settings_after_remove
            .recent_campuses()
            .expect("读取最近记录")
            .len(),
        1,
        "移除必须持久化"
    );
    drop(settings_after_remove);

    // 进入上海交通大学方案列表
    let sjtu_recent = window
        .get_campus_select_model()
        .row_data(0)
        .expect("上海交大最近记录");
    window.invoke_campus_select_campus_clicked(sjtu_recent.id.clone());
    assert_eq!(window.get_active_screen(), 2);

    // ── 7. 创建 / 复制 / 改名 / 删除均由方案管理入口完成 ──
    window.invoke_plan_list_create_clicked();
    assert!(window.get_input_dialog_visible(), "新建方案弹出输入窗");
    window.set_input_dialog_text("全景复刻方案".into());
    window.invoke_input_dialog_confirmed();
    assert!(!window.get_input_dialog_visible(), "创建成功后关闭输入窗");
    assert_eq!(window.get_plan_list_model().row_count(), 1);
    let plan_a = window.get_plan_list_model().row_data(0).expect("方案卡片");
    assert_eq!(plan_a.name.as_str(), "全景复刻方案");

    window.invoke_plan_list_duplicate_clicked(plan_a.plan_id.clone());
    assert_eq!(window.get_plan_list_model().row_count(), 2);
    let plan_b = window.get_plan_list_model().row_data(0).expect("复制卡片");
    assert_eq!(plan_b.name.as_str(), "全景复刻方案 副本");

    window.invoke_plan_list_rename_clicked(plan_b.plan_id.clone());
    assert!(window.get_input_dialog_visible());
    window.set_input_dialog_text("方案 B".into());
    window.invoke_input_dialog_confirmed();
    let renamed = window.get_plan_list_model().row_data(0).expect("改名卡片");
    assert_eq!(renamed.name.as_str(), "方案 B");

    // 单个操作失败：方案页保持可用 + 正确错误
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
    assert!(window.get_input_dialog_visible(), "失败后输入窗保持打开");
    window.invoke_input_dialog_cancelled();

    window.invoke_plan_list_delete_clicked(plan_a.plan_id.clone());
    assert!(window.get_confirm_dialog_visible(), "删除前必须确认");
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_plan_list_model().row_count(), 2, "取消不提交");
    window.invoke_plan_list_delete_clicked(plan_a.plan_id.clone());
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(
        window.get_plan_list_model().row_count(),
        1,
        "确认后删除进回收站"
    );

    // ── 8. 回收站：恢复 / 永久删除 / 清空均停留并短暂提示 ──
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 6);
    assert_eq!(window.get_trash_page_model().row_count(), 1);
    let trash_item = window
        .get_trash_page_model()
        .row_data(0)
        .expect("回收站条目");
    assert_eq!(trash_item.name.as_str(), "全景复刻方案");
    assert_eq!(trash_item.campus_name.as_str(), "上海交通大学");
    let trash_id = trash_item.trash_id.clone();

    window.invoke_trash_restore_clicked(trash_id.clone());
    assert_eq!(window.get_active_screen(), 6, "恢复后停留回收站");
    assert_eq!(window.get_trash_page_model().row_count(), 0);

    window.invoke_switch_campus_toolbar_button_clicked();
    let sjtu_recent = window
        .get_campus_select_model()
        .row_data(0)
        .expect("上海交大最近记录");
    window.invoke_campus_select_campus_clicked(sjtu_recent.id.clone());
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

    window.invoke_switch_campus_toolbar_button_clicked();
    let sjtu_recent = window
        .get_campus_select_model()
        .row_data(0)
        .expect("上海交大最近记录");
    window.invoke_campus_select_campus_clicked(sjtu_recent.id.clone());
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
    window.invoke_confirm_dialog_cancelled();
    assert_eq!(window.get_trash_page_model().row_count(), 2, "取消不提交");
    window.invoke_trash_purge_all_clicked();
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(window.get_active_screen(), 6, "清空后停留回收站");
    assert_eq!(window.get_trash_page_model().row_count(), 0);
}
