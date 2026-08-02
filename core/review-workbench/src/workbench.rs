//! 评审台核心：一次性读入、状态变更操作、批量确认闸、暂停/恢复、封账写回
//!
//! 缝 4 契约的功能层一侧（无卡顿铁律）：
//! - [`ReviewWorkbench::load`] 进台时向 B2 一次性读入候选集到内存；
//! - 评审期间所有操作纯内存（本类型不持有数据库句柄，结构上保证零写库）；
//! - [`ReviewWorkbench::seal`] 封账时把最终三态一次性批量写回 B2，
//!   写回失败则封账不生效（评审状态保持可改）。

use data_persistence::{CandidateProjectionsApi, Database, ReviewDecision, ReviewDecisionsApi};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use std::path::Path;

use crate::candidate::{Candidate, CandidateKey};
use crate::command::{CommandOutcome, ConfirmationRequest, StateChange};
use crate::error::{Error, Result};
use crate::session::{SessionEntry, SessionSnapshot};
use crate::view_models::{
    category_text_key, state_text_key, text_keys, CandidateCardView, CategoryTabView,
    ExportSummary, InfoPanelView, MapObjectView, WorkbenchView,
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
    candidates: Vec<Candidate>,
    active_category: CandidateCategory,
    highlighted: Option<CandidateKey>,
    /// 等待二次确认的批量剔除操作（弹窗期间暂存，确认后执行）
    pending_confirmation: Option<StateChange>,
    sealed: bool,
}

impl ReviewWorkbench {
    // ── 缝 4：一次性读入 ─────────────────────────────────

    /// 进入评审台：向 B2 一次性读入候选集到内存。
    ///
    /// 候选只来自当前已发布且可评审的 B2 投影，初始态一律"待定"（ADR-0022）；
    /// 若评审终态表已有本方案的记录（上一轮封账结果），按候选标识对回。
    pub fn load(db: &Database, plan_id: &PlanId) -> Result<Self> {
        let plan_key = plan_id.to_string();
        let projections = db.list_reviewable_candidate_projections(&plan_key)?;
        let mut candidates: Vec<Candidate> =
            projections.iter().map(Candidate::from_projection).collect();

        // 上一轮封账写回的终态对回内存（没有记录的保持"待定"）
        for decision in db.list_review_decisions(&plan_key)? {
            let key = CandidateKey::new(decision.category, decision.candidate_id.clone());
            if let Some(candidate) = candidates.iter_mut().find(|c| c.key == key) {
                candidate.state = decision.review_state;
            }
        }

        let active_category = CATEGORY_ORDER
            .into_iter()
            .find(|category| candidates.iter().any(|c| c.key.category == *category))
            .unwrap_or(CandidateCategory::Building);

        Ok(Self {
            plan_id: plan_key,
            candidates,
            active_category,
            highlighted: None,
            pending_confirmation: None,
            sealed: false,
        })
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
            if candidate.key.category == category {
                candidate.selected = true;
            }
        }
    }

    /// 点“取消全选”：当前激活类别的所有卡片取消勾选。
    pub fn deselect_all_in_active_category(&mut self) {
        let category = self.active_category;
        for candidate in &mut self.candidates {
            if candidate.key.category == category {
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
        SessionSnapshot::new(self.plan_id.clone(), self.active_category, entries).save_to_file(path)
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
            .any(|candidate| candidate.key.category == snapshot.active_category)
        {
            self.active_category = snapshot.active_category;
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
                .filter(|c| c.key.category == category && c.state.is_keep())
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
                    c.key.category,
                    c.key.candidate_id.clone(),
                    c.state,
                )
            })
            .collect();
        db.batch_update_review_decisions(&decisions)?;
        self.sealed = true;
        self.pending_confirmation = None;
        Ok(self.export_summary())
    }

    /// 是否已封账（评审入口禁用信号）
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    // ── 三栏布局视图 ─────────────────────────────────────

    /// 产出三栏布局整体视图（左卡片列表 + 中大地图 + 右信息面板）
    pub fn view(&self) -> WorkbenchView {
        let category_tabs = CATEGORY_ORDER
            .into_iter()
            .map(|category| CategoryTabView {
                category,
                label_key: category_text_key(category),
                count: self
                    .candidates
                    .iter()
                    .filter(|c| c.key.category == category)
                    .count(),
                active: category == self.active_category,
            })
            .collect();

        let cards = self
            .candidates
            .iter()
            .filter(|c| c.key.category == self.active_category)
            .map(|c| CandidateCardView {
                candidate_id: c.key.candidate_id.clone(),
                title: c.title.clone(),
                state: c.state,
                state_key: state_text_key(c.state),
                selected: c.selected,
                highlighted: self.highlighted.as_ref() == Some(&c.key),
            })
            .collect();

        let map_objects = self
            .candidates
            .iter()
            .map(|c| MapObjectView {
                candidate_id: c.key.candidate_id.clone(),
                category: c.key.category,
                state: c.state,
                highlighted: self.highlighted.as_ref() == Some(&c.key),
            })
            .collect();

        let info_panel = self.highlighted.as_ref().and_then(|key| {
            self.candidates
                .iter()
                .find(|c| &c.key == key)
                .map(|c| InfoPanelView {
                    title: c.title.clone(),
                    category_label_key: text_keys::INFO_CATEGORY,
                    category_key: category_text_key(c.key.category),
                    tags_label_key: text_keys::INFO_TAGS,
                    tags: c.tags.clone(),
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
            sealed: self.sealed,
        }
    }
}
