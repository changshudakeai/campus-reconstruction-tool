//! 安静哨兵集成测试 —— 贴着 ADR-0019 的用户可见行为测试
//!
//! 只测外部行为：给计数 → 看弹窗/报告/裁决记忆，不测实现细节。

use coverage_audit::{IssueRule, QuietSentinel, ALL_CATEGORIES};
use data_persistence::Database;
use localization::{Language, Localization};
use shared_domain_types::{CandidateCategory, PlanId};

fn l10n() -> Localization {
    Localization::new(Language::ZhCn).expect("内嵌 zh-CN 资源必定可用")
}

/// 工单指定用例：0 建筑 + 100 其他 → 触发"其他占比"疑点
#[test]
fn zero_buildings_and_all_other_triggers_high_other_ratio() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let sentinel = QuietSentinel::new();

    // 只采了建筑与其他两类，其余四类跳过
    let skipped = vec![
        CandidateCategory::Road,
        CandidateCategory::Water,
        CandidateCategory::Vegetation,
        CandidateCategory::Sports,
    ];
    let outcome = sentinel
        .after_collection(&mut db, &plan_id, &[0, 0, 0, 0, 0, 100], skipped, &l10n())
        .unwrap();

    let other_issue = outcome
        .result
        .issues
        .iter()
        .find(|issue| issue.rule == IssueRule::HighOtherRatio)
        .expect("100% 其他必须触发占比疑点");
    assert_eq!(other_issue.other_percentage, Some(100));
    assert_eq!(other_issue.other_count, Some(100));

    // 弹窗合并一窗：同时含"建筑空类别"与"其他占比"两条疑点问句
    let popup = outcome.popup.expect("有疑点必须弹窗");
    assert!(popup.body.contains("建筑"), "空建筑类别问句应在弹窗内");
    assert!(popup.body.contains("100%"), "其他占比问句应在弹窗内");
    assert!(popup.body.contains("？"), "报告措辞一律问句");
    assert_eq!(popup.ack_label, "知道了");
}

/// 无疑点 → 界面无任何打扰（安静哨兵）
#[test]
fn healthy_collection_stays_silent() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let sentinel = QuietSentinel::new();

    let outcome = sentinel
        .after_collection(
            &mut db,
            &plan_id,
            &[30, 20, 5, 10, 5, 5],
            Vec::new(),
            &l10n(),
        )
        .unwrap();

    assert!(outcome.popup.is_none(), "无疑点不得打扰用户");
    assert!(!outcome.result.has_issues());
}

/// "知道了"后的裁决被记住：重开方案不再弹；重新采集才重新评估
#[test]
fn acknowledged_issues_stay_quiet_until_recollection() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let sentinel = QuietSentinel::new();
    let counts = [10, 5, 0, 3, 2, 1]; // 水域空 → 一条疑点

    // 采集完成：弹窗一次（弹窗返回 = 用户已点"知道了"）
    let first = sentinel
        .after_collection(&mut db, &plan_id, &counts, Vec::new(), &l10n())
        .unwrap();
    assert!(first.popup.is_some());

    // 重开方案（同一批数据）：同一批疑点不再主动出现
    let reopened = sentinel
        .on_plan_opened(&mut db, &plan_id, &counts, Vec::new(), &l10n())
        .unwrap();
    assert!(reopened.popup.is_none(), "已裁决的疑点不得再弹");
    // 报告里仍可查看历史疑点
    assert_eq!(reopened.result.issues.len(), 1);

    // 重新采集（数据未变也算重做体检）：裁决作废，弹窗重新出现
    let recollected = sentinel
        .after_collection(&mut db, &plan_id, &counts, Vec::new(), &l10n())
        .unwrap();
    assert!(recollected.popup.is_some(), "重新采集后疑点重新评估");
}

/// 跳过的类别在报告中标注"未采集（跳过）"，不算疑点
#[test]
fn report_view_annotates_skipped_categories() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let sentinel = QuietSentinel::new();
    let localization = l10n();

    let outcome = sentinel
        .after_collection(
            &mut db,
            &plan_id,
            &[12, 0, 0, 0, 0, 0],
            vec![
                CandidateCategory::Road,
                CandidateCategory::Water,
                CandidateCategory::Vegetation,
                CandidateCategory::Sports,
                CandidateCategory::Other,
            ],
            &localization,
        )
        .unwrap();

    // 最短路径用户（只采建筑）不被空类别疑点轰炸
    assert!(outcome.popup.is_none());

    let report = sentinel.report_view(&outcome.result, &localization);
    assert_eq!(report.title, "采集报告");
    assert_eq!(report.category_lines.len(), ALL_CATEGORIES.len());
    assert!(report.category_lines[0].contains("12 项"));
    assert!(report.category_lines[1].contains("未采集（跳过）"));
    assert_eq!(
        report.no_issues_line,
        Some("本次体检没有发现疑点".to_owned())
    );
}

/// 有疑点的报告：疑点问句在常驻报告中可查
#[test]
fn report_view_lists_issue_questions() {
    let sentinel = QuietSentinel::new();
    let localization = l10n();
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();

    let outcome = sentinel
        .after_collection(
            &mut db,
            &plan_id,
            &[10, 5, 0, 3, 2, 1],
            Vec::new(),
            &localization,
        )
        .unwrap();

    let report = sentinel.report_view(&outcome.result, &localization);
    assert_eq!(report.issue_lines.len(), 1);
    assert!(report.issue_lines[0].contains("水域"));
    assert!(report.issue_lines[0].ends_with("？"));
    assert!(report.no_issues_line.is_none());
}
