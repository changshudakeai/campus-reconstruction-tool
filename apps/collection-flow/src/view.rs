//! A1 返回给呈现层的已决定页面状态、进度与通知事实。
//!
//! S1 只绘制这些视图、解析 B6 文本键并把通知事实交给 B7；A1 负责组合
//! F4/B2/B14/F7 的结构化结果并决定本次操作的影响范围（ADR-0039）。

use data_acquisition::CollectionProgressView;
use data_acquisition::CollectionStage;
use notification_center::Notification;

/// 采集页面阶段（A1 已决定；S1 据此绘制进度与状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionStatus {
    /// 尚未采集（或上次结果已被取消/过期）。
    Pending,
    /// 后台采集进行中（窗口不得冻结）。
    Fetching,
    /// 本次采集失败（只暂停本次候选采集，不取消基础导出资格）。
    Failed,
    /// 采集完成：原始观测已保存、候选投影完整发布、报告完成。
    Completed,
}

/// 失败时的用户可见影响（A1 汇总；B7 只消费解析后的成品文字）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionFailureView {
    /// 通知正文（失败类别已按 zh-CN.json 本地化）。
    pub message: String,
    /// 结构化诊断详情（开发者排查；不冒充成功）。
    pub diagnostic: String,
}

/// 当前方案已决定的采集页面状态。
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionPageView {
    /// 已决定的阶段。
    pub status: CollectionStatus,
    /// 进度视图（标题/状态文本键 + 各类别计数）。
    pub progress: CollectionProgressView,
    /// 增量差异摘要（F4 事实，已本地化；未采集时为 None）。
    pub diff_summary: Option<String>,
    /// 常驻采集报告（查看采集报告入口内容；未完成时为 None）。
    pub report: Option<CollectionReportView>,
    /// 评审入口是否解锁（原始观测保存 + 候选投影完整发布 + 报告完成）。
    pub review_unlocked: bool,
    /// 本次失败的用户可见影响（成功/进行中为 None）。
    pub failure: Option<CollectionFailureView>,
}

/// 常驻"采集报告"视图（历史疑点仍可查看，只是不再主动打扰）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionReportView {
    /// 报告标题（B6）。
    pub title: String,
    /// 入口链接文字（B6）。
    pub entry_label: String,
    /// 六类别汇总行（数量或"未采集（跳过）"）。
    pub category_lines: Vec<String>,
    /// 候选投影汇总行（可评审/已隔离/自动修复，ADR-0040 事实）。
    pub candidate_lines: Vec<String>,
    /// 全部疑点问句（含已关闭的）。
    pub issue_lines: Vec<String>,
    /// 无疑点时的说明行（有疑点时为 None）。
    pub no_issues_line: Option<String>,
}

/// 一次采集成功后的完整事实（页面 + 报告 + 通知 + 评审解锁）。
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionSummary {
    /// 已决定的完成页面状态。
    pub page: CollectionPageView,
    /// 交给 B7 的完成通知事实（成功通常为 None；疑点弹窗由 F7 呈现）。
    pub notification: Option<Notification>,
}

/// 一次采集失败后的完整事实（结构化错误 + 页面 + 通知）。
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionFailure {
    /// 已决定的失败页面状态。
    pub page: CollectionPageView,
    /// 交给 B7 的错误通知事实（error 级；用户主动取消/过期时无通知）。
    pub notification: Option<Notification>,
    /// 结构化诊断详情（A1 不吞错；原错误 Display 全文保留）。
    pub diagnostic: String,
}

/// 后台采集操作的终态结果。
#[derive(Debug, Clone, PartialEq)]
pub enum CollectionOutcome {
    /// 采集成功（原始观测已保存、候选投影完整发布、报告完成）。
    Succeeded(CollectionSummary),
    /// 采集失败（结构化失败 → A1 汇总 → 通知能力呈现，无伪成功产物）。
    Failed(CollectionFailure),
}

/// A1 按方案保存的最近一次完整结果（切换方案后恢复呈现，结果按方案隔离）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlanCollectionState {
    pub(crate) state: PlanCollectionStateKind,
}

impl PlanCollectionState {
    /// 评审入口是否解锁（只认成功且未过期的结果）。
    pub(crate) fn review_unlocked(&self) -> bool {
        matches!(
            &self.state,
            PlanCollectionStateKind::Outcome(outcome)
                if matches!(
                    outcome.as_ref(),
                    CollectionOutcome::Succeeded(summary) if summary.page.review_unlocked
                )
        )
    }

    /// 报告视图（仅成功结果提供常驻报告）。
    pub(crate) fn report(&self) -> Option<CollectionReportView> {
        match &self.state {
            PlanCollectionStateKind::Outcome(outcome) => match outcome.as_ref() {
                CollectionOutcome::Succeeded(summary) => summary.page.report.clone(),
                CollectionOutcome::Failed(_) => None,
            },
            PlanCollectionStateKind::Fetching { .. } => None,
        }
    }
}

/// 按方案保存的采集阶段（后台运行中或最近一次完整结果）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlanCollectionStateKind {
    /// 后台采集进行中（T36：记录当前阶段与开始时刻，供页面呈现阶段/已用时长）。
    Fetching {
        /// 当前阶段（拉取数据 / 补名 / 写库）
        stage: CollectionStage,
        /// 开始时刻（UNIX 毫秒）
        started_at_millis: u64,
    },
    /// 最近一次完整结果（成功或失败）。
    Outcome(Box<CollectionOutcome>),
}
