//! 候选生命周期使用的边界指纹与评审作废历史查询。
//!
//! 边界变化后的投影演进、决定作废与新批次发布由
//! `candidate_projections` 生命周期 module 在同一事务中完成；这里仅保留
//! 指纹读取以及历史追溯所需的窄接口。

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use shared_domain_types::{Boundary, ReviewState};

use crate::database::Database;
use crate::entities::{review_state_from_db, timestamp_from_db, timestamp_to_db};
use crate::error::Result;

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

/// 边界指纹与评审作废历史查询接口。
pub trait BoundaryRevalidationApi {
    /// 记录方案最近一次采集（或重验证）使用的边界指纹。
    fn save_plan_collection_boundary(&mut self, plan_id: &str, fingerprint: &str) -> Result<()>;

    /// 读取方案最近一次采集使用的边界指纹；无记录返回 `None`。
    fn load_plan_collection_boundary(&self, plan_id: &str) -> Result<Option<String>>;

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
