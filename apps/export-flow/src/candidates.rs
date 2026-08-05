//! A2 候选导出存储 seam：经 B2 读取封账摘要与保留候选投影。
//!
//! ADR-0040/0043：增强导出的封账摘要由应用流程读取后传入 F9；F9 只消费
//! 封账后状态为“保留”的稳定候选标识与同一份规范化投影。本模块是生产实现，
//! 测试可经 B2 公开 trait 构造真实发布批次 + 封账写回。

use std::sync::{Arc, Mutex};

use data_persistence::{
    CandidateEligibility, CandidateProjectionsApi, Database, ReviewDecisionsApi,
};
use export_console::{
    CandidateExportReader, CandidateExportSummary, Error, KeptCandidateProjection,
};
use shared_domain_types::CandidateCategory;

/// B2 连接 + 候选/封账事实读取（与壳内 F3/A1 共用同一连接组）。
#[derive(Clone)]
pub(crate) struct ExportCandidateStore {
    db: Arc<Mutex<Database>>,
}

impl ExportCandidateStore {
    pub(crate) fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// 封账摘要（保留/待定/剔除计数 + 投影数 + 决定数）。
    pub(crate) fn seal_summary(&self, plan_id: &str) -> Result<CandidateExportSummary, Error> {
        let db = self.db.lock().map_err(lock_error)?;
        let (pending, keep, remove) = db.count_review_states(plan_id).map_err(read_error)?;
        let decisions = db.list_review_decisions(plan_id).map_err(read_error)?;
        let mut keep_by_category: Vec<(CandidateCategory, usize)> = Vec::new();
        for decision in &decisions {
            if decision.review_state.is_keep() {
                if let Some((_, count)) = keep_by_category
                    .iter_mut()
                    .find(|(category, _)| *category == decision.category)
                {
                    *count += 1;
                } else {
                    keep_by_category.push((decision.category, 1));
                }
            }
        }
        let candidate_projection_count = db
            .list_reviewable_candidate_projections(plan_id)
            .map_err(read_error)?
            .len();
        Ok(CandidateExportSummary {
            candidate_projection_count,
            review_decision_count: decisions.len(),
            keep_total: keep,
            keep_by_category,
            pending_count: pending,
            remove_count: remove,
        })
    }

    /// 封账后状态为保留的稳定候选标识（B2 review_decisions 终态）。
    pub(crate) fn kept_candidate_ids(&self, plan_id: &str) -> Result<Vec<String>, Error> {
        let db = self.db.lock().map_err(lock_error)?;
        let decisions = db.list_review_decisions(plan_id).map_err(read_error)?;
        Ok(decisions
            .into_iter()
            .filter(|decision| decision.review_state.is_keep())
            .map(|decision| decision.candidate_id)
            .collect())
    }
}

impl CandidateExportReader for ExportCandidateStore {
    fn kept_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<KeptCandidateProjection>, Error> {
        let db = self.db.lock().map_err(lock_error)?;
        let projection = db
            .get_current_candidate_projection(plan_id, candidate_id)
            .map_err(read_error)?;
        Ok(projection.map(|projection| KeptCandidateProjection {
            candidate_id: projection.candidate_id,
            category: projection.category,
            display_title: projection.display.title,
            tags: projection.display.tags,
            shape_kind: projection.shape.kind,
            coordinates: projection.shape.coordinates,
            reviewable: projection.eligibility == CandidateEligibility::Reviewable,
        }))
    }
}

fn lock_error<E>(_: std::sync::PoisonError<E>) -> Error {
    Error::CandidateRead("候选导出存储锁损坏".to_owned())
}

fn read_error(error: impl std::fmt::Display) -> Error {
    Error::CandidateRead(error.to_string())
}
