//! 安静哨兵编排与采集报告视图（ADR-0019 第一节）
//!
//! - 采集结束后体检瞬时完成（纯内存数数，不拖慢采集）；
//! - **无疑点**：界面无任何打扰；
//! - **有疑点**：一次采集最多弹一次，所有疑点合并一窗（error 级模态弹窗，
//!   禁横幅），点"知道了"关闭后记住裁决；
//! - "采集报告"入口常驻可查（各类别数量汇总 + "未采集（跳过）"标注 +
//!   历史疑点仍可查看，只是不再主动打扰）。
//!
//! 本 crate 零 slint：弹窗经 B7 通知中心分派（同步留底进公告栏），
//! 视图结构体只是纯数据 ViewModel，由壳绑定呈现。

use data_persistence::AppSettingsApi;
use localization::Localization;
use shared_domain_types::{CandidateCategory, PlanId};

use crate::audit::CoverageAudit;
use crate::error::Result;
use crate::model::{AuditResult, ALL_CATEGORIES};
use crate::resolver::DecisionResolver;

/// 合并疑点弹窗的视图数据（全部疑点合一窗，一次采集最多弹一次）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPopupView {
    /// 弹窗标题（成品文字）
    pub title: String,
    /// 弹窗正文：疑点问句逐行合并
    pub body: String,
    /// 确认按钮文字（"知道了"）
    pub ack_label: String,
}

/// 常驻"采集报告"的视图数据（随时可点开看全貌）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReportView {
    /// 报告标题
    pub title: String,
    /// 入口链接文字（F4 采集页底部 / 评审页角落）
    pub entry_label: String,
    /// 各类别汇总行：数量或"未采集（跳过）"
    pub category_lines: Vec<String>,
    /// 全部疑点问句（含已关闭的 —— 报告里仍可查看）
    pub issue_lines: Vec<String>,
    /// 无疑点时的说明行（有疑点时为 None）
    pub no_issues_line: Option<String>,
}

/// 一次体检编排的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditOutcome {
    /// 完整审计结果（供常驻报告使用）
    pub result: AuditResult,
    /// 本次弹出的合并疑点窗（无疑点或全部已裁决时为 None —— 安静通过）
    pub popup: Option<AuditPopupView>,
}

/// 安静哨兵 —— 采集后体检的编排入口
#[derive(Debug, Clone, Default)]
pub struct QuietSentinel {
    audit: CoverageAudit,
}

impl QuietSentinel {
    /// 用默认阈值创建哨兵
    pub fn new() -> Self {
        Self::default()
    }

    /// 用自定义"其他"占比阈值创建哨兵（ADR-0019：实现层可调）
    pub fn with_threshold(percent: u32) -> Self {
        Self {
            audit: CoverageAudit::with_threshold(percent),
        }
    }

    /// 采集结束后的体检编排。
    ///
    /// 1. 旧裁决作废（重新采集 → 数据变化 → 疑点重新评估，ADR-0019 第三节）；
    /// 2. 纯内存体检；
    /// 3. 有疑点 → 经 B7 弹一个合并窗（error 级 = 模态、点掉才能继续，
    ///    同步留底进公告栏）；弹窗返回即用户已点"知道了" → 记住裁决并落库；
    /// 4. 无疑点 → 静默返回，界面无任何打扰。
    pub fn after_collection(
        &self,
        db: &mut impl AppSettingsApi,
        plan_id: &PlanId,
        counts: &[u32; 6],
        skipped_categories: Vec<CandidateCategory>,
        l10n: &Localization,
    ) -> Result<AuditOutcome> {
        let outcome = self.after_collection_facts(db, plan_id, counts, skipped_categories, l10n)?;
        if let Some(popup) = &outcome.popup {
            notification_center::error(
                l10n.t("audit.source_tag"),
                popup.title.clone(),
                popup.body.clone(),
            );
        }
        Ok(outcome)
    }

    /// 采集结束后的体检编排（**事实变体**）：返回审计结果与待呈现的合并
    /// 疑点窗，**不调用 B7 呈现**。
    ///
    /// 供后台执行的 A1 collection-flow 在接口后汇总影响，并把弹窗事实交给
    /// S1 通知能力在 UI 线程呈现（ADR-0039：S1 只呈现 A1 返回的页面状态与
    /// 通知事实）；裁决记忆仍在此处按同一规则落库。
    pub fn after_collection_facts(
        &self,
        db: &mut impl AppSettingsApi,
        plan_id: &PlanId,
        counts: &[u32; 6],
        skipped_categories: Vec<CandidateCategory>,
        l10n: &Localization,
    ) -> Result<AuditOutcome> {
        let result = self.audit.audit(counts, skipped_categories);
        let mut resolver = DecisionResolver::new(); // 重新采集：旧裁决作废
        let popup = self.build_popup(&result, &mut resolver, l10n);
        resolver.save(db, plan_id)?;
        Ok(AuditOutcome { result, popup })
    }

    /// 方案打开时的复检编排：只弹**尚未裁决**的疑点。
    ///
    /// 正常情况下（采集完成时已点"知道了"）这里安静通过 ——
    /// "同一批疑点不再主动出现"；仅当上次弹窗未能落库（如异常退出）时兜底。
    pub fn on_plan_opened(
        &self,
        db: &mut impl AppSettingsApi,
        plan_id: &PlanId,
        counts: &[u32; 6],
        skipped_categories: Vec<CandidateCategory>,
        l10n: &Localization,
    ) -> Result<AuditOutcome> {
        let result = self.audit.audit(counts, skipped_categories);
        let mut resolver = DecisionResolver::load(db, plan_id)?;
        let popup = self.build_popup(&result, &mut resolver, l10n);
        if let Some(popup) = &popup {
            notification_center::error(
                l10n.t("audit.source_tag"),
                popup.title.clone(),
                popup.body.clone(),
            );
        }
        resolver.save(db, plan_id)?;
        Ok(AuditOutcome { result, popup })
    }

    /// 组装常驻"采集报告"视图（历史疑点仍可查看，只是不再主动打扰）
    pub fn report_view(&self, result: &AuditResult, l10n: &Localization) -> AuditReportView {
        let category_lines = ALL_CATEGORIES
            .iter()
            .map(|category| {
                let key = if result.skipped_categories.contains(category) {
                    "audit.skipped_line"
                } else {
                    "audit.category_line"
                };
                l10n.t_with_args(
                    key,
                    serde_json::json!({
                        "category": category.display_name(),
                        "count": result.category_counts[AuditResult::category_index(*category)],
                    }),
                )
            })
            .collect();
        let issue_lines: Vec<String> = result
            .issues
            .iter()
            .map(|issue| issue.message(l10n))
            .collect();
        let no_issues_line = if issue_lines.is_empty() {
            Some(l10n.t("audit.no_issues"))
        } else {
            None
        };
        AuditReportView {
            title: l10n.t("audit.report_title"),
            entry_label: l10n.t("audit.report_entry"),
            category_lines,
            issue_lines,
            no_issues_line,
        }
    }

    /// 疑点合并共通流程：过滤已裁决 → 合并一窗 → 记住裁决。
    ///
    /// 返回待呈现的窗口视图（不含 B7 呈现调用）；无待弹疑点时返回 None
    /// （安静通过）。
    fn build_popup(
        &self,
        result: &AuditResult,
        resolver: &mut DecisionResolver,
        l10n: &Localization,
    ) -> Option<AuditPopupView> {
        let pending = resolver.undecided(&result.issues);
        if pending.is_empty() {
            return None;
        }

        let view = AuditPopupView {
            title: l10n.t("audit.popup_title"),
            body: pending
                .iter()
                .map(|issue| issue.message(l10n))
                .collect::<Vec<_>>()
                .join("\n"),
            ack_label: l10n.t("dialog.ok_button"),
        };

        for issue in &result.issues {
            resolver.record_decision(issue);
        }
        Some(view)
    }
}
