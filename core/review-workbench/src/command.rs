//! 状态变更操作（B8 撤销重做接口预留，ADR-0022 验收标准）
//!
//! 评审台上的每一次状态变更都是一个 [`StateChange`] 值——而非散落的直接赋值。
//! 将来补建 B8 命令历史层时，只需在提交入口记录这些值即可回放/撤销，
//! 无须翻修既有代码。
//!
//! 批量确认规则（ADR-0016/0022）：批量**剔除 ≥5 项**需二次确认弹窗；
//! 批量剔除 1-4 项直接执行；批量改保留/待定不需确认（可自愈的无害动作）。

use shared_domain_types::ReviewState;

use crate::candidate::CandidateKey;
use crate::view_models::text_keys;

/// 批量剔除需要二次确认的阈值（≥5 项弹窗，ADR-0016）
pub const BATCH_REMOVE_CONFIRM_THRESHOLD: usize = 5;

/// 一次明确的状态变更操作：把一批候选改为目标三态
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    /// 变更目标（一个或多个候选）
    pub targets: Vec<CandidateKey>,
    /// 目标三态
    pub to: ReviewState,
}

impl StateChange {
    /// 单个候选的状态变更
    pub fn single(target: CandidateKey, to: ReviewState) -> Self {
        Self {
            targets: vec![target],
            to,
        }
    }

    /// 一批候选的状态变更
    pub fn batch(targets: Vec<CandidateKey>, to: ReviewState) -> Self {
        Self { targets, to }
    }

    /// 是否需要二次确认：只有批量剔除 ≥5 项需要（ADR-0022 第二节）
    pub fn needs_confirmation(&self) -> bool {
        self.to.is_remove() && self.targets.len() >= BATCH_REMOVE_CONFIRM_THRESHOLD
    }
}

/// 提交状态变更操作的结果
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandOutcome {
    /// 已执行：`changed` 为实际改变了状态的候选数（原状态相同的不计）
    Applied {
        /// 实际改变状态的候选数
        changed: usize,
    },
    /// 危险批量操作被拦下，等待二次确认弹窗的结果
    NeedsConfirmation(ConfirmationRequest),
}

/// 二次确认弹窗请求（UI 层按文本键渲染，ADR-0021 弹窗铁律）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfirmationRequest {
    /// 弹窗标题文本键（`review.batch_reject_confirm_title`）
    pub title_key: &'static str,
    /// 弹窗正文文本键（`review.batch_reject_confirm_body`，含 `{count}` 占位符）
    pub body_key: &'static str,
    /// 待剔除的候选数（正文占位符插值用）
    pub count: usize,
    /// 确认按钮文本键
    pub confirm_key: &'static str,
    /// 取消按钮文本键
    pub cancel_key: &'static str,
}

impl ConfirmationRequest {
    /// 为批量剔除操作构建确认弹窗请求
    pub(crate) fn batch_remove(count: usize) -> Self {
        Self {
            title_key: text_keys::BATCH_REJECT_CONFIRM_TITLE,
            body_key: text_keys::BATCH_REJECT_CONFIRM_BODY,
            count,
            confirm_key: text_keys::CONFIRM_BUTTON,
            cancel_key: text_keys::CANCEL_BUTTON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_domain_types::CandidateCategory;

    fn keys(count: usize) -> Vec<CandidateKey> {
        (0..count)
            .map(|index| CandidateKey::new(CandidateCategory::Building, format!("way/{index}")))
            .collect()
    }

    #[test]
    fn batch_remove_of_five_needs_confirmation() {
        assert!(StateChange::batch(keys(5), ReviewState::Remove).needs_confirmation());
        assert!(StateChange::batch(keys(9), ReviewState::Remove).needs_confirmation());
    }

    #[test]
    fn batch_remove_below_threshold_is_direct() {
        assert!(!StateChange::batch(keys(4), ReviewState::Remove).needs_confirmation());
        assert!(!StateChange::single(keys(1).remove(0), ReviewState::Remove).needs_confirmation());
    }

    #[test]
    fn batch_keep_or_pending_never_needs_confirmation() {
        assert!(!StateChange::batch(keys(50), ReviewState::Keep).needs_confirmation());
        assert!(!StateChange::batch(keys(50), ReviewState::Pending).needs_confirmation());
    }
}
