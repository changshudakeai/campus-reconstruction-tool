//! T19B-2 UI 接线集成测试（独立进程，单个 #[test] 串行）。
//!
//! Slint 平台只能在一个线程初始化一次，因此所有需要真 AppWindow 的
//! 场景集中在本文件的一个测试函数里顺序验证：
//! 1. VM 视图状态注入（标题/着陆状态/向导文案与默认值）；
//! 2. 首跑向导完成设置（F1 落库 + 跳下一屏）；
//! 3. 设置页"重新查看教程"（F2 进度清零落库，债务②）；
//! 4. B7 ShellPresenter 错误模态遮罩（弹窗铁律 ADR-0021，装喇叭）。

use data_persistence::Database;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{Notification, NotificationCenter, Presenter, PresenterRegistry};
use onboarding_tutorial::{OnboardingTutorial, TutorialStatus};

#[test]
fn ui_bindings_cover_wizard_replay_and_error_dialog() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");

    // ── 场景 1：全新库 → 首跑向导屏，文案与默认值全部来自 F1/B6 ──
    // 跨模块落库断言用同一临时文件连接组（内存库两连接各自独立）
    let dir = tempfile::tempdir().expect("建临时目录");
    let database_path = dir.path().join("ui-test.db");
    let db = ShellDatabases::open(&database_path).expect("文件库连接组");
    let injector = ViewModelInjector::new(db).expect("构造注入器");
    let window = AppWindow::new().expect("创建 Slint 窗口");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let _runtime = assemble_application(&window, injector, center.clone());

    assert_eq!(window.get_app_title().as_str(), l10n.t("app.welcome_title"));
    assert_eq!(
        window.get_status_text().as_str(),
        l10n.t("app.shell_status_first_run")
    );
    assert_eq!(window.get_active_screen(), 0, "全新库应落在首跑向导屏");
    assert!(!window.get_wizard_acknowledged(), "知情告知默认未勾选");
    assert_eq!(
        window.get_wizard_language().as_str(),
        global_settings::DEFAULT_LANGUAGE
    );
    assert_eq!(
        window.get_wizard_version().as_str(),
        global_settings::DEFAULT_MINECRAFT_VERSION
    );
    // 文本键全部已入 zh-CN.json（缺键时 l10n 原样回退键名）
    for text in [
        window.get_wizard_title().to_string(),
        window.get_wizard_language_label().to_string(),
        window.get_wizard_version_label().to_string(),
        window.get_wizard_notice_text().to_string(),
        window.get_wizard_continue_label().to_string(),
        window.get_tutorial_replay_label().to_string(),
        window.get_error_dialog_ok_label().to_string(),
        window.get_settings_title().to_string(),
        window.get_settings_back_label().to_string(),
    ] {
        assert!(
            !text.starts_with("settings.")
                && !text.starts_with("tutorial.")
                && !text.starts_with("app.")
                && !text.starts_with("dialog."),
            "文本键未入库: {text}"
        );
    }

    // ── 场景 2：向导完成设置（ADR-0004 双保险 + F1 落库 + 跳屏）──
    let is_first_run = || {
        SettingsManager::new(Database::open(&database_path).expect("重开设置库"))
            .is_first_run()
            .expect("读首次运行标记")
    };
    // 未勾选知情告知 → F1 拒绝，仍停在向导屏（UI 按钮禁用之外的兜底）
    window.set_wizard_acknowledged(false);
    window.invoke_wizard_continue_clicked();
    assert!(is_first_run());
    assert_eq!(window.get_active_screen(), 0);

    // 勾选后完成 → F1 落库；无上次校区 → 校区选择页占位文案
    window.set_wizard_acknowledged(true);
    window.invoke_wizard_continue_clicked();
    assert!(!is_first_run());
    assert_eq!(window.get_active_screen(), 1, "向导完成应跳到着陆占位屏");
    assert_eq!(
        window.get_status_text().as_str(),
        l10n.t("app.shell_status_campus_select")
    );

    // ── 场景 3：设置页（经设置入口刷新）+ 重新查看教程（债务②，F2 规矩④）──
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(
        window.get_settings_title().as_str(),
        l10n.t("app.settings_title"),
        "设置页文案必须经设置入口注入"
    );
    assert_eq!(window.get_active_screen(), 3, "设置入口应导航到设置页");
    window.invoke_replay_tutorial_clicked();
    let database = Database::open(&database_path).expect("重开引导库");
    let reloaded = OnboardingTutorial::load(&database).expect("重新装载引导进度");
    assert_eq!(
        reloaded.status(),
        TutorialStatus::NotStarted,
        "进度已落库清零"
    );

    // ── 场景 4：B7 ShellPresenter 错误模态遮罩（装喇叭）──
    let presenter = ShellPresenter::install(&window);
    let notification = Notification::error("测试来源", "演示标题", "演示正文");
    presenter.show_error_dialog(&notification);
    assert!(
        window.get_error_dialog_visible(),
        "Error 级必须点亮模态遮罩"
    );
    assert_eq!(window.get_error_dialog_title().as_str(), "演示标题");
    assert_eq!(window.get_error_dialog_source().as_str(), "测试来源");
    assert_eq!(window.get_error_dialog_body().as_str(), "演示正文");
    // 用户点"知道了" → 遮罩熄灭
    window.invoke_error_dialog_dismissed();
    assert!(!window.get_error_dialog_visible());
}
