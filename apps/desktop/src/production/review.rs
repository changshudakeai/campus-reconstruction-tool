//! 评审呈现适配器：S1 只转发一次完整评审意图，并呈现 F5 工作台返回的页面状态与通知。
// ignore-tidy-filelength: 轻量建议辅助（建议筛选/一键应用/撤销上一批）并入既有评审呈现适配器，
// 保持 S1"只转发完整意图"接缝的内聚（本文件不含功能模块呈现翻译之外的业务）。失效里程碑：
// v2.1.0（2026-12-31），届时将建议相关呈现翻译拆入独立模块后消除

use super::collection::COLLECTION_CATEGORY_KEYS;
use super::workspace_boundary::WorkspaceProductionContext;
use crate::presentation::{
    ConfirmationPresentation, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, ReviewPageState, ReviewRequest, Screen,
};
use crate::ReviewCandidateData;
use localization::Localization;
use notification_center::Notification;
use review_workbench::{
    text_keys, CandidateKey, CommandOutcome, ExportSummary, MapObjectView, ReviewWorkbench,
    StateChange, SuggestFilter, WorkbenchView,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

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
pub(crate) struct ReviewProductionAdapter {
    pub(crate) context: WorkspaceProductionContext,
    /// T39：评审地图回推同步状态（全量只推一次，之后增量只推对应候选）。
    map_sync: Rc<RefCell<ReviewMapSync>>,
}

/// T39：评审地图回推同步状态（T41：地图只绘制当前类别 + 当前页的可见集合）。
#[derive(Default)]
struct ReviewMapSync {
    /// 已推送到 JS 的三态（candidate_id -> state 标识符；增量 diff 基准）。
    pushed_states: HashMap<String, String>,
    /// 已推送的高亮候选（None = 无高亮）。
    pushed_highlight: Option<String>,
    /// 已推送的可见集合身份（active_category_index, page_index）。
    /// 分类/翻页变化时触发一次全量重推（清旧 overlay + 画新集合）。
    pushed_visible: Option<(usize, usize)>,
    /// 全量推送是否已排定（幂等，防止多次 map_ready 重复排定）。
    full_push_scheduled: bool,
}

/// 当前应绘制到评审地图的可见候选集合（当前类别 + 当前分页）。
struct VisibleReviewSet {
    active_category_index: usize,
    page_index: usize,
    objects: Vec<MapObjectView>,
}

/// T39：评审候选列表分页页大小（Slint 无虚拟化；50–100 内取 60）。
const REVIEW_PAGE_SIZE: usize = 60;

impl ReviewProductionAdapter {
    pub(crate) fn new(context: WorkspaceProductionContext) -> Self {
        Self {
            context,
            map_sync: Rc::new(RefCell::new(ReviewMapSync::default())),
        }
    }

    fn empty_page(&self) -> ReviewPageState {
        let workspace = self.context.page();
        let injector = self.context.injector();
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
            page_size: REVIEW_PAGE_SIZE as i32,
            page_index: 0,
            page_total: 1,
            page_label: l10n.t_with_args(
                "review.page_label",
                serde_json::json!({ "current": 1, "total": 1 }),
            ),
            page_prev_label: l10n.t("review.page_prev"),
            page_next_label: l10n.t("review.page_next"),
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
            legend: l10n.t("review.legend"),
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
            suggestion_filters_label: l10n.t(text_keys::SUGGESTION_FILTERS_LABEL),
            suggestion_filter_labels: Vec::new(),
            suggestion_filter_counts: Vec::new(),
            suggestion_filter_active: Vec::new(),
            apply_suggestions_label: l10n.t(text_keys::APPLY_SUGGESTIONS),
            undo_suggestions_label: l10n.t(text_keys::UNDO_SUGGESTIONS),
            apply_suggestions_enabled: false,
            undo_available: false,
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
        let workspace = self.context.page();
        let injector = self.context.injector();
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
            .map(|tab| {
                l10n.t_with_args(
                    "review.category_tab",
                    serde_json::json!({
                        "label": l10n.t(tab.label_key),
                        "count": tab.count,
                    }),
                )
            })
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
        let mut cards: Vec<ReviewCandidateData> = view
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
                suggestion_action_label: card
                    .suggestion
                    .as_ref()
                    .map(|s| l10n.t(s.action_key))
                    .unwrap_or_default()
                    .into(),
                suggestion_reason: card
                    .suggestion
                    .as_ref()
                    .map(|s| l10n.t_with_args(s.reason_key, s.reason_args.clone()))
                    .unwrap_or_default()
                    .into(),
            })
            .collect();
        // T39：Slint 无虚拟化 → 卡片分页（每页 60，50–100 内）。全量卡片只在
        // 切分类/翻页时重建；三态/高亮变更由呈现层单卡更新（set_row_data）。
        let page_total = cards.len().div_ceil(REVIEW_PAGE_SIZE).max(1);
        let page_index = {
            let mut session = self.context.session.borrow_mut();
            let index = session.review_page_index.min(page_total - 1);
            session.review_page_index = index;
            index
        };
        let start = page_index * REVIEW_PAGE_SIZE;
        let end = (start + REVIEW_PAGE_SIZE).min(cards.len());
        cards = cards[start..end].to_vec();
        let page_label = l10n.t_with_args(
            "review.page_label",
            serde_json::json!({ "current": page_index + 1, "total": page_total }),
        );
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
        let suggestion_filter_labels: Vec<String> = view
            .suggestion_filters
            .iter()
            .map(|filter| {
                l10n.t_with_args(
                    text_keys::SUGGESTION_FILTER_TAB,
                    serde_json::json!({
                        "label": l10n.t(filter.label_key),
                        "count": filter.count,
                    }),
                )
            })
            .collect();
        let suggestion_filter_counts: Vec<i32> = view
            .suggestion_filters
            .iter()
            .map(|filter| filter.count as i32)
            .collect();
        let suggestion_filter_active: Vec<i32> = view
            .suggestion_filters
            .iter()
            .map(|filter| i32::from(filter.active))
            .collect();
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
            page_size: REVIEW_PAGE_SIZE as i32,
            page_index: page_index as i32,
            page_total: page_total as i32,
            page_label,
            page_prev_label: l10n.t("review.page_prev"),
            page_next_label: l10n.t("review.page_next"),
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
            legend: l10n.t("review.legend"),
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
            suggestion_filters_label: l10n.t(view.suggestion_filters_label_key),
            suggestion_filter_labels,
            suggestion_filter_counts,
            suggestion_filter_active,
            apply_suggestions_label: l10n.t(view.apply_suggestions_label_key),
            undo_suggestions_label: l10n.t(view.undo_suggestions_label_key),
            apply_suggestions_enabled: view.apply_suggestions_enabled,
            undo_available: view.undo_available,
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

    /// 把 F5 当前**可见集合**（当前类别 + 当前分页）的候选标注与高亮同步到
    /// 评审地图（T39/T41）。
    ///
    /// 分类/翻页变化触发一次 `setReviewCandidates`（清旧 overlay + 画新集合），
    /// 同页内的 state/highlight 变更才走 `updateReviewCandidate` /
    /// `highlightReviewCandidate` 增量路径。地图不可用或非评审页时为空操作。
    fn sync_review_map(&self) {
        if !crate::map_webview::is_review_page() || !crate::map_webview::review_push_visible() {
            return;
        }
        let injector = self.context.injector();
        let injector = injector.borrow();
        let Some(workbench) = injector.review() else {
            return;
        };
        let view = workbench.view();
        let visible = visible_review_set_for(&self.context, &view);
        drop(injector);

        let mut sync = self.map_sync.borrow_mut();
        let full_needed =
            sync.pushed_visible != Some((visible.active_category_index, visible.page_index));
        if full_needed {
            push_full_visible_sync(&visible, &mut sync);
            sync.full_push_scheduled = false;
            return;
        }

        let mut scripts = Vec::new();
        for object in &visible.objects {
            let state = object.state.to_identifier().to_string();
            if sync.pushed_states.get(&object.candidate_id) != Some(&state) {
                let json = serde_json::to_string(&map_object_json(object))
                    .unwrap_or_else(|_| "{}".to_string());
                scripts.push(format!("window.updateReviewCandidate({json});"));
                sync.pushed_states
                    .insert(object.candidate_id.clone(), state);
            }
        }
        let highlighted = visible
            .objects
            .iter()
            .find(|object| object.highlighted)
            .map(|object| object.candidate_id.clone());
        if sync.pushed_highlight != highlighted {
            match &highlighted {
                Some(key) => {
                    let id = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    scripts.push(format!("window.highlightReviewCandidate({id});"));
                }
                None => scripts.push("window.clearReviewHighlight();".to_string()),
            }
            sync.pushed_highlight = highlighted;
        }
        sync.full_push_scheduled = false;
        drop(sync);
        for script in scripts {
            push_review_script(&script);
        }
    }

    /// map_ready 后的全量推送：在事件循环安全上下文排定一次，按当前可见集合
    /// 生成脚本（IPC 回调栈内不执行 WebView2 脚本调用，T35/T38 纪律）。
    fn schedule_review_map_full_push(&self) {
        {
            let mut sync = self.map_sync.borrow_mut();
            if sync.full_push_scheduled {
                return;
            }
            sync.full_push_scheduled = true;
        }
        let context = self.context.clone();
        let map_sync = Rc::clone(&self.map_sync);
        slint::Timer::single_shot(std::time::Duration::from_millis(1), move || {
            let injector = context.injector();
            let injector = injector.borrow();
            let Some(workbench) = injector.review() else {
                map_sync.borrow_mut().full_push_scheduled = false;
                return;
            };
            let view = workbench.view();
            let visible = visible_review_set_for(&context, &view);
            drop(injector);
            let mut sync = map_sync.borrow_mut();
            let visible_key = (visible.active_category_index, visible.page_index);
            if sync.pushed_visible == Some(visible_key) {
                // 该可见集合已由用户操作路径内联推送（测试无事件循环或生产
                // 1ms 窗口内用户先操作）：不再重复推一次。
                sync.full_push_scheduled = false;
                return;
            }
            push_full_visible_sync(&visible, &mut sync);
            sync.full_push_scheduled = false;
        });
    }

    /// 创建评审地图（候选不再内嵌 HTML；无密钥/锚点时跳过，抽屉仍可操作）。
    /// 候选标注在页面 map_ready 后由 Rust 按当前可见集合推送。
    fn show_review_map(&self) {
        let Some((keys, anchor)) = self.context.map_credentials() else {
            return;
        };
        if keys.0.is_empty() {
            return;
        }
        let injector = self.context.injector();
        let injector = injector.borrow();
        let map_text_label = injector.l10n().t("review.map_text_toggle");
        drop(injector);
        crate::map_webview::show_review(
            self.context.window.clone(),
            keys.0,
            keys.1,
            anchor.0,
            anchor.1,
            map_text_label,
        );
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
        let injector = self.context.injector();
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
        let injector = self.context.injector();
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
        let injector = self.context.injector();
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
                let injector = self.context.injector();
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
            CommandOutcome::NeedsSuggestionConfirmation(request) => {
                let injector = self.context.injector();
                let injector = injector.borrow();
                let l10n = injector.l10n();
                let main = l10n.t_with_args(
                    text_keys::APPLY_SUGGEST_CONFIRM_BODY,
                    serde_json::json!({
                        "count": request.count,
                        "keep": request.keep_count,
                        "remove": request.remove_count,
                    }),
                );
                let lines: Vec<String> = request
                    .reason_lines
                    .iter()
                    .map(|line| {
                        l10n.t_with_args(
                            text_keys::APPLY_SUGGEST_REASON_LINE,
                            serde_json::json!({
                                "label": l10n.t(line.summary_key),
                                "count": line.count,
                            }),
                        )
                    })
                    .collect();
                let body = if lines.is_empty() {
                    main
                } else {
                    format!(
                        "{main}\n{}\n{}",
                        l10n.t(text_keys::APPLY_SUGGEST_REASON_LABEL),
                        lines.join("\n")
                    )
                };
                let confirmation = ConfirmationPresentation::new(
                    l10n.t(text_keys::APPLY_SUGGEST_CONFIRM_TITLE),
                    body,
                    l10n.t(text_keys::CONFIRM_BUTTON),
                    l10n.t(text_keys::CANCEL_BUTTON),
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
        let injector = self.context.injector();
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
        let injector = self.context.injector();
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
        let Some(plan_id) = self.context.active_plan_id() else {
            return Presentation::ready(self.empty_page())
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        };
        let result = PlanId::parse(&plan_id)
            .map_err(|_| anyhow::anyhow!("invalid plan id"))
            .and_then(|plan_id| {
                let injector = self.context.injector();
                let mut injector = injector.borrow_mut();
                injector.enter_review(&plan_id)
            });
        match result {
            Ok(()) => {
                // T38/T39：评审地图在候选装载后创建——候选不再内嵌 HTML；
                // 页面加载后发送 map_ready，Rust 在事件循环安全上下文全量
                // 推送一次（回调栈内不执行 WebView2 脚本调用，T35 同纪律）。
                self.show_review_map();
                Presentation::ready(self.page_state())
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Err(_) => self
                .enter_failure_presentation()
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
        }
    }
}

/// 当前应绘制到评审地图的候选集合：当前类别 + 当前分页（每页 ≤60）。
///
/// 分页索引由工作区会话保存；此处同时把越界索引钳制回写，保证地图与列表
/// 永远绘制同一批候选，且全量推送不会把 12,000 条候选全部排进 JS 缓冲。
fn visible_review_set_for(
    context: &WorkspaceProductionContext,
    view: &WorkbenchView,
) -> VisibleReviewSet {
    let active_category_index = view
        .category_tabs
        .iter()
        .position(|tab| tab.active)
        .unwrap_or(0);
    let page_total = view.cards.len().div_ceil(REVIEW_PAGE_SIZE).max(1);
    let mut session = context.session.borrow_mut();
    let page_index = session.review_page_index.min(page_total - 1);
    session.review_page_index = page_index;
    drop(session);

    let start = page_index * REVIEW_PAGE_SIZE;
    let end = (start + REVIEW_PAGE_SIZE).min(view.cards.len());
    let id_set: std::collections::HashSet<String> = view.cards[start..end]
        .iter()
        .map(|card| card.candidate_id.clone())
        .collect();
    let objects: Vec<MapObjectView> = view
        .map_objects
        .iter()
        .filter(|object| id_set.contains(&object.candidate_id))
        .cloned()
        .collect();

    VisibleReviewSet {
        active_category_index,
        page_index,
        objects,
    }
}

/// 全量推送当前可见集合：`setReviewCandidates` 会清掉旧 overlay 再画新集合，
/// 因此分类/翻页时不会残留上一页/上一类别的标注。
fn push_full_visible_sync(visible: &VisibleReviewSet, sync: &mut ReviewMapSync) {
    let objects: Vec<serde_json::Value> = visible.objects.iter().map(map_object_json).collect();
    let states: HashMap<String, String> = visible
        .objects
        .iter()
        .map(|object| {
            (
                object.candidate_id.clone(),
                object.state.to_identifier().to_string(),
            )
        })
        .collect();
    let highlight = visible
        .objects
        .iter()
        .find(|object| object.highlighted)
        .map(|object| object.candidate_id.clone());
    let scripts = full_push_scripts(&objects, highlight.as_ref());
    for script in &scripts {
        push_review_script(script);
    }
    sync.pushed_states = states;
    sync.pushed_highlight = highlight;
    sync.pushed_visible = Some((visible.active_category_index, visible.page_index));
}

/// 评审地图对象 → JS 载荷（与 B3 回推协议同构）。
fn map_object_json(object: &review_workbench::MapObjectView) -> serde_json::Value {
    serde_json::json!({
        "candidate_id": object.candidate_id,
        "kind": object.shape_kind,
        "coordinates": object.shape_coordinates,
        "state": object.state.to_identifier(),
    })
}

/// 全量推送脚本：一条 `setReviewCandidates(可见集合)` + 高亮命令（如有）。
/// 可见集合受当前类别 + 当前分页限制，因此不会把全量候选排进 JS 缓冲。
fn full_push_scripts(objects: &[serde_json::Value], highlight: Option<&String>) -> Vec<String> {
    let json = serde_json::to_string(objects).unwrap_or_else(|_| "[]".to_string());
    let mut scripts = vec![format!("window.setReviewCandidates({json});")];
    if let Some(key) = highlight {
        let id = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
        scripts.push(format!("window.highlightReviewCandidate({id});"));
    }
    scripts
}

/// 评审地图回推命令：先计数（T39 验收观察），再执行（无 WebView 时为空操作）。
fn push_review_script(script: &str) {
    crate::map_webview::note_review_push(script);
    crate::map_webview::evaluate_script(script);
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
                // T39：切分类复位分页到第一页（新分类从第一张卡开始浏览）。
                self.context.session.borrow_mut().review_page_index = 0;
                self.mutate_simple(|workbench| workbench.set_active_category(category))
            }
            ReviewRequest::PagePrev => {
                let mut session = self.context.session.borrow_mut();
                session.review_page_index = session.review_page_index.saturating_sub(1);
                drop(session);
                self.mutate_simple(|_| {})
            }
            ReviewRequest::PageNext => {
                // 上限由 page_state_quiet 按当前分类总页数钳制回写。
                let mut session = self.context.session.borrow_mut();
                session.review_page_index += 1;
                drop(session);
                self.mutate_simple(|_| {})
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
                let mut target_page_index = None;
                let result = self.apply(|workbench| {
                    let target_category = workbench
                        .view()
                        .map_objects
                        .iter()
                        .find(|object| object.candidate_id == candidate_id)
                        .map(|object| object.category);
                    if let Some(category) = target_category {
                        workbench.set_active_category(category);
                        target_page_index = workbench
                            .view()
                            .cards
                            .iter()
                            .position(|card| card.candidate_id == candidate_id)
                            .map(|index| index / REVIEW_PAGE_SIZE);
                    }
                    workbench.highlight(&key)
                });
                if let Some(page_index) = target_page_index {
                    self.context.session.borrow_mut().review_page_index = page_index;
                }
                let located = matches!(&result, Ok(Some(())));
                if located {
                    // page_state 同步时不额外推一次 highlight；跨类别/分页时仍会
                    // 先 setReviewCandidates 目标可见集合，再由下方 locate 定位。
                    self.map_sync.borrow_mut().pushed_highlight = Some(candidate_id.clone());
                }
                let presentation = self.present_apply(result);
                // 地图中心跳转 + 高亮（JS 侧；无 WebView/非评审页时为空操作）。
                // 必须放在 page_state 的目标页全量同步之后，避免 pending 静默丢失。
                if located && crate::map_webview::is_review_page() {
                    let id = serde_json::to_string(&candidate_id).unwrap_or_else(|_| "\"\"".into());
                    push_review_script(&format!("window.locateReviewCandidate({id});"));
                }
                presentation
            }
            ReviewRequest::MapReady => {
                // T39：评审地图就绪——候选不再内嵌 HTML，这里排定一次全量
                // 推送（事件循环安全上下文执行，不在 IPC 回调栈内
                // evaluate_script）；后续 state/highlight/locate 变更由
                // page_state 尾部的 sync_review_map 增量只推对应候选。
                self.schedule_review_map_full_push();
                Presentation::ready(self.page_state_quiet())
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ReviewRequest::MapFailed { message } => {
                // 用户只看到可行动的 B7 文案；受控文件日志仅记录有限阶段码，
                // 不写候选 ID、坐标、密钥、异常文本或未知 IPC 原文。
                let diagnostic_code = match message.as_str() {
                    "review_map_draw_failed:payload_validation"
                    | "review_map_draw_failed:centroid_build"
                    | "review_map_draw_failed:overlay_construct"
                    | "review_map_draw_failed:overlay_bind"
                    | "review_map_draw_failed:map_add"
                    | "review_map_draw_failed:centroid_index"
                    | "review_map_draw_failed:candidate_update"
                    | "review_map_draw_failed:locate"
                    | "review_map_draw_failed:fit_view"
                    | "review_map_locate_hidden"
                    | "review_map_locate_unavailable"
                    | "review_map_page_error"
                    | "review_map_sdk_timeout"
                    | "review_map_init_failed" => message.as_str(),
                    marker if marker == crate::map_webview::MAP_LOAD_TIMEOUT_MARKER => marker,
                    _ => "review_map_unclassified_failure",
                };
                log::warn!(target: "review_map", "failure_code={diagnostic_code}");
                let injector = self.context.injector();
                let injector = injector.borrow();
                let l10n = injector.l10n();
                let (title, body) = if message.starts_with("review_map_draw_failed:") {
                    (
                        l10n.t("review.map_draw_failed_title"),
                        l10n.t("review.map_draw_failed_body"),
                    )
                } else if message == "review_map_locate_hidden" {
                    (
                        l10n.t("review.map_locate_failed_title"),
                        l10n.t("review.map_locate_hidden_body"),
                    )
                } else if message == "review_map_locate_unavailable" {
                    (
                        l10n.t("review.map_locate_failed_title"),
                        l10n.t("review.map_locate_unavailable_body"),
                    )
                } else if message == crate::map_webview::MAP_LOAD_TIMEOUT_MARKER {
                    (
                        l10n.t("review.map_unavailable_title"),
                        l10n.t("map.load_timeout_body"),
                    )
                } else {
                    (
                        l10n.t("review.map_unavailable_title"),
                        l10n.t("review.map_unavailable_body"),
                    )
                };
                let notification = Notification::error(l10n.t("app.source_tag"), title, body);
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
            ReviewRequest::ToggleSuggestionFilter { index } => {
                let Some(filter) = SuggestFilter::ALL.get(index).copied() else {
                    return Presentation::ready(self.page_state())
                        .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                self.mutate_simple(|workbench| workbench.toggle_suggestion_filter(filter))
            }
            ReviewRequest::ApplySuggestions => {
                self.present_submit(self.submit(|workbench| workbench.apply_suggestions()))
            }
            ReviewRequest::ConfirmSuggestionApply => {
                self.present_submit(self.submit(|workbench| workbench.confirm_suggestion_apply()))
            }
            ReviewRequest::CancelSuggestionApply => {
                self.present_apply(self.apply(|workbench| workbench.cancel_suggestion_apply()))
            }
            ReviewRequest::UndoSuggestionApply => self.present_apply(
                self.apply(|workbench| workbench.undo_last_suggestion_apply().map(|_| ())),
            ),
            ReviewRequest::Pause => {
                let plan_id = self.context.active_plan_id().unwrap_or_default();
                let path = self.session_path(&plan_id);
                let result = {
                    let injector = self.context.injector();
                    let injector = injector.borrow();
                    injector
                        .review()
                        .map(|workbench| workbench.save_session(&path))
                };
                match result {
                    Some(Ok(())) => {
                        let injector = self.context.injector();
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
                let plan_id = self.context.active_plan_id().unwrap_or_default();
                let path = self.session_path(&plan_id);
                let result = self.apply(|workbench| workbench.restore_session(&path));
                match result {
                    Ok(Some(())) => {
                        let injector = self.context.injector();
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
                    let injector = self.context.injector();
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
