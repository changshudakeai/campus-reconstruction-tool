//! 边界变化后的本地候选资格重验证持久化（D 工单）。
//!
//! 三份事实都由本模块单事务落库，保证"换边界 → 重验证"原子可见：
//! 1. 方案级"上次采集时使用的边界"指纹（触发依据：确认边界时与它对比，
//!    不同才重验证；没有记录时按"不同"处理并补记）；
//! 2. 候选投影资格更新（Reviewable/Isolated + 隔离原因）；
//! 3. 旧评审决定作废标注（保留记录不物理删除）+ 作废历史留痕；
//!    新进入边界的候选评审状态回到待定。

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use shared_domain_types::{Boundary, ReviewState};

use crate::database::Database;
use crate::entities::{review_state_from_db, timestamp_from_db, timestamp_to_db};
use crate::error::Result;
use crate::CandidateEligibility;

/// 把已确认边界序列化后的 SHA256 指纹（坐标变化即指纹变化）。
pub fn boundary_fingerprint(boundary: &Boundary) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_string(boundary).unwrap_or_default();
    let hash = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// 一条候选投影的资格更新（只改资格与隔离原因，几何/展示等事实不动）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEligibilityUpdate {
    pub candidate_id: String,
    pub eligibility: CandidateEligibility,
    pub isolation_reason: Option<String>,
}

/// 一条待作废的评审决定（原因按新边界判定结论标注）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionVoid {
    pub candidate_id: String,
    pub reason: String,
}

/// 一次重验证写库的汇总计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RevalidationWriteSummary {
    /// 实际更新了资格/隔离原因的候选投影数。
    pub eligibility_updated: usize,
    /// 被作废标注的评审决定数（含无历史决定时的 0）。
    pub decisions_voided: usize,
    /// 被重置回"待定"的评审决定数。
    pub decisions_reset_to_pending: usize,
}

/// 当前被作废标注的评审决定（review_decisions 行保留原状态 + 作废标注）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoidedReviewDecision {
    pub plan_id: String,
    pub candidate_id: String,
    pub review_state: ReviewState,
    pub voided_reason: Option<String>,
    pub voided_at: Option<DateTime<Utc>>,
}

/// 评审决定作废历史（每次作废事件永久留痕，含被作废前的评审三态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDecisionInvalidation {
    pub plan_id: String,
    pub candidate_id: String,
    pub previous_state: ReviewState,
    pub reason: String,
    pub invalidated_at: DateTime<Utc>,
}

/// 边界重验证持久化接口（A1 调用：比较指纹 → 计算 → 单事务写库）。
pub trait BoundaryRevalidationApi {
    /// 记录方案最近一次采集（或重验证）使用的边界指纹。
    fn save_plan_collection_boundary(&mut self, plan_id: &str, fingerprint: &str) -> Result<()>;

    /// 读取方案最近一次采集使用的边界指纹；无记录返回 `None`。
    fn load_plan_collection_boundary(&self, plan_id: &str) -> Result<Option<String>>;

    /// 单事务应用一次边界重验证结果：更新候选投影资格、作废旧评审决定、
    /// 新进入候选回到待定、写入最新边界指纹。任一步失败整批回滚。
    fn apply_boundary_revalidation(
        &mut self,
        plan_id: &str,
        eligibility_updates: &[CandidateEligibilityUpdate],
        voids: &[DecisionVoid],
        reset_to_pending: &[String],
        fingerprint: &str,
    ) -> Result<RevalidationWriteSummary>;

    /// 列出方案下当前被作废标注的评审决定（历史可查，不物理删除）。
    fn list_voided_review_decisions(&self, plan_id: &str) -> Result<Vec<VoidedReviewDecision>>;

    /// 列出方案下全部评审决定作废历史。
    fn list_review_decision_invalidations(
        &self,
        plan_id: &str,
    ) -> Result<Vec<ReviewDecisionInvalidation>>;
}

impl BoundaryRevalidationApi for Database {
    fn save_plan_collection_boundary(&mut self, plan_id: &str, fingerprint: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plan_collection_boundary (plan_id, fingerprint, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_id) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                updated_at = excluded.updated_at",
            params![plan_id, fingerprint, timestamp_to_db(Utc::now())],
        )?;
        Ok(())
    }

    fn load_plan_collection_boundary(&self, plan_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT fingerprint FROM plan_collection_boundary WHERE plan_id = ?1",
                params![plan_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(crate::error::Error::from)
    }

    fn apply_boundary_revalidation(
        &mut self,
        plan_id: &str,
        eligibility_updates: &[CandidateEligibilityUpdate],
        voids: &[DecisionVoid],
        reset_to_pending: &[String],
        fingerprint: &str,
    ) -> Result<RevalidationWriteSummary> {
        let now = Utc::now();
        let now_db = timestamp_to_db(now);
        let tx = self.conn.transaction()?;
        let mut eligibility_updated = 0usize;
        {
            let mut stmt = tx.prepare(
                "UPDATE candidate_projections
                 SET eligibility = ?1,
                     isolation_reason = ?2,
                     updated_at = ?3
                 WHERE candidate_id = ?4
                   AND collection_batch_id = (
                       SELECT collection_batch_id FROM current_candidate_batches
                       WHERE plan_id = ?5
                   )",
            )?;
            for update in eligibility_updates {
                eligibility_updated += stmt.execute(params![
                    match update.eligibility {
                        CandidateEligibility::Reviewable => "reviewable",
                        CandidateEligibility::Isolated => "isolated",
                    },
                    update.isolation_reason,
                    now_db,
                    update.candidate_id,
                    plan_id,
                ])?;
            }
        }
        let mut decisions_voided = 0usize;
        {
            let mut read_state = tx.prepare(
                "SELECT review_state FROM review_decisions
                 WHERE plan_id = ?1 AND candidate_id = ?2 AND voided = 0",
            )?;
            let mut history = tx.prepare(
                "INSERT INTO review_decision_invalidations
                    (plan_id, candidate_id, previous_state, reason, invalidated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut mark = tx.prepare(
                "UPDATE review_decisions
                 SET voided = 1, voided_reason = ?3, voided_at = ?4, updated_at = ?4
                 WHERE plan_id = ?1 AND candidate_id = ?2 AND voided = 0",
            )?;
            for void in voids {
                let previous: Option<String> = read_state
                    .query_row(params![plan_id, void.candidate_id], |row| row.get(0))
                    .ok();
                let Some(previous) = previous else {
                    continue;
                };
                history.execute(params![
                    plan_id,
                    void.candidate_id,
                    previous,
                    void.reason,
                    now_db,
                ])?;
                decisions_voided +=
                    mark.execute(params![plan_id, void.candidate_id, void.reason, now_db,])?;
            }
        }
        let mut decisions_reset_to_pending = 0usize;
        {
            let mut stmt = tx.prepare(
                "UPDATE review_decisions
                 SET review_state = 'pending',
                     voided = 0,
                     voided_reason = NULL,
                     voided_at = NULL,
                     updated_at = ?2
                 WHERE plan_id = ?1 AND candidate_id = ?3",
            )?;
            for candidate_id in reset_to_pending {
                decisions_reset_to_pending +=
                    stmt.execute(params![plan_id, now_db, candidate_id])?;
            }
        }
        tx.execute(
            "INSERT INTO plan_collection_boundary (plan_id, fingerprint, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_id) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                updated_at = excluded.updated_at",
            params![plan_id, fingerprint, now_db],
        )?;
        tx.commit()?;
        Ok(RevalidationWriteSummary {
            eligibility_updated,
            decisions_voided,
            decisions_reset_to_pending,
        })
    }

    fn list_voided_review_decisions(&self, plan_id: &str) -> Result<Vec<VoidedReviewDecision>> {
        let mut stmt = self.conn.prepare(
            "SELECT plan_id, candidate_id, review_state, voided_reason, voided_at
             FROM review_decisions
             WHERE plan_id = ?1 AND voided = 1
             ORDER BY candidate_id",
        )?;
        let rows = stmt.query_map(params![plan_id], |row| {
            let voided_at: Option<String> = row.get(4)?;
            Ok(VoidedReviewDecision {
                plan_id: row.get(0)?,
                candidate_id: row.get(1)?,
                review_state: review_state_from_db(&row.get::<_, String>(2)?)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                voided_reason: row.get(3)?,
                voided_at: voided_at
                    .map(|value| timestamp_from_db(&value))
                    .transpose()
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(crate::error::Error::from)
    }

    fn list_review_decision_invalidations(
        &self,
        plan_id: &str,
    ) -> Result<Vec<ReviewDecisionInvalidation>> {
        let mut stmt = self.conn.prepare(
            "SELECT plan_id, candidate_id, previous_state, reason, invalidated_at
             FROM review_decision_invalidations
             WHERE plan_id = ?1
             ORDER BY invalidated_at, candidate_id",
        )?;
        let rows = stmt.query_map(params![plan_id], |row| {
            Ok(ReviewDecisionInvalidation {
                plan_id: row.get(0)?,
                candidate_id: row.get(1)?,
                previous_state: review_state_from_db(&row.get::<_, String>(2)?)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                reason: row.get(3)?,
                invalidated_at: timestamp_from_db(&row.get::<_, String>(4)?)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(crate::error::Error::from)
    }
}
