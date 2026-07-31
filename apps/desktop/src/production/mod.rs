//! 工单 02/03 的生产呈现装配。
//!
//! 每个适配器一次调用一个功能模块接口。仍未实施界面的功能只呈现当前占位页，
//! 不在 S1 读取或推演后续业务状态。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use localization::Localization;
use notification_center::{Notification, NotificationActionOutcome, NotificationCenter};
use project_management::CampusPlanSnapshot;
use slint::ComponentHandle;

use crate::presentation::{
    CampusPlanPageState, CampusPlanPresentationEntry, CollectionPageState,
    CollectionPresentationEntry, CoveragePageState, CoveragePresentationEntry, ExportPageState,
    ExportPresentationEntry, NavigationDecision, NotificationPageState,
    NotificationPresentationEntry, Presentation, PresentationAdapter, Progress, ReviewPageState,
    ReviewPresentationEntry, Screen, SettingsPresentationEntry, SettingsRequest,
    StartupPresentationEntry, StartupRequest, ToolbarPageState, WorkspacePageState,
};
mod startup_settings;

use startup_settings::{SettingsProductionAdapter, StartupProductionAdapter};

use crate::presenter::DiagnosticActionRunner;
use crate::theme::format_relative_time;
use crate::{AppWindow, CampusData, NoticeData, PlanCardData, ViewModelInjector};

#[cfg(test)]
static ENTRY_CALLS: [std::sync::atomic::AtomicUsize; 8] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 8];

#[cfg(test)]
pub(crate) fn record_entry_call(index: usize) {
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

pub(crate) fn campus_plan_page(
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
/// 等待用户确认后由设置入口执行的操作。
#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingConfirmation {
    ClearGaodeKeys,
}

pub(crate) struct ProductionEntries {
    startup: StartupPresentationEntry<'static, StartupRequest>,
    settings: SettingsPresentationEntry<'static, SettingsRequest>,
    campus_plan: CampusPlanPresentationEntry<'static, CampusPlanRequest>,
    collection: CollectionPresentationEntry<'static, ()>,
    review: ReviewPresentationEntry<'static, ()>,
    _coverage: CoveragePresentationEntry<'static, ()>,
    export: ExportPresentationEntry<'static, ()>,
    notification: NotificationPresentationEntry<'static, NotificationRequest>,
    center: Arc<NotificationCenter>,
    action_runner: DiagnosticActionRunner,
    diagnostic_failure: DiagnosticFailureLabels,
    pending_confirmation: Option<PendingConfirmation>,
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
            pending_confirmation: None,
        }
    }

    fn supersede_diagnostic(&self, window: &AppWindow) {
        self.action_runner.invalidate();
        window.set_diagnostic_operation_state(crate::OperationPresentationState::Ready);
        window.set_diagnostic_operation_progress(0);
    }

    pub(crate) fn show_startup(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        self.startup
            .show(window, &self.center, StartupRequest::Show);
    }

    pub(crate) fn complete_first_run(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        let request = StartupRequest::CompleteFirstRun {
            language: window.get_wizard_language().to_string(),
            minecraft_version: window.get_wizard_version().to_string(),
            acknowledged: window.get_wizard_acknowledged(),
        };
        self.startup.show(window, &self.center, request);
    }

    pub(crate) fn show_settings(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        self.settings
            .show(window, &self.center, SettingsRequest::Show);
    }

    pub(crate) fn save_settings(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings.show(
            window,
            &self.center,
            SettingsRequest::SaveGeneral {
                language: window.get_settings_language().to_string(),
                minecraft_version: window.get_settings_version().to_string(),
                default_export_location: window.get_settings_export_location().to_string(),
            },
        );
    }

    pub(crate) fn save_keys(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings.show(
            window,
            &self.center,
            SettingsRequest::SaveKeys {
                api_key: window.get_gaode_api_key().to_string(),
                security_key: window.get_gaode_security_key().to_string(),
            },
        );
    }

    pub(crate) fn test_gaode_connection(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings.show(
            window,
            &self.center,
            SettingsRequest::TestConnection {
                api_key: window.get_gaode_api_key().to_string(),
                security_key: window.get_gaode_security_key().to_string(),
            },
        );
    }

    pub(crate) fn request_clear_keys(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.pending_confirmation = Some(PendingConfirmation::ClearGaodeKeys);
        self.settings
            .show(window, &self.center, SettingsRequest::ClearKeys);
    }

    /// 用户确认清除密钥后执行；返回是否消费了设置入口的确认。
    pub(crate) fn confirm_pending_settings(&mut self, window: &AppWindow) -> bool {
        let Some(pending) = self.pending_confirmation.take() else {
            return false;
        };
        match pending {
            PendingConfirmation::ClearGaodeKeys => {
                self.settings
                    .show(window, &self.center, SettingsRequest::ConfirmClearKeys);
                true
            }
        }
    }

    pub(crate) fn cancel_pending_settings(&mut self) {
        self.pending_confirmation = None;
    }

    pub(crate) fn replay_tutorial(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings
            .show(window, &self.center, SettingsRequest::ReplayTutorial);
    }

    pub(crate) fn show_campus_select(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::CampusSelect);
    }

    pub(crate) fn show_plan_list(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
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
        window.on_wizard_continue_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().complete_first_run(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_save_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().save_settings(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_save_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().save_keys(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_test_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().test_gaode_connection(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_clear_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_clear_keys(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_replay_tutorial_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().replay_tutorial(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_back_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().show_campus_select(&window);
            }
        });

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
