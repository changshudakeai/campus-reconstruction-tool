//! S1-07 验收：采集入口只转发一次完整意图，页面状态与通知由 A1 返回。
//!
//! F4 → B2 → B14 → F7 已在 A1 collection-flow 入口后执行（后台 worker）；
//! S1 只呈现 A1 返回的页面状态、进度与通知，不再包含 F4/F7 编排。

use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::{ComponentHandle, Model};

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
                slint::quit_event_loop().expect("停止采集验收事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行采集验收事件循环");
}

#[test]
fn collection_page_forwards_start_intent_and_presents_a1_outcome() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-07.db");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("连接数据库"))
            .expect("创建注入器");
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

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();

    window.invoke_workspace_step_clicked(2);

    assert_eq!(window.get_workspace_active_step(), 2);
    assert_eq!(window.get_collection_category_labels().row_count(), 6);
    assert_eq!(window.get_collection_category_statuses().row_count(), 6);
    for index in 0..6 {
        assert_eq!(
            window.get_collection_category_statuses().row_data(index),
            Some(l10n.t("common.pending").into()),
            "第 {index} 类对象初始状态必须为待定"
        );
    }
    assert_eq!(
        window.get_collection_category_skip_label().as_str(),
        l10n.t("collection.skippable")
    );
    assert_eq!(
        window.get_collection_report_entry_label().as_str(),
        l10n.t("audit.report_entry")
    );

    // 点击采集：S1 只提交一次完整开始意图；A1 后台执行，页面立即呈现处理中。
    window.invoke_collection_start_clicked();
    assert_eq!(
        window.get_collection_progress_label().as_str(),
        l10n.t("collection.progress_fetching"),
        "耗时采集启动后先呈现处理中状态，不能冻结窗口"
    );

    // 推送罐头响应：A1 后台链完成时把疑点弹窗作为通知事实交给 S1，
    // S1 在 UI 线程发布 → 弹窗显示。
    window.invoke_workspace_map_ipc(
        r#"{"type":"collection_response","request_id":1,"payload":"{\"status\":\"1\",\"info\":\"OK\",\"pois\":[]}"}"#
            .into(),
    );
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_error_dialog_visible()
    });
    assert!(
        window.get_error_dialog_visible(),
        "有疑点必须经 B7 弹窗呈现（A1 汇总事实、S1 在 UI 线程发布）"
    );
    assert_eq!(
        window.get_error_dialog_body().lines().count(),
        6,
        "所有覆盖率疑点必须合并到同一扇窗口"
    );
    window.invoke_error_dialog_dismissed();
    pump_until(&window, Duration::from_secs(5), |window| {
        window.get_workspace_completed_steps() == 2
            && window
                .get_collection_progress_label()
                .as_str()
                .contains('0')
    });

    assert!(
        window
            .get_collection_progress_label()
            .as_str()
            .contains('0'),
        "采集完成后应呈现对象总数"
    );
    assert_eq!(window.get_workspace_completed_steps(), 2);
    assert!(!window.get_workspace_step_locked().row_data(3).unwrap());
}
