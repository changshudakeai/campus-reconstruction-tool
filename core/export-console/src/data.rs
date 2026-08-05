//! 导出数据模型（缝 5 的输入 / 导出结果的账本 / 阶段枚举）
//!
//! - [`ExportRequest`]：F5 点"导出地基"后向 F9 递交的导出请求
//!   （保留项集合 + 类别汇总 + 待定计数，窗口契约缝 5）；
//! - [`ExportSummary`]：导出成功后跳转目标携带的汇总；
//! - [`ExportStage`]：导出阶段状态机（进度条文本键来源）。

use shared_domain_types::CandidateCategory;

/// 缝 5 导出请求：F5 向 F9 递交的账本。
///
/// 由壳把 F5 `ReviewWorkbench::export_summary()` 与保留项标识列表
/// 组装而成（F* 功能模块横向零依赖，本 crate 不 import F5 类型）。
///
/// 保留项为零仍是合法请求——最小路径（只圈已确认边界）导出一块
/// 平整空地，一切校验在弹窗内如实呈现，不做静默拦截（缝 5 契约）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    /// 所属方案 ID（字符串形态，与 F5 `plan_id()` 一致）
    pub plan_id: String,
    /// 各类别保留数（类别汇总表格用；仅列出保留数 > 0 的类别）
    pub keep_by_category: Vec<(CandidateCategory, usize)>,
    /// 保留总数（唯一被导出的状态，ADR-0022）
    pub keep_total: usize,
    /// 待定项数（"尚有 N 项待定，它们不会被导出"如实报数，ADR-0022）
    pub pending_count: usize,
    /// 剔除项数
    pub remove_count: usize,
    /// 保留项标识列表（"类别/实体 ID"形态，供后续生成规则取数）
    pub keep_candidates: Vec<String>,
}

impl ExportRequest {
    /// 组装一个导出请求（字段一一对应，无隐藏加工）
    pub fn new(
        plan_id: String,
        keep_by_category: Vec<(CandidateCategory, usize)>,
        keep_total: usize,
        pending_count: usize,
        remove_count: usize,
        keep_candidates: Vec<String>,
    ) -> Self {
        Self {
            plan_id,
            keep_by_category,
            keep_total,
            pending_count,
            remove_count,
            keep_candidates,
        }
    }
}

/// 导出成功后的汇总（跳转到导出完成页时携带的数据）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportSummary {
    /// 方案 ID
    pub plan_id: String,
    /// 实际导出的候选数（= 请求的保留总数）
    pub export_count: usize,
    /// 各类别导出数
    pub by_category: Vec<(CandidateCategory, usize)>,
    /// 落成的 .schem 文件路径
    pub output_path: String,
}

/// 导出阶段（进度条状态机；索引与 [`ProgressTracker`](crate::ProgressTracker) 互转）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportStage {
    /// 等待确认（确认弹窗未点确认）
    Waiting,
    /// 封账中（F5 批量写回）
    Sealing,
    /// 生成中（B18 产出方块模型）
    Generating,
    /// 落盘中（B4 写 .schem）
    Writing,
    /// 完成
    Done,
    /// 失败（封账已回滚）
    Failed,
}

impl ExportStage {
    /// 阶段 → 进度条文案文本键（zh-CN.json 既有键，禁改 B6 地盘）
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Waiting => "export.confirm_title",
            Self::Sealing | Self::Generating | Self::Writing => "export.in_progress",
            Self::Done => "export.done",
            Self::Failed => "error.export_failed",
        }
    }

    /// 阶段 → 稳定索引（跨线程原子存储用）
    pub fn index(self) -> u32 {
        match self {
            Self::Waiting => 0,
            Self::Sealing => 1,
            Self::Generating => 2,
            Self::Writing => 3,
            Self::Done => 4,
            Self::Failed => 5,
        }
    }

    /// 索引 → 阶段（未知索引兜底回"等待确认"）
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Sealing,
            2 => Self::Generating,
            3 => Self::Writing,
            4 => Self::Done,
            5 => Self::Failed,
            _ => Self::Waiting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_index_roundtrips() {
        for stage in [
            ExportStage::Waiting,
            ExportStage::Sealing,
            ExportStage::Generating,
            ExportStage::Writing,
            ExportStage::Done,
            ExportStage::Failed,
        ] {
            assert_eq!(ExportStage::from_index(stage.index()), stage);
        }
        // 未知索引兜底
        assert_eq!(ExportStage::from_index(99), ExportStage::Waiting);
    }

    #[test]
    fn zero_keep_request_is_still_constructible() {
        // 最小路径合法：保留项为零不拦截（缝 5 契约）
        let request = ExportRequest::new("plan-1".into(), vec![], 0, 3, 2, vec![]);
        assert_eq!(request.keep_total, 0);
        assert_eq!(request.pending_count, 3);
    }
}
