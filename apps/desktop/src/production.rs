//! 工单 02 的生产呈现装配。
//!
//! 每个适配器一次调用一个功能模块接口。仍未实施界面的功能只呈现当前占位页，
//! 不在 S1 读取或推演后续业务状态。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use global_settings::{StartupDestination, StartupLandingContentProvider, StartupSnapshot};
use localization::Localization;
use notification_center::{Notification, NotificationActionOutcome, NotificationCenter};
use project_management::{CampusPlanSnapshot, ProjectManager};
use slint::ComponentHandle;

use crate::presentation::{
    CampusPlanPageState, CampusPlanPresentationEntry, CollectionPageState,
    CollectionPresentationEntry, CoveragePageState, CoveragePresentationEntry, ExportPageState,
    ExportPresentationEntry, NavigationDecision, NotificationPageState,
    NotificationPresentationEntry, Presentation, PresentationAdapter, Progress, ReviewPageState,
    ReviewPresentationEntry, Screen, SettingsPageState, SettingsPresentationEntry,
    StartupPageState, StartupPresentationEntry, ToolbarPageState, WorkspacePageState,
};
use crate::presenter::DiagnosticActionRunner;
use crate::theme::format_relative_time;
use crate::{AppWindow, CampusData, NoticeData, PlanCardData, ViewModelInjector};

#[cfg(test)]
static ENTRY_CALLS: [std::sync::atomic::AtomicUsize; 8] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 8];

#[cfg(test)]
fn record_entry_call(index: usize) {
    ENTRY_CALLS[index].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn reset_entry_calls() {
    for counter in &ENTRY_CALLS {
        counter.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) fn entry_calls() -> [usize; 8] {
    std::array::from_fn(|index| ENTRY_CALLS[index].load(std::sync::atomic::Ordering::SeqCst))
}

struct CampusPlanLandingProvider<'a>(&'a ProjectManager);

impl StartupLandingContentProvider for CampusPlanLandingProvider<'_> {
    type Content = CampusPlanSnapshot;
    type Error = project_management::Error;

    fn landing_content(&self) -> Result<Self::Content, Self::Error> {
        self.0.campus_plan_snapshot()
    }
}

struct StartupProductionAdapter {
    injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<(), StartupPageState> for StartupProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<StartupPageState> {
        #[cfg(test)]
        record_entry_call(0);
        let injector = self.injector.borrow();
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
                startup_presentation(injector.l10n(), snapshot, landing_page)
            }
            Err(_) => Presentation::failed(startup_page(injector.l10n(), None)),
        }
    }
}

fn startup_presentation(
    l10n: &Localization,
    snapshot: StartupSnapshot,
    landing_page: Option<CampusPlanPageState>,
) -> Presentation<StartupPageState> {
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
    Presentation::ready(StartupPageState {
        status_text,
        landing_page,
        ..startup_page(l10n, Some(snapshot))
    })
    .with_navigation(NavigationDecision::Show(destination))
}

fn startup_page(l10n: &Localization, snapshot: Option<StartupSnapshot>) -> StartupPageState {
    let settings = snapshot
        .map(|snapshot| snapshot.settings)
        .unwrap_or_default();
    StartupPageState {
        app_title: l10n.t("app.welcome_title"),
        status_text: l10n.t("app.shell_status_first_run"),
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
        selected_language: settings.language,
        selected_version: settings.minecraft_version,
        acknowledged: false,
        landing_page: None,
    }
}

struct SettingsProductionAdapter {
    injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<(), SettingsPageState> for SettingsProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<SettingsPageState> {
        #[cfg(test)]
        record_entry_call(1);
        let injector = self.injector.borrow();
        let presentation = match injector.settings().settings_snapshot() {
            Ok(snapshot) => Presentation::ready(settings_page(
                injector.l10n(),
                snapshot.gaode_api_key.unwrap_or_default(),
                snapshot.gaode_security_key.unwrap_or_default(),
            )),
            Err(_) => {
                Presentation::failed(settings_page(injector.l10n(), String::new(), String::new()))
            }
        };
        presentation.with_navigation(NavigationDecision::Show(Screen::Settings))
    }
}

fn settings_page(l10n: &Localization, api_key: String, security_key: String) -> SettingsPageState {
    SettingsPageState {
        title: l10n.t("app.settings_title"),
        back_label: l10n.t("app.back_button"),
        tutorial_replay_label: l10n.t("tutorial.replay_button"),
        gaode_group_title: l10n.t("settings.gaode_group_title"),
        api_key_label: l10n.t("settings.gaode_api_key_label"),
        api_key_placeholder: l10n.t("settings.gaode_api_key_placeholder"),
        api_key,
        security_key_label: l10n.t("settings.gaode_security_key_label"),
        security_key_placeholder: l10n.t("settings.gaode_security_key_placeholder"),
        security_key,
        save_label: l10n.t("settings.gaode_save_button"),
        test_label: l10n.t("settings.gaode_test_button"),
        status_message: String::new(),
    }
}

#[derive(Clone, Copy)]
enum CampusPlanRequest {
    CampusSelect,
    PlanList,
}

struct CampusPlanProductionAdapter {
    injector: Rc<RefCell<ViewModelInjector>>,
}

impl PresentationAdapter<CampusPlanRequest, CampusPlanPageState> for CampusPlanProductionAdapter {
    fn present(&mut self, request: CampusPlanRequest) -> Presentation<CampusPlanPageState> {
        #[cfg(test)]
        record_entry_call(2);
        let injector = self.injector.borrow();
        let presentation = match injector.projects().campus_plan_snapshot() {
            Ok(snapshot) => Presentation::ready(campus_plan_page(
                &injector,
                snapshot,
                matches!(request, CampusPlanRequest::PlanList),
            )),
            Err(_) => Presentation::failed(campus_plan_page(
                &injector,
                CampusPlanSnapshot {
                    campuses: Vec::new(),
                    landing_campus: None,
                    plans: Vec::new(),
                },
                matches!(request, CampusPlanRequest::PlanList),
            )),
        };
        let screen = match request {
            CampusPlanRequest::CampusSelect => Screen::CampusSelect,
            CampusPlanRequest::PlanList => Screen::PlanList,
        };
        presentation.with_navigation(NavigationDecision::Show(screen))
    }
}

fn campus_plan_page(
    injector: &ViewModelInjector,
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
        })
        .collect();
    let plans = snapshot
        .plans
        .into_iter()
        .map(|card| PlanCardData {
            progress_desc: injector
                .plan_card_progress_text(&card.plan_id, &l10n.t(card.progress.text_key()))
                .into(),
            plan_id: card.plan_id.into(),
            name: card.name.into(),
            last_modified: format_relative_time(l10n, &card.last_modified_at).into(),
        })
        .collect();
    CampusPlanPageState {
        toolbar: toolbar(l10n, toolbar_visible),
        campus_select_title: l10n.t("app.campus_select_title"),
        campus_empty_text: l10n.t("app.campus_select_no_campus"),
        new_demo_campus_label: l10n.t("app.new_demo_button"),
        campus_settings_label: l10n.t("app.settings_button"),
        campuses,
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

fn workspace(l10n: &Localization, window: Option<&AppWindow>) -> WorkspacePageState {
    WorkspacePageState {
        toolbar: toolbar(l10n, true),
        completed_steps: window.map_or(0, AppWindow::get_workspace_completed_steps),
        placeholder_title: l10n.t("workspace.placeholder_title"),
        placeholder_subtitle: l10n.t("workspace.placeholder_subtitle"),
        pending_notice: l10n.t("workspace.step_pending_notice"),
        title_step_label: l10n.t("collection.title"),
        boundary_step_label: l10n.t("collection.boundary_step"),
        orientation_step_label: l10n.t("collection.orientation_step"),
        collection_step_label: l10n.t("collection.collect_button"),
        review_step_label: l10n.t("review.workbench_title"),
        export_step_label: l10n.t("export.confirm_title"),
        tutorial_visible: window.is_some_and(AppWindow::get_workspace_tutorial_visible),
        tutorial_text: window
            .map(AppWindow::get_workspace_tutorial_text)
            .unwrap_or_default()
            .to_string(),
        tutorial_dismiss_label: l10n.t("tutorial.dismiss_button"),
        tutorial_skip_all_label: window
            .map(AppWindow::get_workspace_tutorial_skip_all_label)
            .unwrap_or_default()
            .to_string(),
    }
}

#[derive(Clone)]
struct WorkspaceProductionContext {
    injector: Rc<RefCell<ViewModelInjector>>,
    window: slint::Weak<AppWindow>,
}

impl WorkspaceProductionContext {
    fn page(&self) -> WorkspacePageState {
        let injector = self.injector.borrow();
        let window = self.window.upgrade();
        workspace(injector.l10n(), window.as_ref())
    }
}

struct CollectionProductionAdapter(WorkspaceProductionContext);

impl PresentationAdapter<(), CollectionPageState> for CollectionProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<CollectionPageState> {
        #[cfg(test)]
        record_entry_call(3);
        Presentation::ready(CollectionPageState {
            workspace: self.0.page(),
        })
        .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }
}

struct ReviewProductionAdapter(WorkspaceProductionContext);

impl PresentationAdapter<(), ReviewPageState> for ReviewProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<ReviewPageState> {
        #[cfg(test)]
        record_entry_call(4);
        Presentation::ready(ReviewPageState {
            workspace: self.0.page(),
        })
        .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }
}

struct CoverageProductionAdapter(WorkspaceProductionContext);

impl PresentationAdapter<(), CoveragePageState> for CoverageProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<CoveragePageState> {
        #[cfg(test)]
        record_entry_call(5);
        Presentation::ready(CoveragePageState {
            workspace: self.0.page(),
        })
    }
}

struct ExportProductionAdapter(WorkspaceProductionContext);

impl PresentationAdapter<(), ExportPageState> for ExportProductionAdapter {
    fn present(&mut self, (): ()) -> Presentation<ExportPageState> {
        #[cfg(test)]
        record_entry_call(6);
        Presentation::ready(ExportPageState {
            workspace: self.0.page(),
        })
        .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }
}

#[derive(Clone)]
struct NotificationLabels {
    toolbar: ToolbarPageState,
    title: String,
    empty_list_text: String,
    archive_label: String,
    date_today: String,
    date_yesterday: String,
    importance_high_label: String,
    unread_marker: String,
    diagnostic_action_label: String,
}

impl NotificationLabels {
    fn new(l10n: &Localization) -> Self {
        Self {
            toolbar: toolbar(l10n, true),
            title: l10n.t("notice.page_title"),
            empty_list_text: l10n.t("notice.empty_list"),
            archive_label: l10n.t("notice.archive_button"),
            date_today: l10n.t("notice.date_today"),
            date_yesterday: l10n.t("notice.date_yesterday"),
            importance_high_label: l10n.t("notice.importance_high"),
            unread_marker: l10n.t("notice.unread_marker"),
            diagnostic_action_label: l10n.t("notice.diagnostic_action"),
        }
    }
}

struct DiagnosticFailureLabels {
    source: String,
    title: String,
    panicked_body: String,
}

#[derive(Clone)]
enum NotificationRequest {
    Open,
    Processing,
    Succeeded,
    Failed,
}

struct NotificationProductionAdapter {
    center: Arc<NotificationCenter>,
    labels: NotificationLabels,
}

impl PresentationAdapter<NotificationRequest, NotificationPageState>
    for NotificationProductionAdapter
{
    fn present(&mut self, request: NotificationRequest) -> Presentation<NotificationPageState> {
        #[cfg(test)]
        record_entry_call(7);
        if matches!(&request, NotificationRequest::Open) {
            self.center.mark_board_opened();
        }
        let notices = self
            .center
            .board_records()
            .into_iter()
            .map(|record| {
                let notification = record.notification();
                NoticeData {
                    id: notification.id.to_string().into(),
                    title: notification.title.clone().into(),
                    body: notification.body.clone().into(),
                    date: self.labels.date_today.clone().into(),
                    importance: if notification.level.is_error() {
                        "high".into()
                    } else {
                        "normal".into()
                    },
                    read: false,
                    has_diagnostic_action: record.has_diagnostic_action(),
                }
            })
            .collect();
        let page = NotificationPageState {
            toolbar: self.labels.toolbar.clone(),
            title: self.labels.title.clone(),
            empty_list_text: self.labels.empty_list_text.clone(),
            archive_label: self.labels.archive_label.clone(),
            date_today: self.labels.date_today.clone(),
            date_yesterday: self.labels.date_yesterday.clone(),
            importance_high_label: self.labels.importance_high_label.clone(),
            unread_marker: self.labels.unread_marker.clone(),
            diagnostic_action_label: self.labels.diagnostic_action_label.clone(),
            notices,
        };
        match request {
            NotificationRequest::Open => Presentation::ready(page)
                .with_navigation(NavigationDecision::Show(Screen::Notifications)),
            NotificationRequest::Processing => Presentation::processing(page, Progress::ZERO),
            NotificationRequest::Succeeded => Presentation::succeeded(page),
            NotificationRequest::Failed => Presentation::failed(page),
        }
    }
}

fn toolbar(l10n: &Localization, visible: bool) -> ToolbarPageState {
    ToolbarPageState {
        title: l10n.t("app.welcome_title"),
        notice_visible: visible,
        notice_label: l10n.t("messages.notification_center"),
        switch_campus_visible: visible,
        switch_campus_label: l10n.t("app.switch_campus"),
        trash_visible: visible,
        trash_label: l10n.t("trash.page_title"),
        settings_visible: visible,
        settings_label: l10n.t("app.settings_button"),
    }
}

/// 与应用窗口同寿命的八类生产呈现入口；组合根只持有端口，不理解功能内部步骤。
pub(crate) struct ProductionEntries {
    startup: StartupPresentationEntry<'static, ()>,
    settings: SettingsPresentationEntry<'static, ()>,
    campus_plan: CampusPlanPresentationEntry<'static, CampusPlanRequest>,
    collection: CollectionPresentationEntry<'static, ()>,
    review: ReviewPresentationEntry<'static, ()>,
    _coverage: CoveragePresentationEntry<'static, ()>,
    export: ExportPresentationEntry<'static, ()>,
    notification: NotificationPresentationEntry<'static, NotificationRequest>,
    center: Arc<NotificationCenter>,
    action_runner: DiagnosticActionRunner,
    diagnostic_failure: DiagnosticFailureLabels,
}

impl ProductionEntries {
    pub(crate) fn new(
        injector: Rc<RefCell<ViewModelInjector>>,
        window: &AppWindow,
        center: Arc<NotificationCenter>,
    ) -> Self {
        let labels = NotificationLabels::new(injector.borrow().l10n());
        let diagnostic_failure = {
            let injector = injector.borrow();
            DiagnosticFailureLabels {
                source: injector.l10n().t("app.source_tag"),
                title: injector.l10n().t("dialog.error_title"),
                panicked_body: injector.l10n().t("notice.diagnostic_action_panicked"),
            }
        };
        let workspace = WorkspaceProductionContext {
            injector: Rc::clone(&injector),
            window: window.as_weak(),
        };
        Self {
            startup: StartupPresentationEntry::new(StartupProductionAdapter {
                injector: Rc::clone(&injector),
            }),
            settings: SettingsPresentationEntry::new(SettingsProductionAdapter {
                injector: Rc::clone(&injector),
            }),
            campus_plan: CampusPlanPresentationEntry::new(CampusPlanProductionAdapter { injector }),
            collection: CollectionPresentationEntry::new(CollectionProductionAdapter(
                workspace.clone(),
            )),
            review: ReviewPresentationEntry::new(ReviewProductionAdapter(workspace.clone())),
            _coverage: CoveragePresentationEntry::new(CoverageProductionAdapter(workspace.clone())),
            export: ExportPresentationEntry::new(ExportProductionAdapter(workspace)),
            notification: NotificationPresentationEntry::new(NotificationProductionAdapter {
                center: Arc::clone(&center),
                labels,
            }),
            center,
            action_runner: DiagnosticActionRunner::default(),
            diagnostic_failure,
        }
    }

    fn supersede_diagnostic(&self, window: &AppWindow) {
        self.action_runner.invalidate();
        window.set_diagnostic_operation_state(crate::OperationPresentationState::Ready);
        window.set_diagnostic_operation_progress(0);
    }

    pub(crate) fn show_startup(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.startup.show(window, &self.center, ());
    }

    pub(crate) fn show_settings(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings.show(window, &self.center, ());
    }

    pub(crate) fn show_campus_select(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::CampusSelect);
    }

    pub(crate) fn show_plan_list(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::PlanList);
    }

    pub(crate) fn show_collection(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.collection.show(window, &self.center, ());
    }

    pub(crate) fn show_review(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.review.show(window, &self.center, ());
    }

    pub(crate) fn show_export(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.export.show(window, &self.center, ());
    }

    #[cfg(test)]
    pub(crate) fn show_coverage_for_test(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self._coverage.show(window, &self.center, ());
    }

    pub(crate) fn show_notifications(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.notification
            .show(window, &self.center, NotificationRequest::Open);
    }

    fn start_diagnostic_action(&mut self, window: &AppWindow, notification_id: &str) {
        let Some(action) = self.center.diagnostic_action(notification_id) else {
            return;
        };
        self.notification
            .show_diagnostic(window, &self.center, NotificationRequest::Processing);
        self.action_runner.start(action, window.as_weak());
    }

    fn finish_diagnostic_actions(&mut self, window: &AppWindow) {
        let mut latest_request = None;
        for completed in self.action_runner.drain() {
            let (is_latest, outcome) = completed.into_parts();
            match outcome {
                Some(outcome) => {
                    if is_latest {
                        latest_request = Some(if outcome.is_failed() {
                            NotificationRequest::Failed
                        } else {
                            NotificationRequest::Succeeded
                        });
                    }
                    self.center.publish_action_outcome(outcome, is_latest);
                }
                None => {
                    if is_latest {
                        latest_request = Some(NotificationRequest::Failed);
                    }
                    self.center.publish_action_outcome(
                        NotificationActionOutcome::failed(Notification::error(
                            self.diagnostic_failure.source.clone(),
                            self.diagnostic_failure.title.clone(),
                            self.diagnostic_failure.panicked_body.clone(),
                        )),
                        is_latest,
                    );
                }
            }
        }
        if let Some(request) = latest_request {
            self.notification
                .show_diagnostic(window, &self.center, request);
        }
    }

    pub(crate) fn bind_actions(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_notice_diagnostic_action_clicked(move |notification_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .start_diagnostic_action(&window, &notification_id);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_error_dialog_diagnostic_action_clicked(move |notification_id| {
            let Some(window) = weak.upgrade() else { return };
            window.invoke_error_dialog_dismissed();
            shared
                .borrow_mut()
                .start_diagnostic_action(&window, &notification_id);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_diagnostic_actions_completed(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().finish_diagnostic_actions(&window);
        });
    }
}
