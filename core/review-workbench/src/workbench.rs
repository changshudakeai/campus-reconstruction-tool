//! 评审台核心：一次性读入、状态变更操作、批量确认闸、暂停/恢复、封账写回
//!
//! 缝 4 契约的功能层一侧（无卡顿铁律）：
//! - [`ReviewWorkbench::load`] 进台时向 B2 一次性读入候选集到内存；
//! - 评审期间所有操作纯内存（本类型不持有数据库句柄，结构上保证零写库）；
//! - [`ReviewWorkbench::seal`] 封账时把最终三态一次性批量写回 B2，
//!   写回失败则封账不生效（评审状态保持可改）。

use data_persistence::{
    CandidateProjectionsApi, Database, ReviewDecision, ReviewDecisionsApi, ReviewDraftEntry,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use std::path::Path;

use crate::candidate::{Candidate, CandidateKey};
use crate::command::{CommandOutcome, ConfirmationRequest, StateChange};
use crate::error::{Error, Result};
use crate::session::{SessionEntry, SessionSnapshot};
use crate::suggestion::{
    apply_request, AppliedSuggestionBatch, SuggestFilter, SuggestionApplyRequest, SuggestionEngine,
};
use crate::view_models::{
    category_text_key, state_text_key, suggestion_action_text_key, suggestion_category_text_key,
    text_keys, CandidateCardView, CategoryTabView, ExportSummary, InfoPanelView, MapObjectView,
    SuggestionCardView, SuggestionFilterView, WorkbenchView,
};

/// 类别抽屉的固定展示顺序（ADR-0016 左栏标签页）
const CATEGORY_ORDER: [CandidateCategory; 6] = [
    CandidateCategory::Building,
    CandidateCategory::Road,
    CandidateCategory::Water,
    CandidateCategory::Vegetation,
    CandidateCategory::Sports,
    CandidateCategory::Other,
];

/// 评审工作台（纯内存状态机；读入与封账之外不接触数据库）
#[derive(Debug, Clone)]
pub struct ReviewWorkbench {
    plan_id: String,
    /// 打开评审台时的候选批次 revision；封账时用来拒绝旧页面写回。
    projection_revision: Option<String>,
    candidates: Vec<Candidate>,
    active_category: CandidateCategory,
    highlighted: Option<CandidateKey>,
    /// 等待二次确认的批量剔除操作（弹窗期间暂存，确认后执行）
    pending_confirmation: Option<StateChange>,
    /// 等待确认的一键应用建议计划（弹窗期间暂存，确认后执行）
    pending_suggestion_apply: Option<SuggestionApplyPlan>,
    /// 最近一批已应用建议（撤销与追溯记录；封账后不可撤销）
    undo: Option<AppliedSuggestionBatch>,
    /// 当前激活的建议筛选器（多选，与类别组合）
    suggestion_filters: Vec<SuggestFilter>,
    sealed: bool,
}

/// 一键应用建议的待确认计划：按当前筛选范围预生成的两批状态变更。
#[derive(Debug, Clone)]
struct SuggestionApplyPlan {
    /// 建议保留的候选
    keep: Vec<CandidateKey>,
    /// 建议剔除的候选
    remove: Vec<CandidateKey>,
    /// 确认弹窗数据（对象数量 + 主要理由分布）
    request: SuggestionApplyRequest,
}

impl ReviewWorkbench {
    // ── 缝 4：一次性读入 ─────────────────────────────────

    /// 进入评审台：向 B2 一次性读入候选集到内存。
    ///
    /// 候选只来自当前已发布且可评审的 B2 投影，初始态一律"待定"（ADR-0022）；
    /// 若评审终态表已有本方案的记录（上一轮封账结果），按候选标识对回。
    pub fn load(db: &Database, plan_id: &PlanId) -> Result<Self> {
        let plan_key = plan_id.to_string();
        let projection_revision = db.current_candidate_batch_revision(&plan_key)?;
        let projections = db.list_reviewable_candidate_projections(&plan_key)?;
        let mut candidates: Vec<Candidate> =
            projections.iter().map(Candidate::from_projection).collect();

        // 上一轮封账写回的终态对回内存（没有记录的保持"待定"）
        for decision in db.list_review_decisions(&plan_key)? {
            let key = CandidateKey::new(decision.candidate_id.clone());
            if let Some(candidate) = candidates.iter_mut().find(|c| c.key == key) {
                candidate.state = decision.review_state;
            }
        }

        let active_category = CATEGORY_ORDER
            .into_iter()
            .find(|category| candidates.iter().any(|c| c.category == *category))
            .unwrap_or(CandidateCategory::Building);

        let mut workbench = Self {
            plan_id: plan_key,
            projection_revision,
            candidates,
            active_category,
            highlighted: None,
            pending_confirmation: None,
            pending_suggestion_apply: None,
            undo: None,
            suggestion_filters: Vec::new(),
            sealed: false,
        };
        // 轻量建议：进台时按候选数据确定性生成；只读，不改变 ReviewState。
        let suggestions = SuggestionEngine::compute(&workbench.candidates);
        for (key, suggestion) in suggestions {
            if let Some(candidate) = workbench
                .candidates
                .iter_mut()
                .find(|candidate| candidate.key == key)
            {
                candidate.suggestion = Some(suggestion);
            }
        }
        Ok(workbench)
    }

    /// 所属方案 ID
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// 候选总数
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// 查询某候选的当前三态
    pub fn state_of(&self, key: &CandidateKey) -> Option<ReviewState> {
        self.candidates
            .iter()
            .find(|c| &c.key == key)
            .map(|c| c.state)
    }

    // ── 轻量建议（验收 1/2/5：可解释、只读现有数据、不改三态）────────

    /// 某候选的建议（进台时确定性生成）。
    pub fn suggestion_of(
        &self,
        key: &CandidateKey,
    ) -> Option<&crate::suggestion::CandidateSuggestion> {
        self.candidates
            .iter()
            .find(|c| &c.key == key)
            .and_then(|c| c.suggestion.as_ref())
    }

    /// 全部建议（按候选标识升序）。
    pub fn suggestions(&self) -> Vec<(&CandidateKey, &crate::suggestion::CandidateSuggestion)> {
        let mut pairs: Vec<_> = self
            .candidates
            .iter()
            .filter_map(|c| c.suggestion.as_ref().map(|s| (&c.key, s)))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        pairs
    }

    /// 建议筛选器是否已激活。
    pub fn suggestion_filter_active(&self, filter: SuggestFilter) -> bool {
        self.suggestion_filters.contains(&filter)
    }

    /// 当前激活的建议筛选器（固定顺序）。
    pub fn active_suggestion_filters(&self) -> Vec<SuggestFilter> {
        SuggestFilter::ALL
            .into_iter()
            .filter(|filter| self.suggestion_filter_active(*filter))
            .collect()
    }

    /// 命中某筛选器的候选总数（跨类别）。
    pub fn suggestion_filter_count(&self, filter: SuggestFilter) -> usize {
        self.candidates
            .iter()
            .filter(|c| {
                c.suggestion
                    .as_ref()
                    .is_some_and(|suggestion| filter.matches(suggestion))
            })
            .count()
    }

    /// 切换建议筛选器（多选，与类别组合）。
    pub fn toggle_suggestion_filter(&mut self, filter: SuggestFilter) {
        if let Some(position) = self.suggestion_filters.iter().position(|f| *f == filter) {
            self.suggestion_filters.remove(position);
        } else {
            self.suggestion_filters.push(filter);
        }
    }

    /// 一键应用是否可用：未封账，且当前筛选范围内存在可执行（保留/剔除）建议。
    pub fn apply_suggestions_enabled(&self) -> bool {
        if self.sealed || self.active_suggestion_filters().is_empty() {
            return false;
        }
        let (keep, remove) = self.actionable_scope();
        !keep.is_empty() || !remove.is_empty()
    }

    /// 是否存在可撤销的上一批（封账后不可撤销）。
    pub fn can_undo_suggestion_apply(&self) -> bool {
        !self.sealed && self.undo.is_some()
    }

    /// 最近一批已应用建议的追溯记录（批次与理由；封账后仍可读但不可撤销）。
    pub fn last_applied_suggestion_batch(&self) -> Option<&AppliedSuggestionBatch> {
        self.undo.as_ref()
    }

    /// 当前筛选范围内的可执行建议：当前类别 ∩ 激活筛选器（至少一个激活）。
    fn actionable_scope(&self) -> (Vec<CandidateKey>, Vec<CandidateKey>) {
        let filters = self.active_suggestion_filters();
        if filters.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let mut keep = Vec::new();
        let mut remove = Vec::new();
        for candidate in &self.candidates {
            if candidate.category != self.active_category {
                continue;
            }
            let Some(suggestion) = candidate.suggestion.as_ref() else {
                continue;
            };
            if !filters.iter().any(|filter| filter.matches(suggestion)) {
                continue;
            }
            match suggestion.action {
                crate::suggestion::SuggestionAction::Keep => keep.push(candidate.key.clone()),
                crate::suggestion::SuggestionAction::Remove => remove.push(candidate.key.clone()),
                crate::suggestion::SuggestionAction::HumanReview => {}
            }
        }
        keep.sort();
        remove.sort();
        (keep, remove)
    }

    /// 一键应用建议（验收 4）：对当前筛选范围生成保留/剔除批次并请求确认；
    /// 生成建议本身不改变任何 `ReviewState`（验收 5）。
    pub fn apply_suggestions(&mut self) -> Result<CommandOutcome> {
        self.ensure_not_sealed()?;
        let (keep, remove) = self.actionable_scope();
        if keep.is_empty() && remove.is_empty() {
            return Ok(CommandOutcome::Applied { changed: 0 });
        }
        let request = apply_request(&keep, &remove, &self.candidates);
        self.pending_suggestion_apply = Some(SuggestionApplyPlan {
            keep,
            remove,
            request: request.clone(),
        });
        Ok(CommandOutcome::NeedsSuggestionConfirmation(request))
    }

    /// 确认弹窗点了"确认"：复用既有批量状态变更机制执行保留/剔除两批，
    /// 记录批次与理由供撤销与追溯；取消路径见
    /// [`Self::cancel_suggestion_apply`]。
    pub fn confirm_suggestion_apply(&mut self) -> Result<CommandOutcome> {
        self.ensure_not_sealed()?;
        let plan = self
            .pending_suggestion_apply
            .take()
            .ok_or(Error::NoSuggestionApplyPending)?;
        let before_states: Vec<(CandidateKey, shared_domain_types::ReviewState)> = plan
            .keep
            .iter()
            .chain(plan.remove.iter())
            .filter_map(|key| self.state_of(key).map(|state| (key.clone(), state)))
            .collect();
        let mut changed = 0;
        if !plan.keep.is_empty() {
            changed += self.apply(&StateChange::batch(
                plan.keep.clone(),
                shared_domain_types::ReviewState::Keep,
            ));
        }
        if !plan.remove.is_empty() {
            changed += self.apply(&StateChange::batch(
                plan.remove.clone(),
                shared_domain_types::ReviewState::Remove,
            ));
        }
        let mut targets: Vec<CandidateKey> =
            before_states.iter().map(|(key, _)| key.clone()).collect();
        targets.sort();
        self.undo = Some(AppliedSuggestionBatch {
            targets,
            keep_count: plan.request.keep_count,
            remove_count: plan.request.remove_count,
            reason_lines: plan.request.reason_lines,
            before_states,
        });
        Ok(CommandOutcome::Applied { changed })
    }

    /// 确认弹窗点了"取消"：丢弃待确认计划，状态原样不动。
    pub fn cancel_suggestion_apply(&mut self) -> Result<()> {
        self.pending_suggestion_apply
            .take()
            .map(|_| ())
            .ok_or(Error::NoSuggestionApplyPending)
    }

    /// 撤销上一批（验收 6）：只覆盖未封账前的最近一批；封账后拒绝。
    pub fn undo_last_suggestion_apply(&mut self) -> Result<usize> {
        self.ensure_not_sealed()?;
        let batch = self.undo.take().ok_or(Error::NoSuggestionApplyToUndo)?;
        let mut changed = 0;
        for (key, state) in &batch.before_states {
            if let Some(candidate) = self.candidates.iter_mut().find(|c| &c.key == key) {
                if candidate.state != *state {
                    candidate.state = *state;
                    changed += 1;
                }
            }
        }
        Ok(changed)
    }

    // ── 状态变更操作（B8 接口预留，ADR-0022 验收标准）──────

    /// 提交一次状态变更操作。
    ///
    /// 批量剔除 ≥5 项被拦下并返回 [`CommandOutcome::NeedsConfirmation`]
    /// （操作暂存，等 [`confirm_pending`](Self::confirm_pending) /
    /// [`cancel_pending`](Self::cancel_pending)）；其余情况立即执行，
    /// 纯内存无卡顿。点错状态直接再提交另一个状态即可（状态即后悔药）。
    pub fn submit(&mut self, change: StateChange) -> Result<CommandOutcome> {
        self.ensure_not_sealed()?;
        for target in &change.targets {
            if !self.candidates.iter().any(|c| &c.key == target) {
                return Err(Error::CandidateNotFound(target.to_string()));
            }
        }
        if change.needs_confirmation() {
            let request = ConfirmationRequest::batch_remove(change.targets.len());
            self.pending_confirmation = Some(change);
            return Ok(CommandOutcome::NeedsConfirmation(request));
        }
        let changed = self.apply(&change);
        Ok(CommandOutcome::Applied { changed })
    }

    /// 二次确认弹窗点了"确认"：执行暂存的批量剔除
    pub fn confirm_pending(&mut self) -> Result<CommandOutcome> {
        self.ensure_not_sealed()?;
        let change = self
            .pending_confirmation
            .take()
            .ok_or(Error::NoPendingConfirmation)?;
        let changed = self.apply(&change);
        Ok(CommandOutcome::Applied { changed })
    }

    /// 二次确认弹窗点了"取消"：丢弃暂存的批量剔除，状态原样不动
    pub fn cancel_pending(&mut self) -> Result<()> {
        self.pending_confirmation
            .take()
            .map(|_| ())
            .ok_or(Error::NoPendingConfirmation)
    }

    /// 便捷入口：把当前勾选的全部候选改为目标三态
    ///（批量改保留/待定直接执行；批量剔除 ≥5 项走确认闸）
    pub fn submit_for_selected(&mut self, to: ReviewState) -> Result<CommandOutcome> {
        let targets: Vec<CandidateKey> = self
            .candidates
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.key.clone())
            .collect();
        self.submit(StateChange::batch(targets, to))
    }

    /// 执行状态变更，返回实际改变状态的候选数
    fn apply(&mut self, change: &StateChange) -> usize {
        let mut changed = 0;
        for target in &change.targets {
            if let Some(candidate) = self.candidates.iter_mut().find(|c| &c.key == target) {
                if candidate.state != change.to {
                    candidate.state = change.to;
                    changed += 1;
                }
            }
        }
        changed
    }

    fn ensure_not_sealed(&self) -> Result<()> {
        if self.sealed {
            return Err(Error::AlreadySealed);
        }
        Ok(())
    }

    // ── 勾选与全选浮现（ADR-0016）─────────────────────────

    /// 切换某候选的复选框，返回新勾选状态
    pub fn toggle_selected(&mut self, key: &CandidateKey) -> Result<bool> {
        let candidate = self
            .candidates
            .iter_mut()
            .find(|c| &c.key == key)
            .ok_or_else(|| Error::CandidateNotFound(key.to_string()))?;
        candidate.selected = !candidate.selected;
        Ok(candidate.selected)
    }

    /// 点“全选”：当前激活类别的所有卡片勾选。
    pub fn select_all_in_active_category(&mut self) {
        let category = self.active_category;
        for candidate in &mut self.candidates {
            if candidate.category == category {
                candidate.selected = true;
            }
        }
    }

    /// 点“取消全选”：当前激活类别的所有卡片取消勾选。
    pub fn deselect_all_in_active_category(&mut self) {
        let category = self.active_category;
        for candidate in &mut self.candidates {
            if candidate.category == category {
                candidate.selected = false;
            }
        }
    }

    /// 当前勾选数（跨类别）
    pub fn selected_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.selected).count()
    }

    /// 勾选 ≥2 个时自动浮现“全选/取消全选”按钮（ADR-0016 交互规则）。
    pub fn bulk_buttons_visible(&self) -> bool {
        self.selected_count() >= 2
    }

    // ── 类别抽屉与高亮联动（ADR-0016）────────────────────

    /// 切换激活的类别抽屉
    pub fn set_active_category(&mut self, category: CandidateCategory) {
        self.active_category = category;
    }

    /// 当前激活的类别抽屉
    pub fn active_category(&self) -> CandidateCategory {
        self.active_category
    }

    /// 高亮一个候选：点地图上的对象高亮对应卡片，点卡片高亮地图对象——
    /// 两个方向共用同一份高亮状态（双向联动）。
    pub fn highlight(&mut self, key: &CandidateKey) -> Result<()> {
        let candidate = self
            .candidates
            .iter()
            .find(|candidate| &candidate.key == key)
            .ok_or_else(|| Error::CandidateNotFound(key.to_string()))?;
        self.highlighted = Some(candidate.key.clone());
        Ok(())
    }

    /// 清除高亮
    pub fn clear_highlight(&mut self) {
        self.highlighted = None;
    }

    /// 当前高亮的候选
    pub fn highlighted(&self) -> Option<&CandidateKey> {
        self.highlighted.as_ref()
    }

    // ── 暂停/恢复（内存状态持久化到临时文件）──────────────

    /// 暂停评审：把内存状态（三态 + 勾选 + 激活抽屉）写入临时文件
    pub fn save_session(&self, path: &Path) -> Result<()> {
        let entries = self
            .candidates
            .iter()
            .map(|c| SessionEntry {
                candidate_id: c.key.candidate_id.clone(),
                state: c.state.to_identifier().to_owned(),
                selected: c.selected,
            })
            .collect();
        SessionSnapshot::new(
            self.plan_id.clone(),
            self.projection_revision.clone(),
            self.active_category,
            entries,
        )
        .save_to_file(path)
    }

    /// 恢复评审：从临时文件把状态对回内存。
    ///
    /// 方案 ID 不匹配时拒绝（防串档）；文件里对不上现有候选的条目安静跳过
    ///（候选集以 B2 已发布投影为事实来源）。
    pub fn restore_session(&mut self, path: &Path) -> Result<()> {
        self.ensure_not_sealed()?;
        let snapshot = SessionSnapshot::load_from_file(path)?;
        if snapshot.plan_id != self.plan_id {
            return Err(Error::SessionPlanMismatch {
                expected: self.plan_id.clone(),
                found: snapshot.plan_id,
            });
        }
        if snapshot.projection_revision != self.projection_revision {
            return Err(Error::SessionRevisionMismatch {
                expected: self
                    .projection_revision
                    .clone()
                    .unwrap_or_else(|| "no-published-batch".to_owned()),
                found: snapshot
                    .projection_revision
                    .unwrap_or_else(|| "legacy-without-revision".to_owned()),
            });
        }
        for entry in &snapshot.entries {
            let state = SessionSnapshot::parse_state(entry)?;
            if let Some(candidate) = self
                .candidates
                .iter_mut()
                .find(|candidate| candidate.key.candidate_id == entry.candidate_id)
            {
                candidate.state = state;
                candidate.selected = entry.selected;
            }
        }
        if self
            .candidates
            .iter()
            .any(|candidate| candidate.category == snapshot.active_category)
        {
            self.active_category = snapshot.active_category;
        }
        Ok(())
    }

    /// 导出当前全部候选的三态与勾选（安全检查点；应用流层据此落库）。
    ///
    /// 与 [`Self::save_session`] 的数据同源：核心工作台保持零写库，持久化
    /// 由应用流层在每次状态变更后调用本方法 + B2 草稿 API 完成。
    pub fn draft_entries(&self) -> Vec<ReviewDraftEntry> {
        self.candidates
            .iter()
            .map(|c| ReviewDraftEntry {
                candidate_id: c.key.candidate_id.clone(),
                review_state: c.state,
                selected: c.selected,
            })
            .collect()
    }

    /// 从安全检查点把三态与勾选对回内存（候选集以 B2 已发布投影为事实
    /// 来源；对不上的条目安静跳过，与 [`Self::restore_session`] 同一规则）。
    pub fn restore_draft_entries(&mut self, entries: &[ReviewDraftEntry]) -> Result<()> {
        self.ensure_not_sealed()?;
        for entry in entries {
            if let Some(candidate) = self
                .candidates
                .iter_mut()
                .find(|candidate| candidate.key.candidate_id == entry.candidate_id)
            {
                candidate.state = entry.review_state;
                candidate.selected = entry.selected;
            }
        }
        Ok(())
    }

    // ── 封账（缝 4：批量写回；缝 5：导出请求汇总）──────────

    /// 导出请求汇总：各类别保留数 + 待定报数（F9 确认弹窗的账本，ADR-0022）
    pub fn export_summary(&self) -> ExportSummary {
        let mut keep_by_category = Vec::new();
        for category in CATEGORY_ORDER {
            let count = self
                .candidates
                .iter()
                .filter(|c| c.category == category && c.state.is_keep())
                .count();
            if count > 0 {
                keep_by_category.push((category, count));
            }
        }
        ExportSummary {
            keep_by_category,
            keep_total: self.candidates.iter().filter(|c| c.state.is_keep()).count(),
            pending_count: self
                .candidates
                .iter()
                .filter(|c| c.state.is_pending())
                .count(),
            remove_count: self
                .candidates
                .iter()
                .filter(|c| c.state.is_remove())
                .count(),
        }
    }

    /// 封账：把全部候选的最终三态一次性批量写回 B2（单事务原子提交）。
    ///
    /// 写回失败时返回 Err 且封账**不生效**（评审状态保持可改），
    /// 保证不出现"账封了但没存上"的半截状态；成功后评审决定不可再改。
    pub fn seal(&mut self, db: &mut Database) -> Result<ExportSummary> {
        self.ensure_not_sealed()?;
        let decisions: Vec<ReviewDecision> = self
            .candidates
            .iter()
            .map(|c| {
                ReviewDecision::new(
                    self.plan_id.clone(),
                    c.category,
                    c.key.candidate_id.clone(),
                    c.state,
                )
            })
            .collect();
        if let Some(revision) = &self.projection_revision {
            db.batch_update_review_decisions_at_revision(&self.plan_id, revision, &decisions)?;
        } else if !decisions.is_empty() {
            return Err(data_persistence::Error::CandidateBatchRejected(
                "没有已发布候选批次却出现了评审对象".to_owned(),
            )
            .into());
        }
        self.sealed = true;
        self.pending_confirmation = None;
        self.pending_suggestion_apply = None;
        Ok(self.export_summary())
    }

    /// 是否已封账（评审入口禁用信号）
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    // ── 抽屉布局视图（地图为主区 + 左侧抽屉）────────────────

    /// 产出抽屉布局整体视图（左抽屉 + 中大地图）
    pub fn view(&self) -> WorkbenchView {
        let category_tabs = CATEGORY_ORDER
            .into_iter()
            .map(|category| CategoryTabView {
                category,
                label_key: category_text_key(category),
                count: self
                    .candidates
                    .iter()
                    .filter(|c| c.category == category)
                    .count(),
                active: category == self.active_category,
            })
            .collect();

        // 卡片列表 = 当前类别 ∩ 激活建议筛选器（无激活筛选器时显示全部）。
        let active_filters = self.active_suggestion_filters();
        let cards = self
            .candidates
            .iter()
            .filter(|c| {
                c.category == self.active_category
                    && (active_filters.is_empty()
                        || c.suggestion.as_ref().is_some_and(|suggestion| {
                            active_filters
                                .iter()
                                .any(|filter| filter.matches(suggestion))
                        }))
            })
            .map(|c| {
                let suggestion = c.suggestion.as_ref().map(|s| SuggestionCardView {
                    category_key: suggestion_category_text_key(s.category),
                    action_key: suggestion_action_text_key(s.action),
                    reason_key: s.reason_key,
                    reason_args: s.reason_args.clone(),
                });
                CandidateCardView {
                    candidate_id: c.key.candidate_id.clone(),
                    title: c.title.clone(),
                    named: c.named,
                    state: c.state,
                    state_key: state_text_key(c.state),
                    selected: c.selected,
                    highlighted: self.highlighted.as_ref() == Some(&c.key),
                    suggestion,
                }
            })
            .collect();

        let map_objects = self
            .candidates
            .iter()
            .map(|c| MapObjectView {
                candidate_id: c.key.candidate_id.clone(),
                category: c.category,
                state: c.state,
                shape_kind: c.shape.kind.clone(),
                shape_coordinates: c.shape.coordinates.clone(),
                source: c.source.clone(),
                highlighted: self.highlighted.as_ref() == Some(&c.key),
            })
            .collect();

        let info_panel = self.highlighted.as_ref().and_then(|key| {
            self.candidates
                .iter()
                .find(|c| &c.key == key)
                .map(|c| InfoPanelView {
                    title: c.title.clone(),
                    named: c.named,
                    category_label_key: text_keys::INFO_CATEGORY,
                    category_key: category_text_key(c.category),
                    tags_label_key: text_keys::INFO_TAGS,
                    tags: c.tags.clone(),
                    source_label_key: text_keys::INFO_SOURCE,
                    source: c.source.clone(),
                    state_label_key: text_keys::STATE_LABEL,
                    state_key: state_text_key(c.state),
                })
        });

        WorkbenchView {
            title_key: text_keys::WORKBENCH_TITLE,
            category_tabs,
            cards,
            map_objects,
            info_panel,
            selected_count: self.selected_count(),
            bulk_buttons_visible: self.bulk_buttons_visible(),
            pending_confirmation: self
                .pending_confirmation
                .as_ref()
                .map(|change| ConfirmationRequest::batch_remove(change.targets.len())),
            suggestion_filters_label_key: text_keys::SUGGESTION_FILTERS_LABEL,
            suggestion_filters: SuggestFilter::ALL
                .into_iter()
                .map(|filter| SuggestionFilterView {
                    filter,
                    label_key: filter.label_key(),
                    count: self.suggestion_filter_count(filter),
                    active: self.suggestion_filter_active(filter),
                })
                .collect(),
            apply_suggestions_label_key: text_keys::APPLY_SUGGESTIONS,
            undo_suggestions_label_key: text_keys::UNDO_SUGGESTIONS,
            apply_suggestions_enabled: self.apply_suggestions_enabled(),
            undo_available: self.can_undo_suggestion_apply(),
            pending_suggestion_apply: self
                .pending_suggestion_apply
                .as_ref()
                .map(|plan| plan.request.clone()),
            sealed: self.sealed,
        }
    }
}
