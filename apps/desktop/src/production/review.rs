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
            locate_label: l10n.t("review.locate"),
            detail_visible: false,
            detail_title: String::new(),
            detail_category_label: String::new(),
            detail_tags_label: String::new(),
            detail_tags: Vec::new(),
            detail_source_label: String::new(),
            detail_source: String::new(),
            detail_state_label: String::new(),
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
        let page = self.page_state_quiet();
        self.sync_review_map();
        page
    }

    /// 与 [`page_state`] 相同但不回推地图标注（供 WebView2 IPC 回调栈内使用——
    /// 回调栈内不再执行 WebView2 脚本调用，避免 COM 通道时序竞争；标注由
    /// 用户操作后的安全上下文（`page_state` 尾部）统一推送）。
    fn page_state_quiet(&self) -> ReviewPageState {
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
                title: self.display_title(l10n, card.named, &card.title).into(),
                named: card.named,
                state_label: l10n.t(card.state_key).into(),
                state_key: card.state.to_identifier().into(),
                selected: card.selected,
                highlighted: card.highlighted,
            })
            .collect();
        let (
            detail_visible,
            detail_title,
            detail_category_label,
            detail_tags_label,
            detail_tags,
            detail_source_label,
            detail_source,
            detail_state_label,
        ) = match view.info_panel.as_ref() {
            Some(info) => {
                let tags: Vec<String> = info
                    .tags
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                (
                    true,
                    self.display_title(l10n, info.named, &info.title),
                    format!(
                        "{}：{}",
                        l10n.t(info.category_label_key),
                        l10n.t(info.category_key)
                    ),
                    format!("{}：", l10n.t(info.tags_label_key)),
                    tags,
                    format!("{}：{}", l10n.t(info.source_label_key), info.source),
                    info.source.clone(),
                    l10n.t_with_args(
                        info.state_label_key,
                        serde_json::json!({ "state": l10n.t(info.state_key) }),
                    ),
                )
            }
            None => (
                false,
                String::new(),
                String::new(),
                String::new(),
                Vec::new(),
                String::new(),
                String::new(),
                String::new(),
            ),
        };
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
            locate_label: l10n.t("review.locate"),
            detail_visible,
            detail_title,
            detail_category_label,
            detail_tags_label,
            detail_tags,
            detail_source_label,
            detail_source,
            detail_state_label,
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

    /// 候选标题：有真实名称原样显示；未命名显示"未命名建筑 #id"（id 为回退标识）。
    fn display_title(&self, l10n: &Localization, named: bool, title: &str) -> String {
        if named {
            title.to_owned()
        } else {
            l10n.t_with_args(
                "review.unnamed_building",
                serde_json::json!({ "id": title }),
            )
        }
    }

    /// 把 F5 当前候选标注（待定虚线/保留实线/剔除隐藏）与高亮状态同步到评审地图。
    ///
    /// 地图不可用或非评审页时为空操作；剔除候选由 JS 侧跳过（卡片保留可改回）。
    /// 分批推送（清空 + 每批多条 addReviewCandidate）：真实 OSM 几何使全量
    /// JSON 可达数百 KB，拆分小载荷降低 WebView2 ExecuteScript 通道压力
    /// （JS 侧再以 50/150ms 分批上屏，避免一次创建上千多边形过载）。
    fn sync_review_map(&self) {
        if !crate::map_webview::is_review_page() || !crate::map_webview::is_visible() {
            return;
        }
        let injector = self.0.injector();
        let injector = injector.borrow();
        let Some(workbench) = injector.review() else {
            return;
        };
        let view = workbench.view();
        let objects: Vec<serde_json::Value> = view
            .map_objects
            .iter()
            .map(|object| {
                serde_json::json!({
                    "candidate_id": object.candidate_id,
                    "kind": object.shape_kind,
                    "coordinates": object.shape_coordinates,
                    "state": object.state.to_identifier(),
                })
            })
            .collect();
        log::debug!(
            "sync_review_map: 推送 {} 个候选标注（分批 {}）",
            objects.len(),
            objects.len().div_ceil(CHUNK_SIZE)
        );
        const CHUNK_SIZE: usize = 50;
        let mut scripts = vec!["window.clearReviewOverlays();".to_string()];
        for chunk in objects.chunks(CHUNK_SIZE) {
            let mut script = String::new();
            for object in chunk {
                let json = serde_json::to_string(object).unwrap_or_else(|_| "{}".to_string());
                script.push_str("window.addReviewCandidate(");
                script.push_str(&json);
                script.push_str(");");
            }
            scripts.push(script);
        }
        match workbench.highlighted() {
            Some(key) => {
                let id = serde_json::to_string(&key.candidate_id).unwrap_or_else(|_| "\"\"".into());
                scripts.push(format!("window.highlightReviewCandidate({id});"));
            }
            None => scripts.push("window.clearReviewHighlight();".to_string()),
        }
        for script in scripts {
            crate::map_webview::evaluate_script(&script);
        }
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

    /// 不带地图回推的 apply 呈现（WebView2 IPC 回调栈内专用）。
    fn present_apply_quiet(
        &self,
        result: std::result::Result<Option<()>, review_workbench::Error>,
    ) -> Presentation<ReviewPageState> {
        match result {
            Ok(_) => Presentation::ready(self.page_state_quiet())
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
            Ok(()) => {
                // T38：评审地图在候选装载后创建——候选标注内嵌 HTML、页面加载后
                // 自绘并发送 map_ready，Rust 侧无需在 IPC 回调栈内回推脚本
                // （回调栈内不再执行 WebView2 脚本调用，避免 COM 通道时序竞争）。
                self.show_review_map();
                Presentation::ready(self.page_state())
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Err(_) => self
                .enter_failure_presentation()
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }

    /// 创建评审地图（候选标注内嵌 HTML；无密钥/锚点时跳过，抽屉仍可操作）。
    fn show_review_map(&self) {
        let Some((keys, anchor)) = self.0.map_credentials() else {
            return;
        };
        if keys.0.is_empty() {
            return;
        }
        let injector = self.0.injector();
        let injector = injector.borrow();
        let Some(workbench) = injector.review() else {
            return;
        };
        let objects: Vec<serde_json::Value> = workbench
            .view()
            .map_objects
            .iter()
            .map(|object| {
                serde_json::json!({
                    "candidate_id": object.candidate_id,
                    "kind": object.shape_kind,
                    "coordinates": object.shape_coordinates,
                    "state": object.state.to_identifier(),
                })
            })
            .collect();
        drop(injector);
        crate::map_webview::show_review(
            self.0.window.clone(),
            keys.0,
            keys.1,
            anchor.0,
            anchor.1,
            objects,
        );
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
            ReviewRequest::Highlight { candidate_id } => {
                let key = CandidateKey::new(candidate_id);
                self.present_apply(self.apply(|workbench| workbench.highlight(&key)))
            }
            ReviewRequest::MapObjectHighlight { candidate_id } => {
                // 地图对象点击（IPC 栈内）：JS 已自高亮，这里只同步卡片与详情，
                // 不回推地图脚本（回调栈内不再执行 WebView2 脚本调用）。
                let key = CandidateKey::new(candidate_id);
                self.present_apply_quiet(self.apply(|workbench| workbench.highlight(&key)))
            }
            ReviewRequest::Locate { candidate_id } => {
                let key = CandidateKey::new(candidate_id.clone());
                let result = self.apply(|workbench| workbench.highlight(&key));
                // 地图中心跳转 + 高亮（JS 侧；无 WebView/非评审页时为空操作）
                if result.is_ok() && crate::map_webview::is_review_page() {
                    let id = serde_json::to_string(&candidate_id).unwrap_or_else(|_| "\"\"".into());
                    crate::map_webview::evaluate_script(&format!(
                        "window.locateReviewCandidate({id});"
                    ));
                }
                self.present_apply(result)
            }
            ReviewRequest::MapReady => {
                // 评审地图就绪：仅呈现页面（候选标注已内嵌 HTML 自绘；后续
                // 状态变更由 page_state 尾部的 sync_review_map 统一同步）。
                Presentation::ready(self.page_state_quiet())
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ReviewRequest::MapFailed { message } => {
                let injector = self.0.injector();
                let injector = injector.borrow();
                let l10n = injector.l10n();
                let body = if message == crate::map_webview::MAP_LOAD_TIMEOUT_MARKER {
                    l10n.t("map.load_timeout_body")
                } else {
                    l10n.t_with_array("map.load_failed_body", &[&message])
                };
                let notification = Notification::error(
                    l10n.t("app.source_tag"),
                    l10n.t("boundary.map_notice_title"),
                    body,
                );
                drop(injector);
                Presentation::failed(self.page_state())
                    .with_notification(NotificationFact::new(notification))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
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
