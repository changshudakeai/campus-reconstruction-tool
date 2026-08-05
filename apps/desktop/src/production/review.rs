//! 评审呈现适配器：S1 只转发一次完整评审意图，并呈现 F5 工作台返回的页面状态与通知。

use super::collection::COLLECTION_CATEGORY_KEYS;
use super::workspace_boundary::WorkspaceProductionContext;
use crate::presentation::{
    ConfirmationPresentation, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, ReviewPageState, ReviewRequest, Screen,
};
use crate::ReviewCandidateData;
use localization::Localization;
use notification_center::Notification;
use review_workbench::{CandidateKey, CommandOutcome, ExportSummary, ReviewWorkbench, StateChange};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

#[cfg(test)]
use super::record_entry_call;

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
pub(crate) struct ReviewProductionAdapter(pub(crate) WorkspaceProductionContext);

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
