//! S1 工单 03 正式验收：启动与设置流程迁移到呈现入口（独立进程）。
//!
//! Slint 平台每个进程只能初始化一次，因此所有窗口场景集中在一个 #[test]
//! 内顺序验证：启动失败与重试、设置失败与重试、首跑完成、常规设置读写、
//! 保存/测试/清除密钥的通知事实与失败路径。

use data_persistence::Database;
use desktop_shell::{
    assemble_application, AppWindow, NavigationDecision, NotificationFact,
    OperationPresentationState, Presentation, PresentationAdapter, Screen, SettingsPageState,
    SettingsPresentationEntry, SettingsRequest, ShellDatabases, StartupPageState,
    StartupPresentationEntry, StartupRequest, ViewModelInjector,
};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{Notification, NotificationCenter, PresenterRegistry};
use std::sync::Arc;
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

fn startup_page(suffix: &str) -> StartupPageState {
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

fn settings_page(suffix: &str) -> SettingsPageState {
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
fn s1_03_startup_and_settings_flow_through_presentation_seams() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建公开 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(desktop_shell::ShellPresenter::install(&window));

    // ── 1. 启动入口失败 → 明确失败 + 通知事实；同一入口重试可恢复 ──
    let mut startup_entry = StartupPresentationEntry::new(TestAdapter::returning(
        Presentation::failed(startup_page("失败")).with_notification(NotificationFact::new(
            Notification::error(
                l10n.t("app.source_tag"),
                l10n.t("dialog.error_title"),
                l10n.t("app.startup_failure_body"),
            ),
        )),
    ));
    startup_entry.show(&window, &center, StartupRequest::Show);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Failed,
        "启动数据读取失败必须呈现明确失败状态"
    );
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.body == l10n.t("app.startup_failure_body")),
        "启动失败必须产生通知事实（B7 留底，由通知中心决定呈现）"
    );
    startup_entry.replace_adapter(TestAdapter::returning(
        Presentation::ready(startup_page("重试"))
            .with_navigation(NavigationDecision::Show(Screen::FirstRunSetup)),
    ));
    startup_entry.show(&window, &center, StartupRequest::Show);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Ready,
        "同一入口再次请求即重试，失败后必须可恢复"
    );

    // ── 2. 设置入口失败 → 明确失败；重试成功 ──
    let mut settings_entry = SettingsPresentationEntry::new(TestAdapter::returning(
        Presentation::failed(settings_page("失败")).with_notification(NotificationFact::new(
            Notification::error(
                l10n.t("app.source_tag"),
                l10n.t("dialog.error_title"),
                l10n.t("settings.settings_load_failed"),
            ),
        )),
    ));
    settings_entry.show(&window, &center, SettingsRequest::Show);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Failed,
        "设置读取失败必须呈现明确失败状态"
    );
    settings_entry.replace_adapter(TestAdapter::returning(Presentation::succeeded(
        settings_page("重试"),
    )));
    settings_entry.show(&window, &center, SettingsRequest::Show);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded,
        "设置入口重试成功后必须离开失败状态"
    );

    // ── 3. 生产装配：首跑 → 设置页读写 → 保存/测试/清除密钥通知事实 ──
    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-03.db");
    let injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("正式连接组"))
            .expect("正式注入器");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    assert_eq!(window.get_active_screen(), 0, "全新库必须落在首次设置");
    window.set_wizard_acknowledged(true);

    // 首启向导集成高德配置（ADR-0004）：缺失必填 Key 时"继续"被拒绝，页面停留
    window.invoke_wizard_continue_clicked();
    assert_eq!(
        window.get_active_screen(),
        0,
        "缺少必填的高德 API Key 时不得完成首启"
    );
    assert!(
        center.board_snapshot().iter().any(|record| {
            record.title == l10n.t("dialog.error_title") && record.body.contains("API Key")
        }),
        "缺失必填 Key 必须明确指出缺失项（通知事实）"
    );

    // 填写三个 Key 后继续：一并保存（B2/设置快照校验），进入校区选择页
    let wizard_api_key = "abc123DEF456ghi789";
    let wizard_security_key = "xyz789GHI012mno345";
    let wizard_web_service_key = "web123DEF456ghi789";
    window.set_wizard_gaode_api_key(wizard_api_key.into());
    window.set_wizard_gaode_security_key(wizard_security_key.into());
    window.set_wizard_gaode_web_service_key(wizard_web_service_key.into());
    window.invoke_wizard_continue_clicked();
    assert_eq!(
        window.get_active_screen(),
        1,
        "首次设置完成后进入校区搜索与最近记录页"
    );
    assert_eq!(
        window.get_status_text().as_str(),
        l10n.t("app.shell_status_campus_select")
    );

    let first_run_saved = SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    assert_eq!(
        first_run_saved.gaode_api_key().expect("读 API Key"),
        Some(wizard_api_key.to_owned()),
        "首启完成时必须把 Web 端 JS API Key 一并保存"
    );
    assert_eq!(
        first_run_saved.gaode_security_key().expect("读安全密钥"),
        Some(wizard_security_key.to_owned()),
        "首启完成时必须把安全密钥一并保存"
    );
    assert_eq!(
        first_run_saved
            .gaode_web_service_key()
            .expect("读 Web 服务 Key"),
        Some(wizard_web_service_key.to_owned()),
        "首启完成时必须把 Web 服务 Key 一并保存"
    );

    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "设置入口导航到常规设置页");
    assert_eq!(
        window.get_gaode_api_key().as_str(),
        wizard_api_key,
        "设置页必须读回首启保存的 API Key，不要求用户重复配置"
    );
    assert_eq!(
        window.get_gaode_security_key().as_str(),
        wizard_security_key,
        "设置页必须读回首启保存的安全密钥"
    );
    assert_eq!(
        window.get_settings_language().as_str(),
        global_settings::DEFAULT_LANGUAGE
    );
    assert_eq!(
        window.get_settings_version().as_str(),
        global_settings::DEFAULT_MINECRAFT_VERSION
    );
    assert!(
        !window.get_settings_export_location().is_empty(),
        "设置入口必须读回默认导出位置"
    );

    // 常规设置保存：默认导出位置经设置入口读写
    let export_location = format!("{}\\导出测试", directory.path().display());
    window.set_settings_export_location(export_location.clone().into());
    window.invoke_settings_save_clicked();
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("settings.save_success")),
        "保存常规设置成功必须产生通知事实"
    );
    let settings_manager =
        SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    assert_eq!(
        settings_manager
            .default_export_location()
            .expect("读取默认导出位置"),
        export_location,
        "默认导出位置必须经 F1 持久化"
    );

    // 保存密钥（与测试连通性分开）
    let api_key = "abc123DEF456ghi789";
    let security_key = "xyz789GHI012mno345";
    window.set_gaode_api_key(api_key.into());
    window.set_gaode_security_key(security_key.into());
    window.invoke_gaode_save_clicked();
    assert!(
        center.board_snapshot().iter().any(|record| {
            record.title == l10n.t("settings.save_success")
                && record.body == l10n.t("settings.gaode_save_success_body")
        }),
        "保存密钥成功必须产生对应通知事实"
    );

    // 测试连通性失败：错误通知事实，页面保留可重试
    window.set_gaode_api_key("short".into());
    window.set_gaode_security_key("short".into());
    window.invoke_gaode_test_clicked();
    assert!(
        center.board_snapshot().iter().any(|record| {
            record.title == l10n.t("dialog.error_title")
                && record.body.contains("无法连接高德地图服务")
        }),
        "测试失败必须产生错误通知事实"
    );

    // 测试连通性成功：独立成功通知事实
    window.set_gaode_api_key("abc123DEF456ghi789jkl012".into());
    window.set_gaode_security_key("xyz789GHI012mno345pqr678".into());
    window.invoke_gaode_test_clicked();
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("settings.gaode_test_success_title")),
        "测试成功必须产生对应通知事实"
    );

    // 清除密钥：先确认，取消不提交，确认后一次清除并通知
    window.invoke_gaode_clear_clicked();
    assert!(
        window.get_confirm_dialog_visible(),
        "清除密钥必须显示确认窗"
    );
    assert!(
        !center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("settings.gaode_cleared_title")),
        "确认前不得产生清除成功通知"
    );
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    let before_cancel = SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    assert_eq!(
        before_cancel.gaode_api_key().expect("读密钥"),
        Some(api_key.to_owned()),
        "取消清除不得改动已保存密钥"
    );
    window.invoke_gaode_clear_clicked();
    window.invoke_confirm_dialog_confirmed();
    let after_clear = SettingsManager::new(Database::open(&database_path).expect("重开设置库"));
    assert_eq!(after_clear.gaode_api_key().expect("读密钥"), None);
    assert_eq!(after_clear.gaode_security_key().expect("读密钥"), None);
    assert!(
        center
            .board_snapshot()
            .iter()
            .any(|record| record.title == l10n.t("settings.gaode_cleared_title")),
        "确认清除后必须产生通知事实"
    );
    assert_eq!(
        window.get_gaode_api_key().as_str(),
        "",
        "清除后设置页输入与已保存值必须为空"
    );

    // 重看教程仍可用（F2 进度清零，经设置入口转发）
    window.invoke_replay_tutorial_clicked();
    let database = Database::open(&database_path).expect("重开引导库");
    let reloaded =
        onboarding_tutorial::OnboardingTutorial::load(&database).expect("重新装载引导进度");
    assert_eq!(
        reloaded.status(),
        onboarding_tutorial::TutorialStatus::NotStarted,
        "重看教程必须经设置入口清零 F2 进度"
    );
}
