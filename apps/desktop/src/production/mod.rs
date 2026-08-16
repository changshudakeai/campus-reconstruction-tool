//! 工单 02/03 的生产呈现装配。
// ignore-tidy-filelength: M5 后组合根本体（ProductionEntries 全部入口持有、UI 回调绑定与确认路由，
// ADR-0037 允许的构造期接线）仍超红线；流程适配器已按入口拆出（collection/review/export/notification/
// workspace），此处不再包含任何功能模块的呈现翻译。失效里程碑：v2.1.0（2026-12-31），届时按入口把
// ProductionEntries 回调方法拆入独立 impl 文件后消除
//!
//! 每个适配器一次调用一个功能模块接口；组合根只持有各呈现入口、绑定 UI 回调和
//! 转发功能入口已经决定好的页面事件，不在 S1 读取或推演后续业务状态。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gaode_client::IpcMessage;
use localization::Localization;
use notification_center::{Notification, NotificationActionOutcome, NotificationCenter};
use slint::ComponentHandle;

use crate::presentation::{
    CampusPlanPresentationEntry, CollectionPresentationEntry, CollectionRequest,
    ExportPresentationEntry, ExportPresentationRequest, NavigationDecision,
    NotificationPresentationEntry, OperationState, ReviewPresentationEntry, ReviewRequest, Screen,
    SettingsPresentationEntry, SettingsRequest, StartupPresentationEntry, StartupRequest,
    ToolbarPageState, TrashPresentationEntry, TrashRequest, WorkspacePresentationEntry,
    WorkspaceRequest,
};
mod campus_plan_trash;
pub(crate) mod campus_search;
mod collection;
mod export;
mod notification;
mod review;
mod review_draft;
mod review_map;
mod startup_settings;
mod workspace_adapter;
mod workspace_boundary;
mod workspace_boundary_fetch;
mod workspace_leave;

use campus_plan_trash::{CampusPlanProductionAdapter, CampusPlanRequest, TrashProductionAdapter};
use campus_search::CampusSearchController;
use collection::CollectionProductionAdapter;
use export::ExportProductionAdapter;
use notification::NotificationRequest;
use notification::{DiagnosticFailureLabels, NotificationLabels, NotificationProductionAdapter};
use review::ReviewProductionAdapter;
use shared_domain_types::PlanId;
use startup_settings::{SettingsProductionAdapter, StartupProductionAdapter};
use workspace_adapter::WorkspaceProductionAdapter;
use workspace_boundary::WorkspaceProductionContext;

use crate::presenter::DiagnosticActionRunner;
use crate::{AppWindow, ViewModelInjector};

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
    /// 批量剔除 >=5 项的二次确认（M3：确认后 F5 执行批量剔除）
    ReviewBatchReject,
    /// 一键应用建议的确认（确认后 F5 按当前筛选范围执行保留/剔除）
    ReviewApplySuggestions,
    /// 校区搜索候选详情确认窗（D-3：确认后经 F1 建/选校区）
    ConfirmCampusSelection {
        poi_id: String,
    },
    /// 校区搜索失败弹窗点"重试"（D-3：按原关键词重新搜索）
    RetryCampusSearch {
        query: String,
    },
}

/// 等待用户确认输入窗后由方案入口执行的操作。
#[derive(Clone, PartialEq, Eq)]
enum PendingInput {
    CreatePlan,
    RenamePlan { plan_id: String },
}

pub(crate) struct ProductionEntries {
    /// 组合根持有的注入器（工作现场恢复查找"上次打开方案"用；构造期接线）。
    injector: Rc<RefCell<ViewModelInjector>>,
    startup: StartupPresentationEntry<'static, StartupRequest>,
    settings: SettingsPresentationEntry<'static, SettingsRequest>,
    campus_plan: CampusPlanPresentationEntry<'static, CampusPlanRequest>,
    collection: CollectionPresentationEntry<'static, CollectionRequest>,
    workspace: WorkspacePresentationEntry<'static, WorkspaceRequest>,
    review: ReviewPresentationEntry<'static, ReviewRequest>,
    export: ExportPresentationEntry<'static, ExportPresentationRequest>,
    notification: NotificationPresentationEntry<'static, NotificationRequest>,
    trash: TrashPresentationEntry<'static, TrashRequest>,
    center: Arc<NotificationCenter>,
    action_runner: DiagnosticActionRunner,
    diagnostic_failure: DiagnosticFailureLabels,
    pending_confirmation: Option<PendingConfirmation>,
    pending_input: Option<PendingInput>,
    export_poll_timer: slint::Timer,
    collection_poll_timer: slint::Timer,
    boundary_fetch_poll_timer: slint::Timer,
    campus_search_ipc: std::sync::mpsc::Sender<String>,
    campus_search_poll_timer: slint::Timer,
    /// T36：采集失败弹窗“重试”按钮文案（B6 文本键解析后注入）
    collection_retry_label: String,
}

impl ProductionEntries {
    pub(crate) fn new(
        injector: Rc<RefCell<ViewModelInjector>>,
        window: &AppWindow,
        center: Arc<NotificationCenter>,
    ) -> Self {
        let labels = NotificationLabels::new(injector.borrow().l10n());
        let collection_retry_label = injector.borrow().l10n().t("collection.retry_button");
        let diagnostic_failure = {
            let injector = injector.borrow();
            DiagnosticFailureLabels {
                source: injector.l10n().t("app.source_tag"),
                title: injector.l10n().t("dialog.error_title"),
                panicked_body: injector.l10n().t("notice.diagnostic_action_panicked"),
            }
        };
        let export_flow = {
            let injector_ref = injector.borrow();
            let flow = injector_ref.export_flow();
            flow.sync_settings(injector_ref.settings());
            flow
        };
        let collection_flow = {
            let injector_ref = injector.borrow();
            injector_ref.collection_flow()
        };
        let (campus_search_ipc, campus_search_transport) = {
            let injector_ref = injector.borrow();
            (
                injector_ref.campus_search_ipc_sender(),
                injector_ref.campus_search_transport(),
            )
        };
        let workspace = WorkspaceProductionContext::new(
            Rc::clone(&injector),
            window,
            export_flow.clone(),
            collection_flow.clone(),
        );
        Self {
            injector: Rc::clone(&injector),
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
                search: CampusSearchController::new(campus_search_transport),
            }),
            collection: CollectionPresentationEntry::new(CollectionProductionAdapter {
                context: workspace.clone(),
                flow: collection_flow,
                operation: None,
            }),
            workspace: WorkspacePresentationEntry::new(WorkspaceProductionAdapter {
                context: workspace.clone(),
            }),
            review: ReviewPresentationEntry::new(ReviewProductionAdapter::new(workspace.clone())),
            export: ExportPresentationEntry::new(ExportProductionAdapter {
                context: workspace.clone(),
                flow: export_flow,
                operation: None,
            }),
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
            export_poll_timer: slint::Timer::default(),
            collection_poll_timer: slint::Timer::default(),
            boundary_fetch_poll_timer: slint::Timer::default(),
            campus_search_ipc,
            campus_search_poll_timer: slint::Timer::default(),
            collection_retry_label,
        }
    }

    fn supersede_diagnostic(&self, window: &AppWindow) {
        self.action_runner.invalidate();
        window.set_diagnostic_operation_state(crate::OperationPresentationState::Ready);
        window.set_diagnostic_operation_progress(0);
        // T36：任何新操作接管时隐藏采集失败弹窗的“重试”按钮
        window.set_error_dialog_retry_visible(false);
    }

    pub(crate) fn show_startup(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        // 工作现场恢复（A.1）：启动时若存在"上次打开方案"且方案仍在，
        // 直接恢复该方案的工作区（含步骤/边界/朝向）；方案已删除或数据
        // 读取失败时清除标记并回落正常启动流程，绝不伪造恢复。
        let last_plan = self
            .injector
            .borrow()
            .load_last_active_plan()
            .ok()
            .flatten();
        if let Some(plan_id) = last_plan {
            let parsed = PlanId::parse(&plan_id).ok();
            let still_exists = parsed
                .as_ref()
                .and_then(|plan| self.injector.borrow().projects().plan_context(plan).ok())
                .flatten()
                .is_some();
            if still_exists {
                self.open_workspace_plan(window, &plan_id);
                return;
            }
            let mut injector = self.injector.borrow_mut();
            if let Err(error) = injector.save_last_active_plan(None) {
                log::warn!("清除失效的“上次打开方案”失败: {error}");
            }
            drop(injector);
        }
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
                web_service_key: window.get_gaode_web_service_key().to_string(),
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

    /// 用户确认后执行对应的待确认操作；返回是否进入"校区搜索处理中"
    /// （调用方据此拉起搜索轮询定时器；其余确认操作返回 false）。
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
                // 与直接离开（NavigationDecision::Show）等价：离开工作区会使当前
                // 交付 generation 过期，旧 worker 的结果不得交给新页面（ADR-0042 §6）。
                self.export_poll_timer.stop();
                self.collection_poll_timer.stop();
                self.boundary_fetch_poll_timer.stop();
                self.export
                    .show(window, &self.center, ExportPresentationRequest::Abandon);
                self.collection
                    .show(window, &self.center, CollectionRequest::Abandon);
                self.navigate_to(window, target);
            }
            PendingConfirmation::OrientationRecalc => {
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::ConfirmOrientation);
            }
            PendingConfirmation::ReviewBatchReject => {
                self.review
                    .show(window, &self.center, ReviewRequest::ConfirmPending);
            }
            PendingConfirmation::ReviewApplySuggestions => {
                self.review
                    .show(window, &self.center, ReviewRequest::ConfirmSuggestionApply);
            }
            PendingConfirmation::ConfirmCampusSelection { poi_id } => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmSelectCampus { poi_id },
                );
            }
            PendingConfirmation::RetryCampusSearch { query } => {
                window.set_campus_search_text(query.into());
                return self.request_campus_search(window);
            }
        }
        // 其余确认操作不进入校区搜索轮询
        false
    }

    pub(crate) fn cancel_pending_action(&mut self, window: &AppWindow) {
        let pending = self.pending_confirmation.take();
        if matches!(pending, Some(PendingConfirmation::ReviewBatchReject)) {
            self.review
                .show(window, &self.center, ReviewRequest::CancelPending);
            return;
        }
        if matches!(pending, Some(PendingConfirmation::ReviewApplySuggestions)) {
            self.review
                .show(window, &self.center, ReviewRequest::CancelSuggestionApply);
            return;
        }
        if matches!(
            pending,
            Some(PendingConfirmation::ConfirmCampusSelection { .. })
        ) {
            self.campus_plan
                .show(window, &self.center, CampusPlanRequest::CancelSelectCampus);
            return;
        }
        if matches!(pending, Some(PendingConfirmation::RetryCampusSearch { .. })) {
            // 取消：停留校区选择页，不创建校区、不能绕过搜索（ADR-0008 第 9 条）
            self.show_campus_select(window);
            return;
        }
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

    pub(crate) fn show_plan_list(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_webview::hide();
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::PlanList);
    }

    pub(crate) fn show_collection(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.collection
            .show(window, &self.center, CollectionRequest::Open);
    }

    pub(crate) fn start_collection(&mut self, window: &AppWindow) -> bool {
        self.supersede_diagnostic(window);
        let presentation = self
            .collection
            .show(window, &self.center, CollectionRequest::Start);
        matches!(presentation.operation(), OperationState::Processing { .. })
    }

    /// T36：取消采集（抽屉按钮 → A1 CollectionFlow::cancel；停轮询并回到待定）。
    pub(crate) fn cancel_collection(&mut self, window: &AppWindow) {
        self.collection_poll_timer.stop();
        self.collection
            .show(window, &self.center, CollectionRequest::Cancel);
    }

    pub(crate) fn show_collection_report(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.collection
            .show(window, &self.center, CollectionRequest::ShowReport);
    }

    pub(crate) fn show_review(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.submit_review(window, ReviewRequest::Open);
    }

    /// 转发一次评审操作；批量剔除 >=5 项时记录待二次确认。
    fn submit_review(&mut self, window: &AppWindow, request: ReviewRequest) {
        self.supersede_diagnostic(window);
        let apply_requested = matches!(request, ReviewRequest::ApplySuggestions);
        let presentation = self.review.show(window, &self.center, request);
        if presentation.operation() == &OperationState::NeedsConfirmation {
            self.pending_confirmation = Some(if apply_requested {
                PendingConfirmation::ReviewApplySuggestions
            } else {
                PendingConfirmation::ReviewBatchReject
            });
        }
    }

    pub(crate) fn review_set_category(&mut self, window: &AppWindow, index: i32) {
        self.submit_review(
            window,
            ReviewRequest::SetCategory {
                index: index as usize,
            },
        );
    }

    /// T39：评审候选列表上一页（分页；翻页时才重建当前页切片）。
    pub(crate) fn review_page_prev(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::PagePrev);
    }

    /// T39：评审候选列表下一页（分页）。
    pub(crate) fn review_page_next(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::PageNext);
    }

    pub(crate) fn review_set_state(
        &mut self,
        window: &AppWindow,
        candidate_id: String,
        state: String,
    ) {
        self.submit_review(
            window,
            ReviewRequest::SetState {
                candidate_id,
                state,
            },
        );
    }

    pub(crate) fn review_highlight(&mut self, window: &AppWindow, candidate_id: String) {
        self.submit_review(window, ReviewRequest::Highlight { candidate_id });
    }

    pub(crate) fn review_locate(&mut self, window: &AppWindow, candidate_id: String) {
        self.submit_review(window, ReviewRequest::Locate { candidate_id });
    }

    pub(crate) fn review_toggle_selected(&mut self, window: &AppWindow, candidate_id: String) {
        self.submit_review(window, ReviewRequest::ToggleSelected { candidate_id });
    }

    pub(crate) fn review_select_all(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::SelectAllActive);
    }

    pub(crate) fn review_deselect_all(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::DeselectAllActive);
    }

    pub(crate) fn review_bulk_state(&mut self, window: &AppWindow, state: String) {
        self.submit_review(window, ReviewRequest::SetBulk { state });
    }

    pub(crate) fn review_toggle_suggestion_filter(&mut self, window: &AppWindow, index: i32) {
        self.submit_review(
            window,
            ReviewRequest::ToggleSuggestionFilter {
                index: index as usize,
            },
        );
    }

    pub(crate) fn review_apply_suggestions(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::ApplySuggestions);
    }

    pub(crate) fn review_undo_suggestions(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::UndoSuggestionApply);
    }

    pub(crate) fn review_pause(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::Pause);
    }

    pub(crate) fn review_resume(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::Resume);
    }

    pub(crate) fn review_seal(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::Seal);
    }

    pub(crate) fn show_export(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.export
            .show(window, &self.center, ExportPresentationRequest::Open);
    }

    pub(crate) fn start_export(&mut self, window: &AppWindow) -> bool {
        self.supersede_diagnostic(window);
        let presentation = self
            .export
            .show(window, &self.center, ExportPresentationRequest::Start);
        matches!(presentation.operation(), OperationState::Processing { .. })
    }

    fn poll_export(&mut self, window: &AppWindow) -> bool {
        let presentation = self
            .export
            .show(window, &self.center, ExportPresentationRequest::Poll);
        !matches!(presentation.operation(), OperationState::Processing { .. })
    }

    fn poll_collection(&mut self, window: &AppWindow) -> bool {
        let presentation = self
            .collection
            .show(window, &self.center, CollectionRequest::Poll);
        // T36：采集失败必须弹错误对话框并给“重试”
        let failed = matches!(presentation.operation(), OperationState::Failed);
        window.set_error_dialog_retry_visible(failed);
        window.set_error_dialog_retry_label(self.collection_retry_label.clone().into());
        !matches!(presentation.operation(), OperationState::Processing { .. })
    }

    fn start_export_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().export_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let completed = entries.borrow_mut().poll_export(&window);
                if completed {
                    entries.borrow_mut().export_poll_timer.stop();
                }
            },
        );
    }

    fn start_collection_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().collection_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let completed = entries.borrow_mut().poll_collection(&window);
                if completed {
                    entries.borrow_mut().collection_poll_timer.stop();
                }
            },
        );
    }

    /// D-3：校区搜索后台轮询（20ms 采样，终态即停；失败终态同样停表，
    /// 由确认弹窗"重试/取消"接管）。
    pub(crate) fn start_campus_search_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().campus_search_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let center = entries.borrow().center.clone();
                let presentation = entries.borrow_mut().campus_plan.show(
                    &window,
                    &center,
                    CampusPlanRequest::PollSearch,
                );
                if !matches!(presentation.operation(), OperationState::Processing { .. }) {
                    entries.borrow_mut().campus_search_poll_timer.stop();
                }
            },
        );
    }

    /// T31：Rust 侧 OSM 边界自动获取后台轮询（20ms 采样，终态即停）。
    fn start_boundary_fetch_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().boundary_fetch_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let center = entries.borrow().center.clone();
                let presentation = entries.borrow_mut().workspace.show(
                    &window,
                    &center,
                    WorkspaceRequest::PollBoundaryFetch,
                );
                if !matches!(presentation.operation(), OperationState::Processing { .. }) {
                    entries.borrow_mut().boundary_fetch_poll_timer.stop();
                }
            },
        );
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
        // 工作现场恢复：OpenPlan 内部已导航到上次停留步骤（A.1）；步骤 ③④⑤
        // 需要各自入口装载（评审进台/采集状态/导出页），与步骤点击路径一致。
        self.open_restored_step_entry(window);
    }

    /// 恢复步骤落在 ③采集/④评审/⑤导出时，由对应入口呈现页面。
    fn open_restored_step_entry(&mut self, window: &AppWindow) {
        let step = window.get_workspace_active_step();
        match step {
            2 => self.show_collection(window),
            3 => self.show_review(window),
            4 => self.show_export(window),
            _ => {}
        }
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

    pub(crate) fn handle_boundary_delete_selected(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::BoundaryDeleteSelected,
        );
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

    pub(crate) fn handle_boundary_refresh(&mut self, window: &AppWindow) -> bool {
        self.supersede_diagnostic(window);
        let presentation =
            self.workspace
                .show(window, &self.center, WorkspaceRequest::BoundaryRefresh);
        matches!(presentation.operation(), OperationState::Processing { .. })
    }

    pub(crate) fn handle_orientation_submit(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        let presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::OrientationSubmit {
                // T34：抽屉 ② 的"确认朝向"按钮提交手动输入的角度（方位角模式）
                mode: "bearing-angle".to_string(),
                angle_text: window.get_workspace_orientation_input_text().to_string(),
            },
        );
        if presentation.operation() == &OperationState::NeedsConfirmation {
            // 覆盖既有朝向：确认后应用待定角度
            self.pending_confirmation = Some(PendingConfirmation::OrientationRecalc);
        }
    }

    /// T34：抽屉 ② 的"确认两点朝向"按钮——提交地图两点草稿（two-points 模式）。
    pub(crate) fn handle_orientation_confirm_two_points(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        let presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::OrientationSubmit {
                mode: "two-points".to_string(),
                angle_text: String::new(),
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

    /// T34：左侧抽屉开合（做法 A：展开时地图右移让位）。
    pub(crate) fn handle_workspace_drawer_toggle(&mut self, window: &AppWindow) {
        self.workspace
            .show(window, &self.center, WorkspaceRequest::DrawerToggle);
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
        // 采集/导出步骤的地图只是让位显示：状态回调不渲染工作区页面，
        // 避免用 Ready 页面覆盖在途导出/采集进度（恢复工作流实测竞态；
        // 边界步①/朝向步②仍需要地图可用性呈现）。
        if !matches!(window.get_workspace_active_step(), 0 | 1) {
            return;
        }
        self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::MapStatus { available },
        );
    }

    /// 地图 WebView 转交的原始 IPC 消息：原样转交候选数据源桥的响应通道
    /// （信封匹配在数据源适配器内，S1 不读取采集内容），同时转交校区搜索
    /// 响应通道（D-3：信封匹配在校区搜索传输内），并交给工作区功能入口
    /// 解析边界/朝向消息。
    pub(crate) fn handle_map_ipc(&mut self, window: &AppWindow, message: &str) {
        // T38：评审地图页（步骤 ④）的 IPC 路由到评审入口，不进入边界/朝向解析，
        // 也不触发 OSM 边界自动获取。
        if crate::map_webview::is_review_page() {
            match gaode_client::parse_ipc_message(message) {
                Ok(IpcMessage::MapReady) => {
                    // T39：评审地图就绪——候选不再内嵌 HTML；评审入口排定
                    // 一次全量推送，由事件循环安全上下文执行（回调栈内不再
                    // 执行 WebView2 脚本调用，T35/T38 同纪律）。
                    self.submit_review(window, ReviewRequest::MapReady);
                }
                Ok(IpcMessage::ReviewObjectClicked { candidate_id }) => {
                    self.submit_review(window, ReviewRequest::MapObjectHighlight { candidate_id });
                }
                Ok(IpcMessage::ReviewMapTextToggled { visible }) => {
                    // 地图文字开关只影响 WebView 内的 POI 标签显示；把状态记入
                    // WebView 会话，评审页隐藏/弹窗恢复重建时按此状态重新生成。
                    crate::map_webview::set_review_map_text_visible(visible);
                }
                Ok(IpcMessage::Error { message }) => self.review_map_error(window, &message),
                _ => {}
            }
            return;
        }
        let _ = self.campus_search_ipc.send(message.to_owned());
        let map_ready = matches!(
            gaode_client::parse_ipc_message(message),
            Ok(IpcMessage::MapReady)
        );
        // B 工单/恢复实测竞态：采集/导出步骤（2/4）的迟到 map_ready 只确认
        // 让位地图加载完成——不渲染工作区页面，避免用 Ready 页面覆盖在途
        // 导出/采集进度；边界获取只属于步骤①（步骤②由朝向入口处理）。
        if map_ready && !matches!(window.get_workspace_active_step(), 0 | 1) {
            return;
        }
        let mut presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::MapIpc {
                message: message.to_string(),
            },
        );
        // 重复 map_ready 在请求仍进行时既不能新开请求，也不能只等待下一次
        // 计时器采样：顺手非阻塞轮询一次，若后台结果已经完成则立即命中。
        if map_ready && matches!(presentation.operation(), OperationState::Processing { .. }) {
            presentation =
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::PollBoundaryFetch);
        }
        if presentation.operation() == &OperationState::NeedsConfirmation {
            // 地图确认朝向覆盖既有朝向：与方位角输入提交走同一确认路径
            self.pending_confirmation = Some(PendingConfirmation::OrientationRecalc);
        }
    }

    /// 评审地图加载失败：如实标记不可用并隐藏地图（评审抽屉仍可继续操作），
    /// 经 B7 呈现明确错误（与边界页失败同纪律，T36）。
    fn review_map_error(&mut self, window: &AppWindow, message: &str) {
        let recoverable_failure = message.starts_with("review_map_draw_failed:")
            || message.starts_with("review_map_locate_");
        if !recoverable_failure {
            crate::map_webview::mark_map_failed();
        }
        crate::map_webview::hide();
        if !recoverable_failure {
            self.workspace.show(
                window,
                &self.center,
                WorkspaceRequest::MapStatus { available: false },
            );
        }
        self.submit_review(
            window,
            ReviewRequest::MapFailed {
                message: message.to_string(),
            },
        );
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
            NavigationDecision::Show(screen) => {
                self.export_poll_timer.stop();
                self.collection_poll_timer.stop();
                self.boundary_fetch_poll_timer.stop();
                self.export
                    .show(window, &self.center, ExportPresentationRequest::Abandon);
                self.collection
                    .show(window, &self.center, CollectionRequest::Abandon);
                self.navigate_to(window, screen);
            }
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
            Screen::PlanList => self.show_plan_list(window),
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
                let processing = shared.borrow_mut().request_campus_search(&window);
                if processing {
                    ProductionEntries::start_campus_search_polling(&shared, &window);
                }
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
                // T34：弹窗遮挡统一机制——输入窗关闭后按当前步骤模式恢复地图
                crate::map_webview::restore_after_modal(window.as_weak());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_input_dialog_cancelled(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().cancel_pending_input();
                // T34：弹窗遮挡统一机制——输入窗取消后按当前步骤模式恢复地图
                crate::map_webview::restore_after_modal(window.as_weak());
            }
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
        window.on_workspace_back_to_plan_list_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::PlanList);
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
        window.on_workspace_boundary_delete_selected_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_delete_selected(&window);
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
        window.on_workspace_boundary_refresh_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let processing = shared.borrow_mut().handle_boundary_refresh(&window);
                if processing {
                    ProductionEntries::start_boundary_fetch_polling(&shared, &window);
                }
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
        window.on_workspace_orientation_confirm_two_points_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_orientation_confirm_two_points(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_drawer_toggle_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_workspace_drawer_toggle(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_start_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let started = shared.borrow_mut().start_collection(&window);
                if started {
                    ProductionEntries::start_collection_polling(&shared, &window);
                }
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_report_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().show_collection_report(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_cancel_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().cancel_collection(&window);
            }
        });

        // T36：采集失败错误弹窗点“重试”→ 隐藏弹窗并重新发起同一开始意图
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_error_dialog_retry_clicked(move || {
            if let Some(window) = weak.upgrade() {
                window.set_error_dialog_visible(false);
                window.set_error_dialog_retry_visible(false);
                crate::map_webview::restore_after_modal(window.as_weak());
                let started = shared.borrow_mut().start_collection(&window);
                if started {
                    ProductionEntries::start_collection_polling(&shared, &window);
                }
            }
        });

        // 鈹€鈹€ M3 璇勫椤靛洖璋冿細S1 鍙浆鍙戠敤鎴锋搷浣滅粰 F5 鍏ュ彛 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_category_clicked(move |index| {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_set_category(&window, index);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_page_prev_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_page_prev(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_page_next_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_page_next(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_state_clicked(move |candidate_id, state| {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_set_state(
                &window,
                candidate_id.to_string(),
                state.to_string(),
            );
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_highlight_clicked(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_highlight(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_locate_clicked(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_locate(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_selection_toggled(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_toggle_selected(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_select_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_select_all(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_deselect_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_deselect_all(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_bulk_state_clicked(move |state| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_bulk_state(&window, state.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_pause_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_pause(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_resume_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_resume(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_seal_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_seal(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_suggestion_filter_clicked(move |index| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_toggle_suggestion_filter(&window, index);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_apply_suggestions_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_apply_suggestions(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_undo_suggestions_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_undo_suggestions(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_export_start_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let processing = shared.borrow_mut().start_export(&window);
                if processing {
                    ProductionEntries::start_export_polling(&shared, &window);
                }
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
                // T31：地图就绪 → Rust 侧 OSM 边界自动获取（后台线程轮询取回）
                // T38：评审地图就绪走评审入口（不触发边界获取）；标注推送
                // 定时器由地图创建完成回调（handle_map_status 绑定）排定，
                // 不在 WebView2 IPC 回调栈内启动。
                if let Ok(IpcMessage::MapReady) = gaode_client::parse_ipc_message(&message) {
                    if crate::map_webview::is_review_page() {
                        return;
                    }
                    // 只有边界步（步骤①）的 map_ready 会真正启动边界获取；
                    // 采集/导出步的 map_ready 只是让位显示，启动轮询会把迟到
                    // 的 Idle 呈现覆盖掉在途导出/采集进度（恢复工作流实测竞态）。
                    if window.get_workspace_active_step() != 0 {
                        return;
                    }
                    ProductionEntries::start_boundary_fetch_polling(&shared, &window);
                }
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
                // T31：同上（wry IPC 直达路径同样触发后台获取轮询）
                if let Ok(IpcMessage::MapReady) = gaode_client::parse_ipc_message(message) {
                    // T38 根因：评审页 map_ready 不得重启边界获取轮询——评审步
                    // 轮询空态会触发 poll_boundary_fetch 的 RefCell 借用 panic
                    // （见 workspace_adapter 修复），与 on_workspace_map_ipc 同纪律。
                    if crate::map_webview::is_review_page() {
                        return;
                    }
                    if window.get_workspace_active_step() != 0 {
                        return;
                    }
                    ProductionEntries::start_boundary_fetch_polling(&shared, &window);
                }
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
