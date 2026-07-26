//! 裁决记忆机制（ADR-0019 第三节）
//!
//! 用户点"知道了"关闭疑点弹窗后**记住该裁决**，同一批疑点不再主动出现；
//! 仅当该方案重新采集（数据变化，体检重做）时，裁决作废、疑点重新评估。
//!
//! 裁决按方案持久化到 B2 app_settings（键 `coverage_audit_decisions`，
//! 值为 `{ 方案 ID: [疑点稳定 ID] }` 的 JSON），只经 `AppSettingsApi`
//! 公开 trait 读写，不触碰 SQL。

use std::collections::{BTreeMap, BTreeSet};

use data_persistence::{AppSettingKey, AppSettingsApi};
use shared_domain_types::PlanId;

use crate::error::Result;
use crate::model::AuditIssue;

/// 全部方案的裁决记忆（app_settings 单键内的 JSON 结构）
type DecisionMap = BTreeMap<String, BTreeSet<String>>;

/// 裁决解析器 —— 管理单个方案的疑点裁决记忆
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionResolver {
    /// 已裁决的疑点稳定 ID（见 [`AuditIssue::stable_id`]）
    decided: BTreeSet<String>,
}

impl DecisionResolver {
    /// 创建空解析器（无任何已裁决疑点 —— 重新采集后的初始态）
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 B2 加载某方案的裁决记忆（无记录时返回空解析器）
    pub fn load(db: &impl AppSettingsApi, plan_id: &PlanId) -> Result<Self> {
        let map = load_map(db)?;
        let decided = map.get(&plan_id.to_string()).cloned().unwrap_or_default();
        Ok(Self { decided })
    }

    /// 把当前裁决记忆写回 B2（覆盖该方案的旧记录，其他方案不受影响）
    pub fn save(&self, db: &mut impl AppSettingsApi, plan_id: &PlanId) -> Result<()> {
        let mut map = load_map(db)?;
        if self.decided.is_empty() {
            map.remove(&plan_id.to_string());
        } else {
            map.insert(plan_id.to_string(), self.decided.clone());
        }
        let json = serde_json::to_string(&map)?;
        db.set_setting(AppSettingKey::CoverageAuditDecisions, &json)?;
        Ok(())
    }

    /// 某疑点是否已被用户裁决（点过"知道了"）
    pub fn is_decided(&self, issue: &AuditIssue) -> bool {
        self.decided.contains(&issue.stable_id())
    }

    /// 记录用户对疑点的裁决
    pub fn record_decision(&mut self, issue: &AuditIssue) {
        self.decided.insert(issue.stable_id());
    }

    /// 过滤出尚未裁决的疑点（这些才允许主动弹窗）
    pub fn undecided<'a>(&self, issues: &'a [AuditIssue]) -> Vec<&'a AuditIssue> {
        issues
            .iter()
            .filter(|issue| !self.is_decided(issue))
            .collect()
    }

    /// 作废全部裁决（重新采集时调用：数据变化，体检重做）
    pub fn reset(&mut self) {
        self.decided.clear();
    }
}

/// 读出 app_settings 中全部方案的裁决记忆（键缺失时为空表）
fn load_map(db: &impl AppSettingsApi) -> Result<DecisionMap> {
    let raw = db.get_setting(AppSettingKey::CoverageAuditDecisions)?;
    match raw {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(DecisionMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::IssueRule;
    use data_persistence::Database;
    use shared_domain_types::CandidateCategory;

    fn water_empty_issue() -> AuditIssue {
        AuditIssue {
            rule: IssueRule::EmptyCategory,
            message_key: "audit.empty_category",
            category: Some(CandidateCategory::Water),
            other_count: None,
            other_percentage: None,
        }
    }

    fn high_other_issue() -> AuditIssue {
        AuditIssue {
            rule: IssueRule::HighOtherRatio,
            message_key: "audit.high_other_ratio",
            category: None,
            other_count: Some(35),
            other_percentage: Some(29),
        }
    }

    #[test]
    fn undecided_filters_out_recorded_decisions() {
        let mut resolver = DecisionResolver::new();
        let issues = vec![water_empty_issue(), high_other_issue()];

        assert_eq!(resolver.undecided(&issues).len(), 2);

        resolver.record_decision(&issues[0]);
        let remaining = resolver.undecided(&issues);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].rule, IssueRule::HighOtherRatio);
    }

    #[test]
    fn decisions_roundtrip_through_app_settings() {
        let mut db = Database::open_in_memory().expect("内存库可打开");
        let plan_id = PlanId::generate();

        let mut resolver = DecisionResolver::load(&db, &plan_id).unwrap();
        assert!(!resolver.is_decided(&water_empty_issue()));

        resolver.record_decision(&water_empty_issue());
        resolver.save(&mut db, &plan_id).unwrap();

        // 重新加载（模拟重启应用）：裁决仍被记住
        let reloaded = DecisionResolver::load(&db, &plan_id).unwrap();
        assert!(reloaded.is_decided(&water_empty_issue()));
        assert!(!reloaded.is_decided(&high_other_issue()));
    }

    #[test]
    fn plans_have_independent_decisions() {
        let mut db = Database::open_in_memory().expect("内存库可打开");
        let plan_a = PlanId::generate();
        let plan_b = PlanId::generate();

        let mut resolver_a = DecisionResolver::new();
        resolver_a.record_decision(&water_empty_issue());
        resolver_a.save(&mut db, &plan_a).unwrap();

        let resolver_b = DecisionResolver::load(&db, &plan_b).unwrap();
        assert!(!resolver_b.is_decided(&water_empty_issue()));
    }

    #[test]
    fn reset_then_save_clears_stored_record() {
        let mut db = Database::open_in_memory().expect("内存库可打开");
        let plan_id = PlanId::generate();

        let mut resolver = DecisionResolver::new();
        resolver.record_decision(&water_empty_issue());
        resolver.save(&mut db, &plan_id).unwrap();

        // 重新采集：裁决作废并落库
        resolver.reset();
        resolver.save(&mut db, &plan_id).unwrap();

        let reloaded = DecisionResolver::load(&db, &plan_id).unwrap();
        assert!(!reloaded.is_decided(&water_empty_issue()));
    }
}
