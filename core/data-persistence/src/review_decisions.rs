//! 评审终态表 API
//!
//! 缝 4 契约（无卡顿铁律）：评审期间零写库；导出确认（封账）后，
//! F5 把最终三态**一次性批量写回**本表——单事务原子提交，
//! 任何一条失败整批回滚，保证不出现"账封了但没存上"的半截状态。

use rusqlite::params;

use crate::database::Database;
use crate::entities::{
    category_from_db, category_to_db, review_state_from_db, review_state_to_db, timestamp_from_db,
    timestamp_to_db, ReviewDecision,
};
use crate::error::Result;

/// 评审终态批量写回接口（供 F5 封账、F9 导出读取调用）
pub trait ReviewDecisionsApi {
    /// 封账批量写回：整批评审终态在单事务内原子写入（UPSERT）。
    ///
    /// 任一条写入失败即整批回滚（Err 返回时数据库保持写入前状态），
    /// 调用方（F5）据此保持封账不生效、评审状态可改。
    fn batch_update_review_decisions(&mut self, decisions: &[ReviewDecision]) -> Result<()>;

    /// 列出方案下的全部评审终态（按稳定候选 ID 排序）。
    fn list_review_decisions(&self, plan_id: &str) -> Result<Vec<ReviewDecision>>;

    /// 统计方案下各三态的条数（不含已作废标注的决定），返回 (待定, 保留, 剔除)
    fn count_review_states(&self, plan_id: &str) -> Result<(usize, usize, usize)>;
}

impl ReviewDecisionsApi for Database {
    fn batch_update_review_decisions(&mut self, decisions: &[ReviewDecision]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO review_decisions
                    (plan_id, category, candidate_id, review_state, reviewer_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(plan_id, candidate_id) DO UPDATE SET
                    category = excluded.category,
                    review_state = excluded.review_state,
                    reviewer_id = excluded.reviewer_id,
                    updated_at = excluded.updated_at",
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
