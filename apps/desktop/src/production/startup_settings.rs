//! 工单 03 的启动与设置生产适配器。
//!
//! 每个请求一次调用 F1（global-settings）与 F2 的公开接口，并把结果整理成
//! S1 可直接呈现的页面状态、操作结果、导航决定与通知事实；不持有正式设置副本，
//! 数据读写失败时明确失败并允许同一入口重试。

use std::cell::RefCell;
use std::rc::Rc;

use crate::presentation::{
    CampusPlanPageState, ConfirmationPresentation, NavigationDecision, NotificationFact,
    Presentation, PresentationAdapter, Screen, SettingsPageState, SettingsRequest,
    StartupPageState, StartupRequest,
};
use crate::runtime::format_relative_time;
use crate::{CampusData, PlanCardData, ViewModelInjector};

use super::workspace_boundary::WorkspaceProductionContext;
use global_settings::{
    FirstRunSetup, SettingsSnapshot, StartupDestination, StartupLandingContentProvider,
    StartupSnapshot,
};
use localization::Localization;
use notification_center::Notification;
use project_management::{CampusPlanSnapshot, ProjectManager};

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
    pub(crate) workspace: WorkspaceProductionContext,
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
            api_key,
            security_key,
            web_service_key,
        } = &request
        {
            // ADR-0004：JS API Key 与安全密钥必填，缺失时不得保存并明确指出
            // 缺失项；三个 Key 复用设置页 save_gaode_keys 语义一并落库。
            if let Err(error) =
                save_first_run_gaode_keys(&mut injector, api_key, security_key, web_service_key)
            {
                // 校验失败仍停留首次设置页，错误由通知中心呈现；保留已填输入
                let mut page = startup_page_with_values(
                    injector.l10n(),
                    language,
                    minecraft_version,
                    api_key,
                    security_key,
                    web_service_key,
                );
                page.acknowledged = *acknowledged;
                page.status_text = injector.l10n().t("app.shell_status_first_run");
                return Presentation::failed(page)
                    .with_notification(error_fact(injector.l10n(), &error.to_string()));
            }
            let setup = FirstRunSetup {
                language: language.clone(),
                minecraft_version: minecraft_version.clone(),
                acknowledged: *acknowledged,
            };
            if let Err(error) = injector.settings_mut().complete_first_run(&setup) {
                // 校验失败仍停留首次设置页，错误由通知中心呈现；保留已填输入
                let mut page = startup_page_with_values(
                    injector.l10n(),
                    language,
                    minecraft_version,
                    api_key,
                    security_key,
                    web_service_key,
                );
                page.acknowledged = *acknowledged;
                page.status_text = injector.l10n().t("app.shell_status_first_run");
                return Presentation::failed(page)
                    .with_notification(error_fact(injector.l10n(), &error.to_string()));
            }
        }

        let provider = CampusPlanLandingProvider(injector.projects());
        // 首启向导输入预填：已保存的高德配置（部分完成的首次设置不丢输入）
        let saved_keys = match injector.settings().settings_snapshot() {
            Ok(snapshot) => (
                snapshot.gaode_api_key.unwrap_or_default(),
                snapshot.gaode_security_key.unwrap_or_default(),
                snapshot.gaode_web_service_key.unwrap_or_default(),
            ),
            Err(_) => (String::new(), String::new(), String::new()),
        };
        match injector.settings().startup_result(&provider) {
            Ok(result) => {
                let snapshot = result.snapshot;
                let landing_page = result.landing_content.map(|campus_plan| {
                    let show_plan_list = matches!(
                        &snapshot.destination,
                        StartupDestination::LastUsedCampus { .. }
                    );
                    campus_plan_page(&injector, &self.workspace, campus_plan, show_plan_list)
                });
                let (page, destination) = startup_landing(
                    injector.l10n(),
                    snapshot,
                    landing_page,
                    saved_keys.0,
                    saved_keys.1,
                    saved_keys.2,
                );
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
    gaode_api_key: String,
    gaode_security_key: String,
    gaode_web_service_key: String,
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
        ..startup_page(
            l10n,
            Some(snapshot),
            &gaode_api_key,
            &gaode_security_key,
            &gaode_web_service_key,
        )
    };
    (page, destination)
}

fn startup_page(
    l10n: &Localization,
    snapshot: Option<StartupSnapshot>,
    gaode_api_key: &str,
    gaode_security_key: &str,
    gaode_web_service_key: &str,
) -> StartupPageState {
    let settings = snapshot
        .map(|snapshot| snapshot.settings)
        .unwrap_or_default();
    startup_page_with_values(
        l10n,
        &settings.language,
        &settings.minecraft_version,
        gaode_api_key,
        gaode_security_key,
        gaode_web_service_key,
    )
}

fn startup_page_with_values(
    l10n: &Localization,
    language: &str,
    minecraft_version: &str,
    gaode_api_key: &str,
    gaode_security_key: &str,
    gaode_web_service_key: &str,
) -> StartupPageState {
    StartupPageState {
        app_title: l10n.t("app.welcome_title"),
        status_text: l10n.t("app.shell_status_load_failed"),
        wizard_title: l10n.t("settings.wizard_title"),
        language_label: l10n.t("settings.language_label"),
        version_label: l10n.t("settings.minecraft_version_label"),
        notice_text: l10n.t("settings.notice_checkbox"),
        continue_label: l10n.t("settings.continue_button"),
        wizard_gaode_group_title: l10n.t("settings.wizard_gaode_group_title"),
        wizard_gaode_api_key_label: l10n.t("settings.wizard_gaode_api_key_label"),
        wizard_gaode_api_key_placeholder: l10n.t("settings.wizard_gaode_api_key_placeholder"),
        wizard_gaode_security_key_label: l10n.t("settings.wizard_gaode_security_key_label"),
        wizard_gaode_security_key_placeholder: l10n
            .t("settings.wizard_gaode_security_key_placeholder"),
        wizard_gaode_web_service_key_label: l10n.t("settings.wizard_gaode_web_service_key_label"),
        wizard_gaode_web_service_key_placeholder: l10n
            .t("settings.wizard_gaode_web_service_key_placeholder"),
        wizard_gaode_api_key: gaode_api_key.to_owned(),
        wizard_gaode_security_key: gaode_security_key.to_owned(),
        wizard_gaode_web_service_key: gaode_web_service_key.to_owned(),
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
        wizard_gaode_group_title: l10n.t("settings.wizard_gaode_group_title"),
        wizard_gaode_api_key_label: l10n.t("settings.wizard_gaode_api_key_label"),
        wizard_gaode_api_key_placeholder: l10n.t("settings.wizard_gaode_api_key_placeholder"),
        wizard_gaode_security_key_label: l10n.t("settings.wizard_gaode_security_key_label"),
        wizard_gaode_security_key_placeholder: l10n
            .t("settings.wizard_gaode_security_key_placeholder"),
        wizard_gaode_web_service_key_label: l10n.t("settings.wizard_gaode_web_service_key_label"),
        wizard_gaode_web_service_key_placeholder: l10n
            .t("settings.wizard_gaode_web_service_key_placeholder"),
        wizard_gaode_api_key: String::new(),
        wizard_gaode_security_key: String::new(),
        wizard_gaode_web_service_key: String::new(),
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
                    Ok(()) => {
                        let flow = injector.export_flow();
                        flow.sync_settings(injector.settings());
                        Presentation::succeeded(page).with_notification(info_fact(
                            injector.l10n(),
                            "settings.save_success",
                            "settings.save_general_success_body",
                        ))
                    }
                    Err(error) => Presentation::failed(page)
                        .with_notification(error_fact(injector.l10n(), &error.to_string())),
                }
            }
            SettingsRequest::SaveKeys {
                api_key,
                security_key,
                web_service_key,
            } => {
                let result =
                    save_gaode_keys(&mut injector, &api_key, &security_key, &web_service_key);
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
    web_service_key: &str,
) -> global_settings::Result<()> {
    injector.settings_mut().set_gaode_api_key(api_key)?;
    injector
        .settings_mut()
        .set_gaode_security_key(security_key)?;
    // Web 服务 Key（T31 regeo）允许留空：ADR-0004 开发人员使用，非必填
    if !web_service_key.is_empty() {
        injector
            .settings_mut()
            .set_gaode_web_service_key(web_service_key)?;
    }
    Ok(())
}

/// 首启向导高德配置保存（复用 [`save_gaode_keys`] 语义）：
/// JS API Key 与安全密钥必填（缺失时明确指出缺失项，ADR-0004），
/// Web 服务 Key 开发人员使用、可留空。
fn save_first_run_gaode_keys(
    injector: &mut ViewModelInjector,
    api_key: &str,
    security_key: &str,
    web_service_key: &str,
) -> global_settings::Result<()> {
    if api_key.trim().is_empty() {
        return Err(global_settings::Error::MissingGaodeKeys(
            "API Key".to_owned(),
        ));
    }
    if security_key.trim().is_empty() {
        return Err(global_settings::Error::MissingGaodeKeys(
            "安全密钥".to_owned(),
        ));
    }
    save_gaode_keys(injector, api_key, security_key, web_service_key)
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
        snapshot
            .gaode_web_service_key
            .as_deref()
            .unwrap_or_default(),
        &snapshot.default_export_location,
    )
}

/// 设置数据无法读取时的明确失败页：不显示任何内存默认值。
fn settings_failure_page(l10n: &Localization) -> SettingsPageState {
    settings_page_values(l10n, "", "", "", "", "", "")
}

fn settings_page_values(
    l10n: &Localization,
    language: &str,
    minecraft_version: &str,
    api_key: &str,
    security_key: &str,
    web_service_key: &str,
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
        web_service_key_label: l10n.t("settings.gaode_web_service_key_label"),
        web_service_key_placeholder: l10n.t("settings.gaode_web_service_key_placeholder"),
        web_service_key: web_service_key.to_owned(),
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

pub(crate) fn campus_plan_page(
    injector: &ViewModelInjector,
    workspace: &WorkspaceProductionContext,
    snapshot: CampusPlanSnapshot,
    toolbar_visible: bool,
) -> CampusPlanPageState {
    let l10n = injector.l10n();
    let campuses = snapshot
        .campuses
        .into_iter()
        .map(|campus| CampusData {
            id: campus.id.into(),
            name: campus.name.into(),
            address: campus.address.into(),
        })
        .collect();
    let plans = snapshot
        .plans
        .into_iter()
        .map(|card| PlanCardData {
            progress_desc: workspace
                .plan_card_progress_text(&card.plan_id, &l10n.t(card.progress.text_key()))
                .into(),
            plan_id: card.plan_id.into(),
            name: card.name.into(),
            last_modified: format_relative_time(l10n, &card.last_modified_at).into(),
        })
        .collect();
    CampusPlanPageState {
        toolbar: super::toolbar(l10n, toolbar_visible),
        campus_select_title: l10n.t("app.campus_select_title"),
        campus_empty_text: l10n.t("app.campus_select_no_campus"),
        campus_settings_label: l10n.t("app.settings_button"),
        campuses,
        campus_search_query: String::new(),
        campus_search_placeholder: l10n.t("app.campus_search_placeholder"),
        campus_search_button_label: l10n.t("campus.search_button"),
        campus_recent_title: l10n.t("campus.recent_section"),
        campus_search_results: Vec::new(),
        campus_search_status: String::new(),
        campus_show_results: false,
        plan_list_title: l10n.t("plan.list_header"),
        campus_name: snapshot
            .landing_campus
            .map(|campus| l10n.t_with_array("app.shell_status_last_campus", &[&campus.name]))
            .unwrap_or_default(),
        create_plan_label: l10n.t("plan.create"),
        back_to_campus_label: l10n.t("app.switch_campus"),
        plan_empty_text: l10n.t("plan.empty_list"),
        rename_label: l10n.t("plan.rename"),
        duplicate_label: l10n.t("plan.duplicate"),
        delete_label: l10n.t("plan.delete"),
        plans,
        tutorial_visible: false,
        tutorial_text: String::new(),
        tutorial_dismiss_label: l10n.t("tutorial.dismiss_button"),
        tutorial_skip_all_label: String::new(),
    }
}

// ────────────────────────────────────────────────────────────────────────────
