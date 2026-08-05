//! 工单 02/03 的生产呈现装配。
// ignore-tidy-filelength: 组合根承载全部呈现入口与回调绑定；工作区入口已独立成文件（S1-05），采集/评审/导出迁出后收窄
//!
//! 每个适配器一次调用一个功能模块接口。仍未实施界面的功能只呈现当前占位页，
//! 不在 S1 读取或推演后续业务状态。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use export_flow::{BoundaryExportFlow, Error as ExportError};
use localization::Localization;
use notification_center::{
    Notification, NotificationActionOutcome, NotificationCenter, OpaqueNotificationAction,
};
use review_workbench::{CandidateKey, CommandOutcome, ExportSummary, ReviewWorkbench, StateChange};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use slint::ComponentHandle;

use crate::presentation::{
    CampusPlanPresentationEntry, CollectionPageState, CollectionPresentationEntry,
    CollectionRequest, ConfirmationPresentation, CoveragePageState, CoveragePresentationEntry,
    ExportPageState, ExportPresentationEntry, ExportPresentationRequest, NavigationDecision,
    NotificationFact, NotificationPageState, NotificationPresentationEntry, OperationState,
    Presentation, PresentationAdapter, Progress, ReviewPageState, ReviewPresentationEntry,
    ReviewRequest, Screen, SettingsPresentationEntry, SettingsRequest, StartupPresentationEntry,
    StartupRequest, ToolbarPageState, TrashPresentationEntry, TrashRequest,
    WorkspacePresentationEntry, WorkspaceRequest,
};
mod campus_plan_trash;
mod startup_settings;
mod workspace_boundary;

use campus_plan_trash::{CampusPlanProductionAdapter, CampusPlanRequest, TrashProductionAdapter};
use startup_settings::{SettingsProductionAdapter, StartupProductionAdapter};
use workspace_boundary::{WorkspaceProductionAdapter, WorkspaceProductionContext};

use crate::presenter::DiagnosticActionRunner;
use crate::{AppWindow, NoticeData, ReviewCandidateData, ViewModelInjector};

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

const COLLECTION_CATEGORY_KEYS: [&str; 6] = [
    "collection.category_building",
    "collection.category_road",
    "collection.category_water",
    "collection.category_vegetation",
    "collection.category_sports",
    "collection.category_other",
];

/// 采集呈现适配器：S1 只转发一次完整意图，并呈现 A1 返回的页面状态与通知。
struct CollectionProductionAdapter {
    context: WorkspaceProductionContext,
    flow: Arc<collection_flow::CollectionFlow>,
    operation: Option<collection_flow::CollectionOperation>,
}

impl CollectionProductionAdapter {
    fn page_state(&self, view: &collection_flow::CollectionPageView) -> CollectionPageState {
        let injector = self.context.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let statuses = match view.status {
            collection_flow::CollectionStatus::Pending
            | collection_flow::CollectionStatus::Failed => vec![l10n.t("common.pending"); 6],
            collection_flow::CollectionStatus::Fetching => {
                vec![l10n.t("collection.progress_fetching"); 6]
            }
            collection_flow::CollectionStatus::Completed => view
                .progress
                .categories
                .iter()
                .map(|category| category.collected.to_string())
                .collect(),
        };
        let progress_label = match view.status {
            collection_flow::CollectionStatus::Pending
            | collection_flow::CollectionStatus::Failed => l10n.t("collection.progress_title"),
            collection_flow::CollectionStatus::Fetching => l10n.t("collection.progress_fetching"),
            collection_flow::CollectionStatus::Completed => l10n.t_with_args(
                "collection.progress_done",
                serde_json::json!({ "count": view.progress.collected_total }),
            ),
        };
        let report_body = view.report.as_ref().map_or_else(String::new, |report| {
            let mut lines = report.category_lines.clone();
            lines.extend(report.candidate_lines.clone());
            lines.extend(report.issue_lines.clone());
            if let Some(no_issues) = &report.no_issues_line {
                lines.push(no_issues.clone());
            }
            lines.join("\n")
        });
        let category_labels = COLLECTION_CATEGORY_KEYS
            .iter()
            .map(|key| l10n.t(key))
            .collect();
        let source_label = l10n.t("collection.source_gaode");
        let collect_label = l10n.t("collection.collect_button");
        let category_skip_label = l10n.t("collection.skippable");
        let report_entry_label = l10n.t("audit.report_entry");
        drop(injector);
        CollectionPageState {
            workspace: self.context.page(),
            source_label,
            collect_label,
            progress_label,
            category_labels,
            category_statuses: statuses,
            category_skip_label,
            diff_summary: view.diff_summary.clone().unwrap_or_default(),
            report_entry_label,
            report_body,
        }
    }

    /// Start 同步错误（无后台操作）与后台失败共用的失败呈现：A1 已汇总
    /// 页面状态与通知事实，S1 只转发给 B7 并绘制。
    fn start_failure(
        &self,
        error: &collection_flow::CollectionError,
    ) -> Presentation<CollectionPageState> {
        let failure = self.flow.failure_view(error);
        let mut presentation = Presentation::failed(self.page_state(&failure.page));
        if let Some(notification) = failure.notification {
            presentation = presentation.with_notification(NotificationFact::new(notification));
        }
        presentation
    }
}

impl PresentationAdapter<CollectionRequest, CollectionPageState> for CollectionProductionAdapter {
    fn present(&mut self, request: CollectionRequest) -> Presentation<CollectionPageState> {
        #[cfg(test)]
        record_entry_call(3);
        match request {
            CollectionRequest::Open => Presentation::ready(self.page_state(&self.flow.page_view()))
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            CollectionRequest::Start => match self.flow.start() {
                Ok(operation) => {
                    self.operation = Some(operation);
                    Presentation::processing(
                        self.page_state(&self.flow.page_view()),
                        Progress::ZERO,
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
                }
                Err(error) => self
                    .start_failure(&error)
                    .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            },
            CollectionRequest::Poll => self.poll(),
            CollectionRequest::ShowReport => {
                let mut view = self.flow.page_view();
                if let Some(report) = self.flow.report_view() {
                    view.report = Some(report);
                }
                Presentation::ready(self.page_state(&view))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            CollectionRequest::Abandon => {
                self.flow.leave();
                self.operation = None;
                Presentation::ready(self.page_state(&self.flow.page_view()))
            }
        }
    }
}

impl CollectionProductionAdapter {
    fn poll(&mut self) -> Presentation<CollectionPageState> {
        let Some(mut operation) = self.operation.take() else {
            return Presentation::ready(self.page_state(&self.flow.page_view()))
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        };
        match operation.try_complete() {
            Some(Ok(collection_flow::CollectionOutcome::Succeeded(summary))) => {
                let mut presentation = Presentation::succeeded(self.page_state(&summary.page));
                if let Some(notification) = summary.notification {
                    presentation =
                        presentation.with_notification(NotificationFact::new(notification));
                }
                presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Ok(collection_flow::CollectionOutcome::Failed(failure))) => {
                let mut presentation = Presentation::failed(self.page_state(&failure.page));
                if let Some(notification) = failure.notification {
                    presentation =
                        presentation.with_notification(NotificationFact::new(notification));
                }
                presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Err(collection_flow::CollectionError::Expired)) => {
                // 取消/切换方案后旧结果不得拉回：回到当前页面状态。
                Presentation::ready(self.page_state(&self.flow.page_view()))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Err(error)) => self
                .start_failure(&error)
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            None => {
                self.operation = Some(operation);
                Presentation::processing(self.page_state(&self.flow.page_view()), Progress::ZERO)
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
        }
    }
}

/// 六类标签页固定顺序（与 F5 CATEGORY_ORDER、collection 类别键一致，ADR-0016）。
const REVIEW_CATEGORY_ORDER: [CandidateCategory; 6] = [
    CandidateCategory::Building,
    CandidateCategory::Road,
    CandidateCategory::Water,
    CandidateCategory::Vegetation,
    CandidateCategory::Sports,
    CandidateCategory::Other,
];

/// 评审呈现适配器：S1 只转发用户操作，F5 ReviewWorkbench 返回完整页面状态与通知事实。
///
/// 每次进入评审台（Open）都从 B2 一次性装载当前方案的 Reviewable 候选并恢复上一轮
/// 封账终态；Isolated 与原始观测由 B2 资格接口排除，S1 不做业务编排。
struct ReviewProductionAdapter(WorkspaceProductionContext);

impl ReviewProductionAdapter {
    fn empty_page(&self) -> ReviewPageState {
        let workspace = self.0.page();
        let injector = self.0.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let page = ReviewPageState {
            workspace,
            title: l10n.t("review.workbench_title"),
            empty_text: l10n.t("review.empty"),
            candidate_count: 0,
            category_labels: Vec::new(),
            category_counts: Vec::new(),
            active_category: 0,
            cards: Vec::new(),
            selected_count_label: String::new(),
            bulk_buttons_visible: false,
            set_keep_label: l10n.t("review.set_keep"),
            set_reject_label: l10n.t("review.set_reject"),
            set_pending_label: l10n.t("review.set_pending"),
            select_all_label: l10n.t("review.select_all"),
            deselect_all_label: l10n.t("review.deselect_all"),
            card_pending_label: l10n.t("review.pending"),
            card_keep_label: l10n.t("review.keep"),
            card_reject_label: l10n.t("review.reject"),
            pause_label: l10n.t("review.pause"),
            resume_label: l10n.t("review.resume"),
            seal_label: l10n.t("review.seal"),
            sealed: false,
            summary_visible: false,
            summary_text: String::new(),
        };
        drop(injector);
        page
    }

    fn page_state(&self) -> ReviewPageState {
        let workspace = self.0.page();
        let injector = self.0.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let Some(workbench) = injector.review() else {
            drop(injector);
            return self.empty_page();
        };
        let view = workbench.view();
        let category_labels: Vec<String> = view
            .category_tabs
            .iter()
            .map(|tab| l10n.t(tab.label_key))
            .collect();
        let category_counts: Vec<i32> = view
            .category_tabs
            .iter()
            .map(|tab| tab.count as i32)
            .collect();
        let active_category = view
            .category_tabs
            .iter()
            .position(|tab| tab.active)
            .unwrap_or(0) as i32;
        let cards: Vec<ReviewCandidateData> = view
            .cards
            .iter()
            .map(|card| ReviewCandidateData {
                candidate_id: card.candidate_id.clone().into(),
                title: card.title.clone().into(),
                state_label: l10n.t(card.state_key).into(),
                state_key: card.state.to_identifier().into(),
                selected: card.selected,
            })
            .collect();
        let selected_count_label = l10n.t_with_args(
            "review.selected_count",
            serde_json::json!({ "count": view.selected_count }),
        );
        let (summary_visible, summary_text) = if view.sealed {
            (true, self.summary_text(l10n, workbench.export_summary()))
        } else {
            (false, String::new())
        };
        let page = ReviewPageState {
            workspace,
            title: l10n.t(view.title_key),
            empty_text: l10n.t("review.empty"),
            candidate_count: workbench.candidate_count() as i32,
            category_labels,
            category_counts,
            active_category,
            cards,
            selected_count_label,
            bulk_buttons_visible: view.bulk_buttons_visible,
            set_keep_label: l10n.t("review.set_keep"),
            set_reject_label: l10n.t("review.set_reject"),
            set_pending_label: l10n.t("review.set_pending"),
            select_all_label: l10n.t("review.select_all"),
            deselect_all_label: l10n.t("review.deselect_all"),
            card_pending_label: l10n.t("review.pending"),
            card_keep_label: l10n.t("review.keep"),
            card_reject_label: l10n.t("review.reject"),
            pause_label: l10n.t("review.pause"),
            resume_label: l10n.t("review.resume"),
            seal_label: l10n.t("review.seal"),
            sealed: view.sealed,
            summary_visible,
            summary_text,
        };
        drop(injector);
        page
    }

    /// 封账后的导出摘要文案（保留/待定/剔除计数 + 按类别保留明细）。
    fn summary_text(&self, l10n: &Localization, summary: ExportSummary) -> String {
        let category_lines: Vec<String> = summary
            .keep_by_category
            .iter()
            .map(|(category, count)| {
                let label = COLLECTION_CATEGORY_KEYS
                    .iter()
                    .zip(REVIEW_CATEGORY_ORDER.iter())
                    .find(|(_, order)| *order == category)
                    .map(|(key, _)| l10n.t(key))
                    .unwrap_or_else(|| l10n.t("collection.category_other"));
                format!("{label} {count}")
            })
            .collect();
        let main = l10n.t_with_args(
            "review.export_summary",
            serde_json::json!({
                "keep": summary.keep_total,
                "pending": summary.pending_count,
                "remove": summary.remove_count,
            }),
        );
        if category_lines.is_empty() {
            main
        } else {
            format!("{main}\n{}", category_lines.join(" · "))
        }
    }

    fn session_path(&self, plan_id: &str) -> std::path::PathBuf {
        let safe = plan_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        std::env::temp_dir().join(format!("campus-rebuild-review-{safe}.json"))
    }

    /// 简单变更（无需确认）：应用后呈现最新页面状态。
    fn mutate_simple(&self, f: impl FnOnce(&mut ReviewWorkbench)) -> Presentation<ReviewPageState> {
        let injector = self.0.injector();
        let mut injector = injector.borrow_mut();
        if let Some(workbench) = injector.review_mut() {
            f(workbench);
        }
        drop(injector);
        Presentation::ready(self.page_state())
            .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    /// 带错误传播的变更（复选/暂停恢复等）。
    fn apply(
        &self,
        f: impl FnOnce(&mut ReviewWorkbench) -> review_workbench::Result<()>,
    ) -> std::result::Result<Option<()>, review_workbench::Error> {
        let injector = self.0.injector();
        let mut injector = injector.borrow_mut();
        match injector.review_mut() {
            Some(workbench) => f(workbench).map(Some),
            None => Ok(None),
        }
    }

    /// 状态变更操作（可能返回二次确认）。
    fn submit(
        &self,
        f: impl FnOnce(&mut ReviewWorkbench) -> review_workbench::Result<CommandOutcome>,
    ) -> std::result::Result<Option<CommandOutcome>, review_workbench::Error> {
        let injector = self.0.injector();
        let mut injector = injector.borrow_mut();
        match injector.review_mut() {
            Some(workbench) => f(workbench).map(Some),
            None => Ok(None),
        }
    }

    fn command_presentation(&self, outcome: &CommandOutcome) -> Presentation<ReviewPageState> {
        match outcome {
            CommandOutcome::Applied { .. } => Presentation::ready(self.page_state())
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            CommandOutcome::NeedsConfirmation(request) => {
                let injector = self.0.injector();
                let injector = injector.borrow();
                let l10n = injector.l10n();
                let confirmation = ConfirmationPresentation::new(
                    l10n.t(request.title_key),
                    l10n.t_with_args(
                        request.body_key,
                        serde_json::json!({ "count": request.count }),
                    ),
                    l10n.t(request.confirm_key),
                    l10n.t(request.cancel_key),
                );
                drop(injector);
                Presentation::needs_confirmation(self.page_state(), confirmation)
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            _ => Presentation::ready(self.page_state())
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }

    fn present_submit(
        &self,
        result: std::result::Result<Option<CommandOutcome>, review_workbench::Error>,
    ) -> Presentation<ReviewPageState> {
        match result {
            Ok(Some(outcome)) => self.command_presentation(&outcome),
            Ok(None) => Presentation::ready(self.empty_page())
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            Err(error) => self
                .review_failure_presentation(&error)
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }

    fn present_apply(
        &self,
        result: std::result::Result<Option<()>, review_workbench::Error>,
    ) -> Presentation<ReviewPageState> {
        match result {
            Ok(_) => Presentation::ready(self.page_state())
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            Err(error) => self
                .review_failure_presentation(&error)
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }

    /// 结构化失败：经 B7 通知中心呈现错误弹窗，页面保持 F5 返回状态。
    fn review_failure_presentation(
        &self,
        error: &review_workbench::Error,
    ) -> Presentation<ReviewPageState> {
        let injector = self.0.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let (title, body) = match error {
            review_workbench::Error::Persistence(_) => (
                l10n.t("review.seal_failed_title"),
                l10n.t("review.seal_failed_body"),
            ),
            review_workbench::Error::SessionIo(_)
            | review_workbench::Error::SessionCorrupt(_)
            | review_workbench::Error::SessionPlanMismatch { .. } => (
                l10n.t("review.session_failed_title"),
                l10n.t("review.session_failed_body"),
            ),
            _ => (
                l10n.t("review.enter_failed_title"),
                l10n.t("review.enter_failed_body"),
            ),
        };
        let source = l10n.t("app.source_tag");
        let notification = Notification::error(source, title, body);
        drop(injector);
        Presentation::failed(self.page_state())
            .with_notification(NotificationFact::new(notification))
    }

    /// 评审台装载失败：B7 呈现结构化失败，页面回退到空态。
    fn enter_failure_presentation(&self) -> Presentation<ReviewPageState> {
        let injector = self.0.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let notification = Notification::error(
            l10n.t("app.source_tag"),
            l10n.t("review.enter_failed_title"),
            l10n.t("review.enter_failed_body"),
        );
        drop(injector);
        Presentation::failed(self.empty_page())
            .with_notification(NotificationFact::new(notification))
    }

    fn open(&self) -> Presentation<ReviewPageState> {
        let Some(plan_id) = self.0.active_plan_id() else {
            return Presentation::ready(self.empty_page())
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        };
        let result = PlanId::parse(&plan_id)
            .map_err(|_| anyhow::anyhow!("invalid plan id"))
            .and_then(|plan_id| {
                let injector = self.0.injector();
                let mut injector = injector.borrow_mut();
                injector.enter_review(&plan_id)
            });
        match result {
            Ok(()) => Presentation::ready(self.page_state())
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            Err(_) => self
                .enter_failure_presentation()
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }
}

impl PresentationAdapter<ReviewRequest, ReviewPageState> for ReviewProductionAdapter {
    fn present(&mut self, request: ReviewRequest) -> Presentation<ReviewPageState> {
        #[cfg(test)]
        record_entry_call(4);
        match request {
            ReviewRequest::Open => self.open(),
            ReviewRequest::SetCategory { index } => {
                let Some(category) = REVIEW_CATEGORY_ORDER.get(index).copied() else {
                    return Presentation::ready(self.page_state())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                self.mutate_simple(|workbench| workbench.set_active_category(category))
            }
            ReviewRequest::SetState {
                candidate_id,
                state,
            } => {
                let Some(state) = ReviewState::parse(&state) else {
                    return Presentation::ready(self.page_state())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                let key = CandidateKey::new(candidate_id);
                self.present_submit(
                    self.submit(|workbench| workbench.submit(StateChange::single(key, state))),
                )
            }
            ReviewRequest::ToggleSelected { candidate_id } => {
                let key = CandidateKey::new(candidate_id);
                self.present_apply(self.apply(|workbench| {
                    workbench.toggle_selected(&key)?;
                    Ok(())
                }))
            }
            ReviewRequest::SelectAllActive => {
                self.mutate_simple(|workbench| workbench.select_all_in_active_category())
            }
            ReviewRequest::DeselectAllActive => {
                self.mutate_simple(|workbench| workbench.deselect_all_in_active_category())
            }
            ReviewRequest::SetBulk { state } => {
                let Some(state) = ReviewState::parse(&state) else {
                    return Presentation::ready(self.page_state())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                self.present_submit(self.submit(|workbench| workbench.submit_for_selected(state)))
            }
            ReviewRequest::ConfirmPending => {
                self.present_submit(self.submit(|workbench| workbench.confirm_pending()))
            }
            ReviewRequest::CancelPending => {
                self.present_apply(self.apply(|workbench| workbench.cancel_pending()))
            }
            ReviewRequest::Pause => {
                let plan_id = self.0.active_plan_id().unwrap_or_default();
                let path = self.session_path(&plan_id);
                let result = {
                    let injector = self.0.injector();
                    let injector = injector.borrow();
                    injector
                        .review()
                        .map(|workbench| workbench.save_session(&path))
                };
                match result {
                    Some(Ok(())) => {
                        let injector = self.0.injector();
                        let injector = injector.borrow();
                        let l10n = injector.l10n();
                        let notification = Notification::info(
                            l10n.t("app.source_tag"),
                            l10n.t("review.session_saved_title"),
                            l10n.t("review.session_saved_body"),
                        );
                        drop(injector);
                        Presentation::ready(self.page_state())
                            .with_notification(NotificationFact::new(notification))
                            .with_navigation(NavigationDecision::Show(Screen::Workspace))
                    }
                    Some(Err(error)) => self
                        .review_failure_presentation(&error)
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                    None => Presentation::ready(self.empty_page())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                }
            }
            ReviewRequest::Resume => {
                let plan_id = self.0.active_plan_id().unwrap_or_default();
                let path = self.session_path(&plan_id);
                let result = self.apply(|workbench| workbench.restore_session(&path));
                match result {
                    Ok(Some(())) => {
                        let injector = self.0.injector();
                        let injector = injector.borrow();
                        let l10n = injector.l10n();
                        let notification = Notification::info(
                            l10n.t("app.source_tag"),
                            l10n.t("review.session_restored_title"),
                            l10n.t("review.session_restored_body"),
                        );
                        drop(injector);
                        Presentation::ready(self.page_state())
                            .with_notification(NotificationFact::new(notification))
                            .with_navigation(NavigationDecision::Show(Screen::Workspace))
                    }
                    Ok(None) => Presentation::ready(self.empty_page())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                    Err(error) => self
                        .review_failure_presentation(&error)
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                }
            }
            ReviewRequest::Seal => {
                let result = {
                    let injector = self.0.injector();
                    let mut injector = injector.borrow_mut();
                    injector.seal_review()
                };
                match result {
                    Some(Ok(_summary)) => Presentation::succeeded(self.page_state())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                    Some(Err(error)) => self
                        .review_failure_presentation(&error)
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                    None => Presentation::ready(self.empty_page())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace)),
                }
            }
        }
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

struct ExportProductionAdapter {
    context: WorkspaceProductionContext,
    flow: Arc<BoundaryExportFlow>,
    operation: Option<export_flow::BoundaryExportOperation>,
}

impl ExportProductionAdapter {
    fn page_with_status(&self, title_key: &str, subtitle: impl Into<String>) -> ExportPageState {
        let injector = self.context.injector();
        let l10n = injector.borrow();
        let mut workspace = self.context.page();
        workspace.placeholder_title = l10n.l10n().t(title_key);
        workspace.placeholder_subtitle = subtitle.into();
        ExportPageState { workspace }
    }
    fn failure_presentation(&self, error: &ExportError) -> Presentation<ExportPageState> {
        let (body, action) = {
            let injector = self.context.injector();
            let l10n = injector.borrow();
            let category = l10n.l10n().t(export_error_category_key(error));
            let body = l10n
                .l10n()
                .t_with_array("export.failure_user_message", &[&category]);
            let diagnostic_source = l10n.l10n().t("app.source_tag");
            let diagnostic_title = l10n.l10n().t("notice.diagnostic_action");
            let diagnostic_detail = export_diagnostic_detail(error);
            let action = OpaqueNotificationAction::new(move || {
                NotificationActionOutcome::succeeded(Notification::info(
                    diagnostic_source.clone(),
                    diagnostic_title.clone(),
                    diagnostic_detail.clone(),
                ))
            });
            (body, action)
        };
        let injector = self.context.injector();
        let l10n = injector.borrow();
        let notification = NotificationFact::new(Notification::error(
            l10n.l10n().t("app.source_tag"),
            l10n.l10n().t("dialog.error_title"),
            body.clone(),
        ))
        .with_diagnostic_action(action);
        Presentation::failed(self.page_with_status("error.export_failed", body))
            .with_notification(notification)
    }
}

impl PresentationAdapter<ExportPresentationRequest, ExportPageState> for ExportProductionAdapter {
    fn present(&mut self, request: ExportPresentationRequest) -> Presentation<ExportPageState> {
        #[cfg(test)]
        record_entry_call(6);
        match request {
            ExportPresentationRequest::Open => Presentation::ready(
                self.page_with_status(
                    "export.confirm_title",
                    self.context
                        .injector()
                        .borrow()
                        .l10n()
                        .t("export.boundary_only_summary"),
                ),
            )
            .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            ExportPresentationRequest::Start => match self.flow.start() {
                Ok(operation) => {
                    let progress = operation.progress_view();
                    self.operation = Some(operation);
                    Presentation::processing(
                        self.page_with_status(
                            progress.stage_key,
                            self.context
                                .injector()
                                .borrow()
                                .l10n()
                                .t("export.boundary_only_summary"),
                        ),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                }
                Err(error) => self.failure_presentation(&error),
            }
            .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            ExportPresentationRequest::Poll => {
                let Some(operation) = self.operation.as_mut() else {
                    return Presentation::ready(
                        self.page_with_status(
                            "export.confirm_title",
                            self.context
                                .injector()
                                .borrow()
                                .l10n()
                                .t("export.boundary_only_summary"),
                        ),
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                if let Some(result) = operation.try_complete() {
                    self.operation = None;
                    match result {
                        Ok(result) => {
                            let injector = self.context.injector();
                            let l10n = injector.borrow();
                            let dimensions = result.schematic_dimensions;
                            let subtitle = l10n.l10n().t_with_array(
                                "export.done_with_dimensions",
                                &[
                                    &result.schematic_path.display().to_string(),
                                    &dimensions[0].to_string(),
                                    &dimensions[1].to_string(),
                                    &dimensions[2].to_string(),
                                ],
                            );
                            let mut presentation = Presentation::succeeded(
                                self.page_with_status("export.done", subtitle),
                            );
                            if let Some(detail) = result.cleanup_warning {
                                let source = l10n.l10n().t("app.source_tag");
                                let title = l10n.l10n().t("export.done");
                                let warning_body = l10n.l10n().t("export.cleanup_warning");
                                let diagnostic_title = l10n.l10n().t("notice.diagnostic_action");
                                let diagnostic_source = source.clone();
                                let action = OpaqueNotificationAction::new(move || {
                                    NotificationActionOutcome::succeeded(Notification::info(
                                        diagnostic_source.clone(),
                                        diagnostic_title.clone(),
                                        detail.clone(),
                                    ))
                                });
                                presentation = presentation.with_notification(
                                    NotificationFact::new(Notification::warn(
                                        source,
                                        title,
                                        warning_body,
                                    ))
                                    .with_diagnostic_action(action),
                                );
                            }
                            presentation
                        }
                        Err(error) => self.failure_presentation(&error),
                    }
                } else {
                    let progress = operation.progress_view();
                    Presentation::processing(
                        self.page_with_status(
                            progress.stage_key,
                            self.context
                                .injector()
                                .borrow()
                                .l10n()
                                .t("export.boundary_only_summary"),
                        ),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                }
                .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ExportPresentationRequest::Abandon => {
                self.flow.leave();
                self.operation = None;
                Presentation::ready(
                    self.page_with_status(
                        "export.confirm_title",
                        self.context
                            .injector()
                            .borrow()
                            .l10n()
                            .t("export.boundary_only_summary"),
                    ),
                )
            }
        }
    }
}

fn export_diagnostic_detail(error: &ExportError) -> String {
    error.to_string()
}

fn export_error_category_key(error: &ExportError) -> &'static str {
    match error {
        ExportError::Boundary(_) => "error.export_boundary_failed",
        ExportError::SettingsRead(_) => "error.export_settings_failed",
        ExportError::Version(_) => "error.export_version_failed",
        ExportError::Generation(_) => "error.export_generation_failed",
        ExportError::ManifestWrite(_) => "error.export_manifest_write_failed",
        ExportError::SchematicWrite(_) => "error.export_schematic_write_failed",
        ExportError::ArtifactWrite(_) => "error.export_artifact_write_failed",
        ExportError::ArtifactRecovery(_) => "error.export_recovery_failed",
        ExportError::BackgroundTask => "error.export_background_failed",
        _ => "error.export_failed",
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
    /// 批量剔除 >=5 项的二次确认（M3：确认后 F5 执行批量剔除）
    ReviewBatchReject,
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
    collection: CollectionPresentationEntry<'static, CollectionRequest>,
    workspace: WorkspacePresentationEntry<'static, WorkspaceRequest>,
    review: ReviewPresentationEntry<'static, ReviewRequest>,
    _coverage: CoveragePresentationEntry<'static, ()>,
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
    collection_ipc: std::sync::mpsc::Sender<String>,
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
        let collection_ipc = {
            let injector_ref = injector.borrow();
            injector_ref.collection_ipc_sender()
        };
        let workspace = WorkspaceProductionContext::new(
            Rc::clone(&injector),
            window,
            export_flow.clone(),
            collection_flow.clone(),
        );
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
            collection: CollectionPresentationEntry::new(CollectionProductionAdapter {
                context: workspace.clone(),
                flow: collection_flow,
                operation: None,
            }),
            workspace: WorkspacePresentationEntry::new(WorkspaceProductionAdapter {
                context: workspace.clone(),
            }),
            review: ReviewPresentationEntry::new(ReviewProductionAdapter(workspace.clone())),
            _coverage: CoveragePresentationEntry::new(CoverageProductionAdapter(workspace.clone())),
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
            collection_ipc,
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
                // 与直接离开（NavigationDecision::Show）等价：离开工作区会使当前
                // 交付 generation 过期，旧 worker 的结果不得交给新页面（ADR-0042 §6）。
                self.export_poll_timer.stop();
                self.collection_poll_timer.stop();
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
        }
        true
    }

    pub(crate) fn cancel_pending_action(&mut self, window: &AppWindow) {
        let pending = self.pending_confirmation.take();
        if matches!(pending, Some(PendingConfirmation::ReviewBatchReject)) {
            self.review
                .show(window, &self.center, ReviewRequest::CancelPending);
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
        let presentation = self.review.show(window, &self.center, request);
        if presentation.operation() == &OperationState::NeedsConfirmation {
            self.pending_confirmation = Some(PendingConfirmation::ReviewBatchReject);
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

    /// 地图 WebView 转交的原始 IPC 消息：原样转交候选数据源桥的响应通道
    /// （信封匹配在数据源适配器内，S1 不读取采集内容），同时交给工作区
    /// 功能入口解析边界/朝向消息。
    pub(crate) fn handle_map_ipc(&mut self, window: &AppWindow, message: &str) {
        let _ = self.collection_ipc.send(message.to_owned());
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
            NavigationDecision::Show(screen) => {
                self.export_poll_timer.stop();
                self.collection_poll_timer.stop();
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

        // 鈹€鈹€ M3 璇勫椤靛洖璋冿細S1 鍙浆鍙戠敤鎴锋搷浣滅粰 F5 鍏ュ彛 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_category_clicked(move |index| {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_set_category(&window, index);
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
