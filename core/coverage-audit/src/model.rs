//! 疑点数据结构与审计结果（ADR-0019 第二节）
//!
//! 报告是"问题清单"而非"判决书"：措辞一律问句，裁判永远是用户。
//! 文案走文本键（`audit.*`），由 B6 解析为成品文字后递给 B7 呈现。

use localization::Localization;
use shared_domain_types::CandidateCategory;

/// 六类别全集，顺序与 [`AuditResult::category_counts`] 下标一致。
///
/// B1 的 `CandidateCategory` 是 `#[non_exhaustive]` 枚举且不提供迭代器，
/// 本 crate 以此常量遍历；上游新增类别时在此扩容。
pub const ALL_CATEGORIES: [CandidateCategory; 6] = [
    CandidateCategory::Building,
    CandidateCategory::Road,
    CandidateCategory::Water,
    CandidateCategory::Vegetation,
    CandidateCategory::Sports,
    CandidateCategory::Other,
];

/// 疑点规则（ADR-0019：仅两条，纯统计，无"标准答案"）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueRule {
    /// ① 空类别：某类采集过但结果为 0 项（跳过的类别不触发）
    EmptyCategory,
    /// ② "其他"过多："其他"占采集总数比例超过阈值
    HighOtherRatio,
}

impl IssueRule {
    /// 规则标识符（裁决记忆存储用）
    pub fn id(&self) -> &'static str {
        match self {
            Self::EmptyCategory => "empty_category",
            Self::HighOtherRatio => "high_other_ratio",
        }
    }
}

/// 单个疑点项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIssue {
    /// 触发的规则
    pub rule: IssueRule,
    /// 疑点问句的文本键（`audit.*`，由 B6 解析）
    pub message_key: &'static str,
    /// 相关类别（仅规则①有值）
    pub category: Option<CandidateCategory>,
    /// 进入"其他"的对象数（仅规则②有值）
    pub other_count: Option<u32>,
    /// "其他"占比整数百分比，如 29 表示 29%（仅规则②有值）
    pub other_percentage: Option<u32>,
}

impl AuditIssue {
    /// 疑点稳定 ID —— 裁决记忆的键。
    ///
    /// 同一方案同一批数据体检出的同一疑点 ID 不变（占比数字不参与，
    /// 避免同一"其他过多"疑点因四舍五入差异被视为新疑点）。
    pub fn stable_id(&self) -> String {
        match self.rule {
            IssueRule::EmptyCategory => format!(
                "{}:{}",
                self.rule.id(),
                self.category.map_or("unknown", category_slug)
            ),
            IssueRule::HighOtherRatio => self.rule.id().to_owned(),
        }
    }

    /// 解析为成品问句（B7 只收成品文字，窗口契约缝 1）
    pub fn message(&self, l10n: &Localization) -> String {
        let mut args = serde_json::Map::new();
        if let Some(category) = self.category {
            args.insert(
                "category".to_owned(),
                serde_json::Value::String(category.display_name().to_owned()),
            );
        }
        if let Some(count) = self.other_count {
            args.insert("count".to_owned(), serde_json::Value::from(count));
        }
        if let Some(percent) = self.other_percentage {
            args.insert("percent".to_owned(), serde_json::Value::from(percent));
        }
        l10n.t_with_args(self.message_key, serde_json::Value::Object(args))
    }
}

/// 审计结果：一次体检的完整账目（疑点 + 各类别数量汇总）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditResult {
    /// 疑点列表（可能为空 —— 无疑点时界面无任何打扰）
    pub issues: Vec<AuditIssue>,
    /// 各类别采集数量，下标顺序同 [`ALL_CATEGORIES`]
    pub category_counts: [u32; 6],
    /// 采集总数
    pub total_count: u32,
    /// 跳过的类别（用户主动未采集，报告标注"未采集（跳过）"）
    pub skipped_categories: Vec<CandidateCategory>,
}

impl AuditResult {
    /// 是否有疑点
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// 类别在 [`category_counts`](Self::category_counts) 中的下标。
    ///
    /// 上游新增而本 crate 尚未扩容的类别暂归"其他"桶（最后一位）。
    pub fn category_index(category: CandidateCategory) -> usize {
        ALL_CATEGORIES
            .iter()
            .position(|c| *c == category)
            .unwrap_or(ALL_CATEGORIES.len() - 1)
    }
}

/// 类别的英文标识（裁决记忆键用，与界面文案无关）
fn category_slug(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "building",
        CandidateCategory::Road => "road",
        CandidateCategory::Water => "water",
        CandidateCategory::Vegetation => "vegetation",
        CandidateCategory::Sports => "sports",
        CandidateCategory::Other => "other",
        // B1 枚举是 #[non_exhaustive]：上游新增类别时先落此兜底
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_distinguishes_categories_but_not_percentages() {
        let water_empty = AuditIssue {
            rule: IssueRule::EmptyCategory,
            message_key: "audit.empty_category",
            category: Some(CandidateCategory::Water),
            other_count: None,
            other_percentage: None,
        };
        let sports_empty = AuditIssue {
            category: Some(CandidateCategory::Sports),
            ..water_empty.clone()
        };
        assert_eq!(water_empty.stable_id(), "empty_category:water");
        assert_eq!(sports_empty.stable_id(), "empty_category:sports");

        let other_29 = AuditIssue {
            rule: IssueRule::HighOtherRatio,
            message_key: "audit.high_other_ratio",
            category: None,
            other_count: Some(35),
            other_percentage: Some(29),
        };
        let other_31 = AuditIssue {
            other_percentage: Some(31),
            ..other_29.clone()
        };
        // 占比数字不参与 ID：同一疑点不因数值波动被当成新疑点
        assert_eq!(other_29.stable_id(), other_31.stable_id());
    }

    #[test]
    fn category_index_matches_all_categories_order() {
        for (index, category) in ALL_CATEGORIES.iter().enumerate() {
            assert_eq!(AuditResult::category_index(*category), index);
        }
    }
}
