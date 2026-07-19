use campus_services::acquisition::{
    BoundaryCandidateValidity as ServiceBoundaryCandidateValidity,
    VerifiedBoundaryDiscoverySnapshot,
};
use campus_state::{
    BoundaryCandidateAssessment, BoundaryCandidateDerivation, BoundaryCandidateValidity,
    BoundaryDiscoverySnapshot, BoundaryEdgeRef, BoundaryEvidenceAvailability, BoundaryEvidenceDesk,
    BoundaryVertexRef, InstallationId, Schema2Project, SourceGeometry,
};
use campus_tool_protocol::{
    MapBoundaryCandidate, MapBoundaryDesk, MapBoundaryEditOperation, MapCoordinate, ToolEvent,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoundaryToolEventOutcome {
    ReviewUpdated,
    AcquisitionQueued,
    AcquisitionStarted,
    RetryRequested,
    ReturnToCampusTargetRequested,
    Ignored,
}

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

pub(crate) fn map_boundary_desk(desk: &BoundaryEvidenceDesk) -> MapBoundaryDesk {
    let projection = desk.projection();
    let mut candidates = desk
        .snapshot()
        .map(|snapshot| {
            snapshot
                .candidates
                .iter()
                .map(|candidate| {
                    let assessment = snapshot.assessments.get(&candidate.id);
                    let invalid_reasons = match desk.candidate_validity(&candidate.id) {
                        BoundaryCandidateValidity::Valid => Vec::new(),
                        BoundaryCandidateValidity::Invalid { reasons } => reasons,
                    };
                    let valid = invalid_reasons.is_empty();
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
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    candidates.sort_by_key(|candidate| candidate.rank);
    let (dataset_bundle_summary, coverage_summary) = desk
        .snapshot()
        .map(|snapshot| {
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
            (
                format!(
                    "{} · OSM {} · Overture {}",
                    snapshot.manifest.bundle.id,
                    snapshot.manifest.bundle.osm_snapshot,
                    snapshot.manifest.bundle.overture_release
                ),
                format!(
                    "{complete}/{} provider scopes complete",
                    snapshot.manifest.coverage_report.outcomes.len()
                ),
            )
        })
        .unwrap_or_else(|| {
            (
                "Dataset Bundle unavailable".into(),
                "Boundary discovery did not return a coverage snapshot".into(),
            )
        });
    MapBoundaryDesk {
        candidates,
        selected_candidate_id: desk.selected_candidate_id().map(str::to_string),
        working_points: desk
            .working_geometry()
            .map(editable_outer_ring)
            .unwrap_or_default(),
        can_undo: projection.can_undo,
        dataset_bundle_summary,
        coverage_summary,
        confirmation_blocked_reason: projection.confirmation_blocked_reason,
        recovery_message: (projection.availability != BoundaryEvidenceAvailability::Ready).then(
            || {
                projection.actionable_feedback.unwrap_or_else(|| {
                    "No valid automatic Campus Boundary candidate is available".into()
                })
            },
        ),
    }
}

pub(crate) fn apply_boundary_tool_event(
    project: &mut Schema2Project,
    acquisition_idempotency_key: &str,
    actor: InstallationId,
    event: ToolEvent,
) -> Result<BoundaryToolEventOutcome, String> {
    match event {
        ToolEvent::MapBoundaryCandidateSelected { candidate_id } => {
            project.edit_boundary_review(actor, |desk| desk.select_candidate(&candidate_id))?;
            Ok(BoundaryToolEventOutcome::ReviewUpdated)
        }
        ToolEvent::MapBoundaryOperation {
            candidate_id,
            operation,
        } => {
            project.edit_boundary_review(actor, |desk| {
                require_selected_candidate(desk, &candidate_id)?;
                apply_boundary_operation(desk, operation)
            })?;
            Ok(BoundaryToolEventOutcome::ReviewUpdated)
        }
        ToolEvent::MapBoundaryConfirmed { candidate_id } => {
            let evidence = {
                let desk = project
                    .boundary_review()
                    .ok_or("Load automatic Campus Boundary evidence before confirming it")?;
                require_selected_candidate(desk, &candidate_id)?;
                desk.to_pinned_evidence()?
            };
            project.confirm_boundary_and_queue_acquisition(
                evidence,
                acquisition_idempotency_key,
                actor,
            )?;
            Ok(BoundaryToolEventOutcome::AcquisitionQueued)
        }
        ToolEvent::MapBoundaryRetryRequested => Ok(BoundaryToolEventOutcome::RetryRequested),
        ToolEvent::MapBoundaryReturnToCampusRequested => {
            Ok(BoundaryToolEventOutcome::ReturnToCampusTargetRequested)
        }
        ToolEvent::Error { message } => {
            if project.boundary_review().is_some() {
                project.edit_boundary_review(actor, |desk| {
                    desk.report_tool_failure(
                        format!("Campus Boundary map helper failed: {message}"),
                        "Retry the same boundary review or return to Campus Target confirmation",
                    );
                    Ok(())
                })?;
                Ok(BoundaryToolEventOutcome::ReviewUpdated)
            } else {
                Ok(BoundaryToolEventOutcome::Ignored)
            }
        }
        ToolEvent::Closed { .. } => {
            if project.boundary_review().is_some() {
                project.edit_boundary_review(actor, |desk| {
                    desk.report_tool_failure(
                        "Campus Boundary map helper closed before confirmation",
                        "Retry the same boundary review or return to Campus Target confirmation",
                    );
                    Ok(())
                })?;
                Ok(BoundaryToolEventOutcome::ReviewUpdated)
            } else {
                Ok(BoundaryToolEventOutcome::Ignored)
            }
        }
        _ => Ok(BoundaryToolEventOutcome::Ignored),
    }
}

fn require_selected_candidate(
    desk: &BoundaryEvidenceDesk,
    candidate_id: &str,
) -> Result<(), String> {
    if desk.selected_candidate_id() == Some(candidate_id) {
        Ok(())
    } else {
        Err("The map event does not match the selected Campus Boundary candidate".into())
    }
}

fn apply_boundary_operation(
    desk: &mut BoundaryEvidenceDesk,
    operation: MapBoundaryEditOperation,
) -> Result<(), String> {
    match operation {
        MapBoundaryEditOperation::MoveVertex {
            vertex_index,
            coordinate,
        } => {
            let wgs84 = campus_services::gcj02_to_wgs84(campus_state::GeoPoint {
                lng: coordinate.lng,
                lat: coordinate.lat,
            });
            desk.enter_adjustment()?;
            desk.select_vertex(BoundaryVertexRef::outer(0, vertex_index as usize))?;
            desk.move_selected_vertex([wgs84.lng, wgs84.lat])?;
            desk.leave_adjustment();
            Ok(())
        }
        MapBoundaryEditOperation::InsertVertex { edge_index } => {
            desk.enter_adjustment()?;
            desk.select_edge(BoundaryEdgeRef::outer(0, edge_index as usize))?;
            desk.insert_vertex_on_selected_edge()?;
            desk.leave_adjustment();
            Ok(())
        }
        MapBoundaryEditOperation::DeleteVertex { vertex_index } => {
            desk.enter_adjustment()?;
            desk.select_vertex(BoundaryVertexRef::outer(0, vertex_index as usize))?;
            desk.delete_selected_vertex()?;
            desk.leave_adjustment();
            Ok(())
        }
        MapBoundaryEditOperation::Undo => desk.undo_adjustment(),
        MapBoundaryEditOperation::RestoreCandidateOriginal => {
            desk.enter_adjustment()?;
            desk.restore_candidate_original()?;
            desk.leave_adjustment();
            Ok(())
        }
    }
}

fn editable_outer_ring(geometry: &SourceGeometry) -> Vec<MapCoordinate> {
    let ring = match geometry {
        SourceGeometry::Polygon(rings) => rings.first(),
        SourceGeometry::MultiPolygon(polygons) => {
            polygons.first().and_then(|polygon| polygon.first())
        }
        _ => None,
    };
    let ring = ring.map(Vec::as_slice).unwrap_or_default();
    let editable = if ring.len() > 1 && ring.first() == ring.last() {
        &ring[..ring.len() - 1]
    } else {
        ring
    };
    editable
        .iter()
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
