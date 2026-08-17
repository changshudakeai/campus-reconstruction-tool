//! ADR-0040 候选投影与批次发布：原始观测保持不变，只有完整发布的投影可被评审读取。

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use shared_domain_types::CandidateCategory;
use uuid::Uuid;

use crate::database::Database;
use crate::entities::{category_to_db, timestamp_to_db};
use crate::error::{Error, Result};

mod storage;

use storage::{insert_projection, stable_candidate_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateEligibility {
    Reviewable,
    Isolated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateValidation {
    Retained,
    Repaired,
    Rejected,
}

/// 只有通过验证或唯一修复后的几何才能形成 Reviewable 投影。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewableValidation {
    Retained,
    Repaired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CandidateNameSource {
    /// 名称来自 OSM `name` 标签（原始来源，不得被高德覆盖）。
    Osm,
    /// 名称来自一次真实高德 regeo 调用。
    Gaode,
    /// 名称来自持久化 regeo 缓存命中（没有产生新网络调用）。
    Cache,
    /// 仍无可用名称，展示稳定占位。
    #[default]
    Unnamed,
    /// 补名尝试失败（网络/超时/限流/缺 Key），名称保持占位。
    Failed,
}

impl CandidateNameSource {
    fn from_db(value: &str) -> Self {
        match value {
            "osm" => Self::Osm,
            "gaode" => Self::Gaode,
            "cache" => Self::Cache,
            "failed" => Self::Failed,
            _ => Self::Unnamed,
        }
    }

    fn to_db(self) -> &'static str {
        match self {
            Self::Osm => "osm",
            Self::Gaode => "gaode",
            Self::Cache => "cache",
            Self::Unnamed => "unnamed",
            Self::Failed => "failed",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateShape {
    pub kind: String,
    pub coordinates: serde_json::Value,
}
impl CandidateShape {
    pub fn point(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "point".to_owned(),
            coordinates,
        }
    }
    pub fn line_string(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "line_string".to_owned(),
            coordinates,
        }
    }
    pub fn polygon(coordinates: serde_json::Value) -> Self {
        Self {
            kind: "polygon".to_owned(),
            coordinates,
        }
    }
}

/// F5 展示候选所需的最小投影属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDisplay {
    /// 候选卡片标题。
    pub title: String,
    /// 用于展示的来源标签与属性。
    pub tags: Vec<(String, String)>,
}

/// 来源自然身份；候选稳定身份由 lifecycle module 在此基础上解析和复用。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CandidateSourceIdentity {
    data_source_tag: String,
    source_entity_id: String,
    geometry_part_id: String,
}

impl CandidateSourceIdentity {
    pub fn new(
        data_source_tag: impl Into<String>,
        source_entity_id: impl Into<String>,
        geometry_part_id: impl Into<String>,
    ) -> Self {
        Self {
            data_source_tag: data_source_tag.into(),
            source_entity_id: source_entity_id.into(),
            geometry_part_id: geometry_part_id.into(),
        }
    }

    pub fn data_source_tag(&self) -> &str {
        &self.data_source_tag
    }

    pub fn source_entity_id(&self) -> &str {
        &self.source_entity_id
    }

    pub fn geometry_part_id(&self) -> &str {
        &self.geometry_part_id
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ProjectionOutcome {
    Reviewable {
        shape: CandidateShape,
        validation: ReviewableValidation,
    },
    Isolated {
        shape: CandidateShape,
        validation: CandidateValidation,
        reason: String,
    },
}

/// collection/revalidation 交给 lifecycle module 的来源与验证事实。
///
/// candidate id、资格、修复标记、缺失标记与时间均由 module 维护，调用方不能传入。
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateProjectionDraft {
    source: CandidateSourceIdentity,
    category: CandidateCategory,
    display: CandidateDisplay,
    name_source: CandidateNameSource,
    outcome: ProjectionOutcome,
}

/// 边界重验证交给 lifecycle module 的逐候选物理事实。
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRevalidationFact {
    candidate_id: String,
    outcome: ProjectionOutcome,
}

impl CandidateRevalidationFact {
    pub fn reviewable(
        candidate_id: impl Into<String>,
        shape: CandidateShape,
        validation: ReviewableValidation,
    ) -> Self {
        Self {
            candidate_id: candidate_id.into(),
            outcome: ProjectionOutcome::Reviewable { shape, validation },
        }
    }

    pub fn isolated(
        candidate_id: impl Into<String>,
        shape: CandidateShape,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(Error::CandidateBatchRejected(
                "边界重验证的隔离事实必须提供非空原因".to_owned(),
            ));
        }
        Ok(Self {
            candidate_id: candidate_id.into(),
            outcome: ProjectionOutcome::Isolated {
                shape,
                validation: CandidateValidation::Rejected,
                reason,
            },
        })
    }

    /// 已通过几何验证，但因边界事实而隔离；验证结论不会被降格为 Rejected。
    pub fn isolated_validated(
        candidate_id: impl Into<String>,
        shape: CandidateShape,
        validation: ReviewableValidation,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(Error::CandidateBatchRejected(
                "边界重验证的隔离事实必须提供非空原因".to_owned(),
            ));
        }
        Ok(Self {
            candidate_id: candidate_id.into(),
            outcome: ProjectionOutcome::Isolated {
                shape,
                validation: match validation {
                    ReviewableValidation::Retained => CandidateValidation::Retained,
                    ReviewableValidation::Repaired => CandidateValidation::Repaired,
                },
                reason,
            },
        })
    }
}

impl CandidateProjectionDraft {
    pub fn reviewable(
        source: CandidateSourceIdentity,
        category: CandidateCategory,
        display: CandidateDisplay,
        shape: CandidateShape,
        validation: ReviewableValidation,
    ) -> Self {
        Self {
            source,
            category,
            display,
            name_source: CandidateNameSource::default(),
            outcome: ProjectionOutcome::Reviewable { shape, validation },
        }
    }

    pub fn isolated(
        source: CandidateSourceIdentity,
        category: CandidateCategory,
        display: CandidateDisplay,
        shape: CandidateShape,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(Error::CandidateBatchRejected(
                "隔离候选必须提供非空原因".to_owned(),
            ));
        }
        Ok(Self {
            source,
            category,
            display,
            name_source: CandidateNameSource::default(),
            outcome: ProjectionOutcome::Isolated {
                shape,
                validation: CandidateValidation::Rejected,
                reason,
            },
        })
    }

    /// 已通过几何验证，但因边界事实而隔离；资格与验证是两条独立事实轴。
    pub fn isolated_validated(
        source: CandidateSourceIdentity,
        category: CandidateCategory,
        display: CandidateDisplay,
        shape: CandidateShape,
        validation: ReviewableValidation,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(Error::CandidateBatchRejected(
                "隔离候选必须提供非空原因".to_owned(),
            ));
        }
        Ok(Self {
            source,
            category,
            display,
            name_source: CandidateNameSource::default(),
            outcome: ProjectionOutcome::Isolated {
                shape,
                validation: match validation {
                    ReviewableValidation::Retained => CandidateValidation::Retained,
                    ReviewableValidation::Repaired => CandidateValidation::Repaired,
                },
                reason,
            },
        })
    }

    pub fn with_name_source(mut self, source: CandidateNameSource) -> Self {
        self.name_source = source;
        self
    }
}

impl CandidateDisplay {
    pub fn new(title: impl Into<String>, tags: Vec<(String, String)>) -> Self {
        Self {
            title: title.into(),
            tags,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateProjection {
    pub candidate_id: String,
    pub plan_id: String,
    pub collection_batch_id: Option<String>,
    pub raw_observation_id: String,
    pub data_source_tag: String,
    pub source_entity_id: String,
    pub geometry_part_id: String,
    pub category: CandidateCategory,
    pub display: CandidateDisplay,
    pub shape: CandidateShape,
    validation: CandidateValidation,
    eligibility: CandidateEligibility,
    isolation_reason: Option<String>,
    automatically_repaired: bool,
    missing_in_latest_batch: bool,
    pub name_source: CandidateNameSource,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl CandidateProjection {
    pub fn validation(&self) -> CandidateValidation {
        self.validation
    }

    pub fn eligibility(&self) -> CandidateEligibility {
        self.eligibility
    }

    pub fn isolation_reason(&self) -> Option<&str> {
        self.isolation_reason.as_deref()
    }

    pub fn automatically_repaired(&self) -> bool {
        self.automatically_repaired
    }

    pub fn missing_in_latest_batch(&self) -> bool {
        self.missing_in_latest_batch
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBatch {
    pub id: String,
    pub plan_id: String,
    pub status: CandidateBatchStatus,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateBatchStatus {
    Building,
    Published,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBatchSummary {
    pub batch: CandidateBatch,
    pub projection_count: usize,
    pub reviewable_count: usize,
    pub isolated_count: usize,
    pub repaired_count: usize,
    pub missing_count: usize,
}

pub trait CandidateProjectionsApi {
    /// 从来源/验证事实构建并原子发布一个完整候选批次。
    ///
    /// module 内部复用稳定候选身份、只继承真正未出现的候选，并同时演进旧评审
    /// 决定与未封账草稿。失败时当前批次、当前决定和边界指纹均保持不变。
    fn publish_candidate_batch(
        &mut self,
        plan_id: &str,
        boundary_fingerprint: &str,
        drafts: &[CandidateProjectionDraft],
    ) -> Result<CandidateBatchSummary>;
    /// 把完整边界重验证事实发布为一个新 revision；调用方不传资格/作废/待定数组。
    fn publish_candidate_revalidation(
        &mut self,
        plan_id: &str,
        boundary_fingerprint: &str,
        facts: &[CandidateRevalidationFact],
    ) -> Result<crate::RevalidationWriteSummary>;
    fn list_reviewable_candidate_projections(
        &self,
        plan_id: &str,
    ) -> Result<Vec<CandidateProjection>>;
    /// 导出只读取“当前 Reviewable + 当前有效 Keep”的合法投影。
    fn list_kept_candidate_projections(&self, plan_id: &str) -> Result<Vec<CandidateProjection>>;
    /// 列出方案当前批次的全量候选投影（含隔离；边界重验证读全部）。
    fn list_current_candidate_projections(&self, plan_id: &str)
        -> Result<Vec<CandidateProjection>>;
    /// 当前完整候选批次的 revision；评审封账必须携带它以拒绝旧页面写回。
    fn current_candidate_batch_revision(&self, plan_id: &str) -> Result<Option<String>>;
    fn get_current_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>>;
    fn get_kept_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>>;
    fn candidate_batch_summary(&self, batch_id: &str) -> Result<CandidateBatchSummary>;
}
impl CandidateProjectionsApi for Database {
    fn publish_candidate_batch(
        &mut self,
        plan_id: &str,
        boundary_fingerprint: &str,
        drafts: &[CandidateProjectionDraft],
    ) -> Result<CandidateBatchSummary> {
        use std::collections::{HashMap, HashSet};

        let now = Utc::now();
        let now_db = timestamp_to_db(now);
        let batch = CandidateBatch {
            id: Uuid::new_v4().to_string(),
            plan_id: plan_id.to_owned(),
            status: CandidateBatchStatus::Published,
            created_at: now,
            published_at: Some(now),
        };

        let mut previous_by_source = HashMap::new();
        for projection in self.current_projections(plan_id)? {
            let identity = CandidateSourceIdentity::new(
                &projection.data_source_tag,
                &projection.source_entity_id,
                &projection.geometry_part_id,
            );
            if previous_by_source.insert(identity, projection).is_some() {
                return Err(Error::CandidateBatchRejected(
                    "当前批次存在重复来源身份，不能猜测稳定候选标识".to_owned(),
                ));
            }
        }

        let mut seen_sources = HashSet::new();
        let mut projections = Vec::with_capacity(drafts.len() + previous_by_source.len());
        let mut transitions = Vec::new();
        for draft in drafts {
            if !seen_sources.insert(draft.source.clone()) {
                return Err(Error::CandidateBatchRejected(format!(
                    "同一批次来源身份重复：{}/{}/{}",
                    draft.source.data_source_tag,
                    draft.source.source_entity_id,
                    draft.source.geometry_part_id
                )));
            }
            let raw_observation_id = self
                .conn
                .query_row(
                    "SELECT id FROM raw_observations
                     WHERE plan_id = ?1 AND entity_type = ?2 AND entity_id = ?3",
                    params![
                        plan_id,
                        category_to_db(draft.category)?,
                        draft.source.source_entity_id
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    Error::CandidateBatchRejected(format!(
                        "候选来源原始观测尚未正式保存：{}",
                        draft.source.source_entity_id
                    ))
                })?;
            let previous = previous_by_source.remove(&draft.source);
            let candidate_id = previous.as_ref().map_or_else(
                || stable_candidate_id(plan_id, &draft.source),
                |item| item.candidate_id.clone(),
            );
            let (shape, validation, eligibility, isolation_reason, automatically_repaired) =
                match &draft.outcome {
                    ProjectionOutcome::Reviewable { shape, validation } => (
                        shape.clone(),
                        match validation {
                            ReviewableValidation::Retained => CandidateValidation::Retained,
                            ReviewableValidation::Repaired => CandidateValidation::Repaired,
                        },
                        CandidateEligibility::Reviewable,
                        None,
                        matches!(validation, ReviewableValidation::Repaired),
                    ),
                    ProjectionOutcome::Isolated {
                        shape,
                        validation,
                        reason,
                    } => (
                        shape.clone(),
                        *validation,
                        CandidateEligibility::Isolated,
                        Some(reason.clone()),
                        *validation == CandidateValidation::Repaired,
                    ),
                };
            if let Some(previous) = &previous {
                let reappeared = previous.missing_in_latest_batch;
                let became_isolated = previous.eligibility == CandidateEligibility::Reviewable
                    && eligibility == CandidateEligibility::Isolated;
                let became_reviewable = previous.eligibility == CandidateEligibility::Isolated
                    && eligibility == CandidateEligibility::Reviewable;
                if reappeared || became_isolated || became_reviewable {
                    transitions.push(DecisionTransition {
                        candidate_id: candidate_id.clone(),
                        category: draft.category,
                        reason: if reappeared {
                            "reappeared_after_missing"
                        } else if became_isolated {
                            "candidate_became_isolated"
                        } else {
                            "candidate_became_reviewable"
                        },
                        reviewable_now: eligibility == CandidateEligibility::Reviewable,
                    });
                }
            }
            let (created_at, updated_at) = previous.as_ref().map_or((now, now), |previous| {
                let changed = previous.raw_observation_id != raw_observation_id
                    || previous.category != draft.category
                    || previous.display != draft.display
                    || previous.shape != shape
                    || previous.validation != validation
                    || previous.eligibility != eligibility
                    || previous.isolation_reason != isolation_reason
                    || previous.automatically_repaired != automatically_repaired
                    || previous.missing_in_latest_batch
                    || previous.name_source != draft.name_source;
                (
                    previous.created_at,
                    if changed { now } else { previous.updated_at },
                )
            });
            projections.push(CandidateProjection {
                candidate_id,
                plan_id: plan_id.to_owned(),
                collection_batch_id: Some(batch.id.clone()),
                raw_observation_id,
                data_source_tag: draft.source.data_source_tag.clone(),
                source_entity_id: draft.source.source_entity_id.clone(),
                geometry_part_id: draft.source.geometry_part_id.clone(),
                category: draft.category,
                display: draft.display.clone(),
                shape,
                validation,
                eligibility,
                isolation_reason,
                automatically_repaired,
                missing_in_latest_batch: false,
                name_source: draft.name_source,
                created_at,
                updated_at,
            });
        }

        for (_, mut missing) in previous_by_source {
            if !missing.missing_in_latest_batch {
                transitions.push(DecisionTransition {
                    candidate_id: missing.candidate_id.clone(),
                    category: missing.category,
                    reason: "missing_from_collection",
                    reviewable_now: missing.eligibility == CandidateEligibility::Reviewable,
                });
                missing.updated_at = now;
            }
            missing.collection_batch_id = Some(batch.id.clone());
            missing.missing_in_latest_batch = true;
            projections.push(missing);
        }
        projections.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO candidate_batches
                (id, plan_id, status, created_at, published_at)
             VALUES (?1, ?2, 'published', ?3, ?3)",
            params![batch.id, batch.plan_id, now_db],
        )?;
        for projection in &projections {
            insert_projection(&tx, &batch.id, projection)?;
        }
        for transition in &transitions {
            apply_decision_transition(&tx, plan_id, transition, &now_db)?;
        }
        tx.execute(
            "INSERT INTO current_candidate_batches (plan_id, collection_batch_id)
             VALUES (?1, ?2)
             ON CONFLICT(plan_id) DO UPDATE SET
                collection_batch_id = excluded.collection_batch_id",
            params![plan_id, batch.id],
        )?;
        tx.execute(
            "INSERT INTO plan_collection_boundary (plan_id, fingerprint, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_id) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                updated_at = excluded.updated_at",
            params![plan_id, boundary_fingerprint, now_db],
        )?;
        tx.commit()?;

        let projection_count = projections.len();
        let reviewable_count = projections
            .iter()
            .filter(|item| item.eligibility == CandidateEligibility::Reviewable)
            .count();
        let repaired_count = projections
            .iter()
            .filter(|item| item.automatically_repaired)
            .count();
        let missing_count = projections
            .iter()
            .filter(|item| item.missing_in_latest_batch)
            .count();
        Ok(CandidateBatchSummary {
            batch,
            projection_count,
            reviewable_count,
            isolated_count: projection_count - reviewable_count,
            repaired_count,
            missing_count,
        })
    }
    fn publish_candidate_revalidation(
        &mut self,
        plan_id: &str,
        boundary_fingerprint: &str,
        facts: &[CandidateRevalidationFact],
    ) -> Result<crate::RevalidationWriteSummary> {
        use std::collections::{HashMap, HashSet};

        let stored_fingerprint: Option<String> = self
            .conn
            .query_row(
                "SELECT fingerprint FROM plan_collection_boundary WHERE plan_id=?1",
                params![plan_id],
                |row| row.get(0),
            )
            .optional()?;
        if stored_fingerprint.as_deref() == Some(boundary_fingerprint) {
            return Ok(crate::RevalidationWriteSummary::default());
        }

        let now = Utc::now();
        let now_db = timestamp_to_db(now);
        let batch_id = Uuid::new_v4().to_string();
        let mut current: HashMap<_, _> = self
            .current_projections(plan_id)?
            .into_iter()
            .map(|projection| (projection.candidate_id.clone(), projection))
            .collect();
        let mut seen = HashSet::new();
        let mut projections = Vec::with_capacity(facts.len());
        let mut transitions = Vec::new();
        let mut eligibility_updated = 0usize;
        for fact in facts {
            if !seen.insert(fact.candidate_id.clone()) {
                return Err(Error::CandidateBatchRejected(format!(
                    "边界重验证候选重复：{}",
                    fact.candidate_id
                )));
            }
            let mut projection = current.remove(&fact.candidate_id).ok_or_else(|| {
                Error::CandidateBatchRejected(format!(
                    "边界重验证候选不属于当前批次：{}",
                    fact.candidate_id
                ))
            })?;
            let previous_shape = projection.shape.clone();
            let previous_validation = projection.validation;
            let previous_eligibility = projection.eligibility;
            let previous_reason = projection.isolation_reason.clone();
            let previous_automatically_repaired = projection.automatically_repaired;
            match &fact.outcome {
                ProjectionOutcome::Reviewable { shape, validation } => {
                    projection.shape = shape.clone();
                    projection.validation = match validation {
                        ReviewableValidation::Retained => CandidateValidation::Retained,
                        ReviewableValidation::Repaired => CandidateValidation::Repaired,
                    };
                    projection.eligibility = CandidateEligibility::Reviewable;
                    projection.isolation_reason = None;
                    projection.automatically_repaired =
                        matches!(validation, ReviewableValidation::Repaired);
                }
                ProjectionOutcome::Isolated {
                    shape,
                    validation,
                    reason,
                } => {
                    projection.shape = shape.clone();
                    projection.validation = *validation;
                    projection.eligibility = CandidateEligibility::Isolated;
                    projection.isolation_reason = Some(reason.clone());
                    projection.automatically_repaired =
                        *validation == CandidateValidation::Repaired;
                }
            }
            let projection_changed = previous_shape != projection.shape
                || previous_validation != projection.validation
                || previous_eligibility != projection.eligibility
                || previous_reason != projection.isolation_reason
                || previous_automatically_repaired != projection.automatically_repaired;
            if projection_changed {
                eligibility_updated += 1;
                projection.updated_at = now;
            }
            if previous_eligibility != projection.eligibility {
                transitions.push(DecisionTransition {
                    candidate_id: projection.candidate_id.clone(),
                    category: projection.category,
                    reason: if projection.eligibility == CandidateEligibility::Reviewable {
                        "candidate_became_reviewable_after_boundary_change"
                    } else {
                        "candidate_became_isolated_after_boundary_change"
                    },
                    reviewable_now: projection.eligibility == CandidateEligibility::Reviewable,
                });
            }
            projection.collection_batch_id = Some(batch_id.clone());
            projections.push(projection);
        }
        if !current.is_empty() {
            return Err(Error::CandidateBatchRejected(
                "边界重验证事实未覆盖当前完整候选批次".to_owned(),
            ));
        }
        projections.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO candidate_batches
                (id, plan_id, status, created_at, published_at)
             VALUES (?1, ?2, 'published', ?3, ?3)",
            params![batch_id, plan_id, now_db],
        )?;
        for projection in &projections {
            insert_projection(&tx, &batch_id, projection)?;
        }
        let mut decisions_voided = 0usize;
        let mut decisions_reset_to_pending = 0usize;
        for transition in &transitions {
            let effect = apply_decision_transition(&tx, plan_id, transition, &now_db)?;
            decisions_voided += effect.voided;
            decisions_reset_to_pending += effect.reset_to_pending;
        }
        tx.execute(
            "INSERT INTO current_candidate_batches (plan_id, collection_batch_id)
             VALUES (?1, ?2)
             ON CONFLICT(plan_id) DO UPDATE SET
                collection_batch_id = excluded.collection_batch_id",
            params![plan_id, batch_id],
        )?;
        tx.execute(
            "INSERT INTO plan_collection_boundary (plan_id, fingerprint, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_id) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                updated_at = excluded.updated_at",
            params![plan_id, boundary_fingerprint, now_db],
        )?;
        tx.commit()?;
        Ok(crate::RevalidationWriteSummary {
            eligibility_updated,
            decisions_voided,
            decisions_reset_to_pending,
        })
    }
    fn list_reviewable_candidate_projections(
        &self,
        plan_id: &str,
    ) -> Result<Vec<CandidateProjection>> {
        self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at,p.name_source FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1 AND p.eligibility='reviewable' ORDER BY p.candidate_id",params![plan_id])
    }
    fn list_kept_candidate_projections(&self, plan_id: &str) -> Result<Vec<CandidateProjection>> {
        let projections = self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at,p.name_source FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id JOIN review_decisions d ON d.plan_id=p.plan_id AND d.candidate_id=p.candidate_id WHERE c.plan_id=?1 AND p.eligibility='reviewable' AND p.missing_in_latest_batch=0 AND d.review_state='keep' AND d.voided=0 ORDER BY p.candidate_id",params![plan_id])?;
        let kept_decision_count: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM review_decisions
             WHERE plan_id=?1 AND review_state='keep' AND voided=0",
            params![plan_id],
            |row| row.get(0),
        )?;
        if kept_decision_count != projections.len() {
            return Err(Error::CandidateBatchRejected(
                "有效保留决定与当前 Reviewable 投影不一致".to_owned(),
            ));
        }
        Ok(projections)
    }
    fn list_current_candidate_projections(
        &self,
        plan_id: &str,
    ) -> Result<Vec<CandidateProjection>> {
        self.current_projections(plan_id)
    }
    fn current_candidate_batch_revision(&self, plan_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT collection_batch_id FROM current_candidate_batches WHERE plan_id=?1",
                params![plan_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)
    }
    fn get_current_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>> {
        Ok(self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at,p.name_source FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id WHERE c.plan_id=?1 AND p.candidate_id=?2",params![plan_id,candidate_id])?.into_iter().next())
    }
    fn get_kept_candidate_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<CandidateProjection>> {
        let projection = self.read_projections("SELECT p.collection_batch_id,p.candidate_id,p.plan_id,p.raw_observation_id,p.data_source_tag,p.source_entity_id,p.geometry_part_id,p.category,p.display_title,p.display_tags,p.geometry_kind,p.normalized_geometry,p.validation,p.eligibility,p.isolation_reason,p.automatically_repaired,p.missing_in_latest_batch,p.created_at,p.updated_at,p.name_source FROM current_candidate_batches c JOIN candidate_projections p ON p.collection_batch_id=c.collection_batch_id JOIN review_decisions d ON d.plan_id=p.plan_id AND d.candidate_id=p.candidate_id WHERE c.plan_id=?1 AND p.candidate_id=?2 AND p.eligibility='reviewable' AND p.missing_in_latest_batch=0 AND d.review_state='keep' AND d.voided=0",params![plan_id,candidate_id])?.into_iter().next();
        let has_keep: bool = self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM review_decisions
                WHERE plan_id=?1 AND candidate_id=?2
                  AND review_state='keep' AND voided=0
            )",
            params![plan_id, candidate_id],
            |row| row.get(0),
        )?;
        if has_keep && projection.is_none() {
            return Err(Error::CandidateBatchRejected(format!(
                "保留候选 {candidate_id} 没有当前 Reviewable 投影"
            )));
        }
        Ok(projection)
    }
    fn candidate_batch_summary(&self, batch_id: &str) -> Result<CandidateBatchSummary> {
        let batch = self.batch(batch_id)?;
        let (all,reviewable,isolated,repaired,missing):(usize,usize,usize,usize,usize)=self.conn.query_row("SELECT COUNT(*),SUM(eligibility='reviewable'),SUM(eligibility='isolated'),SUM(automatically_repaired),SUM(missing_in_latest_batch) FROM candidate_projections WHERE collection_batch_id=?1",params![batch_id],|row| Ok((row.get(0)?,row.get::<_,Option<usize>>(1)?.unwrap_or(0),row.get::<_,Option<usize>>(2)?.unwrap_or(0),row.get::<_,Option<usize>>(3)?.unwrap_or(0),row.get::<_,Option<usize>>(4)?.unwrap_or(0))))?;
        Ok(CandidateBatchSummary {
            batch,
            projection_count: all,
            reviewable_count: reviewable,
            isolated_count: isolated,
            repaired_count: repaired,
            missing_count: missing,
        })
    }
}

struct DecisionTransition {
    candidate_id: String,
    category: CandidateCategory,
    reason: &'static str,
    reviewable_now: bool,
}

#[derive(Default)]
struct DecisionTransitionEffect {
    voided: usize,
    reset_to_pending: usize,
}

fn apply_decision_transition(
    tx: &Transaction<'_>,
    plan_id: &str,
    transition: &DecisionTransition,
    now_db: &str,
) -> Result<DecisionTransitionEffect> {
    let mut effect = DecisionTransitionEffect::default();
    let previous = tx
        .query_row(
            "SELECT review_state, voided FROM review_decisions
             WHERE plan_id = ?1 AND candidate_id = ?2",
            params![plan_id, transition.candidate_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?;
    if let Some((previous_state, false)) = &previous {
        if previous_state != "pending" {
            tx.execute(
                "INSERT INTO review_decision_invalidations
                    (plan_id, candidate_id, previous_state, reason, invalidated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    plan_id,
                    transition.candidate_id,
                    previous_state,
                    transition.reason,
                    now_db
                ],
            )?;
            effect.voided = 1;
        }
    }

    if transition.reviewable_now {
        tx.execute(
            "INSERT INTO review_decisions
                (plan_id, category, candidate_id, review_state, reviewer_id, updated_at,
                 voided, voided_reason, voided_at)
             VALUES (?1, ?2, ?3, 'pending', NULL, ?4, 0, NULL, NULL)
             ON CONFLICT(plan_id, candidate_id) DO UPDATE SET
                category = excluded.category,
                review_state = 'pending',
                reviewer_id = NULL,
                updated_at = excluded.updated_at,
                voided = 0,
                voided_reason = NULL,
                voided_at = NULL",
            params![
                plan_id,
                category_to_db(transition.category)?,
                transition.candidate_id,
                now_db
            ],
        )?;
        if previous.is_some() {
            effect.reset_to_pending = 1;
        }
    } else if previous.is_some() {
        tx.execute(
            "UPDATE review_decisions
             SET voided = 1,
                 voided_reason = ?3,
                 voided_at = ?4,
                 updated_at = ?4
             WHERE plan_id = ?1 AND candidate_id = ?2",
            params![plan_id, transition.candidate_id, transition.reason, now_db],
        )?;
    }

    tx.execute(
        "DELETE FROM review_draft_states WHERE plan_id = ?1 AND candidate_id = ?2",
        params![plan_id, transition.candidate_id],
    )?;
    tx.execute(
        "DELETE FROM review_draft_meta
         WHERE plan_id = ?1
           AND NOT EXISTS (
               SELECT 1 FROM review_draft_states WHERE plan_id = ?1
           )",
        params![plan_id],
    )?;
    Ok(effect)
}
