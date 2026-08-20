//! 工单 02/03 的生产呈现装配。
//!
//! 每个适配器一次调用一个功能模块接口；组合根只持有各呈现入口、绑定 UI 回调和
//! 转发功能入口已经决定好的页面事件，不在 S1 读取或推演后续业务状态。
// ignore-tidy-filelength: 生产入口聚合是组合根（小入口、大聚合）；T52 预览
// 入口与导出/评审/采集入口同处维护，拆分后跨模块引用反而增加审计成本。
//!
//! 机械接线（UI 回调绑定、弹窗调度、轮询定时器）在 `bindings`；导航策略
//! （历史栈/返回/离开/确认/取消路由）在 `navigation`。两者均为组合根模块
//! 的内部文件，构造创建与全部 UI 回调绑定仍由组合根拥有。

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
mod bindings;
mod campus_plan_trash;
pub(crate) mod campus_search;
mod collection;
mod export;
mod navigation;
mod notification;
mod preview_entry;
mod review;
mod review_draft;
mod review_map;
mod startup_settings;
mod workspace_adapter;
mod workspace_boundary;
mod workspace_boundary_fetch;
mod workspace_leave;
mod workspace_resume;

use campus_plan_trash::{CampusPlanProductionAdapter, CampusPlanRequest, TrashProductionAdapter};
use campus_search::CampusSearchController;
use collection::CollectionProductionAdapter;
use export::ExportProductionAdapter;
use navigation::{LeaveRoute, LeaveSafety, NavigationStrategy, PendingLeave};
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
        // 校区选择页（visible=false，尚无当前校区）也常驻通知与设置：
        // 用户需要在尚未选校区时就能改高德 Key；回收站/切换校区在
        // 该上下文中隐藏，避免点击后因无当前校区报错。
        notice_visible: true,
        notice_label: l10n.t("messages.notification_center"),
        switch_campus_visible: visible,
        switch_campus_label: l10n.t("app.switch_campus"),
        trash_visible: visible,
        trash_label: l10n.t("trash.page_title"),
        settings_visible: true,
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
    /// 地图上存在未确认边界草稿；确认后丢弃草稿并继续目标步骤。
    DiscardBoundaryDraft {
        step: i32,
    },
    /// 离开工作区的确认（S1-05：确认后按导航策略路由，取消停留）。
    LeaveWorkspace {
        pending: PendingLeave,
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
    preview_poll_timer: slint::Timer,
    collection_poll_timer: slint::Timer,
    boundary_fetch_poll_timer: slint::Timer,
    campus_search_ipc: std::sync::mpsc::Sender<String>,
    campus_search_poll_timer: slint::Timer,
    /// T36：采集失败弹窗“重试”按钮文案（B6 文本键解析后注入）
    collection_retry_label: String,
    /// 全局导航策略：历史栈、stackable、返回/离开/确认/取消路由（ADR-0044）。
    navigation: NavigationStrategy,
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
                preview_operation: None,
                preview_status: String::new(),
                preview_has_content: false,
                preview_generating: false,
                preview_candidates: Vec::new(),
                pending_locate: None,
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
            preview_poll_timer: slint::Timer::default(),
            collection_poll_timer: slint::Timer::default(),
            boundary_fetch_poll_timer: slint::Timer::default(),
            campus_search_ipc,
            campus_search_poll_timer: slint::Timer::default(),
            collection_retry_label,
            navigation: NavigationStrategy::new(),
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
        crate::map_session::hide();
        // 启动着陆是“从零进入”：历史栈清空，任何着陆页都不显示返回按钮。
        self.clear_back_stack(window);
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
        crate::map_session::hide();
        // 首启向导完成后进入校区选择属于“从零进入”：清空历史栈，
        // 校区选择页无返回按钮（验收 C.9/C.13）。
        self.clear_back_stack(window);
        let request = StartupRequest::CompleteFirstRun {
            language: window.get_wizard_language().to_string(),
            minecraft_version: window.get_wizard_version().to_string(),
            acknowledged: window.get_wizard_acknowledged(),
            api_key: window.get_wizard_gaode_api_key().to_string(),
            security_key: window.get_wizard_gaode_security_key().to_string(),
            web_service_key: window.get_wizard_gaode_web_service_key().to_string(),
        };
        self.startup.show(window, &self.center, request);
    }

    pub(crate) fn show_settings(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
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

    pub(crate) fn replay_tutorial(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        self.settings
            .show(window, &self.center, SettingsRequest::ReplayTutorial);
    }

    pub(crate) fn show_campus_select(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
        self.campus_plan
            .show(window, &self.center, CampusPlanRequest::CampusSelect);
    }

    pub(crate) fn show_plan_list(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        crate::map_session::hide();
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

    /// 转发一次评审操作；批量剔除（无数量门槛）与一键应用建议需二次确认时
    /// 记录待确认类型。
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

    pub(crate) fn review_toggle_select_all_page(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::ToggleSelectAllPage);
    }

    pub(crate) fn review_bulk_state(&mut self, window: &AppWindow, state: String) {
        self.submit_review(window, ReviewRequest::SetBulk { state });
    }

    pub(crate) fn review_set_confidence_filter(&mut self, window: &AppWindow, index: i32) {
        self.submit_review(
            window,
            ReviewRequest::SetConfidenceFilter {
                index: index as usize,
            },
        );
    }

    pub(crate) fn review_set_state_tab(&mut self, window: &AppWindow, index: i32) {
        const STATE_ORDER: [&str; 3] = ["pending", "keep", "remove"];
        let Some(state) = STATE_ORDER.get(index as usize) else {
            return;
        };
        self.submit_review(
            window,
            ReviewRequest::SetStateTab {
                state: (*state).to_owned(),
            },
        );
    }

    pub(crate) fn review_apply_suggestions(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::ApplySuggestions);
    }

    pub(crate) fn review_undo_suggestions(&mut self, window: &AppWindow) {
        self.submit_review(window, ReviewRequest::UndoSuggestionApply);
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

    pub(crate) fn show_notifications(&mut self, window: &AppWindow) {
        self.supersede_diagnostic(window);
        // 通知中心页不承载地图：离开工作区/校区搜索页时必须隐藏地图 WebView，
        // 否则原生子窗口会盖住通知中心页面（“点不动”根因，与 show_trash/
        // show_settings/show_plan_list 同一纪律）。
        crate::map_session::hide();
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
        // 工作区总是可返回（栈空时回落方案列表）：刷新返回按钮可见性。
        self.refresh_toolbar_back(window);
    }

    /// 方案列表卡片单击打开工作区：成功切屏时把方案列表压栈（返回=方案列表）。
    pub(crate) fn open_workspace_plan_from_plan_list(&mut self, window: &AppWindow, plan_id: &str) {
        let before = window.get_active_screen();
        self.open_workspace_plan(window, plan_id);
        self.record_forward_navigation(window, before);
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
            self.pending_confirmation =
                if matches!(step, 3 | 4) && crate::map_session::has_boundary_draft() {
                    Some(PendingConfirmation::DiscardBoundaryDraft { step })
                } else {
                    // 进入边界页且缺少高德密钥：确认后前往设置页
                    Some(PendingConfirmation::GoToSettings)
                };
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

    /// 地图会话转交的结构化事件。页面种类和退休代淘汰均已在会话内完成，
    /// 功能入口不再读取 WebView 当前页来猜测路由。
    pub(crate) fn handle_map_event(
        &mut self,
        window: &AppWindow,
        event: crate::map_session::MapEvent,
    ) {
        let (scene, parsed) = match event {
            crate::map_session::MapEvent::CampusSearch(raw) => {
                let _ = self.campus_search_ipc.send(raw);
                return;
            }
            crate::map_session::MapEvent::Preview(raw) => {
                self.handle_preview_ipc(&raw);
                return;
            }
            crate::map_session::MapEvent::Workspace { scene, message } => (scene, message),
        };
        if scene == crate::map_session::MapScene::Review {
            match parsed {
                IpcMessage::MapReady => {
                    // T39：评审地图就绪——候选不再内嵌 HTML；评审入口排定
                    // 一次全量推送，由事件循环安全上下文执行（回调栈内不再
                    // 执行 WebView2 脚本调用，T35/T38 同纪律）。
                    self.submit_review(window, ReviewRequest::MapReady);
                }
                IpcMessage::ReviewObjectClicked { candidate_id } => {
                    self.submit_review(window, ReviewRequest::MapObjectHighlight { candidate_id });
                }
                IpcMessage::ReviewMapTextToggled { visible } => {
                    // 地图文字开关只影响 WebView 内的 POI 标签显示；把状态记入
                    // WebView 会话，评审页隐藏/弹窗恢复重建时按此状态重新生成。
                    let _ = crate::map_session::command(
                        crate::map_session::MapCommand::ReviewMapText(visible),
                    );
                }
                IpcMessage::Error { message } => self.review_map_error(window, &message),
                _ => {}
            }
            return;
        }
        let map_ready = matches!(parsed, IpcMessage::MapReady);
        // B 工单/恢复实测竞态：采集/导出步骤（2/4）的迟到 map_ready 只确认
        // 让位地图加载完成——不渲染工作区页面，避免用 Ready 页面覆盖在途
        // 导出/采集进度；边界获取只属于步骤①（步骤②由朝向入口处理）。
        if map_ready && !matches!(window.get_workspace_active_step(), 0 | 1) {
            return;
        }
        let mut presentation = self.workspace.show(
            window,
            &self.center,
            WorkspaceRequest::MapEvent { message: parsed },
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
            crate::map_session::mark_failed();
            crate::map_session::hide();
        }
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

    /// 工具栏/页面正向跳转离开工作区前先经功能入口判定；需要确认时挂起
    /// 目标页等待确认。确认或允许后把当前页压栈（从哪儿进、从哪儿出）。
    pub(crate) fn leave_workspace_then(&mut self, window: &AppWindow, target: Screen) {
        self.leave_workspace(window, target, false);
    }

    /// 历史栈返回（统一返回按钮）：工作区需先经离开安全判定；确认或允许
    /// 后弹出栈顶返回“进入当前页时的上一页”。
    pub(crate) fn go_back(&mut self, window: &AppWindow) {
        let current = window.get_active_screen();
        let Some(target) = self.navigation.back_target(current) else {
            return;
        };
        self.leave_workspace(window, target, true);
    }

    /// 把“当前屏幕 + 返回/离开意图 + 工作区入口的离开安全判定”交给导航策略，
    /// 再按结构化转移结果执行页面切换、挂起确认或停留。
    fn leave_workspace(&mut self, window: &AppWindow, target: Screen, from_back: bool) {
        // 功能入口（WorkspaceRequest::Leave）可能已把屏幕切到目标页，
        // 因此来源页必须在调用前捕获，供正向跳转压栈使用。
        let before = window.get_active_screen();
        let current = before;
        let safety = if current == 4 {
            let presentation =
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::Leave { target });
            LeaveSafety::from_workspace_presentation(
                presentation.navigation(),
                presentation.operation(),
                target,
            )
        } else {
            LeaveSafety::Allowed(target)
        };
        let route = self.navigation.route_leave(current, from_back, safety);
        self.apply_leave_route(window, before, route);
    }

    /// 执行导航策略给出的转移结果：跳转（离开工作区时先放弃交付上下文）、
    /// 挂起离开确认、或停留当前页。
    fn apply_leave_route(&mut self, window: &AppWindow, before: i32, route: LeaveRoute) {
        match route {
            LeaveRoute::Navigate {
                target,
                from_back,
                abandon_delivery_context,
            } => {
                if abandon_delivery_context {
                    // 离开工作区：丢弃未确认边界草稿并过期当前交付 generation，
                    // 旧 worker 的结果不得交给新页面（ADR-0042 §6）。
                    crate::map_session::discard_boundary_draft();
                    self.export_poll_timer.stop();
                    self.preview_poll_timer.stop();
                    self.collection_poll_timer.stop();
                    self.boundary_fetch_poll_timer.stop();
                    self.export
                        .show(window, &self.center, ExportPresentationRequest::Abandon);
                    self.collection
                        .show(window, &self.center, CollectionRequest::Abandon);
                }
                if from_back {
                    // 历史栈返回：弹出栈顶并导航（含目标页地图隐藏与渲染）。
                    self.navigate_back(window, target);
                } else {
                    // 正向跳转：目标页经目标入口渲染后记录来源页（before 在
                    // 工作区入口切屏前捕获，保证“从哪儿进、从哪儿出”）。
                    self.navigate_to(window, target);
                    self.record_forward_navigation(window, before);
                }
            }
            LeaveRoute::Confirm { target, from_back } => {
                self.pending_confirmation = Some(PendingConfirmation::LeaveWorkspace {
                    pending: PendingLeave { target, from_back },
                });
            }
            LeaveRoute::Stay => {}
        }
    }

    /// 正向导航：把当前页压栈后跳转目标页（目标屏可被功能入口改写）。
    fn navigate_forward(&mut self, window: &AppWindow, target: Screen) {
        let before = window.get_active_screen();
        self.navigate_to(window, target);
        self.record_forward_navigation(window, before);
    }

    /// 屏幕实际切换后把来源页交给导航策略压栈。
    fn record_forward_navigation(&mut self, window: &AppWindow, before: i32) {
        self.navigation
            .record_forward(before, window.get_active_screen());
        self.refresh_toolbar_back(window);
    }

    /// 弹出栈顶并导航到“进入当前页时的上一页”；栈空时回落 `fallback`
    /// （工作区从零进入时回落方案列表）。工作区经 [`WorkspaceRequest::Resume`]
    /// 复用内存会话（同一方案/步骤/未保存边界点），步骤 ③④⑤ 由对应入口装载。
    fn navigate_back(&mut self, window: &AppWindow, fallback: Screen) {
        let target = self.navigation.pop_or_fallback(fallback);
        self.navigate_to(window, target);
        self.refresh_toolbar_back(window);
    }

    /// 启动着陆/首启完成等“从零进入”路径：清空历史栈，不显示返回按钮。
    fn clear_back_stack(&mut self, window: &AppWindow) {
        self.navigation.clear();
        self.refresh_toolbar_back(window);
    }

    fn refresh_toolbar_back(&self, window: &AppWindow) {
        window.set_toolbar_back_visible(self.navigation.back_visible(window.get_active_screen()));
    }

    fn navigate_to(&mut self, window: &AppWindow, target: Screen) {
        match target {
            Screen::Settings => self.show_settings(window),
            Screen::CampusSelect => self.show_campus_select(window),
            Screen::Trash => self.show_trash(window),
            Screen::Notifications => self.show_notifications(window),
            Screen::PlanList => self.show_plan_list(window),
            Screen::Workspace => {
                // 历史栈返回工作区：复用当前内存会话原样重绘（Resume），
                // 步骤 ③④⑤ 的页面/地图由对应入口装载。
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::Resume);
                self.open_restored_step_entry(window);
            }
            Screen::FirstRunSetup => {}
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
}
