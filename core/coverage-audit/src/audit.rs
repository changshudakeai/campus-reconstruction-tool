//! 疑点检测核心逻辑（ADR-0019 第二节：仅两条规则，纯统计）
//!
//! 体检唯一的依据是用户自己采回来的数据（纯内存数数，不联网、不写盘），
//! 系统不假装知道校园里"应该"有什么——没有标配清单，没有高德对照。

use shared_domain_types::CandidateCategory;

use crate::model::{AuditIssue, AuditResult, IssueRule, ALL_CATEGORIES};

/// "其他"占比疑点的默认阈值（百分比）。
///
/// ADR-0019 定为 20%；实现层可经 [`CoverageAudit::with_threshold`] 调整，
/// 不惊动用户。
pub const DEFAULT_OTHER_THRESHOLD: u32 = 20;

/// 覆盖率审计器 —— 采集后默默体检
#[derive(Debug, Clone)]
pub struct CoverageAudit {
    /// "其他"占比阈值（百分比，严格大于才触发）
    other_threshold_percent: u32,
}

impl CoverageAudit {
    /// 用默认阈值（20%）创建审计器
    pub fn new() -> Self {
        Self::with_threshold(DEFAULT_OTHER_THRESHOLD)
    }

    /// 用自定义"其他"占比阈值创建审计器（ADR-0019：实现层可调）
    pub fn with_threshold(percent: u32) -> Self {
        Self {
            other_threshold_percent: percent,
        }
    }

    /// 当前生效的"其他"占比阈值（百分比）
    pub fn threshold(&self) -> u32 {
        self.other_threshold_percent
    }

    /// 对一次采集的账目做覆盖体检。
    ///
    /// - `counts`：各类别采集数量，下标顺序同 [`ALL_CATEGORIES`]；
    /// - `skipped_categories`：用户主动跳过的类别（不触发规则①，
    ///   报告标注"未采集（跳过）"）。
    pub fn audit(
        &self,
        counts: &[u32; 6],
        skipped_categories: Vec<CandidateCategory>,
    ) -> AuditResult {
        let total_count: u32 = counts.iter().sum();
        let mut issues = Vec::new();

        // 规则①：空类别 —— 采集过但结果为 0 项（跳过的类别不算疑点）
        for category in ALL_CATEGORIES {
            if skipped_categories.contains(&category) {
                continue;
            }
            if counts[AuditResult::category_index(category)] == 0 {
                issues.push(AuditIssue {
                    rule: IssueRule::EmptyCategory,
                    message_key: "audit.empty_category",
                    category: Some(category),
                    other_count: None,
                    other_percentage: None,
                });
            }
        }

        // 规则②："其他"占比超阈值 —— 整数交叉相乘精确比较，避免浮点边界误差
        let other_count = counts[AuditResult::category_index(CandidateCategory::Other)];
        if total_count > 0
            && u64::from(other_count) * 100
                > u64::from(self.other_threshold_percent) * u64::from(total_count)
        {
            // 展示用整数百分比（四舍五入）
            let percent = ((u64::from(other_count) * 200 + u64::from(total_count))
                / (2 * u64::from(total_count))) as u32;
            issues.push(AuditIssue {
                rule: IssueRule::HighOtherRatio,
                message_key: "audit.high_other_ratio",
                category: None,
                other_count: Some(other_count),
                other_percentage: Some(percent),
            });
        }

        AuditResult {
            issues,
            category_counts: *counts,
            total_count,
            skipped_categories,
        }
    }
}

impl Default for CoverageAudit {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue_rules(result: &AuditResult) -> Vec<IssueRule> {
        result.issues.iter().map(|issue| issue.rule).collect()
    }

    #[test]
    fn empty_collected_category_raises_issue() {
        let audit = CoverageAudit::new();
        // 水域采集过但 0 项，其余五类都有数据
        let result = audit.audit(&[10, 5, 0, 3, 2, 1], Vec::new());
        let empty: Vec<_> = result
            .issues
            .iter()
            .filter(|issue| issue.rule == IssueRule::EmptyCategory)
            .collect();
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].category, Some(CandidateCategory::Water));
    }

    #[test]
    fn skipped_category_is_not_an_issue() {
        let audit = CoverageAudit::new();
        // 水域被主动跳过（ADR-0019：最短路径用户不被空类别疑点轰炸）
        let result = audit.audit(&[10, 5, 0, 3, 2, 1], vec![CandidateCategory::Water]);
        assert!(!issue_rules(&result).contains(&IssueRule::EmptyCategory));
        assert_eq!(result.skipped_categories, vec![CandidateCategory::Water]);
    }

    #[test]
    fn high_other_ratio_raises_issue_with_rounded_percent() {
        let audit = CoverageAudit::new();
        // 35/120 = 29.17% > 20% → 触发，展示 29%
        let result = audit.audit(&[50, 20, 5, 5, 5, 35], Vec::new());
        let other_issue = result
            .issues
            .iter()
            .find(|issue| issue.rule == IssueRule::HighOtherRatio)
            .expect("应触发'其他'占比疑点");
        assert_eq!(other_issue.other_count, Some(35));
        assert_eq!(other_issue.other_percentage, Some(29));
    }

    #[test]
    fn other_ratio_at_threshold_does_not_trigger() {
        let audit = CoverageAudit::new();
        // 恰好 20%（20/100）不触发：规则是"超过阈值"
        let result = audit.audit(&[50, 20, 4, 3, 3, 20], Vec::new());
        assert!(!issue_rules(&result).contains(&IssueRule::HighOtherRatio));
    }

    #[test]
    fn custom_threshold_is_respected() {
        let audit = CoverageAudit::with_threshold(40);
        assert_eq!(audit.threshold(), 40);
        // 30% 的"其他"在 40% 阈值下不触发
        let result = audit.audit(&[40, 10, 10, 5, 5, 30], Vec::new());
        assert!(!issue_rules(&result).contains(&IssueRule::HighOtherRatio));
    }

    #[test]
    fn all_skipped_and_zero_total_is_silent() {
        let audit = CoverageAudit::new();
        let result = audit.audit(&[0; 6], ALL_CATEGORIES.to_vec());
        assert!(!result.has_issues());
        assert_eq!(result.total_count, 0);
    }
}
