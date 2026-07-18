use campus_services::acquisition::{
    BoundaryCandidateValidity as ServiceBoundaryCandidateValidity,
    VerifiedBoundaryDiscoverySnapshot,
};
use campus_state::{
    BoundaryCandidateAssessment, BoundaryCandidateDerivation, BoundaryCandidateValidity,
    BoundaryDiscoverySnapshot, BoundaryEvidenceAvailability, BoundaryEvidenceDesk, SourceGeometry,
};
use campus_tool_protocol::{MapBoundaryCandidate, MapBoundaryDesk, MapCoordinate};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;

pub(crate) fn evidence_desk_from_verified_snapshot(
    snapshot: &VerifiedBoundaryDiscoverySnapshot,
) -> Result<BoundaryEvidenceDesk, String> {
    let assessments = snapshot
        .candidates
        .iter()
        .map(|candidate| {
            let validity = match snapshot.validity.get(&candidate.id) {
                Some(ServiceBoundaryCandidateValidity::Valid) => BoundaryCandidateValidity::Valid,
                Some(ServiceBoundaryCandidateValidity::Invalid { reasons }) => {
                    BoundaryCandidateValidity::Invalid {
                        reasons: reasons.clone(),
                    }
                }
                None => BoundaryCandidateValidity::Invalid {
                    reasons: vec![
                        "The controlled service omitted candidate validity evidence".into()
                    ],
                },
            };
            let derivation = snapshot
                .derivations
                .get(&candidate.id)
                .map(|value| BoundaryCandidateDerivation {
                    source_records: value.source_records.clone(),
                    rule_versions: value.rule_versions.clone(),
                    steps: value.steps.clone(),
                })
                .unwrap_or_default();
            (
                candidate.id.clone(),
                BoundaryCandidateAssessment {
                    validity,
                    derivation,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    BoundaryEvidenceDesk::new(BoundaryDiscoverySnapshot {
        manifest: copy_typed(&snapshot.manifest)?,
        candidates: copy_typed(&snapshot.candidates)?,
        assessments,
    })
}

pub(crate) fn map_boundary_desk(desk: &BoundaryEvidenceDesk) -> Option<MapBoundaryDesk> {
    let snapshot = desk.snapshot()?;
    let projection = desk.projection();
    let mut candidates = snapshot
        .candidates
        .iter()
        .map(|candidate| {
            let assessment = snapshot.assessments.get(&candidate.id);
            let (valid, invalid_reasons) = match assessment.map(|value| &value.validity) {
                Some(BoundaryCandidateValidity::Valid) => (true, Vec::new()),
                Some(BoundaryCandidateValidity::Invalid { reasons }) => (false, reasons.clone()),
                None => (
                    false,
                    vec!["Candidate validity evidence is unavailable".into()],
                ),
            };
            let source_summary = format!(
                "{} · {} · {}",
                candidate.lineage.provider,
                candidate.lineage.source_record_id,
                candidate.lineage.dataset_release
            );
            let ranking = &candidate.ranking_evidence;
            let ranking_summary = format!(
                "name {:.0}% · distance {:.1} m · anchor {} · area {:.0} m²",
                ranking.name_match * 100.0,
                ranking.distance_m,
                if ranking.contains_anchor {
                    "contained"
                } else {
                    "outside"
                },
                ranking.area_m2
            );
            let lineage_summary = assessment
                .map(|value| value.derivation.steps.join(" · "))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| candidate.lineage.original_classification.clone());
            MapBoundaryCandidate {
                id: candidate.id.clone(),
                rank: candidate.rank,
                label: format!(
                    "{} boundary {}",
                    candidate.lineage.provider.to_ascii_uppercase(),
                    candidate.rank
                ),
                valid,
                invalid_reasons,
                points: editable_outer_ring(&candidate.geometry),
                source_summary,
                ranking_summary,
                lineage_summary,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.rank);
    let complete = snapshot
        .manifest
        .coverage_report
        .outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.status,
                campus_state::ProviderOutcomeStatus::Complete
                    | campus_state::ProviderOutcomeStatus::CompleteEmpty
            )
        })
        .count();
    let total = snapshot.manifest.coverage_report.outcomes.len();
    Some(MapBoundaryDesk {
        candidates,
        selected_candidate_id: desk.selected_candidate_id().map(str::to_string),
        dataset_bundle_summary: format!(
            "{} · OSM {} · Overture {}",
            snapshot.manifest.bundle.id,
            snapshot.manifest.bundle.osm_snapshot,
            snapshot.manifest.bundle.overture_release
        ),
        coverage_summary: format!("{complete}/{total} provider scopes complete"),
        confirmation_blocked_reason: projection.confirmation_blocked_reason,
        recovery_message: (projection.availability != BoundaryEvidenceAvailability::Ready).then(
            || {
                projection.actionable_feedback.unwrap_or_else(|| {
                    "No valid automatic Campus Boundary candidate is available".into()
                })
            },
        ),
    })
}

fn editable_outer_ring(geometry: &SourceGeometry) -> Vec<MapCoordinate> {
    let ring = match geometry {
        SourceGeometry::Polygon(rings) => rings.first(),
        SourceGeometry::MultiPolygon(polygons) => {
            polygons.first().and_then(|polygon| polygon.first())
        }
        _ => None,
    };
    ring.into_iter()
        .flatten()
        .copied()
        .map(|coordinate| {
            let gcj02 = campus_services::wgs84_to_gcj02(campus_state::GeoPoint {
                lng: coordinate[0],
                lat: coordinate[1],
            });
            MapCoordinate {
                lng: gcj02.lng,
                lat: gcj02.lat,
            }
        })
        .collect()
}

fn copy_typed<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
