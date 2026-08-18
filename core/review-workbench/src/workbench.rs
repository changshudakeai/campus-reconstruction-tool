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
use crate::confidence::{ConfidenceFilter, ConfidenceTier};
use crate::error::{Error, Result};
use crate::session::{SessionEntry, SessionSnapshot};
use crate::suggestion::{
    apply_request, AppliedSuggestionBatch, SuggestionApplyRequest, SuggestionEngine,
};
use crate::view_models::{
    category_text_key, state_text_key, suggestion_action_text_key, suggestion_category_text_key,
    text_keys, CandidateCardView, CategoryTabView, ConfidenceFilterView, ExportSummary,
    InfoPanelView, MapObjectView, SuggestionCardView, WorkbenchView,
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

/// 置信度排序键：高→中→低（无建议兜底排最后），同档按稳定候选 ID 升序
/// 保证确定性（T51）。
fn confidence_order(a: &&Candidate, b: &&Candidate) -> std::cmp::Ordering {
    fn rank(candidate: &Candidate) -> u8 {
        match candidate.suggestion.as_ref().map(|s| s.confidence_tier()) {
            Some(ConfidenceTier::High) => 0,
            Some(ConfidenceTier::Medium) => 1,
            Some(ConfidenceTier::Low) => 2,
            None => 3,
        }
    }
    rank(a).cmp(&rank(b)).then_with(|| a.key.cmp(&b.key))
}

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
    /// 当前激活的置信度筛选芯片（单选，默认"全部"）
    confidence_filter: ConfidenceFilter,
    sealed: bool,
}

/// 一键应用建议的待确认计划：按当前筛选范围预生成的两批状态变更。
#[derive(Debug, Clone)]
struct SuggestionApplyPlan {
    /// 高置信、将改为保留的候选
    keep: Vec<CandidateKey>,
    /// 建议剔除的候选（T51 起恒为空：一键应用不再剔除任何候选）
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
            confidence_filter: ConfidenceFilter::All,
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

    /// 当前激活的置信度筛选芯片（默认"全部"）。
    pub fn confidence_filter(&self) -> ConfidenceFilter {
        self.confidence_filter
    }

    /// 切换置信度筛选芯片（单选，T51）。
    pub fn set_confidence_filter(&mut self, filter: ConfidenceFilter) {
        self.confidence_filter = filter;
    }

    /// 命中某置信度筛选的候选总数（跨类别；"全部"为候选总数）。
    pub fn confidence_filter_count(&self, filter: ConfidenceFilter) -> usize {
        self.candidates
            .iter()
            .filter(|c| {
                c.suggestion
                    .as_ref()
                    .is_some_and(|suggestion| filter.matches(suggestion))
            })
            .count()
    }

    /// 一键应用是否可用：未封账，且存在尚未保留的高置信候选（跨类别）。
    pub fn apply_suggestions_enabled(&self) -> bool {
        if self.sealed {
            return false;
        }
        self.candidates.iter().any(|candidate| {
            candidate.state != shared_domain_types::ReviewState::Keep
                && candidate
                    .suggestion
                    .as_ref()
                    .is_some_and(|suggestion| suggestion.confidence_tier() == ConfidenceTier::High)
        })
    }

    /// 是否存在可撤销的上一批（封账后不可撤销）。
    pub fn can_undo_suggestion_apply(&self) -> bool {
        !self.sealed && self.undo.is_some()
    }

    /// 最近一批已应用建议的追溯记录（批次与理由；封账后仍可读但不可撤销）。
    pub fn last_applied_suggestion_batch(&self) -> Option<&AppliedSuggestionBatch> {
        self.undo.as_ref()
    }

    /// 一键应用目标：全部尚未保留的高置信候选（跨类别，按稳定候选 ID 排序）。
    /// T51：只转保留，不剔除任何候选。
    fn high_confidence_keep_targets(&self) -> Vec<CandidateKey> {
        let mut targets: Vec<CandidateKey> =
            self.candidates
                .iter()
                .filter(|candidate| candidate.state != shared_domain_types::ReviewState::Keep)
                .filter(|candidate| {
                    candidate.suggestion.as_ref().is_some_and(|suggestion| {
                        suggestion.confidence_tier() == ConfidenceTier::High
                    })
                })
                .map(|candidate| candidate.key.clone())
                .collect();
        targets.sort();
        targets
    }

    /// 一键应用建议（T51）：把全部尚未保留的高置信候选改为保留并请求确认；
    /// 不剔除任何候选。生成建议本身不改变任何 `ReviewState`（验收 5）。
    pub fn apply_suggestions(&mut self) -> Result<CommandOutcome> {
        self.ensure_not_sealed()?;
        let keep = self.high_confidence_keep_targets();
        if keep.is_empty() {
            return Ok(CommandOutcome::Applied { changed: 0 });
        }
        let request = apply_request(&keep, &[], &self.candidates);
        self.pending_suggestion_apply = Some(SuggestionApplyPlan {
            keep,
            remove: Vec::new(),
            request: request.clone(),
        });
        Ok(CommandOutcome::NeedsSuggestionConfirmation(request))
    }

    /// 确认弹窗点了"确认"：复用既有批量状态变更机制执行保留批，
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
    /// 多目标批量剔除被拦下并返回 [`CommandOutcome::NeedsConfirmation`]
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

    /// 便捷入口：把当前勾选的全部候选改为目标三态。
    ///
    /// T51：批量改保留/待定直接执行；批量剔除无数量门槛，一律先弹一次
    /// 二次确认。
    pub fn submit_for_selected(&mut self, to: ReviewState) -> Result<CommandOutcome> {
        let targets: Vec<CandidateKey> = self
            .candidates
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.key.clone())
            .collect();
        if targets.is_empty() {
            return Ok(CommandOutcome::Applied { changed: 0 });
        }
        if to.is_remove() {
            let request = ConfirmationRequest::batch_remove(targets.len());
            self.pending_confirmation = Some(StateChange::batch(targets, to));
            return Ok(CommandOutcome::NeedsConfirmation(request));
        }
        let changed = self.apply(&StateChange::batch(targets, to));
        Ok(CommandOutcome::Applied { changed })
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

    // ── 勾选与批量行（ADR-0016，T51 修订）─────────────────

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

    /// 把给定候选的勾选状态统一设为 `selected`（页面级全选由呈现层按当前页
    /// 切片调用；对不存在的候选标识安静跳过），返回实际改变的候选数。
    pub fn set_selected(&mut self, keys: &[CandidateKey], selected: bool) -> usize {
        let mut changed = 0;
        for key in keys {
            if let Some(candidate) = self.candidates.iter_mut().find(|c| &c.key == key) {
                if candidate.selected != selected {
                    candidate.selected = selected;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// 当前勾选数（跨类别）
    pub fn selected_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.selected).count()
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

        // 卡片列表 = 当前类别 ∩ 当前置信度筛选；按 高→中→低 排序
        // （同档按稳定候选 ID，保证确定性；T51）。
        let filter = self.confidence_filter;
        let mut visible_cards: Vec<&Candidate> = self
            .candidates
            .iter()
            .filter(|c| {
                c.category == self.active_category
                    && c.suggestion
                        .as_ref()
                        .is_some_and(|suggestion| filter.matches(suggestion))
            })
            .collect();
        visible_cards.sort_by(confidence_order);
        let cards: Vec<CandidateCardView> = visible_cards
            .into_iter()
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

        // 地图对象按 高→中→低 排序：JS 按接收顺序标注，高置信候选优先上屏
        // （T51：地图标注/加载与列表同序）。
        let mut ordered_objects: Vec<&Candidate> = self.candidates.iter().collect();
        ordered_objects.sort_by(confidence_order);
        let map_objects = ordered_objects
            .into_iter()
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
            pending_confirmation: self
                .pending_confirmation
                .as_ref()
                .map(|change| ConfirmationRequest::batch_remove(change.targets.len())),
            confidence_filters_label_key: text_keys::CONFIDENCE_FILTERS_LABEL,
            confidence_filters: ConfidenceFilter::ALL
                .into_iter()
                .map(|filter| ConfidenceFilterView {
                    filter,
                    label_key: filter.label_key(),
                    count: self.confidence_filter_count(filter),
                    active: self.confidence_filter == filter,
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
