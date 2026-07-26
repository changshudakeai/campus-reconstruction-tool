//! 非阻塞进度追踪（缝 6 契约："全程通过进度回调向 F9 报进度"）
//!
//! [`ProgressTracker`] 是多线程共享的原子状态：后台导出线程写、UI 线程读，
//! 主界面不冻结（ADR-0016 非阻塞进度条）。百分比按 5% 步进对齐，
//! 避免高频刷 UI。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crate::data::ExportStage;

/// 进度条的百分比步进（每 5% 更新一次 UI）
pub(crate) const PERCENT_STEP: u32 = 5;

/// 进度追踪器：克隆共享同一份进度状态（内部全是引用计数原子量）
#[derive(Debug, Clone, Default)]
pub struct ProgressTracker {
    percent: Arc<AtomicU32>,
    stage: Arc<AtomicU32>,
}

impl ProgressTracker {
    /// 创建（初始 0%，阶段"等待确认"）
    pub fn new() -> Self {
        Self::default()
    }

    /// 切换阶段
    pub fn set_stage(&self, stage: ExportStage) {
        self.stage.store(stage.index(), Ordering::SeqCst);
    }

    /// 当前阶段
    pub fn stage(&self) -> ExportStage {
        ExportStage::from_index(self.stage.load(Ordering::SeqCst))
    }

    /// 报告百分比：按 5% 步进向下对齐，超 100 截断。
    ///
    /// 返回 `true` 表示对齐后的值发生了变化（UI 需要重绘）——
    /// 这就是"每 5% 更新一次进度条"的实现。
    pub fn report_percent(&self, percent: u32) -> bool {
        let aligned = (percent.min(100) / PERCENT_STEP) * PERCENT_STEP;
        let previous = self.percent.swap(aligned, Ordering::SeqCst);
        previous != aligned
    }

    /// 当前百分比（已对齐）
    pub fn percent(&self) -> u32 {
        self.percent.load(Ordering::SeqCst)
    }

    /// 完成：100% + 阶段"完成"
    pub fn finish(&self) {
        self.percent.store(100, Ordering::SeqCst);
        self.set_stage(ExportStage::Done);
    }

    /// 失败：保留当前百分比，阶段切"失败"
    pub fn fail(&self) {
        self.set_stage(ExportStage::Failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_aligns_to_five_percent_steps() {
        let tracker = ProgressTracker::new();
        // 3% 对齐到 0%，与初始值相同 → 无变化
        assert!(!tracker.report_percent(3));
        assert_eq!(tracker.percent(), 0);
        assert!(tracker.report_percent(7));
        assert_eq!(tracker.percent(), 5);
        // 同一步进内不算变化（7% 与 9% 都对齐到 5%）
        assert!(!tracker.report_percent(9));
        assert_eq!(tracker.percent(), 5);
        // 超过 100 截断
        tracker.report_percent(250);
        assert_eq!(tracker.percent(), 100);
    }

    #[test]
    fn clones_share_progress_state() {
        let tracker = ProgressTracker::new();
        let alias = tracker.clone();
        alias.report_percent(50);
        alias.set_stage(ExportStage::Writing);
        assert_eq!(tracker.percent(), 50);
        assert_eq!(tracker.stage(), ExportStage::Writing);
    }

    #[test]
    fn finish_and_fail_set_terminal_stages() {
        let tracker = ProgressTracker::new();
        tracker.report_percent(80);
        tracker.finish();
        assert_eq!(tracker.percent(), 100);
        assert_eq!(tracker.stage(), ExportStage::Done);

        let failing = ProgressTracker::new();
        failing.report_percent(45);
        failing.fail();
        // 失败保留进度（UI 显示卡在失败前的位置）
        assert_eq!(failing.percent(), 45);
        assert_eq!(failing.stage(), ExportStage::Failed);
    }

    #[test]
    fn first_report_of_zero_counts_as_no_change() {
        let tracker = ProgressTracker::new();
        // 初始就是 0，对齐后仍是 0 → 无变化
        assert!(!tracker.report_percent(0));
    }
}
