//! 确认弹窗 / 进度条 / 跳转目标的纯数据视图（缝 1：壳只做绑定）
//!
//! 全部是面向 UI 的纯数据：Slint 声明层只做绑定，不含业务逻辑。
//! 文案一律产出 B6 文本键（[`text_keys`]），由 UI 层经 `localization::t()`
//! 解析（ADR-0005，禁止硬编码用户可见文字）。

use shared_domain_types::CandidateCategory;

use crate::data::{ExportRequest, ExportStage};
use crate::progress::ProgressTracker;

/// 本 crate 产出的全部文本键（zh-CN.json 既有键；禁改 B6 地盘，由测试保证可解析）
pub mod text_keys {
    /// "导出地基"入口按钮
    pub const START_BUTTON: &str = "export.start_button";
    /// 确认弹窗标题
    pub const CONFIRM_TITLE: &str = "export.confirm_title";
    /// 类别汇总表格标题
    pub const CONFIRM_SUMMARY: &str = "export.confirm_summary";
    /// 封账后果文字（ADR-0022 第四节第 1 条）
    pub const SEAL_NOTICE: &str = "export.seal_notice";
    /// 待定项报数（含 `{count}` 占位符，ADR-0022 第四节第 2 条）
    pub const PENDING_NOTICE: &str = "export.pending_notice";
    /// 进度条进行中文案
    pub const IN_PROGRESS: &str = "export.in_progress";
    /// 导出完成
    pub const DONE: &str = "export.done";
    /// 导出失败（error 命名空间既有键）
    pub const EXPORT_FAILED: &str = "error.export_failed";
    /// 确认按钮（app 命名空间既有键）
    pub const CONFIRM_BUTTON: &str = "app.confirm_button";
    /// 取消按钮（app 命名空间既有键）
    pub const CANCEL_BUTTON: &str = "app.cancel_button";
}

/// 类别 → 显示名文本键（collection 命名空间既有键，与 F5 同源）
pub(crate) fn category_text_key(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "collection.category_building",
        CandidateCategory::Road => "collection.category_road",
        CandidateCategory::Water => "collection.category_water",
        CandidateCategory::Vegetation => "collection.category_vegetation",
        CandidateCategory::Sports => "collection.category_sports",
        // B1 枚举带 #[non_exhaustive]：未知新类别兜底进"其他"显示
        _ => "collection.category_other",
    }
}

/// 类别汇总表格的一行（"建筑 80"由 UI 层拼装：标签文本 + 计数）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SummaryRowView {
    /// 类别
    pub category: CandidateCategory,
    /// 类别显示名文本键
    pub label_key: &'static str,
    /// 本次保留数
    pub keep_count: usize,
}

/// 导出确认弹窗视图（汇总 + 封账后果 + 待定报数，ADR-0022 第四节）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportConfirmDialogView {
    /// 弹窗标题文本键
    pub title_key: &'static str,
    /// 类别汇总表格标题文本键
    pub summary_label_key: &'static str,
    /// 类别汇总表格（仅保留数 > 0 的类别）
    pub summary_rows: Vec<SummaryRowView>,
    /// 封账后果文本键（"确认后评审就此结束，之前的评审决定不可再改…"）
    pub seal_notice_key: &'static str,
    /// 待定项报数文本键（含 `{count}` 占位符）
    pub pending_notice_key: &'static str,
    /// 待定项数（正文占位符插值用）
    pub pending_count: usize,
    /// 确认按钮文本键
    pub confirm_key: &'static str,
    /// 取消按钮文本键
    pub cancel_key: &'static str,
}

impl ExportConfirmDialogView {
    /// 从缝 5 导出请求产出确认弹窗视图（不拦截、不加工，如实呈现）
    pub fn from_request(request: &ExportRequest) -> Self {
        let summary_rows = request
            .keep_by_category
            .iter()
            .map(|(category, count)| SummaryRowView {
                category: *category,
                label_key: category_text_key(*category),
                keep_count: *count,
            })
            .collect();
        Self {
            title_key: text_keys::CONFIRM_TITLE,
            summary_label_key: text_keys::CONFIRM_SUMMARY,
            summary_rows,
            seal_notice_key: text_keys::SEAL_NOTICE,
            pending_notice_key: text_keys::PENDING_NOTICE,
            pending_count: request.pending_count,
            confirm_key: text_keys::CONFIRM_BUTTON,
            cancel_key: text_keys::CANCEL_BUTTON,
        }
    }
}

/// 右上角浮动进度视图（非阻塞：UI 轮询产出，主界面不冻结）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportProgressView {
    /// 是否显示浮动提示（等待确认阶段不显示）
    pub visible: bool,
    /// 百分比数字（0-100，5% 步进）
    pub percent: u32,
    /// 阶段文案文本键（"正在导出…"/"导出完成"/"导出失败"）
    pub stage_key: &'static str,
    /// 是否完成（UI 据此触发自动跳转）
    pub is_done: bool,
    /// 是否失败（UI 据此收起进度条）
    pub is_failed: bool,
}

impl ExportProgressView {
    /// 从进度追踪器产出当前一帧视图
    pub fn from_tracker(tracker: &ProgressTracker) -> Self {
        let stage = tracker.stage();
        Self {
            visible: stage != ExportStage::Waiting,
            percent: tracker.percent(),
            stage_key: stage.label_key(),
            is_done: stage == ExportStage::Done,
            is_failed: stage == ExportStage::Failed,
        }
    }
}

/// 导出结束后的界面跳转目标（壳的导航层消费）
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NavigationTarget {
    /// 跳转到导出完成页（携带导出汇总）
    ExportCompleted(crate::data::ExportSummary),
    /// 返回方案列表
    PlanList,
    /// 回到评审台继续评审（导出失败回滚后）
    ContinueReview,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_dialog_reports_summary_and_pending_faithfully() {
        let request = ExportRequest::new(
            "plan-1".into(),
            vec![
                (CandidateCategory::Building, 80),
                (CandidateCategory::Road, 75),
            ],
            155,
            7,
            3,
            vec![],
        );
        let view = ExportConfirmDialogView::from_request(&request);
        assert_eq!(view.summary_rows.len(), 2);
        assert_eq!(view.summary_rows[0].label_key, "collection.category_building");
        assert_eq!(view.summary_rows[0].keep_count, 80);
        // 待定如实报数、不拦截（ADR-0022）
        assert_eq!(view.pending_count, 7);
        assert_eq!(view.seal_notice_key, "export.seal_notice");
    }

    #[test]
    fn progress_view_is_hidden_before_confirmation() {
        let tracker = ProgressTracker::new();
        let view = ExportProgressView::from_tracker(&tracker);
        assert!(!view.visible);

        tracker.set_stage(ExportStage::Generating);
        tracker.report_percent(45);
        let view = ExportProgressView::from_tracker(&tracker);
        assert!(view.visible);
        assert_eq!(view.percent, 45);
        assert_eq!(view.stage_key, "export.in_progress");
        assert!(!view.is_done);
    }
}
