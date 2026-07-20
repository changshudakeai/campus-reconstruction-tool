use campus_state::{
    CoarseRasterDecision, CoarseRasterRunOutcome, FoundationBatchDecision, FoundationBatchReview,
    FoundationCandidateDecision, FoundationCategory, FoundationReviewBasis, InstallationId,
    ProviderOutcomeStatus, ReviewConflictResolution, Schema2Project, SourceGeometry,
    WidthProvenanceKind,
};
use campus_tool_protocol::{
    MapAreaGeometry, MapCoarseRasterDecision, MapCoarseRasterEvidence, MapCoordinate,
    MapEvidenceAssessment, MapFoundationBatchDecision, MapFoundationCandidateDecision,
    MapFoundationConflictResolution, MapFoundationReviewCandidate, MapFoundationReviewCategory,
    MapFoundationReviewDesk, MapKnownFeatureGap, MapProviderOutcome, MapReviewConflict, ToolEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FoundationReviewToolEventOutcome {
    SelectionChanged {
        category: FoundationCategory,
        subject_id: Option<String>,
    },
    ReviewUpdated {
        category: FoundationCategory,
    },
    CategoryCompleted {
        category: FoundationCategory,
    },
    Ignored,
}

pub(crate) fn map_foundation_review_desk(
    project: &Schema2Project,
    active_category: FoundationCategory,
    selected_subject_id: Option<&str>,
) -> Result<MapFoundationReviewDesk, String> {
    let queues = FoundationCategory::ALL
        .into_iter()
        .map(|category| project.foundation_review_queue(category))
        .collect::<Result<Vec<_>, _>>()?;
    let active = queues
        .iter()
        .find(|queue| queue.category == active_category)
        .ok_or("The selected Foundation category is unavailable")?;
    let categories = queues
        .iter()
        .map(|queue| MapFoundationReviewCategory {
            id: category_id(queue.category).into(),
            label: category_label(queue.category).into(),
            acquisition_state: acquisition_state(queue),
            disposed: queue.progress.disposed,
            total: queue.progress.total,
            pending: queue.progress.pending,
            blockers: queue.progress.unresolved_conflicts
                + queue.progress.unacknowledged_gaps
                + queue.progress.pending,
            complete: queue.progress.complete,
        })
        .collect();
    let candidates = active
        .items
        .iter()
        .map(|item| MapFoundationReviewCandidate {
            id: item.subject_id.clone(),
            label: item
                .subtype
                .as_ref()
                .map(|subtype| format!("{subtype} · {}", item.subject_id))
                .unwrap_or_else(|| item.subject_id.clone()),
            disposition: serde_json::to_value(&item.disposition)
                .ok()
                .and_then(|value| {
                    value
                        .get("state")
                        .and_then(|state| state.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "pending".into()),
            priority: item.assessment.priority.clone(),
            source_summary: item.source_summary.clone(),
            lineage_summary: item.lineage_summary.clone(),
            provenance_summary: item.provenance_summary.clone(),
            geometry_form: item.geometry_form.as_str().into(),
            subtype: item.subtype.clone(),
            width_summary: item.width.as_ref().map(|width| {
                let provenance = match width.provenance {
                    WidthProvenanceKind::Explicit => "explicit",
                    WidthProvenanceKind::RuleDerived => "rule-derived",
                    WidthProvenanceKind::StyleDefault => "style default",
                };
                width
                    .metres
                    .map(|metres| format!("{metres:.1} m · {provenance}"))
                    .unwrap_or_else(|| provenance.into())
            }),
            assessment: MapEvidenceAssessment {
                geometry: format!(
                    "{} · {}",
                    item.assessment.geometry.status, item.assessment.geometry.reason
                ),
                semantics: format!(
                    "{} · {}",
                    item.assessment.semantics.status, item.assessment.semantics.reason
                ),
                entity_match: format!(
                    "{} · {}",
                    item.assessment.entity_match.status, item.assessment.entity_match.reason
                ),
                name_match: format!(
                    "{} · {}",
                    item.assessment.name_match.status, item.assessment.name_match.reason
                ),
            },
            geometry: geometry_paths(&item.review_geometry),
        })
        .collect::<Vec<_>>();
    let selected_candidate_id = selected_subject_id
        .filter(|subject_id| candidates.iter().any(|item| item.id == *subject_id))
        .map(str::to_string)
        .or_else(|| candidates.first().map(|item| item.id.clone()));
    let current_bundle_id = project
        .pinned_evidence()
        .map(|evidence| evidence.acquisition.manifest.bundle.id.as_str());
    let current_gap_ids = active
        .known_gaps
        .iter()
        .map(|gap| gap.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut provider_outcomes = active
        .provider_outcomes
        .iter()
        .map(|outcome| MapProviderOutcome {
            provider: outcome.provider.clone(),
            tile_id: outcome.tile_id.clone(),
            state: provider_state(outcome.status).into(),
            summary: outcome
                .failure
                .as_ref()
                .map(|failure| format!("{} · {}", failure.explanation, failure.suggested_action))
                .unwrap_or_else(|| {
                    format!(
                        "{} raw · {} deduplicated",
                        outcome.raw_count, outcome.deduplicated_count
                    )
                }),
        })
        .collect::<Vec<_>>();
    provider_outcomes.extend(
        project
            .coarse_raster_runs()
            .iter()
            .filter(|run| {
                run.category == active_category
                    && Some(run.dataset_bundle_id.as_str()) == current_bundle_id
                    && current_gap_ids.contains(run.linked_gap_id.as_str())
            })
            .filter_map(|run| {
                let (kind, failure) = match &run.outcome {
                    CoarseRasterRunOutcome::ProviderFailure { failure } => {
                        ("provider-failure", failure)
                    }
                    CoarseRasterRunOutcome::UnusableCoverage { failure } => {
                        ("unusable-coverage", failure)
                    }
                    CoarseRasterRunOutcome::Proposals { .. } => return None,
                };
                Some(MapProviderOutcome {
                    provider: "coarse-raster".into(),
                    tile_id: run.linked_gap_id.clone(),
                    state: format!(
                        "{}-{}",
                        kind,
                        if failure.retryable {
                            "retryable"
                        } else {
                            "terminal"
                        }
                    ),
                    summary: format!(
                        "{} · {} · {}",
                        failure.code, failure.explanation, failure.suggested_action
                    ),
                })
            }),
    );
    Ok(MapFoundationReviewDesk {
        categories,
        active_category: category_id(active_category).into(),
        candidates,
        selected_candidate_id,
        provider_outcomes,
        known_gaps: active
            .known_gaps
            .iter()
            .map(|gap| MapKnownFeatureGap {
                id: gap.id.clone(),
                location_summary: gap.location.tile_id.clone(),
                attempted_evidence: gap.attempted_evidence.join(" · "),
                generation_impact: gap.generation_impact.clone(),
                status: format!("{:?}", gap.status),
                history_summary: gap
                    .history
                    .iter()
                    .map(|entry| format!("{entry:?}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
                acknowledged: gap.acknowledged,
            })
            .collect(),
        coarse_raster_evidence: project
            .coarse_raster_evidence(active_category)
            .into_iter()
            .map(|observation| MapCoarseRasterEvidence {
                id: observation.id.clone(),
                linked_gap_id: observation.linked_gap_id.clone(),
                label: format!("Coarse raster {:?} evidence", observation.subject),
                decision: match project.coarse_raster_decision(
                    active_category,
                    &observation.id,
                ) {
                    CoarseRasterDecision::Unresolved => "unresolved",
                    CoarseRasterDecision::Accepted => "accepted",
                    CoarseRasterDecision::Rejected { .. } => "rejected",
                }
                .into(),
                dataset_summary: format!(
                    "{} · {} · {}",
                    observation.source.provider,
                    observation.source.dataset_version,
                    observation.source.observed_at
                ),
                resolution_class_summary: format!(
                    "{} m · {}",
                    observation.source.native_resolution_metres, observation.source.class_label
                ),
                lineage_summary: format!(
                    "algorithm={} · vectorization={} · thresholds={:?} · min-component={} px · simplify={} m · CRS={} · affine={:?} · cloud={} · nodata={} · source-chunk={} · source-sha256={} · derived-sha256={} · licence={} ({}) · assessment={:?}",
                    observation.algorithm.algorithm_version,
                    observation.algorithm.vectorization_version,
                    observation.algorithm.thresholds,
                    observation.algorithm.minimum_component_pixels,
                    observation.algorithm.simplification_tolerance_metres,
                    observation.grid.crs,
                    observation.grid.affine_transform,
                    observation.grid.cloud_handling,
                    observation.grid.nodata_handling,
                    observation.source.source_chunk_id,
                    observation.source.source_sha256,
                    observation.derived_sha256,
                    observation.source.licence.identifier,
                    observation.source.licence.url,
                    observation.assessment
                ),
                exclusion_summary: if observation.exclusions.is_empty() {
                    "No cells excluded after boundary/gap clipping".into()
                } else {
                    observation
                        .exclusions
                        .iter()
                        .map(|exclusion| {
                            format!(
                                "{} cells · {:?} · {}",
                                exclusion.excluded_cell_count,
                                exclusion.reason,
                                exclusion.explanation
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                },
                assessment: MapEvidenceAssessment {
                    geometry: format!(
                        "{} · {}",
                        observation.assessment.geometry.status,
                        observation.assessment.geometry.reason
                    ),
                    semantics: format!(
                        "{} · {}",
                        observation.assessment.semantics.status,
                        observation.assessment.semantics.reason
                    ),
                    entity_match: format!(
                        "{} · {}",
                        observation.assessment.entity_match.status,
                        observation.assessment.entity_match.reason
                    ),
                    name_match: format!(
                        "{} · {}",
                        observation.assessment.name_match.status,
                        observation.assessment.name_match.reason
                    ),
                },
                approximate_geometry: area_geometry(&observation.approximate_geometry),
                warnings: observation
                    .warnings()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            })
            .collect(),
        conflicts: active
            .conflicts
            .iter()
            .map(|conflict| MapReviewConflict {
                id: conflict.id.clone(),
                kind: format!("{:?}", conflict.kind),
                explanation: conflict.explanation.clone(),
                subject_ids: conflict.subject_ids.clone(),
                resolved: active.resolved_conflict_ids.contains(&conflict.id),
            })
            .collect(),
        basis_token: serde_json::to_string(&active.basis).map_err(|error| error.to_string())?,
        ledger_sequence: active.ledger_sequence,
        completion_blocked_reason: (!active.progress.completion_blockers.is_empty())
            .then(|| active.progress.completion_blockers.join("; ")),
    })
}

pub(crate) fn apply_foundation_review_tool_event(
    project: &mut Schema2Project,
    actor: InstallationId,
    event: ToolEvent,
) -> Result<FoundationReviewToolEventOutcome, String> {
    match event {
        ToolEvent::MapFoundationReviewCategorySelected { category } => {
            Ok(FoundationReviewToolEventOutcome::SelectionChanged {
                category: parse_category(&category)?,
                subject_id: None,
            })
        }
        ToolEvent::MapFoundationReviewCandidateSelected {
            category,
            subject_id,
        } => {
            let category = parse_category(&category)?;
            let queue = project.foundation_review_queue(category)?;
            if !queue.items.iter().any(|item| item.subject_id == subject_id) {
                return Err("The selected queue item is no longer present".into());
            }
            Ok(FoundationReviewToolEventOutcome::SelectionChanged {
                category,
                subject_id: Some(subject_id),
            })
        }
        ToolEvent::MapFoundationReviewDecisionRequested {
            category,
            subject_id,
            decision,
        } => {
            let category = parse_category(&category)?;
            match decision {
                MapFoundationCandidateDecision::Accept => {
                    project.review_foundation_candidate(
                        category,
                        &subject_id,
                        FoundationCandidateDecision::Accept,
                        actor,
                    )?;
                }
                MapFoundationCandidateDecision::Reject => {
                    project.review_foundation_candidate(
                        category,
                        &subject_id,
                        FoundationCandidateDecision::Reject {
                            reason: "explicit queue rejection".into(),
                        },
                        actor,
                    )?;
                }
                MapFoundationCandidateDecision::Revoke => {
                    project.revoke_foundation_candidate_review(category, &subject_id, actor)?;
                }
            }
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapFoundationReviewDeferredRequested {
            category,
            subject_id,
            structured_reason,
            acknowledged_gap_id,
        } => {
            let category = parse_category(&category)?;
            project.review_foundation_candidate(
                category,
                &subject_id,
                FoundationCandidateDecision::Defer {
                    structured_reason,
                    acknowledged_gap_id,
                },
                actor,
            )?;
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapFoundationBatchReviewRequested {
            category,
            exact_subject_ids,
            basis_token,
            expected_ledger_sequence,
            decision,
        } => {
            let category = parse_category(&category)?;
            let expected_basis: FoundationReviewBasis = serde_json::from_str(&basis_token)
                .map_err(|_| {
                    "The batch review dependency basis token is invalid or stale".to_string()
                })?;
            project.batch_review_foundation(
                FoundationBatchReview {
                    category,
                    exact_subject_ids,
                    expected_basis,
                    expected_ledger_sequence,
                    decision: match decision {
                        MapFoundationBatchDecision::Accept => FoundationBatchDecision::Accept,
                        MapFoundationBatchDecision::Reject => FoundationBatchDecision::Reject,
                    },
                },
                actor,
            )?;
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
            category,
            gap_id,
            acknowledged,
        } => {
            let category = parse_category(&category)?;
            if acknowledged {
                project.acknowledge_feature_gap(category, &gap_id, actor)?;
            } else {
                project.reopen_feature_gap(category, &gap_id, actor)?;
            }
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapCoarseRasterDecisionRequested {
            category,
            observation_id,
            decision,
        } => {
            let category = parse_category(&category)?;
            project.review_coarse_raster_observation(
                category,
                &observation_id,
                match decision {
                    MapCoarseRasterDecision::Accept => CoarseRasterDecision::Accepted,
                    MapCoarseRasterDecision::Reject => CoarseRasterDecision::Rejected {
                        reason: "explicit coarse evidence rejection".into(),
                    },
                    MapCoarseRasterDecision::LeaveUnresolved => CoarseRasterDecision::Unresolved,
                },
                actor,
            )?;
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapFoundationConflictResolutionRequested {
            category,
            conflict_id,
            resolution,
        } => {
            let category = parse_category(&category)?;
            project.resolve_foundation_review_conflict(
                category,
                &conflict_id,
                match resolution {
                    MapFoundationConflictResolution::KeepSeparate => {
                        ReviewConflictResolution::KeepSeparate
                    }
                    MapFoundationConflictResolution::Grouping {
                        group_id,
                        primary_subject_id,
                        supporting_subject_ids,
                    } => ReviewConflictResolution::Grouping {
                        group_id,
                        primary_subject_id,
                        supporting_subject_ids,
                    },
                    MapFoundationConflictResolution::Containment {
                        container_id,
                        member_id,
                        container_generates_surface,
                    } => ReviewConflictResolution::Containment {
                        container_id,
                        member_id,
                        container_generates_surface,
                    },
                    MapFoundationConflictResolution::Naming {
                        subject_id,
                        display_name,
                        evidence_ids,
                    } => ReviewConflictResolution::Naming {
                        subject_id,
                        display_name,
                        evidence_ids,
                    },
                    MapFoundationConflictResolution::GeometryRepair {
                        subject_id,
                        review_geometry_sha256,
                    } => ReviewConflictResolution::GeometryRepair {
                        subject_id,
                        review_geometry_sha256,
                    },
                    MapFoundationConflictResolution::Attribute {
                        subject_id,
                        attribute,
                        provenance_ids,
                    } => ReviewConflictResolution::Attribute {
                        subject_id,
                        attribute,
                        provenance_ids,
                    },
                },
                actor,
            )?;
            Ok(FoundationReviewToolEventOutcome::ReviewUpdated { category })
        }
        ToolEvent::MapFoundationCategoryCompletionRequested { category } => {
            let category = parse_category(&category)?;
            project.complete_foundation_category(category, actor)?;
            Ok(FoundationReviewToolEventOutcome::CategoryCompleted { category })
        }
        _ => Ok(FoundationReviewToolEventOutcome::Ignored),
    }
}

pub(crate) fn category_id(category: FoundationCategory) -> &'static str {
    match category {
        FoundationCategory::Building => "building",
        FoundationCategory::Circulation => "circulation",
        FoundationCategory::Water => "water",
        FoundationCategory::Vegetation => "vegetation",
        FoundationCategory::Sports => "sports",
    }
}

fn category_label(category: FoundationCategory) -> &'static str {
    match category {
        FoundationCategory::Building => "Buildings",
        FoundationCategory::Circulation => "Circulation",
        FoundationCategory::Water => "Water",
        FoundationCategory::Vegetation => "Vegetation",
        FoundationCategory::Sports => "Sports",
    }
}

pub(crate) fn parse_category(value: &str) -> Result<FoundationCategory, String> {
    match value {
        "building" => Ok(FoundationCategory::Building),
        "circulation" => Ok(FoundationCategory::Circulation),
        "water" => Ok(FoundationCategory::Water),
        "vegetation" => Ok(FoundationCategory::Vegetation),
        "sports" => Ok(FoundationCategory::Sports),
        _ => Err("The Foundation review category is unknown".into()),
    }
}

fn acquisition_state(queue: &campus_state::FoundationReviewQueueProjection) -> String {
    if queue.provider_outcomes.is_empty() {
        return "unavailable".into();
    }
    let states = queue
        .provider_outcomes
        .iter()
        .map(|outcome| outcome.status)
        .collect::<Vec<_>>();
    if states
        .iter()
        .all(|state| *state == ProviderOutcomeStatus::Complete)
    {
        "complete".into()
    } else if states
        .iter()
        .all(|state| *state == ProviderOutcomeStatus::CompleteEmpty)
    {
        "complete-empty".into()
    } else if states.iter().any(|state| {
        matches!(
            state,
            ProviderOutcomeStatus::Partial
                | ProviderOutcomeStatus::Failed
                | ProviderOutcomeStatus::Cancelled
        )
    }) {
        "terminal-with-gaps".into()
    } else {
        "complete".into()
    }
}

fn provider_state(status: ProviderOutcomeStatus) -> &'static str {
    match status {
        ProviderOutcomeStatus::Complete => "complete",
        ProviderOutcomeStatus::CompleteEmpty => "complete-empty",
        ProviderOutcomeStatus::Partial => "partial",
        ProviderOutcomeStatus::Failed => "failed",
        ProviderOutcomeStatus::Cancelled => "cancelled",
    }
}

fn area_geometry(geometry: &SourceGeometry) -> MapAreaGeometry {
    fn ring(points: &[[f64; 2]]) -> Vec<MapCoordinate> {
        points
            .iter()
            .map(|point| {
                let gcj02 = campus_services::wgs84_to_gcj02(campus_state::GeoPoint {
                    lng: point[0],
                    lat: point[1],
                });
                MapCoordinate {
                    lng: gcj02.lng,
                    lat: gcj02.lat,
                }
            })
            .collect()
    }
    match geometry {
        SourceGeometry::Polygon(rings) => MapAreaGeometry::Polygon {
            rings: rings.iter().map(|points| ring(points)).collect(),
        },
        SourceGeometry::MultiPolygon(polygons) => MapAreaGeometry::MultiPolygon {
            polygons: polygons
                .iter()
                .map(|polygon| polygon.iter().map(|points| ring(points)).collect())
                .collect(),
        },
        _ => unreachable!("validated coarse raster geometry is always an area"),
    }
}
fn geometry_paths(geometry: &SourceGeometry) -> Vec<Vec<MapCoordinate>> {
    fn path(points: &[[f64; 2]]) -> Vec<MapCoordinate> {
        points
            .iter()
            .map(|point| {
                let gcj02 = campus_services::wgs84_to_gcj02(campus_state::GeoPoint {
                    lng: point[0],
                    lat: point[1],
                });
                MapCoordinate {
                    lng: gcj02.lng,
                    lat: gcj02.lat,
                }
            })
            .collect()
    }
    match geometry {
        SourceGeometry::Point(point) => vec![path(std::slice::from_ref(point))],
        SourceGeometry::MultiPoint(points) | SourceGeometry::LineString(points) => {
            vec![path(points)]
        }
        SourceGeometry::MultiLineString(lines) | SourceGeometry::Polygon(lines) => {
            lines.iter().map(|line| path(line)).collect()
        }
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter().map(|ring| path(ring)))
            .collect(),
    }
}

pub(crate) fn map_confirmed_boundary(project: &Schema2Project) -> Vec<MapCoordinate> {
    project
        .pinned_evidence()
        .and_then(|evidence| evidence.boundary.confirmed_geometry())
        .and_then(|geometry| geometry_paths(geometry).into_iter().next())
        .unwrap_or_default()
}
