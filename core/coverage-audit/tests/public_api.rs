//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use coverage_audit::{
    AuditIssue, AuditOutcome, AuditPopupView, AuditReportView, AuditResult, CoverageAudit,
    DecisionResolver, Error, IssueRule, QuietSentinel, ALL_CATEGORIES, DEFAULT_OTHER_THRESHOLD,
};
use data_persistence::Database;
use localization::{Language, Localization};
use shared_domain_types::{CandidateCategory, PlanId};

#[test]
fn public_api_types_exist() {
    // 常量：默认阈值 20%（ADR-0019），六类别全集
    assert_eq!(DEFAULT_OTHER_THRESHOLD, 20);
    assert_eq!(ALL_CATEGORIES.len(), 6);

    // IssueRule：两条规则，各有稳定标识符
    assert_eq!(IssueRule::EmptyCategory.id(), "empty_category");
    assert_eq!(IssueRule::HighOtherRatio.id(), "high_other_ratio");

    // CoverageAudit：默认 / 自定义阈值 + 纯统计体检
    let audit = CoverageAudit::new();
    assert_eq!(audit.threshold(), DEFAULT_OTHER_THRESHOLD);
    assert_eq!(CoverageAudit::with_threshold(40).threshold(), 40);
    let result: AuditResult = audit.audit(&[10, 5, 0, 3, 2, 1], Vec::new());
    assert!(result.has_issues());
    assert_eq!(result.total_count, 21);
    assert_eq!(
        AuditResult::category_index(CandidateCategory::Other),
        ALL_CATEGORIES.len() - 1
    );

    // AuditIssue：稳定 ID + 成品问句渲染
    let issue: &AuditIssue = &result.issues[0];
    assert_eq!(issue.stable_id(), "empty_category:water");
    let l10n = Localization::new(Language::ZhCn).expect("内嵌 zh-CN 资源必定可用");
    assert!(issue.message(&l10n).contains("水域"));

    // DecisionResolver：记忆 → 过滤 → 落库往返
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let mut resolver = DecisionResolver::new();
    assert!(!resolver.is_decided(issue));
    resolver.record_decision(issue);
    assert!(resolver.undecided(&result.issues).is_empty());
    resolver.save(&mut db, &plan_id).unwrap();
    assert!(DecisionResolver::load(&db, &plan_id)
        .unwrap()
        .is_decided(issue));
    resolver.reset();
    assert!(!resolver.is_decided(issue));

    // QuietSentinel：采集后编排 + 常驻报告视图
    let sentinel = QuietSentinel::new();
    let outcome: AuditOutcome = sentinel
        .after_collection(&mut db, &plan_id, &[10, 5, 0, 3, 2, 1], Vec::new(), &l10n)
        .unwrap();
    let popup: &AuditPopupView = outcome.popup.as_ref().expect("有疑点必弹窗");
    assert!(!popup.title.is_empty());
    let report: AuditReportView = sentinel.report_view(&outcome.result, &l10n);
    assert_eq!(report.category_lines.len(), 6);
    let _ = QuietSentinel::with_threshold(40);

    // Error（#[non_exhaustive]）：Display 可用
    let err: Error = serde_json::from_str::<serde_json::Value>("not-json")
        .map_err(Error::from)
        .unwrap_err();
    assert!(!err.to_string().is_empty());
}
