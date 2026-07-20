use crate::{
    AttributeDerivation, AttributeProvenance, FoundationCategory, FoundationReviewBasis,
    GeometryDerivationRecord, LicenceRecord, ProviderOutcome, SourceGeometry, SourceLineage,
    SourceObservation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CandidateReviewDisposition {
    Pending,
    Accepted,
    Rejected,
    SupportingEvidence {
        primary_subject_id: String,
    },
    Deferred {
        structured_reason: String,
        acknowledged_gap_id: String,
    },
}

impl CandidateReviewDisposition {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending)
    }

    pub fn enters_reviewed_model(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum FoundationCandidateDecision {
    Accept,
    Reject {
        #[serde(default)]
        reason: String,
    },
    SupportingEvidence {
        primary_subject_id: String,
    },
    Defer {
        structured_reason: String,
        acknowledged_gap_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationBatchDecision {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationBatchReview {
    pub category: FoundationCategory,
    pub exact_subject_ids: Vec<String>,
    pub expected_basis: FoundationReviewBasis,
    pub expected_ledger_sequence: u64,
    pub decision: FoundationBatchDecision,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationReviewConflictKind {
    GeometryOverlap,
    EntityMatch,
    Classification,
    Naming,
    Attribute,
    Containment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewConflict {
    pub id: String,
    pub category: FoundationCategory,
    pub subject_ids: Vec<String>,
    pub kind: FoundationReviewConflictKind,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "resolution")]
pub enum ReviewConflictResolution {
    KeepSeparate,
    Grouping {
        group_id: String,
        primary_subject_id: String,
        supporting_subject_ids: Vec<String>,
    },
    Containment {
        container_id: String,
        member_id: String,
        container_generates_surface: bool,
    },
    Naming {
        subject_id: String,
        display_name: String,
        evidence_ids: Vec<String>,
    },
    GeometryRepair {
        subject_id: String,
        review_geometry_sha256: String,
    },
    Attribute {
        subject_id: String,
        attribute: String,
        provenance_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum FoundationReviewAction {
    Candidate {
        subject_id: String,
        decision: FoundationCandidateDecision,
    },
    Batch {
        decision: FoundationBatchDecision,
    },
    Revoke {
        subject_id: String,
    },
    ConflictDeclared {
        conflict: FoundationReviewConflict,
    },
    ConflictResolved {
        conflict_id: String,
        resolution: ReviewConflictResolution,
    },
    GapAcknowledged {
        gap_id: String,
    },
    GapReopened {
        gap_id: String,
    },
    GapResolved {
        gap_id: String,
        evidence_ids: Vec<String>,
    },
    CoarseRasterDecision {
        observation_id: String,
        decision: crate::CoarseRasterDecision,
    },
    CategoryCompleted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewState {
    pub candidate_dispositions: BTreeMap<String, CandidateReviewDisposition>,
    pub acknowledged_gap_ids: BTreeSet<String>,
    pub resolved_gap_ids: BTreeSet<String>,
    pub unresolved_conflict_ids: BTreeSet<String>,
    #[serde(default)]
    pub coarse_raster_decisions: BTreeMap<String, crate::CoarseRasterDecision>,
    pub category_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewOperation {
    pub sequence: u64,
    pub category: FoundationCategory,
    pub subjects: Vec<String>,
    pub basis: FoundationReviewBasis,
    pub action: FoundationReviewAction,
    #[serde(default)]
    pub before: FoundationReviewState,
    #[serde(default)]
    pub after: FoundationReviewState,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default)]
    pub carried_from_sequence: Option<u64>,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnownFeatureGapLocation {
    pub tile_id: String,
    pub geometry: Option<SourceGeometry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnownFeatureGapStatus {
    Open,
    Acknowledged,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum KnownFeatureGapHistoryAction {
    Observed,
    Acknowledged {
        sequence: u64,
    },
    Reopened {
        sequence: u64,
    },
    Resolved {
        sequence: u64,
        evidence_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnownFeatureGap {
    pub id: String,
    pub category: FoundationCategory,
    pub location: KnownFeatureGapLocation,
    pub attempted_evidence: Vec<String>,
    pub generation_impact: String,
    pub provider: String,
    pub tile_id: String,
    pub acknowledged: bool,
    pub status: KnownFeatureGapStatus,
    pub history: Vec<KnownFeatureGapHistoryAction>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewGeometryForm {
    Point,
    Centreline,
    Area,
}

impl ReviewGeometryForm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Centreline => "centreline",
            Self::Area => "area",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WidthProvenanceKind {
    Explicit,
    RuleDerived,
    StyleDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedWidth {
    pub metres: Option<f64>,
    pub provenance: WidthProvenanceKind,
    pub rule_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceAssessmentDimension {
    pub status: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CandidateEvidenceAssessment {
    pub geometry: EvidenceAssessmentDimension,
    pub semantics: EvidenceAssessmentDimension,
    pub entity_match: EvidenceAssessmentDimension,
    pub name_match: EvidenceAssessmentDimension,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewQueueItem {
    pub subject_id: String,
    pub category: FoundationCategory,
    pub disposition: CandidateReviewDisposition,
    pub geometry_form: ReviewGeometryForm,
    pub subtype: Option<String>,
    pub width: Option<ReviewedWidth>,
    pub source_geometry: SourceGeometry,
    pub review_geometry: SourceGeometry,
    pub review_geometry_derivation: GeometryDerivationRecord,
    pub source_summary: String,
    pub lineage_summary: String,
    pub provenance_summary: String,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub assessment: CandidateEvidenceAssessment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationCategoryProgress {
    pub total: usize,
    pub disposed: usize,
    pub pending: usize,
    pub unresolved_conflicts: usize,
    pub unacknowledged_gaps: usize,
    pub complete: bool,
    pub completion_blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewQueueProjection {
    pub category: FoundationCategory,
    pub basis: FoundationReviewBasis,
    pub ledger_sequence: u64,
    pub items: Vec<FoundationReviewQueueItem>,
    pub provider_outcomes: Vec<ProviderOutcome>,
    pub known_gaps: Vec<KnownFeatureGap>,
    pub conflicts: Vec<FoundationReviewConflict>,
    pub resolved_conflict_ids: Vec<String>,
    pub progress: FoundationCategoryProgress,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedFeatureResolution {
    pub subject_id: String,
    pub display_name: Option<String>,
    pub name_evidence_ids: Vec<String>,
    pub review_geometry_sha256: Option<String>,
    pub attributes: Vec<ReviewedAttributeResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedAttributeResolution {
    pub attribute: String,
    pub provenance_ids: Vec<String>,
}

pub(crate) fn candidate_dispositions(
    category: FoundationCategory,
    basis: &FoundationReviewBasis,
    observations: &[SourceObservation],
    operations: &[FoundationReviewOperation],
) -> BTreeMap<String, CandidateReviewDisposition> {
    let mut dispositions = observations
        .iter()
        .filter(|item| item.category == category)
        .map(|item| (item.id.clone(), CandidateReviewDisposition::Pending))
        .collect::<BTreeMap<_, _>>();
    for operation in operations
        .iter()
        .filter(|operation| operation.category == category && operation.basis == *basis)
    {
        match &operation.action {
            FoundationReviewAction::Candidate {
                subject_id,
                decision,
            } => {
                if let Some(disposition) = dispositions.get_mut(subject_id) {
                    *disposition = disposition_from_decision(decision);
                }
            }
            FoundationReviewAction::Batch { decision } => {
                for subject_id in &operation.subjects {
                    if let Some(disposition) = dispositions.get_mut(subject_id) {
                        *disposition = match decision {
                            FoundationBatchDecision::Accept => CandidateReviewDisposition::Accepted,
                            FoundationBatchDecision::Reject => CandidateReviewDisposition::Rejected,
                        };
                    }
                }
            }
            FoundationReviewAction::Revoke { subject_id } => {
                if let Some(disposition) = dispositions.get_mut(subject_id) {
                    *disposition = CandidateReviewDisposition::Pending;
                }
            }
            FoundationReviewAction::ConflictResolved {
                resolution:
                    ReviewConflictResolution::Grouping {
                        primary_subject_id,
                        supporting_subject_ids,
                        ..
                    },
                ..
            } => {
                if let Some(disposition) = dispositions.get_mut(primary_subject_id) {
                    *disposition = CandidateReviewDisposition::Accepted;
                }
                for subject_id in supporting_subject_ids {
                    if let Some(disposition) = dispositions.get_mut(subject_id) {
                        *disposition = CandidateReviewDisposition::SupportingEvidence {
                            primary_subject_id: primary_subject_id.clone(),
                        };
                    }
                }
            }
            _ => {}
        }
    }
    dispositions
}

pub(crate) fn review_conflicts(
    category: FoundationCategory,
    basis: &FoundationReviewBasis,
    operations: &[FoundationReviewOperation],
) -> (Vec<FoundationReviewConflict>, BTreeSet<String>) {
    let mut conflicts = BTreeMap::new();
    let mut resolved = BTreeSet::new();
    for operation in operations
        .iter()
        .filter(|operation| operation.category == category && operation.basis == *basis)
    {
        match &operation.action {
            FoundationReviewAction::ConflictDeclared { conflict } => {
                conflicts.insert(conflict.id.clone(), conflict.clone());
            }
            FoundationReviewAction::ConflictResolved { conflict_id, .. } => {
                resolved.insert(conflict_id.clone());
            }
            _ => {}
        }
    }
    (conflicts.into_values().collect(), resolved)
}

pub(crate) fn suggested_conflicts(
    category: FoundationCategory,
    observations: &[SourceObservation],
) -> Vec<FoundationReviewConflict> {
    let mut groups = BTreeMap::<
        String,
        (
            FoundationReviewConflictKind,
            BTreeSet<String>,
            BTreeSet<String>,
        ),
    >::new();
    for observation in observations
        .iter()
        .filter(|observation| observation.category == category)
    {
        for suggestion in observation
            .suggestions
            .iter()
            .filter(|suggestion| suggestion.overlap_group.is_some())
        {
            let group_id = suggestion.overlap_group.as_ref().unwrap().clone();
            let kind = conflict_kind_for_suggestion(&suggestion.kind);
            let entry = groups
                .entry(group_id)
                .or_insert((kind, BTreeSet::new(), BTreeSet::new()));
            entry.1.insert(observation.id.clone());
            entry.2.insert(suggestion.reason.clone());
        }
    }
    groups
        .into_iter()
        .filter(|(_, (_, subjects, _))| subjects.len() > 1)
        .map(
            |(group_id, (kind, subjects, reasons))| FoundationReviewConflict {
                id: format!("suggestion:{category:?}:{group_id}")
                    .to_ascii_lowercase()
                    .replace('/', "-"),
                category,
                subject_ids: subjects.into_iter().collect(),
                kind,
                explanation: reasons.into_iter().collect::<Vec<_>>().join("; "),
            },
        )
        .collect()
}

fn conflict_kind_for_suggestion(kind: &str) -> FoundationReviewConflictKind {
    let kind = kind.to_ascii_lowercase();
    if kind.contains("contain") {
        FoundationReviewConflictKind::Containment
    } else if kind.contains("name") {
        FoundationReviewConflictKind::Naming
    } else if kind.contains("attribute") {
        FoundationReviewConflictKind::Attribute
    } else if kind.contains("class") {
        FoundationReviewConflictKind::Classification
    } else if kind.contains("entity") {
        FoundationReviewConflictKind::EntityMatch
    } else {
        FoundationReviewConflictKind::GeometryOverlap
    }
}

pub(crate) fn reviewed_feature_resolutions(
    category: FoundationCategory,
    basis: &FoundationReviewBasis,
    operations: &[FoundationReviewOperation],
) -> Vec<ReviewedFeatureResolution> {
    let mut resolutions = BTreeMap::<String, ReviewedFeatureResolution>::new();
    for resolution in operations
        .iter()
        .filter(|operation| operation.category == category && operation.basis == *basis)
        .filter_map(|operation| match &operation.action {
            FoundationReviewAction::ConflictResolved { resolution, .. } => Some(resolution),
            _ => None,
        })
    {
        match resolution {
            ReviewConflictResolution::Naming {
                subject_id,
                display_name,
                evidence_ids,
            } => {
                let entry = resolutions.entry(subject_id.clone()).or_default();
                entry.subject_id = subject_id.clone();
                entry.display_name = Some(display_name.clone());
                entry.name_evidence_ids = evidence_ids.clone();
            }
            ReviewConflictResolution::GeometryRepair {
                subject_id,
                review_geometry_sha256,
            } => {
                let entry = resolutions.entry(subject_id.clone()).or_default();
                entry.subject_id = subject_id.clone();
                entry.review_geometry_sha256 = Some(review_geometry_sha256.clone());
            }
            ReviewConflictResolution::Attribute {
                subject_id,
                attribute,
                provenance_ids,
            } => {
                let entry = resolutions.entry(subject_id.clone()).or_default();
                entry.subject_id = subject_id.clone();
                entry.attributes.push(ReviewedAttributeResolution {
                    attribute: attribute.clone(),
                    provenance_ids: provenance_ids.clone(),
                });
            }
            _ => {}
        }
    }
    resolutions.into_values().collect()
}

pub(crate) fn project_queue_item(
    observation: &SourceObservation,
    disposition: CandidateReviewDisposition,
) -> FoundationReviewQueueItem {
    FoundationReviewQueueItem {
        subject_id: observation.id.clone(),
        category: observation.category,
        disposition,
        geometry_form: geometry_form(&observation.geometry),
        subtype: reviewed_subtype(observation),
        width: reviewed_width(observation),
        source_geometry: observation.geometry.clone(),
        review_geometry: observation.review_geometry_proposal.clone(),
        review_geometry_derivation: observation.derivation.clone(),
        source_summary: format!(
            "{} · {}",
            observation.lineage.provider, observation.lineage.source_record_id
        ),
        lineage_summary: format!(
            "{} · {} · {}",
            observation.lineage.dataset_release,
            observation.lineage.source_record_version,
            observation.derivation.rule_version
        ),
        provenance_summary: format!(
            "{} · {} · {}",
            observation.licence.identifier,
            observation.licence.dataset_release,
            observation.coordinate_semantics.crs
        ),
        lineage: observation.lineage.clone(),
        licence: observation.licence.clone(),
        assessment: assessment(observation),
    }
}

pub(crate) fn known_gaps_for_category(
    category: FoundationCategory,
    outcomes: &[ProviderOutcome],
    operations: &[FoundationReviewOperation],
) -> Vec<KnownFeatureGap> {
    outcomes
        .iter()
        .filter(|outcome| outcome.category == category)
        .flat_map(|outcome| {
            let reasons = if outcome.gaps.is_empty()
                && !matches!(
                    outcome.status,
                    crate::ProviderOutcomeStatus::Complete
                        | crate::ProviderOutcomeStatus::CompleteEmpty
                ) {
                vec![outcome
                    .failure
                    .as_ref()
                    .map(|failure| failure.explanation.clone())
                    .unwrap_or_else(|| format!("{:?} provider coverage", outcome.status))]
            } else {
                outcome.gaps.clone()
            };
            reasons.into_iter().enumerate().map(|(index, reason)| {
                let id = gap_id(outcome, index);
                let (status, history) = known_gap_history(category, &id, operations);
                let acknowledged = status == KnownFeatureGapStatus::Acknowledged;
                KnownFeatureGap {
                    acknowledged,
                    id,
                    category,
                    location: KnownFeatureGapLocation {
                        tile_id: outcome.tile_id.clone(),
                        geometry: outcome.gap_geometry.clone(),
                    },
                    attempted_evidence: vec![reason],
                    generation_impact: outcome
                        .failure
                        .as_ref()
                        .map(|failure| failure.explanation.clone())
                        .unwrap_or_else(|| {
                            "Coverage is incomplete; generation leaves unsupported space empty"
                                .into()
                        }),
                    provider: outcome.provider.clone(),
                    tile_id: outcome.tile_id.clone(),
                    status,
                    history,
                }
            })
        })
        .collect()
}

pub(crate) fn known_gap_history(
    category: FoundationCategory,
    gap_id: &str,
    operations: &[FoundationReviewOperation],
) -> (KnownFeatureGapStatus, Vec<KnownFeatureGapHistoryAction>) {
    let mut status = KnownFeatureGapStatus::Open;
    let mut history = vec![KnownFeatureGapHistoryAction::Observed];
    for operation in operations
        .iter()
        .filter(|operation| operation.category == category)
    {
        match &operation.action {
            FoundationReviewAction::GapAcknowledged {
                gap_id: operation_gap_id,
            } if operation_gap_id == gap_id => {
                status = KnownFeatureGapStatus::Acknowledged;
                history.push(KnownFeatureGapHistoryAction::Acknowledged {
                    sequence: operation.sequence,
                });
            }
            FoundationReviewAction::GapReopened {
                gap_id: operation_gap_id,
            } if operation_gap_id == gap_id => {
                status = KnownFeatureGapStatus::Open;
                history.push(KnownFeatureGapHistoryAction::Reopened {
                    sequence: operation.sequence,
                });
            }
            FoundationReviewAction::GapResolved {
                gap_id: operation_gap_id,
                evidence_ids,
            } if operation_gap_id == gap_id => {
                status = KnownFeatureGapStatus::Resolved;
                history.push(KnownFeatureGapHistoryAction::Resolved {
                    sequence: operation.sequence,
                    evidence_ids: evidence_ids.clone(),
                });
            }
            _ => {}
        }
    }
    (status, history)
}

pub(crate) fn gap_id(outcome: &ProviderOutcome, index: usize) -> String {
    format!(
        "gap:{:?}:{}:{}:{index}",
        outcome.category, outcome.provider, outcome.tile_id
    )
    .to_ascii_lowercase()
    .replace('/', "-")
}

pub(crate) fn non_generating_container_ids(
    category: FoundationCategory,
    basis: &FoundationReviewBasis,
    operations: &[FoundationReviewOperation],
) -> BTreeSet<String> {
    operations
        .iter()
        .filter(|operation| operation.category == category && operation.basis == *basis)
        .filter_map(|operation| match &operation.action {
            FoundationReviewAction::ConflictResolved {
                resolution:
                    ReviewConflictResolution::Containment {
                        container_id,
                        container_generates_surface: false,
                        ..
                    },
                ..
            } => Some(container_id.clone()),
            _ => None,
        })
        .collect()
}

fn disposition_from_decision(decision: &FoundationCandidateDecision) -> CandidateReviewDisposition {
    match decision {
        FoundationCandidateDecision::Accept => CandidateReviewDisposition::Accepted,
        FoundationCandidateDecision::Reject { .. } => CandidateReviewDisposition::Rejected,
        FoundationCandidateDecision::SupportingEvidence { primary_subject_id } => {
            CandidateReviewDisposition::SupportingEvidence {
                primary_subject_id: primary_subject_id.clone(),
            }
        }
        FoundationCandidateDecision::Defer {
            structured_reason,
            acknowledged_gap_id,
        } => CandidateReviewDisposition::Deferred {
            structured_reason: structured_reason.clone(),
            acknowledged_gap_id: acknowledged_gap_id.clone(),
        },
    }
}

fn geometry_form(geometry: &SourceGeometry) -> ReviewGeometryForm {
    match geometry {
        SourceGeometry::Point(_) | SourceGeometry::MultiPoint(_) => ReviewGeometryForm::Point,
        SourceGeometry::LineString(_) | SourceGeometry::MultiLineString(_) => {
            ReviewGeometryForm::Centreline
        }
        SourceGeometry::Polygon(_) | SourceGeometry::MultiPolygon(_) => ReviewGeometryForm::Area,
    }
}

fn reviewed_subtype(observation: &SourceObservation) -> Option<String> {
    observation
        .attribute_provenance
        .iter()
        .find_map(|attribute| match attribute {
            AttributeProvenance::Subtype { value, .. } => Some(value.clone()),
            _ => None,
        })
        .or_else(|| {
            [
                "highway", "waterway", "sport", "leisure", "natural", "building", "subtype",
            ]
            .iter()
            .find_map(|key| {
                observation
                    .original_properties
                    .get(*key)
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
        })
}

fn reviewed_width(observation: &SourceObservation) -> Option<ReviewedWidth> {
    observation
        .attribute_provenance
        .iter()
        .find_map(|attribute| match attribute {
            AttributeProvenance::WidthMetres {
                value,
                derivation,
                rule_version,
                ..
            } => Some(ReviewedWidth {
                metres: Some(*value),
                provenance: match derivation {
                    AttributeDerivation::Direct => WidthProvenanceKind::Explicit,
                    AttributeDerivation::Derived => WidthProvenanceKind::RuleDerived,
                },
                rule_version: Some(rule_version.clone()),
            }),
            _ => None,
        })
        .or_else(|| {
            matches!(
                observation.geometry,
                SourceGeometry::LineString(_) | SourceGeometry::MultiLineString(_)
            )
            .then(|| ReviewedWidth {
                metres: None,
                provenance: WidthProvenanceKind::StyleDefault,
                rule_version: None,
            })
        })
}

fn assessment(observation: &SourceObservation) -> CandidateEvidenceAssessment {
    let geometry_changed = observation.geometry != observation.review_geometry_proposal;
    let has_name = observation
        .attribute_provenance
        .iter()
        .any(|attribute| matches!(attribute, AttributeProvenance::Name { .. }));
    CandidateEvidenceAssessment {
        geometry: EvidenceAssessmentDimension {
            status: if geometry_changed {
                "derived-review-geometry"
            } else {
                "source-geometry"
            }
            .into(),
            reason: if geometry_changed {
                format!(
                    "Review geometry is derived by {} without mutating the source geometry",
                    observation.derivation.rule_version
                )
            } else {
                format!(
                    "{} geometry is retained with digest {}",
                    observation.geometry.type_name(),
                    observation.geometry_sha256
                )
            },
        },
        semantics: EvidenceAssessmentDimension {
            status: if reviewed_subtype(observation).is_some() {
                "typed"
            } else {
                "requires-review"
            }
            .into(),
            reason: format!(
                "Classification {} is retained from {}",
                observation.lineage.original_classification, observation.lineage.provider
            ),
        },
        entity_match: EvidenceAssessmentDimension {
            status: if observation.category == FoundationCategory::Building {
                "entity-review"
            } else {
                "feature-review"
            }
            .into(),
            reason: if observation.category == FoundationCategory::Building {
                "Building identity is resolved through stable Building Entities".into()
            } else {
                "Overlaps remain separate until a grouping or containment decision".into()
            },
        },
        name_match: EvidenceAssessmentDimension {
            status: if has_name { "evidence" } else { "unconfirmed" }.into(),
            reason: if has_name {
                "Traceable name evidence is present but does not define identity".into()
            } else {
                "No confirmed name is inferred from geometry or proximity".into()
            },
        },
        priority: if geometry_changed {
            "review-derived-geometry"
        } else if observation.suggestions.is_empty() {
            "normal"
        } else {
            "suggested"
        }
        .into(),
    }
}
