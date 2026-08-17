//! 评审终态表 API
//!
//! 缝 4 契约（无卡顿铁律）：评审期间零写库；导出确认（封账）后，
//! F5 把最终三态**一次性批量写回**本表——单事务原子提交，
//! 任何一条失败整批回滚，保证不出现"账封了但没存上"的半截状态。

use rusqlite::{params, OptionalExtension};

use crate::database::Database;
use crate::entities::{
    category_from_db, category_to_db, review_state_from_db, review_state_to_db, timestamp_from_db,
    timestamp_to_db, ReviewDecision,
};
use crate::error::Result;

/// 评审终态批量写回接口（供 F5 封账、F9 导出读取调用）
pub trait ReviewDecisionsApi {
    /// 仅当候选批次仍是评审页打开时的 revision 才原子封账；整批必须且只能
    /// 覆盖当前 Reviewable 候选，调用方不能把旧决定或隔离候选写回。
    fn batch_update_review_decisions_at_revision(
        &mut self,
        plan_id: &str,
        expected_revision: &str,
        decisions: &[ReviewDecision],
    ) -> Result<()>;

    /// 列出方案下的全部评审终态（按稳定候选 ID 排序）。
    fn list_review_decisions(&self, plan_id: &str) -> Result<Vec<ReviewDecision>>;

    /// 统计方案下各三态的条数（不含已作废标注的决定），返回 (待定, 保留, 剔除)
    fn count_review_states(&self, plan_id: &str) -> Result<(usize, usize, usize)>;
}

impl ReviewDecisionsApi for Database {
    fn batch_update_review_decisions_at_revision(
        &mut self,
        plan_id: &str,
        expected_revision: &str,
        decisions: &[ReviewDecision],
    ) -> Result<()> {
        if decisions.iter().any(|decision| decision.plan_id != plan_id) {
            return Err(crate::Error::CandidateBatchRejected(
                "封账决定包含其他方案的候选".to_owned(),
            ));
        }
        let tx = self.conn.transaction()?;
        let actual_revision = tx
            .query_row(
                "SELECT collection_batch_id FROM current_candidate_batches WHERE plan_id=?1",
                params![plan_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_default();
        if actual_revision != expected_revision {
            return Err(crate::Error::StaleCandidateProjectionRevision {
                expected: expected_revision.to_owned(),
                actual: actual_revision,
            });
        }
        let reviewable_count: usize = tx.query_row(
            "SELECT COUNT(*) FROM candidate_projections
             WHERE collection_batch_id=?1 AND plan_id=?2 AND eligibility='reviewable'",
            params![expected_revision, plan_id],
            |row| row.get(0),
        )?;
        let unique_ids: std::collections::HashSet<_> = decisions
            .iter()
            .map(|decision| decision.candidate_id.as_str())
            .collect();
        if unique_ids.len() != decisions.len() || decisions.len() != reviewable_count {
            return Err(crate::Error::CandidateBatchRejected(
                "封账必须且只能覆盖当前完整 Reviewable 候选集".to_owned(),
            ));
        }
        for decision in decisions {
            let category: Option<String> = tx
                .query_row(
                    "SELECT category FROM candidate_projections
                     WHERE collection_batch_id=?1 AND plan_id=?2
                       AND candidate_id=?3 AND eligibility='reviewable'",
                    params![expected_revision, plan_id, decision.candidate_id],
                    |row| row.get(0),
                )
                .optional()?;
            if category.as_deref() != Some(category_to_db(decision.category)?) {
                return Err(crate::Error::CandidateBatchRejected(format!(
                    "封账候选不属于当前合法评审批次：{}",
                    decision.candidate_id
                )));
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO review_decisions
                    (plan_id, category, candidate_id, review_state, reviewer_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(plan_id, candidate_id) DO UPDATE SET
                    category = excluded.category,
                    review_state = excluded.review_state,
                    reviewer_id = excluded.reviewer_id,
                    updated_at = excluded.updated_at,
                    voided = 0,
                    voided_reason = NULL,
                    voided_at = NULL",
            )?;
            for decision in decisions {
                stmt.execute(params![
                    decision.plan_id,
                    category_to_db(decision.category)?,
                    decision.candidate_id,
                    review_state_to_db(decision.review_state),
                    decision.reviewer_id,
                    timestamp_to_db(decision.updated_at),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn list_review_decisions(&self, plan_id: &str) -> Result<Vec<ReviewDecision>> {
        let mut stmt = self.conn.prepare(
            "SELECT plan_id, category, candidate_id, review_state, reviewer_id, updated_at
             FROM review_decisions WHERE plan_id = ?1 AND voided = 0
             ORDER BY candidate_id",
        )?;
        let mut rows = stmt.query(params![plan_id])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let updated_at: String = row.get(5)?;
            result.push(ReviewDecision {
                plan_id: row.get(0)?,
                category: category_from_db(&row.get::<_, String>(1)?)?,
                candidate_id: row.get(2)?,
                review_state: review_state_from_db(&row.get::<_, String>(3)?)?,
                reviewer_id: row.get(4)?,
                updated_at: timestamp_from_db(&updated_at)?,
            });
        }
        Ok(result)
    }

    fn count_review_states(&self, plan_id: &str) -> Result<(usize, usize, usize)> {
        let mut stmt = self.conn.prepare(
            "SELECT review_state, COUNT(*) FROM review_decisions
             WHERE plan_id = ?1 AND voided = 0 GROUP BY review_state",
        )?;
        let mut rows = stmt.query(params![plan_id])?;
        let (mut pending, mut keep, mut remove) = (0usize, 0usize, 0usize);
        while let Some(row) = rows.next()? {
            let state: String = row.get(0)?;
            let count: usize = row.get(1)?;
            match review_state_from_db(&state)? {
                s if s.is_pending() => pending = count,
                s if s.is_keep() => keep = count,
                _ => remove = count,
            }
        }
        Ok((pending, keep, remove))
    }
}
