//! 工单 02/03 的生产呈现装配。
// ignore-tidy-filelength: 组合根承载全部呈现入口与回调绑定；工作区入口已独立成文件（S1-05），采集/评审/导出迁出后收窄
//!
//! 每个适配器一次调用一个功能模块接口。仍未实施界面的功能只呈现当前占位页，
//! 不在 S1 读取或推演后续业务状态。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use localization::Localization;
use notification_center::{Notification, NotificationActionOutcome, NotificationCenter};
use slint::ComponentHandle;

use crate::presentation::{
    CampusPlanPresentationEntry, CollectionPageState, CollectionPresentationEntry,
    CoveragePageState, CoveragePresentationEntry, ExportPageState, ExportPresentationEntry,
    NavigationDecision, NotificationPageState, NotificationPresentationEntry, OperationState,
    Presentation, PresentationAdapter, Progress, ReviewPageState, ReviewPresentationEntry, Screen,
    SettingsPresentationEntry, SettingsRequest, StartupPresentationEntry, StartupRequest,
    ToolbarPageState, TrashPresentationEntry, TrashRequest, WorkspacePresentationEntry,
    WorkspaceRequest,
};
mod campus_plan_trash;
mod startup_settings;
mod workspace_boundary;

use campus_plan_trash::{CampusPlanProductionAdapter, CampusPlanRequest, TrashProductionAdapter};
use startup_settings::{SettingsProductionAdapter, StartupProductionAdapter};
use workspace_boundary::{WorkspaceProductionAdapter, WorkspaceProductionContext};

use crate::presenter::DiagnosticActionRunner;
use crate::{AppWindow, NoticeData, ViewModelInjector};

#[cfg(test)]
static ENTRY_CALLS: [std::sync::atomic::AtomicUsize; 10] =
    [const { std::sync::atomic::AtomicUsize::new(0) }; 10];

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
pub(crate) fn entry_calls() -> [usize; 10] {
    std::array::from_fn(|index| ENTRY_CALLS[index].load(std::sync::atomic::Ordering::SeqCst))
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
#[derive(Clone, PartialEq, Eq)]
enum PendingConfirmation {
    ClearGaodeKeys,
    DeletePlan {
        plan_id: String,
    },
    PurgePlan {
        trash_id: String,
    },
    ClearTrash,
    /// 边界页缺高德密钥时确认后前往设置页（S1-05）
    GoToSettings,
    /// 离开边界页的确认（S1-05：确认后按目标页导航）
    LeaveWorkspace {
        target: Screen,
    },
    /// 修改朝向的重算确认（S1-05：确认后应用待定角度）
    OrientationRecalc,
}

/// 等待用户确认输入窗后由方案入口执行的操作。
#[derive(Clone, PartialEq, Eq)]
enum PendingInput {
    CreatePlan,
    RenamePlan { plan_id: String },
}

pub(crate) struct ProductionEntries {
    startup: StartupPresentationEntry<'static, StartupRequest>,
    settings: SettingsPresentationEntry<'static, SettingsRequest>,
    campus_plan: CampusPlanPresentationEntry<'static, CampusPlanRequest>,
    collection: CollectionPresentationEntry<'static, ()>,
    workspace: WorkspacePresentationEntry<'static, WorkspaceRequest>,
    review: ReviewPresentationEntry<'static, ()>,
    _coverage: CoveragePresentationEntry<'static, ()>,
    export: ExportPresentationEntry<'static, ()>,
    notification: NotificationPresentationEntry<'static, NotificationRequest>,
    trash: TrashPresentationEntry<'static, TrashRequest>,
    center: Arc<NotificationCenter>,
    action_runner: DiagnosticActionRunner,
    diagnostic_failure: DiagnosticFailureLabels,
    pending_confirmation: Option<PendingConfirmation>,
    pending_input: Option<PendingInput>,
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
        let workspace = WorkspaceProductionContext::new(Rc::clone(&injector), window);
        Self {
            startup: StartupPresentationEntry::new(StartupProductionAdapter {
                injector: Rc::clone(&injector),
                workspace: workspace.clone(),
            }),
            settings: SettingsPresentationEntry::new(SettingsProductionAdapter {
                injector: Rc::clone(&injector),
            }),
            campus_plan: CampusPlanPresentationEntry::new(CampusPlanProductionAdapter {
                injector: Rc::clone(&injector),
                workspace: workspace.clone(),
            }),
            collection: CollectionPresentationEntry::new(CollectionProductionAdapter(
                workspace.clone(),
            )),
            workspace: WorkspacePresentationEntry::new(WorkspaceProductionAdapter {
                context: workspace.clone(),
            }),
            review: ReviewPresentationEntry::new(ReviewProductionAdapter(workspace.clone())),
            _coverage: CoveragePresentationEntry::new(CoverageProductionAdapter(workspace.clone())),
            export: ExportPresentationEntry::new(ExportProductionAdapter(workspace.clone())),
            notification: NotificationPresentationEntry::new(NotificationProductionAdapter {
                center: Arc::clone(&center),
                labels,
            }),
            trash: TrashPresentationEntry::new(TrashProductionAdapter {
                injector: Rc::clone(&injector),
            }),
            center,
            action_runner: DiagnosticActionRunner::default(),
            diagnostic_failure,
            pending_confirmation: None,
            pending_input: None,
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

    /// 用户确认后执行对应的待确认操作；返回是否消费了本次确认。
    pub(crate) fn confirm_pending_action(&mut self, window: &AppWindow) -> bool {
        let Some(pending) = self.pending_confirmation.take() else {
            return false;
        };
        match pending {
            PendingConfirmation::ClearGaodeKeys => {
                self.settings
                    .show(window, &self.center, SettingsRequest::ConfirmClearKeys);
            }
            PendingConfirmation::DeletePlan { plan_id } => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmDeletePlan { plan_id },
                );
            }
            PendingConfirmation::PurgePlan { trash_id } => {
                self.trash.show(
                    window,
                    &self.center,
                    TrashRequest::ConfirmPurge { trash_id },
                );
            }
            PendingConfirmation::ClearTrash => {
                self.trash
                    .show(window, &self.center, TrashRequest::ConfirmClearAll);
            }
            PendingConfirmation::GoToSettings => {
                crate::map_webview::hide();
                self.show_settings(window);
            }
            PendingConfirmation::LeaveWorkspace { target } => {
                self.navigate_to(window, target);
            }
            PendingConfirmation::OrientationRecalc => {
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::ConfirmOrientation);
            }
        }
        true
    }

    pub(crate) fn cancel_pending_action(&mut self, window: &AppWindow) {
        self.pending_confirmation = None;
        self.workspace
            .show(window, &self.center, WorkspaceRequest::CancelConfirmation);
    }

    /// 用户确认输入窗后执行对应的方案操作；返回是否消费了本次输入确认。
    pub(crate) fn confirm_pending_input(&mut self, window: &AppWindow) -> bool {
        let Some(pending) = self.pending_input.take() else {
            return false;
        };
        let name = window.get_input_dialog_text().to_string();
        match pending {
            PendingInput::CreatePlan => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmCreatePlan { name },
                );
            }
            PendingInput::RenamePlan { plan_id } => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmRenamePlan { plan_id, name },
                );
            }
        }
        true
    }

    pub(crate) fn cancel_pending_input(&mut self) {
        self.pending_input = None;
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

    // ── S1-05：工作区导航与边界（统一经工作区功能入口）───────────────────

    /// 方案卡片单击打开工作区（ADR-0027 第 6 轮：单击即开）。
    pub(crate) fn open_workspace_plan(&mut self, window: &AppWindow, plan_id: &str) {
        self.supersede_diagnostic(window);
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::OpenPlan {
                plan_id: plan_id.to_string(),
            },
        );
    }

    /// 步骤点击：功能入口返回允许进入/条件不足/需要确认；占位步骤页由对应入口渲染。
    pub(crate) fn handle_workspace_step_clicked(&mut self, window: &AppWindow, step: i32) {
        self.supersede_diagnostic(window);
        let presentation =
            self.workspace
                .show(window, &self.center, WorkspaceRequest::Navigate { step });
        if presentation.operation() == &OperationState::NeedsConfirmation {
            // 进入边界页且缺少高德密钥：确认后前往设置页
            self.pending_confirmation = Some(PendingConfirmation::GoToSettings);
        }
        if presentation.navigation() == NavigationDecision::Show(Screen::Workspace)
            && (2..=4).contains(&step)
        {
            // 允许进入的占位步骤：与 S1-04 前一致，由对应步骤入口呈现页面
            match step {
                2 => self.show_collection(window),
                3 => self.show_review(window),
                4 => self.show_export(window),
                _ => {}
            }
        }
    }

    pub(crate) fn handle_boundary_canvas_click(&mut self, window: &AppWindow, x: f32, y: f32) {
        self.supersede_diagnostic(window);
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::BoundaryCanvasClick { x, y },
        );
    }

    pub(crate) fn handle_boundary_undo(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.workspace
            .show(window, &self.center, WorkspaceRequest::BoundaryUndo);
    }

    pub(crate) fn handle_boundary_confirm(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.workspace
            .show(window, &self.center, WorkspaceRequest::BoundaryConfirm);
    }

    pub(crate) fn handle_boundary_reset(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.workspace
            .show(window, &self.center, WorkspaceRequest::BoundaryReset);
    }

    pub(crate) fn handle_orientation_submit(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        let presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::OrientationSubmit {
                mode: window.get_workspace_orientation_mode().to_string(),
                angle_text: window.get_workspace_orientation_input_text().to_string(),
            },
        );
        if presentation.operation() == &OperationState::NeedsConfirmation {
            // 覆盖既有朝向：确认后应用待定角度
            self.pending_confirmation = Some(PendingConfirmation::OrientationRecalc);
        }
    }

    pub(crate) fn handle_orientation_reset(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.workspace
            .show(window, &self.center, WorkspaceRequest::OrientationReset);
    }

    pub(crate) fn handle_orientation_mode_changed(&mut self, window: &AppWindow, mode: &str) {
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::OrientationModeChanged {
                mode: mode.to_string(),
            },
        );
    }

    pub(crate) fn handle_workspace_tutorial_dismiss(&mut self, window: &AppWindow) {
        self.workspace
            .show(window, &self.center, WorkspaceRequest::TutorialDismiss);
    }

    pub(crate) fn handle_workspace_tutorial_skip_all(&mut self, window: &AppWindow) {
        self.workspace
            .show(window, &self.center, WorkspaceRequest::TutorialSkipAll);
    }

    /// 地图加载完成状态（成功/故障）：故障只暂停地图相关操作。
    pub(crate) fn handle_map_status(&mut self, window: &AppWindow, available: bool) {
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::MapStatus { available },
        );
    }

    /// 地图 WebView 转交的原始 IPC 消息：由工作区功能入口解析并应用规则。
    pub(crate) fn handle_map_ipc(&mut self, window: &AppWindow, message: &str) {
        let presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::MapIpc {
                message: message.to_string(),
            },
        );
        if presentation.operation() == &OperationState::NeedsConfirmation {
            // 地图确认朝向覆盖既有朝向：与方位角输入提交走同一确认路径
            self.pending_confirmation = Some(PendingConfirmation::OrientationRecalc);
        }
    }

    /// 离开工作区前先经功能入口判定；需要确认时挂起目标页等待确认。
    pub(crate) fn leave_workspace_then(&mut self, window: &AppWindow, target: Screen) {
        if window.get_active_screen() != 4 {
            self.navigate_to(window, target);
            return;
        }
        let presentation =
            self.workspace
                .show(window, &self.center, WorkspaceRequest::Leave { target });
        match presentation.navigation() {
            NavigationDecision::Show(screen) => self.navigate_to(window, screen),
            // 必须停留：功能入口已呈现当前页，不导航
            NavigationDecision::Blocked => {}
            NavigationDecision::Stay => {
                if presentation.operation() == &OperationState::NeedsConfirmation {
                    self.pending_confirmation =
                        Some(PendingConfirmation::LeaveWorkspace { target });
                }
            }
        }
    }

    fn navigate_to(&mut self, window: &AppWindow, target: Screen) {
        match target {
            Screen::Settings => self.show_settings(window),
            Screen::CampusSelect => self.show_campus_select(window),
            Screen::Trash => self.show_trash(window),
            Screen::Notifications => self.show_notifications(window),
            _ => {}
        }
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

        // ── S1-04：校区搜索与最近记录 ────────────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_search_requested(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_campus_search(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_campus_clicked(move |campus_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .select_campus(&window, campus_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_remove_recent_clicked(move |campus_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .remove_recent_campus(&window, campus_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_new_demo_campus_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().create_demo_campus(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_settings_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().show_settings(&window);
            }
        });

        // ── S1-04：方案列表 CRUD ────────────────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_create_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_create_plan(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_rename_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_rename_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_duplicate_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .duplicate_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_delete_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_delete_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_back_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().show_campus_select(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_restore_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .restore_trash_item(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_purge_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_purge_trash_item(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_purge_all_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_clear_trash(&window);
            }
        });

        // ── 输入窗（方案新建/改名，S1-04）──────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_input_dialog_confirmed(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().confirm_pending_input(&window);
            }
        });

        let shared = Rc::clone(entries);
        window.on_input_dialog_cancelled(move || {
            shared.borrow_mut().cancel_pending_input();
        });
        // ── S1-05：工作区导航、边界与朝向门控（统一经工作区功能入口）────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_card_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().open_workspace_plan(&window, &plan_id);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_step_clicked(move |step| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_step_clicked(&window, step);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_canvas_clicked(move |x, y| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_boundary_canvas_click(&window, x, y);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_undo_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_undo(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_confirm_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_confirm(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_reset_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_reset(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_submit_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_orientation_submit(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_reset_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_orientation_reset(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_mode_changed(move |mode| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_orientation_mode_changed(&window, &mode);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_tutorial_dismiss_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_tutorial_dismiss(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_tutorial_skip_all_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_tutorial_skip_all(&window);
            }
        });

        // 地图加载完成状态（map_webview → 窗口回调 → 工作区入口）
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_map_ipc(move |message| {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_map_ipc(&window, &message);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_map_status_changed(move |available| {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_map_status(&window, available);
            }
        });
        let weak = window.as_weak();
        crate::map_webview::register_status_handler(Rc::new(move |available| {
            if let Some(window) = weak.upgrade() {
                window.invoke_workspace_map_status_changed(available);
            }
        }));
        // 地图 IPC：map_webview 只转交原始消息，规则解析在工作区功能入口
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        crate::map_webview::register_ipc_handler(Rc::new(move |message: &str| {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_map_ipc(&window, message);
            }
        }));

        // ── 右上角工具栏（S1-05：离开工作区先经功能入口判定）──────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_notice_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Notifications);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_switch_campus_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::CampusSelect);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Trash);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Settings);
            }
        });
    }
}
