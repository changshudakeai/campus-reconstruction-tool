//! 边界变化后的本地候选资格重验证（D 工单，不联网）。
//!
//! 触发条件：用户确认的边界与"上次采集时使用的边界"指纹不同。尚无记录时
//! 由调用方选择策略（[`LegacyPolicy`]）：边界确认路径只补记当前边界指纹
//! （旧数据/首次确认不重验证，避免历史投影被误隔离）；评审台进入路径则
//! 对旧数据也执行一次本地重验证，使评审台按当前已确认边界鉴别（随后补记
//! 指纹，边界未变化不再重复计算）。
//! 计算完全本地：原始观测 `source_data.source_geometry` + 已确认边界
//! 复用 F4 的 `parse_boundary` / `boundary_disposition` 纯函数与 B14 形状校验；
//! 不调用 `fetch_raw_entities`，网络请求数为 0。
//!
//! 结果单事务落库（B2）：候选投影资格/隔离原因、旧评审决定作废标注 +
//! 作废历史、新进入候选回到待定、最新边界指纹。

use data_acquisition::{boundary_disposition, parse_boundary, SourceGeometry};
use data_persistence::{
    boundary_fingerprint, BoundaryRevalidationApi, CandidateEligibility, CandidateProjectionsApi,
    CandidateRevalidationFact, CandidateShape, CandidateValidation, Database, RawObservationsApi,
    ReviewableValidation,
};
use geometry_validator::{
    CandidateGeometry, GeometryShape, GeometryValidator, ValidationDisposition,
};
use shared_domain_types::{Boundary, PlanId};

use crate::error::{CollectionError, Result};

/// 旧数据（无“上次采集时使用的边界”指纹记录）的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyPolicy {
    /// 只补记当前边界指纹，不重验证（边界确认路径的保守默认：避免历史投影
    /// 因缺 source_geometry 被误隔离）。
    RecordOnly,
    /// 无记录也执行一次本地重验证（评审台进入路径：确保按当前已确认边界
    /// 鉴别，使评审台与 B2 一致；随后即补记指纹，边界未变化不再重复计算）。
    Revalidate,
}

/// 一次边界重验证的结果报告（含触发条件与逐类变化数量）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryRevalidationReport {
    /// 是否真的发生了边界变化（false = 指纹相同，未做任何计算/写库）。
    pub boundary_changed: bool,
    /// 当前批次候选投影总数（重验证扫描范围）。
    pub examined: usize,
    /// 新边界下跑到边界外的候选（Reviewable → Isolated；旧评审决定作废）。
    pub newly_isolated: Vec<String>,
    /// 新边界下新进入的候选（Isolated → Reviewable；回到待定）。
    pub newly_reviewable: Vec<String>,
    /// 资格与隔离原因均未变化的候选数。
    pub unchanged: usize,
    /// 被作废标注的旧评审决定数。
    pub decisions_voided: usize,
    /// 被重置回"待定"的评审决定数。
    pub decisions_reset_to_pending: usize,
    /// 写入的当前边界指纹。
    pub boundary_fingerprint: String,
}

/// 与 F4 采集一致的隔离原因映射（`projection_for` 同源语义）。
fn isolation_reason(disposition: data_acquisition::BoundaryDisposition) -> &'static str {
    match disposition {
        data_acquisition::BoundaryDisposition::Outside => "outside_confirmed_plan_boundary",
        data_acquisition::BoundaryDisposition::Crosses => "crosses_confirmed_plan_boundary",
        data_acquisition::BoundaryDisposition::Invalid => "invalid_source_geometry",
        data_acquisition::BoundaryDisposition::Inside => "unreachable",
    }
}

/// 来源几何 → B14 待验证几何（与采集 `candidate_geometry` 同构；不伪造形状）。
fn geometry_for_revalidation(
    observation_id: &str,
    geometry: &SourceGeometry,
) -> Option<CandidateGeometry> {
    match geometry {
        SourceGeometry::Point(point) => Some(CandidateGeometry::with_shape(
            observation_id.to_owned(),
            GeometryShape::Point(*point),
        )),
        SourceGeometry::LineString(points) => Some(CandidateGeometry::with_shape(
            observation_id.to_owned(),
            GeometryShape::LineString(points.clone()),
        )),
        SourceGeometry::Polygon(points) => {
            let mut candidate =
                CandidateGeometry::with_shape(observation_id.to_owned(), GeometryShape::Polygon);
            candidate.coordinates = points.clone();
            Some(candidate)
        }
        _ => None,
    }
}

/// 执行一次本地边界重验证并单事务写库；指纹相同时不触发任何计算。
pub(crate) fn run_boundary_revalidation(
    db: &mut Database,
    validator: &GeometryValidator,
    plan_id: &PlanId,
    boundary: &Boundary,
    legacy_policy: LegacyPolicy,
) -> Result<BoundaryRevalidationReport> {
    let fingerprint = boundary_fingerprint(boundary);
    let stored = db.load_plan_collection_boundary(&plan_id.to_string())?;
    let mut stored = stored;
    if stored.is_none() {
        // 没有记录（旧数据或首次确认）：只补记当前边界指纹，不触发重验证，
        // 避免把历史投影（可能缺 source_geometry）误判为隔离。
        match legacy_policy {
            LegacyPolicy::RecordOnly => {
                db.save_plan_collection_boundary(&plan_id.to_string(), &fingerprint)?;
                return Ok(BoundaryRevalidationReport {
                    boundary_changed: false,
                    examined: 0,
                    newly_isolated: Vec::new(),
                    newly_reviewable: Vec::new(),
                    unchanged: 0,
                    decisions_voided: 0,
                    decisions_reset_to_pending: 0,
                    boundary_fingerprint: fingerprint,
                });
            }
            // 评审台进入：旧数据也按当前已确认边界鉴别（补记将在事务写入时完成）。
            LegacyPolicy::Revalidate => {
                stored = Some(String::new());
            }
        }
    }
    if stored.as_deref() == Some(fingerprint.as_str()) {
        return Ok(BoundaryRevalidationReport {
            boundary_changed: false,
            examined: 0,
            newly_isolated: Vec::new(),
            newly_reviewable: Vec::new(),
            unchanged: 0,
            decisions_voided: 0,
            decisions_reset_to_pending: 0,
            boundary_fingerprint: fingerprint,
        });
    }

    let multipolygon = parse_boundary(boundary).ok_or(CollectionError::InvalidBoundary)?;
    let projections = db.list_current_candidate_projections(&plan_id.to_string())?;
    let observations = db.list_raw_observations(&plan_id.to_string())?;
    let observations_by_id: std::collections::HashMap<_, _> = observations
        .iter()
        .map(|observation| (observation.id.as_str(), observation))
        .collect();

    let mut facts = Vec::with_capacity(projections.len());
    let mut newly_isolated = Vec::new();
    let mut newly_reviewable = Vec::new();
    let mut unchanged = 0usize;

    for projection in &projections {
        let geometry = observations_by_id
            .get(projection.raw_observation_id.as_str())
            .and_then(|observation| SourceGeometry::from_source_data(&observation.source_data));
        let disposition = boundary_disposition(geometry.as_ref(), &multipolygon);
        let new_eligibility;
        let new_reason;
        let fact;
        match disposition {
            data_acquisition::BoundaryDisposition::Inside => {
                if projection.eligibility() == CandidateEligibility::Reviewable {
                    let validation = if projection.validation() == CandidateValidation::Repaired {
                        ReviewableValidation::Repaired
                    } else {
                        ReviewableValidation::Retained
                    };
                    facts.push(CandidateRevalidationFact::reviewable(
                        projection.candidate_id.clone(),
                        projection.shape.clone(),
                        validation,
                    ));
                    unchanged += 1;
                    continue;
                }
                // 新进入边界：按 B14 形状校验决定 Reviewable 或继续隔离。
                let candidate_geometry = geometry
                    .as_ref()
                    .and_then(|value| geometry_for_revalidation(&projection.candidate_id, value));
                let outcome = candidate_geometry.and_then(|candidate| {
                    let validation = validator.validate_batch(vec![candidate]);
                    validation
                        .outcomes
                        .into_iter()
                        .find(|item| item.candidate_id == projection.candidate_id)
                });
                match outcome {
                    Some(item)
                        if matches!(
                            item.disposition,
                            ValidationDisposition::Retained | ValidationDisposition::Repaired
                        ) =>
                    {
                        new_eligibility = CandidateEligibility::Reviewable;
                        new_reason = None;
                        let validation =
                            if matches!(item.disposition, ValidationDisposition::Repaired) {
                                ReviewableValidation::Repaired
                            } else {
                                ReviewableValidation::Retained
                            };
                        let shape = item
                            .geometry
                            .as_ref()
                            .map(shape_from_candidate)
                            .unwrap_or_else(|| projection.shape.clone());
                        fact = CandidateRevalidationFact::reviewable(
                            projection.candidate_id.clone(),
                            shape,
                            validation,
                        );
                    }
                    Some(_) | None => {
                        new_eligibility = CandidateEligibility::Isolated;
                        new_reason = Some("invalid_source_geometry".to_owned());
                        fact = CandidateRevalidationFact::isolated(
                            projection.candidate_id.clone(),
                            projection.shape.clone(),
                            "invalid_source_geometry",
                        )
                        .expect("静态非空隔离原因");
                    }
                }
                if new_eligibility == CandidateEligibility::Reviewable {
                    newly_reviewable.push(projection.candidate_id.clone());
                }
            }
            data_acquisition::BoundaryDisposition::Outside
            | data_acquisition::BoundaryDisposition::Crosses => {
                new_eligibility = CandidateEligibility::Isolated;
                new_reason = Some(isolation_reason(disposition).to_owned());
                fact = match projection.validation() {
                    CandidateValidation::Retained => CandidateRevalidationFact::isolated_validated(
                        projection.candidate_id.clone(),
                        projection.shape.clone(),
                        ReviewableValidation::Retained,
                        isolation_reason(disposition),
                    ),
                    CandidateValidation::Repaired => CandidateRevalidationFact::isolated_validated(
                        projection.candidate_id.clone(),
                        projection.shape.clone(),
                        ReviewableValidation::Repaired,
                        isolation_reason(disposition),
                    ),
                    CandidateValidation::Rejected => CandidateRevalidationFact::isolated(
                        projection.candidate_id.clone(),
                        projection.shape.clone(),
                        isolation_reason(disposition),
                    ),
                }
                .expect("静态非空隔离原因");
                if projection.eligibility() == CandidateEligibility::Reviewable {
                    newly_isolated.push(projection.candidate_id.clone());
                } else if projection.isolation_reason() != new_reason.as_deref() {
                    // 仍在边界外/相交：隔离原因可能随新边界细化（如 outside→crosses）。
                } else {
                    facts.push(fact);
                    unchanged += 1;
                    continue;
                }
            }
            data_acquisition::BoundaryDisposition::Invalid => {
                new_eligibility = CandidateEligibility::Isolated;
                new_reason = Some(isolation_reason(disposition).to_owned());
                fact = CandidateRevalidationFact::isolated(
                    projection.candidate_id.clone(),
                    projection.shape.clone(),
                    isolation_reason(disposition),
                )
                .expect("静态非空隔离原因");
                if projection.eligibility() == CandidateEligibility::Reviewable {
                    newly_isolated.push(projection.candidate_id.clone());
                } else if projection.isolation_reason() == new_reason.as_deref()
                    && projection.validation() == CandidateValidation::Rejected
                {
                    facts.push(fact);
                    unchanged += 1;
                    continue;
                }
            }
        }
        let _ = (new_eligibility, new_reason);
        facts.push(fact);
    }

    let summary = db.publish_candidate_revalidation(&plan_id.to_string(), &fingerprint, &facts)?;
    Ok(BoundaryRevalidationReport {
        boundary_changed: true,
        examined: projections.len(),
        newly_isolated,
        newly_reviewable,
        unchanged,
        decisions_voided: summary.decisions_voided,
        decisions_reset_to_pending: summary.decisions_reset_to_pending,
        boundary_fingerprint: fingerprint,
    })
}

fn shape_from_candidate(geometry: &CandidateGeometry) -> CandidateShape {
    match &geometry.shape {
        GeometryShape::Point(_) => CandidateShape::point(serde_json::json!(geometry.coordinates)),
        GeometryShape::LineString(_) => {
            CandidateShape::line_string(serde_json::json!(geometry.coordinates))
        }
        GeometryShape::Polygon => CandidateShape::polygon(serde_json::json!(geometry.coordinates)),
        _ => CandidateShape::point(serde_json::json!([])),
    }
}
