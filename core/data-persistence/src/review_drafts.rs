//! 未封账评审草稿检查点 API
//!
//! 评审期间核心工作台保持"零写库"（缝 4 契约），本表由应用流层在每次
//! 状态变更后写安全检查点（工单 workspace-restore A.6），使意外退出后
//! 未封账三态仍可恢复；封账成功后应用流层调用 [`ReviewDraftApi::clear_review_draft`]
//! 清空草稿——封账终态以 `review_decisions` 为唯一权威，草稿不得覆盖终态。

use rusqlite::{params, OptionalExtension};

use crate::database::Database;
use crate::entities::{
    category_from_db, category_to_db, review_state_from_db, review_state_to_db, ReviewDraft,
    ReviewDraftEntry,
};
use crate::error::Result;

/// 未封账评审草稿读写（按方案整批替换）。
pub trait ReviewDraftApi {
    /// 整批替换某方案的草稿（单事务：清旧 + 写新 + 元信息）。
    fn save_review_draft(&mut self, draft: &ReviewDraft) -> Result<()>;

    /// 读取某方案的草稿；无草稿返回 None。
    fn load_review_draft(&self, plan_id: &str) -> Result<Option<ReviewDraft>>;

    /// 清空某方案的草稿（封账成功后调用）。
    fn clear_review_draft(&mut self, plan_id: &str) -> Result<()>;
}

impl ReviewDraftApi for Database {
    fn save_review_draft(&mut self, draft: &ReviewDraft) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM review_draft_states WHERE plan_id = ?1",
                params![draft.plan_id],
            )?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO review_draft_states
                        (plan_id, candidate_id, review_state, selected)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                for entry in &draft.entries {
                    stmt.execute(params![
                        draft.plan_id,
                        entry.candidate_id,
                        review_state_to_db(entry.review_state),
                        if entry.selected { 1 } else { 0 },
                    ])?;
                }
            }
            tx.execute(
                "INSERT INTO review_draft_meta (plan_id, active_category)
                 VALUES (?1, ?2)
                 ON CONFLICT(plan_id) DO UPDATE SET
                    active_category = excluded.active_category,
                    updated_at = datetime('now')",
                params![draft.plan_id, category_to_db(draft.active_category)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn load_review_draft(&self, plan_id: &str) -> Result<Option<ReviewDraft>> {
        let meta = self
            .conn
            .query_row(
                "SELECT active_category FROM review_draft_meta WHERE plan_id = ?1",
                params![plan_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(active_category) = meta else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT candidate_id, review_state, selected
             FROM review_draft_states WHERE plan_id = ?1
             ORDER BY candidate_id",
        )?;
        let mut rows = stmt.query(params![plan_id])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            let selected: i64 = row.get(2)?;
            entries.push(ReviewDraftEntry {
                candidate_id: row.get(0)?,
                review_state: review_state_from_db(&row.get::<_, String>(1)?)?,
                selected: selected != 0,
            });
        }
        Ok(Some(ReviewDraft {
            plan_id: plan_id.to_owned(),
            active_category: category_from_db(&active_category)?,
            entries,
        }))
    }

    fn clear_review_draft(&mut self, plan_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM review_draft_states WHERE plan_id = ?1",
            params![plan_id],
        )?;
        self.conn.execute(
            "DELETE FROM review_draft_meta WHERE plan_id = ?1",
            params![plan_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReviewDraftApi;
    use shared_domain_types::{CandidateCategory, ReviewState};

    #[test]
    fn review_draft_roundtrips_and_clears() {
        let mut db = Database::open_in_memory().unwrap();
        assert!(db.load_review_draft("plan-1").unwrap().is_none());

        let draft = ReviewDraft {
            plan_id: "plan-1".to_owned(),
            active_category: CandidateCategory::Road,
            entries: vec![
                ReviewDraftEntry {
                    candidate_id: "overpass:way/1:outer".to_owned(),
                    review_state: ReviewState::Keep,
                    selected: true,
                },
                ReviewDraftEntry {
                    candidate_id: "overpass:way/2:outer".to_owned(),
                    review_state: ReviewState::Remove,
                    selected: false,
                },
            ],
        };
        db.save_review_draft(&draft).unwrap();
        let loaded = db.load_review_draft("plan-1").unwrap().unwrap();
        assert_eq!(loaded.active_category, CandidateCategory::Road);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].review_state, ReviewState::Keep);
        assert!(loaded.entries[0].selected);
        assert_eq!(loaded.entries[1].review_state, ReviewState::Remove);

        // 整批替换：旧条目消失
        let replaced = ReviewDraft {
            plan_id: "plan-1".to_owned(),
            active_category: CandidateCategory::Building,
            entries: vec![ReviewDraftEntry {
                candidate_id: "overpass:way/3:outer".to_owned(),
                review_state: ReviewState::Pending,
                selected: false,
            }],
        };
        db.save_review_draft(&replaced).unwrap();
        let loaded = db.load_review_draft("plan-1").unwrap().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].candidate_id, "overpass:way/3:outer");

        db.clear_review_draft("plan-1").unwrap();
        assert!(db.load_review_draft("plan-1").unwrap().is_none());
    }
}
