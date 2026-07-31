//! 工单 03 的启动与设置生产适配器。
//!
//! 每个请求一次调用 F1（global-settings）与 F2 的公开接口，并把结果整理成
//! S1 可直接呈现的页面状态、操作结果、导航决定与通知事实；不持有正式设置副本，
//! 数据读写失败时明确失败并允许同一入口重试。

use std::cell::RefCell;
use std::rc::Rc;

use global_settings::{
    FirstRunSetup, SettingsSnapshot, StartupDestination, StartupLandingContentProvider,
    StartupSnapshot,
};
use localization::Localization;
use notification_center::Notification;
use project_management::{CampusPlanSnapshot, ProjectManager};

use crate::presentation::{
    ConfirmationPresentation, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, Screen, SettingsPageState, SettingsRequest, StartupPageState,
    StartupRequest,
};
use crate::production::campus_plan_page;
use crate::ViewModelInjector;

#[cfg(test)]
use crate::production::record_entry_call;

struct CampusPlanLandingProvider<'a>(&'a ProjectManager);

impl StartupLandingContentProvider for CampusPlanLandingProvider<'_> {
    type Content = CampusPlanSnapshot;
    type Error = project_management::Error;

    fn landing_content(&self) -> Result<Self::Content, Self::Error> {
        self.0.campus_plan_snapshot()
    }
}

pub(crate) struct StartupProductionAdapter {
    pub(crate) injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<StartupRequest, StartupPageState> for StartupProductionAdapter {
    fn present(&mut self, request: StartupRequest) -> Presentation<StartupPageState> {
        #[cfg(test)]
        record_entry_call(0);
        let mut injector = self.injector.borrow_mut();
        if let StartupRequest::CompleteFirstRun {
            language,
            minecraft_version,
            acknowledged,
        } = &request
        {
            let setup = FirstRunSetup {
                language: language.clone(),
                minecraft_version: minecraft_version.clone(),
                acknowledged: *acknowledged,
            };
            if let Err(error) = injector.settings_mut().complete_first_run(&setup) {
                // 校验失败仍停留首次设置页，错误由通知中心呈现；不切换到内存数据
                let mut page =
                    startup_page_with_values(injector.l10n(), language, minecraft_version);
                page.status_text = injector.l10n().t("app.shell_status_first_run");
                return Presentation::failed(page)
                    .with_notification(error_fact(injector.l10n(), &error.to_string()));
            }
        }

        let provider = CampusPlanLandingProvider(injector.projects());
        match injector.settings().startup_result(&provider) {
            Ok(result) => {
                let snapshot = result.snapshot;
                let landing_page = result.landing_content.map(|campus_plan| {
                    let show_plan_list = matches!(
                        &snapshot.destination,
                        StartupDestination::LastUsedCampus { .. }
                    );
                    campus_plan_page(&injector, campus_plan, show_plan_list)
                });
                let (page, destination) = startup_landing(injector.l10n(), snapshot, landing_page);
                let presentation = if matches!(request, StartupRequest::CompleteFirstRun { .. }) {
                    Presentation::succeeded(page)
                } else {
                    Presentation::ready(page)
                };
                presentation.with_navigation(NavigationDecision::Show(destination))
            }
            Err(_) => Presentation::failed(startup_failure_page(injector.l10n()))
                .with_notification(error_fact(
                    injector.l10n(),
                    &injector.l10n().t("app.startup_failure_body"),
                )),
        }
    }
}

fn startup_landing(
    l10n: &Localization,
    snapshot: StartupSnapshot,
    landing_page: Option<crate::presentation::CampusPlanPageState>,
) -> (StartupPageState, Screen) {
    let (status_text, destination) = match &snapshot.destination {
        StartupDestination::FirstRunSetup => {
            (l10n.t("app.shell_status_first_run"), Screen::FirstRunSetup)
        }
        StartupDestination::CampusSelect => (
            l10n.t("app.shell_status_campus_select"),
            Screen::CampusSelect,
        ),
        StartupDestination::LastUsedCampus { name } => (
            l10n.t_with_array("app.shell_status_last_campus", &[name]),
            Screen::PlanList,
        ),
    };
    let page = StartupPageState {
        status_text,
        landing_page,
        ..startup_page(l10n, Some(snapshot))
    };
    (page, destination)
}

fn startup_page(l10n: &Localization, snapshot: Option<StartupSnapshot>) -> StartupPageState {
    let settings = snapshot
        .map(|snapshot| snapshot.settings)
        .unwrap_or_default();
    startup_page_with_values(l10n, &settings.language, &settings.minecraft_version)
}

fn startup_page_with_values(
    l10n: &Localization,
    language: &str,
    minecraft_version: &str,
) -> StartupPageState {
    StartupPageState {
        app_title: l10n.t("app.welcome_title"),
        status_text: l10n.t("app.shell_status_load_failed"),
        wizard_title: l10n.t("settings.wizard_title"),
        language_label: l10n.t("settings.language_label"),
        version_label: l10n.t("settings.minecraft_version_label"),
        notice_text: l10n.t("settings.notice_checkbox"),
        continue_label: l10n.t("settings.continue_button"),
        language_options: global_settings::SUPPORTED_LANGUAGES
            .iter()
            .map(ToString::to_string)
            .collect(),
        version_options: global_settings::SUPPORTED_MINECRAFT_VERSIONS
            .iter()
            .map(ToString::to_string)
            .collect(),
        selected_language: language.to_owned(),
        selected_version: minecraft_version.to_owned(),
        acknowledged: false,
        landing_page: None,
    }
}

/// 启动数据无法读取时的明确失败页：不显示任何内存默认值或选项。
fn startup_failure_page(l10n: &Localization) -> StartupPageState {
    StartupPageState {
        app_title: l10n.t("app.welcome_title"),
        status_text: l10n.t("app.shell_status_load_failed"),
        wizard_title: l10n.t("settings.wizard_title"),
        language_label: l10n.t("settings.language_label"),
        version_label: l10n.t("settings.minecraft_version_label"),
        notice_text: l10n.t("settings.notice_checkbox"),
        continue_label: l10n.t("settings.continue_button"),
        language_options: Vec::new(),
        version_options: Vec::new(),
        selected_language: String::new(),
        selected_version: String::new(),
        acknowledged: false,
        landing_page: None,
    }
}

pub(crate) struct SettingsProductionAdapter {
    pub(crate) injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<SettingsRequest, SettingsPageState> for SettingsProductionAdapter {
    fn present(&mut self, request: SettingsRequest) -> Presentation<SettingsPageState> {
        #[cfg(test)]
        record_entry_call(1);
        let mut injector = self.injector.borrow_mut();
        match request {
            SettingsRequest::Show => match injector.settings().settings_snapshot() {
                Ok(snapshot) => Presentation::ready(settings_page(injector.l10n(), &snapshot))
                    .with_navigation(NavigationDecision::Show(Screen::Settings)),
                Err(_) => Presentation::failed(settings_failure_page(injector.l10n()))
                    .with_notification(error_fact(
                        injector.l10n(),
                        &injector.l10n().t("settings.settings_load_failed"),
                    )),
            },
            SettingsRequest::SaveGeneral {
                language,
                minecraft_version,
                default_export_location,
            } => {
                let result = save_general_settings(
                    &mut injector,
                    &language,
                    &minecraft_version,
                    &default_export_location,
                );
                let page = settings_snapshot_page(&injector, injector.l10n());
                match result {
                    Ok(()) => Presentation::succeeded(page).with_notification(info_fact(
                        injector.l10n(),
                        "settings.save_success",
                        "settings.save_general_success_body",
                    )),
                    Err(error) => Presentation::failed(page)
                        .with_notification(error_fact(injector.l10n(), &error.to_string())),
                }
            }
            SettingsRequest::SaveKeys {
                api_key,
                security_key,
            } => {
                let result = save_gaode_keys(&mut injector, &api_key, &security_key);
                let page = settings_snapshot_page(&injector, injector.l10n());
                match result {
                    Ok(()) => Presentation::succeeded(page).with_notification(info_fact(
                        injector.l10n(),
                        "settings.save_success",
                        "settings.gaode_save_success_body",
                    )),
                    Err(error) => Presentation::failed(page)
                        .with_notification(error_fact(injector.l10n(), &error.to_string())),
                }
            }
            SettingsRequest::TestConnection {
                api_key,
                security_key,
            } => {
                let result = injector
                    .settings()
                    .test_gaode_connection(&api_key, &security_key);
                let mut page = settings_snapshot_page(&injector, injector.l10n());
                // 测试使用页面当前输入，结果页保留这些未提交输入
                page.api_key = api_key;
                page.security_key = security_key;
                match result {
                    Ok(()) => Presentation::succeeded(page).with_notification(info_fact(
                        injector.l10n(),
                        "settings.gaode_test_success_title",
                        "settings.gaode_test_success_body",
                    )),
                    Err(error) => Presentation::failed(page).with_notification(error_fact(
                        injector.l10n(),
                        &injector
                            .l10n()
                            .t_with_array("settings.gaode_test_fail_body", &[&error.to_string()]),
                    )),
                }
            }
            SettingsRequest::ClearKeys => {
                let page = settings_snapshot_page(&injector, injector.l10n());
                Presentation::needs_confirmation(
                    page,
                    ConfirmationPresentation::new(
                        injector.l10n().t("settings.gaode_clear_title"),
                        injector.l10n().t("settings.gaode_clear_body"),
                        injector.l10n().t("dialog.confirm_button"),
                        injector.l10n().t("dialog.cancel_button"),
                    ),
                )
            }
            SettingsRequest::ConfirmClearKeys => {
                let result = injector.settings_mut().clear_gaode_keys();
                let page = settings_snapshot_page(&injector, injector.l10n());
                match result {
                    Ok(()) => Presentation::succeeded(page).with_notification(info_fact(
                        injector.l10n(),
                        "settings.gaode_cleared_title",
                        "settings.gaode_cleared_body",
                    )),
                    Err(error) => Presentation::failed(page)
                        .with_notification(error_fact(injector.l10n(), &error.to_string())),
                }
            }
            SettingsRequest::ReplayTutorial => {
                let result = injector.restart_tutorial();
                let page = settings_snapshot_page(&injector, injector.l10n());
                match result {
                    Ok(()) => Presentation::ready(page),
                    Err(error) => Presentation::failed(page)
                        .with_notification(error_fact(injector.l10n(), &error.to_string())),
                }
            }
        }
    }
}

fn save_general_settings(
    injector: &mut ViewModelInjector,
    language: &str,
    minecraft_version: &str,
    default_export_location: &str,
) -> global_settings::Result<()> {
    injector.settings_mut().set_language(language)?;
    injector
        .settings_mut()
        .set_minecraft_version(minecraft_version)?;
    injector
        .settings_mut()
        .set_default_export_location(default_export_location)?;
    Ok(())
}

fn save_gaode_keys(
    injector: &mut ViewModelInjector,
    api_key: &str,
    security_key: &str,
) -> global_settings::Result<()> {
    injector.settings_mut().set_gaode_api_key(api_key)?;
    injector
        .settings_mut()
        .set_gaode_security_key(security_key)?;
    Ok(())
}

fn settings_snapshot_page(injector: &ViewModelInjector, l10n: &Localization) -> SettingsPageState {
    match injector.settings().settings_snapshot() {
        Ok(snapshot) => settings_page(l10n, &snapshot),
        Err(_) => settings_failure_page(l10n),
    }
}

fn settings_page(l10n: &Localization, snapshot: &SettingsSnapshot) -> SettingsPageState {
    settings_page_values(
        l10n,
        &snapshot.settings.language,
        &snapshot.settings.minecraft_version,
        snapshot.gaode_api_key.as_deref().unwrap_or_default(),
        snapshot.gaode_security_key.as_deref().unwrap_or_default(),
        &snapshot.default_export_location,
    )
}

/// 设置数据无法读取时的明确失败页：不显示任何内存默认值。
fn settings_failure_page(l10n: &Localization) -> SettingsPageState {
    settings_page_values(l10n, "", "", "", "", "")
}

fn settings_page_values(
    l10n: &Localization,
    language: &str,
    minecraft_version: &str,
    api_key: &str,
    security_key: &str,
    export_location: &str,
) -> SettingsPageState {
    SettingsPageState {
        title: l10n.t("app.settings_title"),
        back_label: l10n.t("app.back_button"),
        tutorial_replay_label: l10n.t("tutorial.replay_button"),
        general_group_title: l10n.t("settings.general_group_title"),
        language_label: l10n.t("settings.language_label"),
        language_options: global_settings::SUPPORTED_LANGUAGES
            .iter()
            .map(ToString::to_string)
            .collect(),
        selected_language: language.to_owned(),
        version_label: l10n.t("settings.minecraft_version_label"),
        version_options: global_settings::SUPPORTED_MINECRAFT_VERSIONS
            .iter()
            .map(ToString::to_string)
            .collect(),
        selected_version: minecraft_version.to_owned(),
        export_location_label: l10n.t("settings.export_location_label"),
        export_location_placeholder: l10n.t("settings.export_location_placeholder"),
        default_export_location: export_location.to_owned(),
        save_settings_label: l10n.t("settings.save_settings_button"),
        gaode_group_title: l10n.t("settings.gaode_group_title"),
        api_key_label: l10n.t("settings.gaode_api_key_label"),
        api_key_placeholder: l10n.t("settings.gaode_api_key_placeholder"),
        api_key: api_key.to_owned(),
        security_key_label: l10n.t("settings.gaode_security_key_label"),
        security_key_placeholder: l10n.t("settings.gaode_security_key_placeholder"),
        security_key: security_key.to_owned(),
        save_label: l10n.t("settings.gaode_save_button"),
        test_label: l10n.t("settings.gaode_test_button"),
        clear_keys_label: l10n.t("settings.gaode_clear_button"),
        status_message: String::new(),
    }
}

fn info_fact(l10n: &Localization, title_key: &str, body_key: &str) -> NotificationFact {
    NotificationFact::new(Notification::info(
        l10n.t("app.source_tag"),
        l10n.t(title_key),
        l10n.t(body_key),
    ))
}

fn error_fact(l10n: &Localization, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("app.source_tag"),
        l10n.t("dialog.error_title"),
        body.to_owned(),
    ))
}
