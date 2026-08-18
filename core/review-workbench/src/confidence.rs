//! 置信度分档与筛选芯片（T51）。
//!
//! 置信度不是数值评分，而是对既有建议规则的确定性派生分档；本模块只承载
//! 分档枚举与"全部/高/中/低"筛选芯片的匹配/文案键。

use crate::suggestion::CandidateSuggestion;
use crate::view_models::text_keys;

/// 置信度分档：由现有建议规则确定性映射，不引入数值评分（T51）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfidenceTier {
    /// 高置信：名称清晰、形状完整且无异常（建议保留）。
    High,
    /// 中置信：存在不确定信号，需人工确认。
    Medium,
    /// 低置信：未命名、重复投影/嫌疑、重叠、修复过等需关注。
    Low,
}

/// 置信度筛选芯片（T51：全部/高/中/低，单选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfidenceFilter {
    /// 全部：不按置信度过滤。
    All,
    /// 只显示高置信候选。
    High,
    /// 只显示中置信候选。
    Medium,
    /// 只显示低置信候选。
    Low,
}

impl ConfidenceFilter {
    /// 全部筛选芯片（固定顺序，UI 芯片行与筛选索引以此为序）。
    pub const ALL: [ConfidenceFilter; 4] = [Self::All, Self::High, Self::Medium, Self::Low];

    /// 该候选是否命中此筛选器。
    pub fn matches(&self, suggestion: &CandidateSuggestion) -> bool {
        match self {
            Self::All => true,
            Self::High => suggestion.confidence_tier() == ConfidenceTier::High,
            Self::Medium => suggestion.confidence_tier() == ConfidenceTier::Medium,
            Self::Low => suggestion.confidence_tier() == ConfidenceTier::Low,
        }
    }

    /// 筛选器显示名文本键。
    pub fn label_key(&self) -> &'static str {
        match self {
            Self::All => text_keys::FILTER_ALL,
            Self::High => text_keys::FILTER_HIGH,
            Self::Medium => text_keys::FILTER_MEDIUM,
            Self::Low => text_keys::FILTER_LOW,
        }
    }
}
