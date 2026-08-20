//! S1 工单 02 正式验收：完整状态、生产装配与 B7 故障操作路径。
// ignore-tidy-filelength: 测试 seam 逐页枚举是“一页一段”的对照表；T52 新增
// 预览字段后随最新主线评审状态一起集中维护，拆分会破坏逐页对照顺序。

use data_persistence::CampusCrudApi;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, BoundaryViewState, CampusData, CampusPlanPageState,
    CampusPlanPresentationEntry, CollectionPageState, CollectionPresentationEntry,
    ConfirmationPresentation, ExportPageState, ExportPresentationEntry, NavigationDecision,
    NoticeData, NotificationFact, NotificationPageState, NotificationPresentationEntry,
    OpaqueNotificationAction, OperationPresentationState, OrientationViewState, PlanCardData,
    Presentation, PresentationAdapter, Progress, ReviewPageState, ReviewPresentationEntry,
    ReviewRequest, Screen, SettingsPageState, SettingsPresentationEntry, ShellDatabases,
    StartupPageState, StartupPresentationEntry, ToolbarPageState, ViewModelInjector,
    WorkspacePageState,
};
use global_settings::FirstRunSetup;
use notification_center::NotificationActionOutcome;
use shared_domain_types::CampusId;
use slint::Model;

#[derive(Clone)]
struct TestAdapter<Page> {
    response: Presentation<Page>,
}

impl<Page> TestAdapter<Page> {
    fn returning(response: Presentation<Page>) -> Self {
        Self { response }
    }
}

impl<Request, Page> PresentationAdapter<Request, Page> for TestAdapter<Page>
where
    Page: Clone,
{
    fn present(&mut self, _request: Request) -> Presentation<Page> {
        self.response.clone()
    }
}

fn pump_event_loop() {
    slint::Timer::single_shot(Duration::from_millis(100), || {
        slint::quit_event_loop().expect("停止测试事件循环");
    });
    slint::run_event_loop_until_quit().expect("运行测试事件循环");
}

/// 轮询事件循环直到条件满足或超时（后台线程结果经事件循环回调到达；单次 100ms 泵送在慢 CI 上偶发竞态）。
fn pump_until(deadline: Duration, cond: impl Fn() -> bool) {
    let start = Instant::now();
    while start.elapsed() < deadline {
        pump_event_loop();
        if cond() {
            return;
        }
    }
}

fn toolbar(suffix: &str) -> ToolbarPageState {
    ToolbarPageState {
        title: format!("工具栏-{suffix}"),
        notice_visible: true,
        notice_label: format!("通知-{suffix}"),
        switch_campus_visible: true,
        switch_campus_label: format!("切换-{suffix}"),
        trash_visible: true,
        trash_label: format!("回收站-{suffix}"),
        settings_visible: true,
        settings_label: format!("设置-{suffix}"),
    }
}

fn workspace(suffix: &str) -> WorkspacePageState {
    WorkspacePageState {
        toolbar: toolbar(suffix),
        campus_name: format!("校区-{suffix}"),
        plan_name: format!("方案-{suffix}"),
        context_label: format!("校区-{suffix} / 方案-{suffix}"),
        active_step: 0,
        completed_steps: 2,
        step_locked: vec![false, false, true, true, true],
        step_completed: vec![true, true, false, false, false],
        placeholder_title: format!("流程-{suffix}"),
        placeholder_subtitle: format!("占位-{suffix}"),
        pending_notice: format!("状态-{suffix}"),
        title_step_label: format!("标题-{suffix}"),
        boundary_step_label: format!("边界-{suffix}"),
        orientation_step_label: format!("朝向-{suffix}"),
        collection_step_label: format!("采集-{suffix}"),
        review_step_label: format!("评审-{suffix}"),
        export_step_label: format!("导出-{suffix}"),
        drawer_open: false,
        map_available: true,
        map_loading: false,
        map_loading_label: format!("加载中-{suffix}"),
        map_failed_label: format!("失败-{suffix}"),
        boundary_points_label: format!("点数-{suffix}"),
        orientation_current_angle_label: format!("角度-{suffix}"),
        orientation_confirm_two_points_label: format!("两点-{suffix}"),
        drawer_expand_tooltip: format!("展开-{suffix}"),
        drawer_collapse_tooltip: format!("收起-{suffix}"),
        boundary_fetch_status: String::new(),
        boundary: BoundaryViewState {
            refresh_label: format!("refresh-{suffix}"),
            points: Vec::new(),
            path_commands: String::new(),
            title: format!("边界页-{suffix}"),
            hint: format!("边界提示-{suffix}"),
            undo_label: format!("撤销-{suffix}"),
            confirm_label: format!("确认-{suffix}"),
            reset_label: format!("重置-{suffix}"),
            status: format!("边界状态-{suffix}"),
            delete_selected_label: format!("删除选中-{suffix}"),
            delete_selected_enabled: false,
            edited_since_confirmed: false,
            map_placeholder: format!("地图占位-{suffix}"),
            is_determined: false,
            point_count: 0,
        },
        orientation: OrientationViewState {
            points: Vec::new(),
            path_commands: String::new(),
            arrow_commands: String::new(),
            angle: -1.0,
            is_determined: false,
            title: format!("朝向页-{suffix}"),
            two_points_hint: format!("两点提示-{suffix}"),
            bearing_angle_hint: format!("角度提示-{suffix}"),
            angle_input_placeholder: format!("角度占位-{suffix}"),
            angle_display: String::new(),
            clear_input: false,
            fill_input: None,
            submit_label: format!("提交-{suffix}"),
            reset_label: format!("重置-{suffix}"),
            status: format!("朝向状态-{suffix}"),
        },
        tutorial_visible: false,
        tutorial_text: String::new(),
        tutorial_dismiss_label: format!("知道了-{suffix}"),
        tutorial_skip_all_label: String::new(),
    }
}

fn startup(suffix: &str) -> StartupPageState {
    StartupPageState {
        app_title: format!("应用-{suffix}"),
        status_text: format!("启动-{suffix}"),
        wizard_title: format!("向导-{suffix}"),
        language_label: format!("语言-{suffix}"),
        version_label: format!("版本-{suffix}"),
        notice_text: format!("说明-{suffix}"),
        continue_label: format!("继续-{suffix}"),
        wizard_gaode_group_title: format!("高德配置-{suffix}"),
        wizard_gaode_api_key_label: format!("API-{suffix}"),
        wizard_gaode_api_key_placeholder: format!("API占位-{suffix}"),
        wizard_gaode_security_key_label: format!("安全-{suffix}"),
        wizard_gaode_security_key_placeholder: format!("安全占位-{suffix}"),
        wizard_gaode_web_service_key_label: format!("Web-{suffix}"),
        wizard_gaode_web_service_key_placeholder: format!("Web占位-{suffix}"),
        wizard_gaode_api_key: format!("api{suffix}"),
        wizard_gaode_security_key: format!("security{suffix}"),
        wizard_gaode_web_service_key: format!("web{suffix}"),
        language_options: vec![format!("zh-{suffix}")],
        version_options: vec![format!("1-{suffix}")],
        selected_language: format!("zh-{suffix}"),
        selected_version: format!("1-{suffix}"),
        acknowledged: true,
        landing_page: None,
    }
}

fn settings(suffix: &str) -> SettingsPageState {
    SettingsPageState {
        title: format!("设置页-{suffix}"),
        back_label: format!("返回-{suffix}"),
        tutorial_replay_label: format!("重看-{suffix}"),
        general_group_title: format!("常规-{suffix}"),
        language_label: format!("语言-{suffix}"),
        language_options: vec![format!("zh-{suffix}")],
        selected_language: format!("zh-{suffix}"),
        version_label: format!("版本-{suffix}"),
        version_options: vec![format!("1-{suffix}")],
        selected_version: format!("1-{suffix}"),
        export_location_label: format!("导出-{suffix}"),
        export_location_placeholder: format!("导出占位-{suffix}"),
        default_export_location: format!("D:/导出-{suffix}"),
        save_settings_label: format!("保存设置-{suffix}"),
        gaode_group_title: format!("地图-{suffix}"),
        api_key_label: format!("API-{suffix}"),
        api_key_placeholder: format!("API占位-{suffix}"),
        api_key: format!("api{suffix}"),
        security_key_label: format!("安全-{suffix}"),
        security_key_placeholder: format!("安全占位-{suffix}"),
        security_key: format!("security{suffix}"),
        web_service_key_label: format!("Web-{suffix}"),
        web_service_key_placeholder: format!("Web占位-{suffix}"),
        web_service_key: format!("web{suffix}"),
        save_label: format!("保存-{suffix}"),
        test_label: format!("测试-{suffix}"),
        clear_keys_label: format!("清除-{suffix}"),
        status_message: format!("设置状态-{suffix}"),
    }
}

#[test]
fn accepted_presentation_seams_are_complete_and_used_by_production() {
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = notification_center::NotificationCenter::init(
        notification_center::PresenterRegistry::new(),
    );
    center
        .registry()
        .set_presenter(desktop_shell::ShellPresenter::install(&window));

    let mut startup_entry = StartupPresentationEntry::new(TestAdapter::returning(
        Presentation::failed(startup("失败"))
            .with_navigation(NavigationDecision::Show(Screen::FirstRunSetup)),
    ));
    startup_entry.show(&window, &center, ());
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Failed
    );
    assert_eq!(window.get_wizard_title().as_str(), "向导-失败");
    assert_eq!(window.get_wizard_language_options().row_count(), 1);
    assert!(window.get_wizard_acknowledged());

    let mut settings_entry = SettingsPresentationEntry::new(TestAdapter::returning(
        Presentation::succeeded(settings("生产替换"))
            .with_navigation(NavigationDecision::Show(Screen::Settings)),
    ));
    settings_entry.show(&window, &center, ());
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    assert_eq!(window.get_gaode_api_key().as_str(), "api生产替换");
    settings_entry.replace_adapter(TestAdapter::returning(Presentation::ready(settings(
        "测试",
    ))));
    settings_entry.show(&window, &center, ());
    assert_eq!(window.get_settings_title().as_str(), "设置页-测试");

    let campuses = vec![CampusData {
        id: "campus-1".into(),
        name: "校园一".into(),
        address: "中山路1号".into(),
    }];
    let plans = vec![PlanCardData {
        plan_id: "plan-1".into(),
        name: "方案一".into(),
        progress_desc: "尚未确定范围".into(),
        last_modified: "刚刚".into(),
    }];
    let campus_page = CampusPlanPageState {
        toolbar: toolbar("校区"),
        campus_select_title: "校区标题".into(),
        campus_empty_text: "校区空".into(),
        campus_settings_label: "校区设置".into(),
        campuses,
        campus_search_query: "".into(),
        campus_search_placeholder: "搜索占位".into(),
        campus_search_button_label: "搜索".into(),
        campus_recent_title: "最近使用的校区".into(),
        campus_search_results: Vec::new(),
        campus_search_status: String::new(),
        campus_show_results: false,
        plan_list_title: "方案标题".into(),
        campus_name: "校园一".into(),
        create_plan_label: "新方案".into(),
        back_to_campus_label: "返回校区".into(),
        plan_empty_text: "方案空".into(),
        rename_label: "改名".into(),
        duplicate_label: "复制".into(),
        delete_label: "删除".into(),
        plans,
        tutorial_visible: true,
        tutorial_text: "教程".into(),
        tutorial_dismiss_label: "知道了".into(),
        tutorial_skip_all_label: "跳过".into(),
    };
    let empty_campus_page = CampusPlanPageState {
        campuses: Vec::new(),
        plans: Vec::new(),
        tutorial_visible: false,
        ..campus_page.clone()
    };
    let mut campus_entry = CampusPlanPresentationEntry::new(TestAdapter::returning(
        Presentation::ready(campus_page)
            .with_navigation(NavigationDecision::Show(Screen::PlanList)),
    ));
    campus_entry.show(&window, &center, ());
    assert_eq!(window.get_campus_select_model().row_count(), 1);
    assert_eq!(window.get_plan_list_model().row_count(), 1);
    assert!(window.get_plan_list_tutorial_visible());
    assert_eq!(window.get_plan_list_title().as_str(), "方案标题");
    assert_eq!(window.get_plan_list_campus_name().as_str(), "校园一");
    assert_eq!(window.get_plan_list_create_button_text().as_str(), "新方案");
    assert_eq!(window.get_plan_list_back_button_text().as_str(), "返回校区");
    assert_eq!(window.get_plan_list_empty_text().as_str(), "方案空");
    campus_entry.replace_adapter(TestAdapter::returning(Presentation::ready(
        empty_campus_page,
    )));
    campus_entry.show(&window, &center, ());
    assert_eq!(window.get_campus_select_model().row_count(), 0);
    assert_eq!(window.get_plan_list_model().row_count(), 0);
    assert!(!window.get_plan_list_tutorial_visible());

    for (expected, suffix) in [(0, "0"), (37, "37"), (100, "100")] {
        let progress = Progress::try_from(expected).expect("合法进度");
        let mut entry = CollectionPresentationEntry::new(TestAdapter::returning(
            Presentation::processing(
                CollectionPageState {
                    workspace: workspace(suffix),
                    source_label: format!("来源-{suffix}"),
                    collect_label: format!("采集-{suffix}"),
                    progress_label: format!("进度-{suffix}"),
                    category_labels: vec![format!("类别-{suffix}")],
                    category_statuses: vec![format!("待定-{suffix}")],
                    category_skip_label: format!("可跳过-{suffix}"),
                    diff_summary: format!("摘要-{suffix}"),
                    report_entry_label: format!("报告-{suffix}"),
                    report_body: String::new(),
                    stage_label: String::new(),
                    elapsed_label: String::new(),
                    cancel_label: String::new(),
                    cancel_visible: false,
                    partial_naming_label: String::new(),
                },
                progress,
            )
            .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        ));
        entry.show(&window, &center, ());
        assert_eq!(window.get_operation_progress(), i32::from(expected));
        assert_eq!(
            window.get_workspace_placeholder_title(),
            format!("流程-{suffix}")
        );
    }
    assert!(Progress::try_from(101).is_err());

    let mut review_entry =
        ReviewPresentationEntry::new(TestAdapter::returning(Presentation::needs_confirmation(
            ReviewPageState {
                workspace: workspace("评审"),
                title: "评审工作台".into(),
                empty_text: "暂无候选评审".into(),
                candidate_count: 0,
                category_labels: Vec::new(),
                category_counts: Vec::new(),
                active_category: 0,
                cards: Vec::new(),
                page_size: 60,
                page_index: 0,
                page_total: 1,
                page_label: "第 1/1 页".into(),
                page_prev_label: "上一页".into(),
                page_next_label: "下一页".into(),
                selected_count_label: String::new(),
                all_page_selected: false,
                batch_buttons_enabled: false,
                set_keep_label: "改为保留".into(),
                set_reject_label: "改为剔除".into(),
                set_pending_label: "改回待定".into(),
                select_all_label: "全选".into(),
                card_pending_label: "待定".into(),
                card_keep_label: "保留".into(),
                card_reject_label: "剔除".into(),
                locate_label: "定位到地图".into(),
                legend: "图例：待定=虚线".into(),
                detail_visible: false,
                detail_title: String::new(),
                detail_category_label: String::new(),
                detail_tags_label: String::new(),
                detail_tags: Vec::new(),
                detail_source_label: String::new(),
                detail_source: String::new(),
                detail_state_label: String::new(),
                seal_label: "封账完成评审".into(),
                sealed: false,
                confidence_filters_label: "置信度筛选".into(),
                confidence_filter_labels: Vec::new(),
                confidence_filter_counts: Vec::new(),
                confidence_filter_active: Vec::new(),
                state_tab_labels: Vec::new(),
                state_tab_counts: Vec::new(),
                state_tab_active: Vec::new(),
                apply_suggestions_label: "应用建议".into(),
                undo_suggestions_label: "撤销上一批".into(),
                apply_suggestions_enabled: false,
                undo_available: false,
                summary_visible: false,
                summary_text: String::new(),
            },
            ConfirmationPresentation::new("确认", "影响", "继续", "取消"),
        )));
    review_entry.show(&window, &center, ReviewRequest::Open);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::NeedsConfirmation
    );
    assert!(window.get_confirm_dialog_visible());

    let mut export_entry = ExportPresentationEntry::new(TestAdapter::returning(
        Presentation::ready(ExportPageState {
            workspace: workspace("导出"),
            preview_generate_label: "生成 3D 预览".into(),
            preview_status: String::new(),
            preview_reset_label: "复位视角".into(),
            preview_zoom_in_label: "放大".into(),
            preview_zoom_out_label: "缩小".into(),
            preview_controls_hint: "拖动旋转 · 滚轮缩放".into(),
            preview_has_content: false,
            preview_generating: false,
            preview_candidate_ids: Vec::new(),
            preview_candidate_titles: Vec::new(),
            preview_candidate_categories: Vec::new(),
        }),
    ));
    export_entry.show(&window, &center, ());
    assert_eq!(window.get_workspace_active_step(), 4);
    assert_eq!(
        window.get_workspace_step_pending_notice().as_str(),
        "状态-导出"
    );

    let received = Arc::new(Mutex::new(Vec::new()));
    let owner = Arc::clone(&received);
    let notification = notification_center::Notification::error("模块", "失败", "可导出资料");
    let notification_id = notification.id.to_string();
    let fact = NotificationFact::new(notification).with_diagnostic_action(
        OpaqueNotificationAction::new(move || {
            owner.lock().expect("功能模块接收锁").push("原始操作内容");
            notification_center::Notification::info("模块", "故障资料已导出", "原始操作内容")
        }),
    );
    let mut notification_entry = NotificationPresentationEntry::new(TestAdapter::returning(
        Presentation::ready(NotificationPageState {
            toolbar: toolbar("通知"),
            title: "通知中心".into(),
            empty_list_text: "无通知".into(),
            archive_label: "归档".into(),
            date_today: "今天".into(),
            date_yesterday: "昨天".into(),
            importance_high_label: "重要".into(),
            unread_marker: "未读".into(),
            diagnostic_action_label: "导出故障资料".into(),
            notices: vec![NoticeData {
                id: notification_id.clone().into(),
                title: "失败".into(),
                body: "可导出资料".into(),
                date: "今天".into(),
                importance: "high".into(),
                read: false,
                has_diagnostic_action: true,
            }],
        })
        .with_notification(fact),
    ));
    notification_entry.show(&window, &center, ());
    assert_eq!(window.get_notice_board_model().row_count(), 1);
    assert!(window.get_error_dialog_visible());
    assert!(window.get_error_dialog_diagnostic_action_visible());

    let injector = ViewModelInjector::new(ShellDatabases::open_in_memory().expect("内存连接组"))
        .expect("正式模块装配");
    let _initial_runtime = assemble_application(&window, injector, Arc::clone(&center));
    assert!(!window.get_wizard_title().is_empty());
    window.invoke_settings_toolbar_button_clicked();
    assert!(!window.get_settings_title().is_empty());
    assert!(!window.get_settings_language().is_empty());
    assert!(!window.get_settings_version().is_empty());
    assert!(!window.get_settings_export_location().is_empty());
    assert!(!window.get_campus_select_title().is_empty());
    assert!(!window.get_workspace_stepper_collection_label().is_empty());
    assert!(!window.get_workspace_stepper_review_label().is_empty());
    assert!(!window.get_workspace_stepper_export_label().is_empty());
    assert!(!window.get_notice_board_title().is_empty());
    assert!(window.get_notice_board_model().row_count() >= 1);

    // 正式应用装配必须从 F1 的完整启动快照着陆到上次校区的方案列表，
    // 后续页面显示必须由真实 AppWindow 回调刷新，而不是启动时演示一遍。
    let directory = tempfile::tempdir().expect("临时目录");
    let databases = ShellDatabases::open(directory.path().join("presentation-seams.db"))
        .expect("正式文件连接组");
    let mut production_injector = ViewModelInjector::new(databases).expect("正式模块装配");
    production_injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首次设置");
    let campus = production_injector
        .projects_mut()
        .database()
        .create_campus("验收校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("正式校区标识");
    production_injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建正式方案");
    production_injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近使用校区");

    let _production_runtime =
        assemble_application(&window, production_injector, Arc::clone(&center));
    assert_eq!(
        window.get_active_screen(),
        2,
        "正式 F1 快照中的上次校区必须落到方案列表"
    );
    assert_eq!(
        window.get_plan_list_model().row_count(),
        1,
        "启动入口必须同时呈现目标方案页的完整正式状态"
    );

    window.set_workspace_completed_steps(4);
    window.set_operation_state(OperationPresentationState::Failed);
    window.invoke_workspace_step_clicked(2);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "点击采集步骤必须经过采集呈现入口"
    );
    assert_eq!(
        window.get_workspace_active_step(),
        2,
        "点击采集步骤只能呈现采集占位页，不得同时串到其他步骤入口"
    );
    window.set_operation_state(OperationPresentationState::Failed);
    window.invoke_workspace_step_clicked(3);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "点击评审步骤必须经过评审呈现入口"
    );
    assert_eq!(
        window.get_workspace_active_step(),
        3,
        "点击评审步骤只能呈现评审页，不得同时串到其他步骤入口"
    );
    window.set_operation_state(OperationPresentationState::Failed);
    window.invoke_workspace_step_clicked(4);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "点击导出步骤必须经过导出呈现入口"
    );
    assert_eq!(
        window.get_workspace_active_step(),
        4,
        "点击导出步骤只能呈现导出占位页，不得同时串到其他步骤入口"
    );

    assert!(
        !window.get_confirm_dialog_visible(),
        "进入占位步骤必须清除上一页面确认窗"
    );
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Ready,
        "进入占位步骤必须清除上一页面诊断操作状态"
    );

    window.set_gaode_api_key("陈旧输入".into());
    window.set_operation_state(OperationPresentationState::Processing);
    window.set_operation_progress(42);
    window.invoke_settings_toolbar_button_clicked();
    assert!(
        window.get_gaode_api_key().is_empty(),
        "打开设置页必须从设置入口刷新正式状态"
    );
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "切换页面必须清除上一页面遗留的处理中状态"
    );
    assert_eq!(
        window.get_operation_progress(),
        0,
        "切换页面必须清除上一页面遗留的进度"
    );
    assert!(
        !window.get_confirm_dialog_visible(),
        "切到设置页必须清除上一页面确认窗"
    );
    assert_eq!(
        window.get_diagnostic_operation_progress(),
        0,
        "切到设置页必须清除处理中进度"
    );

    window.set_campus_select_model(slint::ModelRc::new(slint::VecModel::default()));
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(
        window.get_campus_select_model().row_count(),
        1,
        "打开校区页必须从校区与方案入口刷新"
    );
    assert!(
        !window.get_confirm_dialog_visible(),
        "切到校区页必须清除上一页面确认窗"
    );
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Ready,
        "切到校区页必须清除上一页面操作状态"
    );

    // 设置页返回也必须经校区与方案入口刷新
    window.set_campus_select_model(slint::ModelRc::new(slint::VecModel::default()));
    window.invoke_settings_toolbar_button_clicked();
    window.invoke_settings_back_clicked();
    assert_eq!(window.get_active_screen(), 1, "设置页返回应落到校区选择页");
    assert_eq!(
        window.get_campus_select_model().row_count(),
        1,
        "设置页返回必须经校区与方案入口刷新"
    );

    let published_after_start =
        notification_center::Notification::info("验收", "启动后通知", "只能在打开公告栏时刷新得到");
    center.publish(published_after_start.clone());
    window.invoke_notice_toolbar_button_clicked();
    assert!(
        (0..window.get_notice_board_model().row_count()).any(|index| {
            window
                .get_notice_board_model()
                .row_data(index)
                .is_some_and(|notice| notice.id.as_str() == published_after_start.id.to_string())
        }),
        "启动后发布的通知必须在点击公告栏后出现"
    );
    assert!(
        !window.get_confirm_dialog_visible(),
        "打开公告栏必须清除上一页面确认窗"
    );

    let popup_notification = notification_center::Notification::error(
        "验收模块",
        "后台线程错误",
        "从错误弹窗触发故障资料",
    );
    let popup_id = popup_notification.id.to_string();
    let (publisher_released, publisher_release) = std::sync::mpsc::channel();
    let publisher_center = Arc::clone(&center);
    std::thread::spawn(move || {
        publisher_center.publish_with_action(
            popup_notification,
            OpaqueNotificationAction::new(|| {
                notification_center::Notification::info(
                    "验收模块",
                    "错误弹窗操作完成",
                    "原功能模块结果",
                )
            }),
        );
        publisher_released.send(()).expect("报告发布线程已释放");
    });
    pump_until(Duration::from_secs(30), || {
        window.get_error_dialog_visible()
    });
    assert!(window.get_error_dialog_visible());
    window.invoke_error_dialog_diagnostic_action_clicked(popup_id.into());
    publisher_release
        .recv_timeout(Duration::from_secs(1))
        .expect("点击故障资料操作必须同时确认弹窗并释放后台发布线程");
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Succeeded
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Succeeded
    );

    let release_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let action_gate = Arc::clone(&release_gate);
    let action_owner = Arc::clone(&received);
    let action_notification =
        notification_center::Notification::info("验收模块", "可导出故障资料", "后台操作入口");
    let action_id = action_notification.id.to_string();
    center.publish_with_action(
        action_notification,
        OpaqueNotificationAction::new(move || {
            let (released, signal) = &*action_gate;
            let released = released.lock().expect("后台操作门锁");
            drop(
                signal
                    .wait_while(released, |released| !*released)
                    .expect("等待释放后台操作"),
            );
            action_owner
                .lock()
                .expect("功能模块接收锁")
                .push("后台原始操作内容");
            notification_center::Notification::info(
                "验收模块",
                "故障资料已导出",
                "后台原始操作内容",
            )
        }),
    );
    window.invoke_notice_toolbar_button_clicked();
    assert!(
        (0..window.get_notice_board_model().row_count()).any(|index| {
            window
                .get_notice_board_model()
                .row_data(index)
                .is_some_and(|notice| {
                    notice.id.as_str() == action_id && notice.has_diagnostic_action
                })
        })
    );

    let clicked_at = Instant::now();
    window.invoke_notice_diagnostic_action_clicked(action_id.into());
    assert!(
        clicked_at.elapsed() < Duration::from_millis(200),
        "点击故障资料操作必须立即返回"
    );
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Processing
    );
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "后台处理中其他页面仍可使用");
    assert!(
        !window.get_confirm_dialog_visible(),
        "后台处理中切页必须清除确认窗"
    );
    assert_eq!(window.get_diagnostic_operation_progress(), 0);

    {
        let (released, signal) = &*release_gate;
        *released.lock().expect("释放门锁") = true;
        signal.notify_all();
    }
    pump_until(Duration::from_secs(30), || {
        window.get_operation_state() == OperationPresentationState::Ready
    });
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "后来打开的设置页不得被较早后台结果覆盖"
    );
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Ready,
        "switching pages must supersede the older public diagnostic state"
    );
    assert!(center
        .board_snapshot()
        .iter()
        .any(|notification| notification.body == "后台原始操作内容"));
    assert!(received
        .lock()
        .expect("读取功能模块接收内容")
        .contains(&"后台原始操作内容"));
    let confirm_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let confirm_action_gate = Arc::clone(&confirm_gate);
    let confirm_notification =
        notification_center::Notification::info("test module", "dialog isolation", "controlled");
    let confirm_action_id = confirm_notification.id.to_string();
    center.publish_with_action(
        confirm_notification,
        OpaqueNotificationAction::new(move || {
            let (released, signal) = &*confirm_action_gate;
            let released = released.lock().expect("dialog action gate");
            drop(
                signal
                    .wait_while(released, |released| !*released)
                    .expect("wait for dialog action release"),
            );
            notification_center::Notification::info(
                "test module",
                "dialog isolation completed",
                "only the diagnostic state changes",
            )
        }),
    );
    window.invoke_notice_toolbar_button_clicked();
    window.invoke_notice_diagnostic_action_clicked(confirm_action_id.into());
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Processing
    );
    let plan = window
        .get_plan_list_model()
        .row_data(0)
        .expect("startup landing keeps the production plan model");
    window.invoke_plan_list_delete_clicked(plan.plan_id);
    assert!(window.get_confirm_dialog_visible());
    let confirm_title = window.get_confirm_dialog_title();
    let confirm_body = window.get_confirm_dialog_body();
    {
        let (released, signal) = &*confirm_gate;
        *released.lock().expect("release dialog action gate") = true;
        signal.notify_all();
    }
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Succeeded
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Succeeded
    );
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(window.get_confirm_dialog_title(), confirm_title);
    assert_eq!(window.get_confirm_dialog_body(), confirm_body);
    window.invoke_confirm_dialog_cancelled();

    let failed_action =
        notification_center::Notification::info("验收模块", "失败路径", "后台失败操作入口");
    let failed_action_id = failed_action.id.to_string();
    center.publish_with_action(
        failed_action,
        OpaqueNotificationAction::new(|| {
            NotificationActionOutcome::failed(notification_center::Notification::error(
                "验收模块",
                "故障资料导出失败",
                "原功能模块失败内容",
            ))
        }),
    );
    window.invoke_notice_toolbar_button_clicked();
    window.invoke_notice_diagnostic_action_clicked(failed_action_id.into());
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Failed
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Failed
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        "原功能模块失败内容"
    );
    window.invoke_error_dialog_dismissed();
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "失败后其他页面仍可继续使用");

    // 设置页返回 = 返回进入设置页时的上一页（此处为通知中心），且不残留状态
    window.invoke_settings_back_clicked();
    assert_eq!(
        window.get_active_screen(),
        5,
        "设置页返回必须回到进入设置页时的上一页（通知中心）"
    );
    assert!(
        (0..window.get_notice_board_model().row_count()).any(|index| {
            window
                .get_notice_board_model()
                .row_data(index)
                .is_some_and(|notice| notice.body.as_str() == "原功能模块失败内容")
        }),
        "返回通知中心必须刷新完整公告列表（含刚发布的失败记录）"
    );
    assert!(
        !window.get_confirm_dialog_visible(),
        "返回通知中心必须清除确认窗"
    );
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "返回通知中心不得残留上一页面操作状态"
    );
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Ready,
        "返回通知中心不得残留诊断处理中状态"
    );

    let panicking_action =
        notification_center::Notification::info("验收模块", "异常中止路径", "后台替身会异常中止");
    let panicking_action_id = panicking_action.id.to_string();
    center.publish_with_action(
        panicking_action,
        OpaqueNotificationAction::new(|| -> notification_center::Notification {
            panic!("受控的后台替身异常中止")
        }),
    );
    window.invoke_notice_toolbar_button_clicked();
    window.invoke_notice_diagnostic_action_clicked(panicking_action_id.into());
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Failed
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Failed,
        "后台操作异常中止也必须离开处理中状态"
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        "故障资料操作意外中止，请重试"
    );
    window.invoke_error_dialog_dismissed();

    let older_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let older_action_gate = Arc::clone(&older_gate);
    let older_notification =
        notification_center::Notification::info("验收模块", "较早故障操作", "用于验证过期结果");
    let older_id = older_notification.id.to_string();
    center.publish_with_action(
        older_notification,
        OpaqueNotificationAction::new(move || {
            let (released, signal) = &*older_action_gate;
            let released = released.lock().expect("较早操作门锁");
            drop(
                signal
                    .wait_while(released, |released| !*released)
                    .expect("等待释放较早操作"),
            );
            NotificationActionOutcome::failed(notification_center::Notification::error(
                "验收模块",
                "较早操作失败",
                "较早结果只应留入 B7",
            ))
        }),
    );

    let newer_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let newer_action_gate = Arc::clone(&newer_gate);
    let newer_notification =
        notification_center::Notification::info("验收模块", "较新故障操作", "用于验证最新结果");
    let newer_id = newer_notification.id.to_string();
    center.publish_with_action(
        newer_notification,
        OpaqueNotificationAction::new(move || {
            let (released, signal) = &*newer_action_gate;
            let released = released.lock().expect("较新操作门锁");
            drop(
                signal
                    .wait_while(released, |released| !*released)
                    .expect("等待释放较新操作"),
            );
            notification_center::Notification::info(
                "验收模块",
                "较新操作完成",
                "最新结果决定公开状态",
            )
        }),
    );

    window.invoke_notice_toolbar_button_clicked();
    window.invoke_notice_diagnostic_action_clicked(older_id.into());
    window.invoke_notice_diagnostic_action_clicked(newer_id.into());
    {
        let (released, signal) = &*newer_gate;
        *released.lock().expect("释放较新门锁") = true;
        signal.notify_all();
    }
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Succeeded
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Succeeded
    );

    let records_before_older_completion = center.board_snapshot().len();
    {
        let (released, signal) = &*older_gate;
        *released.lock().expect("释放较早门锁") = true;
        signal.notify_all();
    }
    pump_until(Duration::from_secs(30), || {
        window.get_diagnostic_operation_state() == OperationPresentationState::Succeeded
    });
    assert_eq!(
        window.get_diagnostic_operation_state(),
        OperationPresentationState::Succeeded,
        "较早任务完成后不得覆盖较新的公开状态"
    );
    assert!(
        !window.get_error_dialog_visible(),
        "a superseded failure must remain in B7 history without interrupting the current UI"
    );
    assert_eq!(
        center.board_snapshot().len(),
        records_before_older_completion + 1
    );
}
