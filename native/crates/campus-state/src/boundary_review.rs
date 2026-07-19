use crate::{BoundaryCandidate, PinnedBoundaryEvidence, ResultManifest, SourceGeometry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const BOUNDARY_HISTORY_LIMIT: usize = 50;
const CONFIRMATION_LABEL: &str =
    "Confirm boundary and acquire Buildings, Circulation, Water, Vegetation, and Sports";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BoundaryCandidateValidity {
    Valid,
    Invalid { reasons: Vec<String> },
}

impl BoundaryCandidateValidity {
    fn first_blocking_reason(&self) -> Option<&str> {
        match self {
            Self::Valid => None,
            Self::Invalid { reasons } => reasons
                .first()
                .map(String::as_str)
                .or(Some("The automatic Campus Boundary candidate is invalid")),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryCandidateDerivation {
    pub source_records: Vec<String>,
    pub rule_versions: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryCandidateAssessment {
    pub validity: BoundaryCandidateValidity,
    pub derivation: BoundaryCandidateDerivation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDiscoverySnapshot {
    pub manifest: ResultManifest,
    pub candidates: Vec<BoundaryCandidate>,
    pub assessments: BTreeMap<String, BoundaryCandidateAssessment>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryInteractionMode {
    #[default]
    Review,
    Adjustment,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryVertexRef {
    pub polygon_index: usize,
    pub ring_index: usize,
    pub vertex_index: usize,
}

impl BoundaryVertexRef {
    pub const fn outer(polygon_index: usize, vertex_index: usize) -> Self {
        Self {
            polygon_index,
            ring_index: 0,
            vertex_index,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryEdgeRef {
    pub polygon_index: usize,
    pub ring_index: usize,
    pub start_vertex_index: usize,
}

impl BoundaryEdgeRef {
    pub const fn outer(polygon_index: usize, start_vertex_index: usize) -> Self {
        Self {
            polygon_index,
            ring_index: 0,
            start_vertex_index,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BoundaryHandleSelection {
    Vertex(BoundaryVertexRef),
    Edge(BoundaryEdgeRef),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryEvidenceAvailability {
    Ready,
    AllInvalid,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryRecoveryAction {
    Retry,
    ReturnToCampusTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryGeometryValidity {
    pub valid: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryEvidenceDeskProjection {
    pub availability: BoundaryEvidenceAvailability,
    pub can_adjust: bool,
    pub can_confirm: bool,
    pub can_undo: bool,
    pub confirmation_label: &'static str,
    pub confirmation_blocked_reason: Option<String>,
    pub recovery_actions: Vec<BoundaryRecoveryAction>,
    pub geometry_validity: BoundaryGeometryValidity,
    pub vertex_count: usize,
    pub actionable_feedback: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct BoundaryDeskFailure {
    explanation: String,
    suggested_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryEvidenceDesk {
    snapshot: Option<BoundaryDiscoverySnapshot>,
    selected_candidate_id: Option<String>,
    working_geometry: Option<SourceGeometry>,
    mode: BoundaryInteractionMode,
    selected_handle: Option<BoundaryHandleSelection>,
    #[serde(default)]
    adjustment_history: Vec<SourceGeometry>,
    #[serde(default)]
    unavailable: Option<BoundaryDeskFailure>,
    #[serde(default)]
    actionable_feedback: Option<String>,
}

impl BoundaryEvidenceDesk {
    pub fn new(mut snapshot: BoundaryDiscoverySnapshot) -> Result<Self, String> {
        validate_snapshot_identity(&snapshot)?;
        for candidate in &snapshot.candidates {
            snapshot
                .assessments
                .entry(candidate.id.clone())
                .or_insert_with(|| BoundaryCandidateAssessment {
                    validity: BoundaryCandidateValidity::Invalid {
                        reasons: vec![
                            "The Boundary Discovery Snapshot omitted candidate validity evidence"
                                .into(),
                        ],
                    },
                    derivation: BoundaryCandidateDerivation::default(),
                });
        }
        let selected_candidate_id = snapshot
            .candidates
            .iter()
            .min_by_key(|candidate| candidate.rank)
            .map(|candidate| candidate.id.clone());
        let working_geometry = selected_candidate_id.as_deref().and_then(|candidate_id| {
            snapshot
                .candidates
                .iter()
                .find(|candidate| candidate.id == candidate_id)
                .map(|candidate| candidate.geometry.clone())
        });
        Ok(Self {
            snapshot: Some(snapshot),
            selected_candidate_id,
            working_geometry,
            mode: BoundaryInteractionMode::Review,
            selected_handle: None,
            adjustment_history: Vec::new(),
            unavailable: None,
            actionable_feedback: None,
        })
    }

    pub fn unavailable(
        explanation: impl Into<String>,
        suggested_action: impl Into<String>,
    ) -> Self {
        Self {
            snapshot: None,
            selected_candidate_id: None,
            working_geometry: None,
            mode: BoundaryInteractionMode::Review,
            selected_handle: None,
            adjustment_history: Vec::new(),
            unavailable: Some(BoundaryDeskFailure {
                explanation: explanation.into(),
                suggested_action: suggested_action.into(),
            }),
            actionable_feedback: None,
        }
    }

    pub fn snapshot(&self) -> Option<&BoundaryDiscoverySnapshot> {
        self.snapshot.as_ref()
    }

    pub fn selected_candidate_id(&self) -> Option<&str> {
        self.selected_candidate_id.as_deref()
    }

    pub fn working_geometry(&self) -> Option<&SourceGeometry> {
        self.working_geometry.as_ref()
    }

    pub fn mode(&self) -> BoundaryInteractionMode {
        self.mode
    }

    pub fn candidate_validity(&self, candidate_id: &str) -> BoundaryCandidateValidity {
        let Some(candidate) = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .candidates
                .iter()
                .find(|candidate| candidate.id == candidate_id)
        }) else {
            return BoundaryCandidateValidity::Invalid {
                reasons: vec![
                    "The automatic Campus Boundary candidate is not in this snapshot".into(),
                ],
            };
        };
        let mut reasons = match self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.assessments.get(candidate_id))
            .map(|assessment| &assessment.validity)
        {
            Some(BoundaryCandidateValidity::Valid) => Vec::new(),
            Some(BoundaryCandidateValidity::Invalid { reasons }) => reasons.clone(),
            None => vec!["Candidate validity evidence is unavailable".into()],
        };
        for reason in validate_boundary_geometry(&candidate.geometry).reasons {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        }
        if reasons.is_empty() {
            BoundaryCandidateValidity::Valid
        } else {
            BoundaryCandidateValidity::Invalid { reasons }
        }
    }

    pub fn projection(&self) -> BoundaryEvidenceDeskProjection {
        let availability = self.availability();
        let selected_validity = self
            .selected_candidate_id
            .as_deref()
            .map(|candidate_id| self.candidate_validity(candidate_id));
        let geometry_validity = self
            .working_geometry
            .as_ref()
            .map(validate_boundary_geometry)
            .unwrap_or_else(|| BoundaryGeometryValidity {
                valid: false,
                reasons: vec!["No automatic Campus Boundary geometry is available".into()],
            });
        let confirmation_blocked_reason = self
            .unavailable
            .as_ref()
            .map(|failure| failure.explanation.clone())
            .or_else(|| {
                selected_validity
                    .as_ref()
                    .and_then(BoundaryCandidateValidity::first_blocking_reason)
                    .map(str::to_string)
            })
            .or_else(|| {
                if self.selected_candidate_id.is_none() {
                    Some("No automatic Campus Boundary candidate is available".into())
                } else {
                    None
                }
            })
            .or_else(|| {
                (!geometry_validity.valid)
                    .then(|| geometry_validity.reasons.first().cloned())
                    .flatten()
            });
        let valid_candidate = matches!(selected_validity, Some(BoundaryCandidateValidity::Valid));
        BoundaryEvidenceDeskProjection {
            availability,
            can_adjust: availability == BoundaryEvidenceAvailability::Ready && valid_candidate,
            can_confirm: availability == BoundaryEvidenceAvailability::Ready
                && valid_candidate
                && geometry_validity.valid,
            can_undo: !self.adjustment_history.is_empty(),
            confirmation_label: CONFIRMATION_LABEL,
            confirmation_blocked_reason,
            recovery_actions: if availability == BoundaryEvidenceAvailability::Ready {
                Vec::new()
            } else {
                vec![
                    BoundaryRecoveryAction::Retry,
                    BoundaryRecoveryAction::ReturnToCampusTarget,
                ]
            },
            vertex_count: self
                .working_geometry
                .as_ref()
                .map(boundary_vertex_count)
                .unwrap_or_default(),
            geometry_validity,
            actionable_feedback: self.actionable_feedback.clone().or_else(|| {
                self.unavailable.as_ref().map(|failure| {
                    format!(
                        "{} Action: {}",
                        failure.explanation, failure.suggested_action
                    )
                })
            }),
        }
    }

    pub fn select_candidate(&mut self, candidate_id: &str) -> Result<(), String> {
        let candidate = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .candidates
                    .iter()
                    .find(|candidate| candidate.id == candidate_id)
            })
            .ok_or("The selected Campus Boundary candidate is not in this discovery snapshot")?;
        self.selected_candidate_id = Some(candidate.id.clone());
        self.working_geometry = Some(candidate.geometry.clone());
        self.mode = BoundaryInteractionMode::Review;
        self.selected_handle = None;
        self.adjustment_history.clear();
        self.actionable_feedback = Some(format!(
            "Selected ranked Campus Boundary candidate {}",
            candidate.rank
        ));
        Ok(())
    }

    pub fn enter_adjustment(&mut self) -> Result<(), String> {
        if !self.projection().can_adjust {
            return Err(self
                .projection()
                .confirmation_blocked_reason
                .unwrap_or_else(|| "Select a valid automatic boundary candidate first".into()));
        }
        self.mode = BoundaryInteractionMode::Adjustment;
        self.selected_handle = None;
        self.actionable_feedback =
            Some("Adjustment mode is active; select one vertex or edge before editing".into());
        Ok(())
    }

    pub fn leave_adjustment(&mut self) {
        self.mode = BoundaryInteractionMode::Review;
        self.selected_handle = None;
        self.actionable_feedback =
            Some("Review mode is active; map interaction cannot change geometry".into());
    }

    pub fn select_vertex(&mut self, vertex: BoundaryVertexRef) -> Result<(), String> {
        self.require_adjustment()?;
        let ring = boundary_ring(
            self.working_geometry
                .as_ref()
                .ok_or("No automatic Campus Boundary geometry is selected")?,
            vertex.polygon_index,
            vertex.ring_index,
        )?;
        if vertex.vertex_index >= unique_ring_vertex_count(ring) {
            return Err("The selected Campus Boundary vertex does not exist".into());
        }
        self.selected_handle = Some(BoundaryHandleSelection::Vertex(vertex));
        self.actionable_feedback = Some(format!(
            "Selected boundary vertex {}",
            vertex.vertex_index + 1
        ));
        Ok(())
    }

    pub fn select_edge(&mut self, edge: BoundaryEdgeRef) -> Result<(), String> {
        self.require_adjustment()?;
        let ring = boundary_ring(
            self.working_geometry
                .as_ref()
                .ok_or("No automatic Campus Boundary geometry is selected")?,
            edge.polygon_index,
            edge.ring_index,
        )?;
        if edge.start_vertex_index >= unique_ring_vertex_count(ring) {
            return Err("The selected Campus Boundary edge does not exist".into());
        }
        self.selected_handle = Some(BoundaryHandleSelection::Edge(edge));
        self.actionable_feedback = Some(format!(
            "Selected boundary edge {}",
            edge.start_vertex_index + 1
        ));
        Ok(())
    }

    pub fn move_selected_vertex(&mut self, coordinate: [f64; 2]) -> Result<(), String> {
        self.require_adjustment()?;
        validate_coordinate(coordinate)?;
        let Some(BoundaryHandleSelection::Vertex(vertex)) = self.selected_handle else {
            return Err("Select one Campus Boundary vertex before dragging it".into());
        };
        let mut next = self
            .working_geometry
            .clone()
            .ok_or("No automatic Campus Boundary geometry is selected")?;
        let ring = boundary_ring_mut(&mut next, vertex.polygon_index, vertex.ring_index)?;
        let unique_count = unique_ring_vertex_count(ring);
        if vertex.vertex_index >= unique_count {
            return Err("The selected Campus Boundary vertex does not exist".into());
        }
        ring[vertex.vertex_index] = coordinate;
        if vertex.vertex_index == 0 && ring_is_closed(ring) {
            let last = ring.len() - 1;
            ring[last] = coordinate;
        }
        self.commit_adjustment(
            next,
            format!("Moved boundary vertex {}", vertex.vertex_index + 1),
        );
        Ok(())
    }

    pub fn insert_vertex_on_selected_edge(&mut self) -> Result<BoundaryVertexRef, String> {
        self.require_adjustment()?;
        let Some(BoundaryHandleSelection::Edge(edge)) = self.selected_handle else {
            return Err("Select one Campus Boundary edge before inserting a vertex".into());
        };
        let mut next = self
            .working_geometry
            .clone()
            .ok_or("No automatic Campus Boundary geometry is selected")?;
        let ring = boundary_ring_mut(&mut next, edge.polygon_index, edge.ring_index)?;
        let unique_count = unique_ring_vertex_count(ring);
        if edge.start_vertex_index >= unique_count {
            return Err("The selected Campus Boundary edge does not exist".into());
        }
        let next_index = (edge.start_vertex_index + 1) % unique_count;
        let start = ring[edge.start_vertex_index];
        let end = ring[next_index];
        let inserted_coordinate = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
        let inserted_index = edge.start_vertex_index + 1;
        ring.insert(inserted_index, inserted_coordinate);
        let inserted = BoundaryVertexRef {
            polygon_index: edge.polygon_index,
            ring_index: edge.ring_index,
            vertex_index: inserted_index,
        };
        self.commit_adjustment(
            next,
            format!("Inserted boundary vertex {}", inserted_index + 1),
        );
        self.selected_handle = Some(BoundaryHandleSelection::Vertex(inserted));
        Ok(inserted)
    }

    pub fn delete_selected_vertex(&mut self) -> Result<(), String> {
        self.require_adjustment()?;
        let Some(BoundaryHandleSelection::Vertex(vertex)) = self.selected_handle else {
            return Err("Select one Campus Boundary vertex before deleting it".into());
        };
        let mut next = self
            .working_geometry
            .clone()
            .ok_or("No automatic Campus Boundary geometry is selected")?;
        let ring = boundary_ring_mut(&mut next, vertex.polygon_index, vertex.ring_index)?;
        let unique_count = unique_ring_vertex_count(ring);
        if unique_count <= 3 {
            return Err("A Campus Boundary ring must retain at least three vertices".into());
        }
        if vertex.vertex_index >= unique_count {
            return Err("The selected Campus Boundary vertex does not exist".into());
        }
        let closed = ring_is_closed(ring);
        ring.remove(vertex.vertex_index);
        if closed && vertex.vertex_index == 0 {
            let first = ring[0];
            let last = ring.len() - 1;
            ring[last] = first;
        }
        self.commit_adjustment(
            next,
            format!("Deleted boundary vertex {}", vertex.vertex_index + 1),
        );
        self.selected_handle = None;
        Ok(())
    }

    pub fn undo_adjustment(&mut self) -> Result<(), String> {
        let previous = self
            .adjustment_history
            .pop()
            .ok_or("No Campus Boundary adjustment is available to undo")?;
        self.working_geometry = Some(previous);
        self.selected_handle = None;
        self.actionable_feedback = Some(format!(
            "Undid the last boundary adjustment; {}",
            validity_feedback(&self.projection().geometry_validity)
        ));
        Ok(())
    }

    pub fn restore_candidate_original(&mut self) -> Result<(), String> {
        self.require_adjustment()?;
        let original = self
            .selected_candidate()
            .map(|candidate| candidate.geometry.clone())
            .ok_or("No automatic Campus Boundary candidate is selected")?;
        if self.working_geometry.as_ref() == Some(&original) {
            self.actionable_feedback =
                Some("The Campus Boundary already matches the automatic candidate".into());
            return Ok(());
        }
        self.commit_adjustment(original, "Restored the automatic boundary candidate".into());
        self.selected_handle = None;
        Ok(())
    }

    pub fn report_tool_failure(
        &mut self,
        explanation: impl Into<String>,
        suggested_action: impl Into<String>,
    ) {
        self.actionable_feedback = Some(format!(
            "{} Action: {}",
            explanation.into(),
            suggested_action.into()
        ));
        self.mode = BoundaryInteractionMode::Review;
        self.selected_handle = None;
    }

    pub fn to_pinned_evidence(&self) -> Result<PinnedBoundaryEvidence, String> {
        let projection = self.projection();
        if !projection.can_confirm {
            return Err(projection
                .confirmation_blocked_reason
                .unwrap_or_else(|| "The Campus Boundary cannot be confirmed".into()));
        }
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or("The Boundary Discovery Snapshot is unavailable")?;
        let selected_candidate_id = self
            .selected_candidate_id
            .clone()
            .ok_or("No automatic Campus Boundary candidate is selected")?;
        Ok(PinnedBoundaryEvidence {
            manifest: snapshot.manifest.clone(),
            candidates: snapshot.candidates.clone(),
            selected_candidate_id,
            confirmed_geometry: self.working_geometry.clone(),
            assessments: snapshot.assessments.clone(),
        })
    }

    fn availability(&self) -> BoundaryEvidenceAvailability {
        if self.unavailable.is_some()
            || self
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.candidates.is_empty())
        {
            return BoundaryEvidenceAvailability::Unavailable;
        }
        if self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.candidates.iter().any(|candidate| {
                matches!(
                    self.candidate_validity(&candidate.id),
                    BoundaryCandidateValidity::Valid
                )
            })
        }) {
            BoundaryEvidenceAvailability::Ready
        } else {
            BoundaryEvidenceAvailability::AllInvalid
        }
    }

    fn selected_candidate(&self) -> Option<&BoundaryCandidate> {
        let selected = self.selected_candidate_id.as_deref()?;
        self.snapshot
            .as_ref()?
            .candidates
            .iter()
            .find(|candidate| candidate.id == selected)
    }

    fn require_adjustment(&self) -> Result<(), String> {
        if self.mode != BoundaryInteractionMode::Adjustment {
            Err("Enter Campus Boundary adjustment mode before changing geometry".into())
        } else {
            Ok(())
        }
    }

    fn commit_adjustment(&mut self, next: SourceGeometry, action: String) {
        if let Some(current) = self.working_geometry.replace(next) {
            self.adjustment_history.push(current);
            if self.adjustment_history.len() > BOUNDARY_HISTORY_LIMIT {
                self.adjustment_history.remove(0);
            }
        }
        self.actionable_feedback = Some(format!(
            "{action}; {}",
            validity_feedback(&self.projection().geometry_validity)
        ));
    }
}

fn validate_snapshot_identity(snapshot: &BoundaryDiscoverySnapshot) -> Result<(), String> {
    if snapshot.manifest.bundle.id.trim().is_empty()
        || snapshot.manifest.result_sha256.trim().is_empty()
    {
        return Err("Boundary Discovery Snapshot identity is incomplete".into());
    }
    let mut ids = BTreeSet::new();
    let mut ranks = BTreeSet::new();
    for candidate in &snapshot.candidates {
        if candidate.id.trim().is_empty() || !ids.insert(candidate.id.clone()) {
            return Err("Boundary Discovery Snapshot candidate identities are not unique".into());
        }
        if candidate.rank == 0 || !ranks.insert(candidate.rank) {
            return Err("Boundary Discovery Snapshot candidate ranks are not unique".into());
        }
    }
    Ok(())
}

fn validity_feedback(validity: &BoundaryGeometryValidity) -> String {
    if validity.valid {
        "the edited boundary is valid".into()
    } else {
        format!(
            "the edited boundary is invalid: {}",
            validity
                .reasons
                .first()
                .map(String::as_str)
                .unwrap_or("unknown geometry error")
        )
    }
}

fn validate_coordinate(coordinate: [f64; 2]) -> Result<(), String> {
    if coordinate[0].is_finite()
        && coordinate[1].is_finite()
        && (-180.0..=180.0).contains(&coordinate[0])
        && (-90.0..=90.0).contains(&coordinate[1])
    {
        Ok(())
    } else {
        Err("Campus Boundary coordinates must be finite WGS-84 longitude/latitude values".into())
    }
}

fn boundary_vertex_count(geometry: &SourceGeometry) -> usize {
    match geometry {
        SourceGeometry::Polygon(rings) => rings
            .iter()
            .map(|ring| unique_ring_vertex_count(ring))
            .sum(),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter())
            .map(|ring| unique_ring_vertex_count(ring))
            .sum(),
        _ => 0,
    }
}

fn boundary_ring(
    geometry: &SourceGeometry,
    polygon_index: usize,
    ring_index: usize,
) -> Result<&Vec<[f64; 2]>, String> {
    match geometry {
        SourceGeometry::Polygon(rings) if polygon_index == 0 => rings.get(ring_index),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .get(polygon_index)
            .and_then(|polygon| polygon.get(ring_index)),
        _ => None,
    }
    .ok_or_else(|| "The selected Campus Boundary ring does not exist".into())
}

fn boundary_ring_mut(
    geometry: &mut SourceGeometry,
    polygon_index: usize,
    ring_index: usize,
) -> Result<&mut Vec<[f64; 2]>, String> {
    match geometry {
        SourceGeometry::Polygon(rings) if polygon_index == 0 => rings.get_mut(ring_index),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .get_mut(polygon_index)
            .and_then(|polygon| polygon.get_mut(ring_index)),
        _ => None,
    }
    .ok_or_else(|| "The selected Campus Boundary ring does not exist".into())
}

fn ring_is_closed(ring: &[[f64; 2]]) -> bool {
    ring.len() >= 2 && ring.first() == ring.last()
}

fn unique_ring_vertex_count(ring: &[[f64; 2]]) -> usize {
    ring.len().saturating_sub(usize::from(ring_is_closed(ring)))
}

pub fn validate_boundary_geometry(geometry: &SourceGeometry) -> BoundaryGeometryValidity {
    let polygons: Vec<&Vec<Vec<[f64; 2]>>> = match geometry {
        SourceGeometry::Polygon(rings) => vec![rings],
        SourceGeometry::MultiPolygon(polygons) => polygons.iter().collect(),
        _ => {
            return BoundaryGeometryValidity {
                valid: false,
                reasons: vec!["Campus Boundary geometry must be a Polygon or MultiPolygon".into()],
            };
        }
    };
    let mut reasons = Vec::new();
    if polygons.is_empty() {
        reasons.push("Campus Boundary geometry contains no polygon".into());
    }
    for (polygon_index, polygon) in polygons.iter().enumerate() {
        if polygon.is_empty() {
            reasons.push(format!(
                "Boundary polygon {} contains no outer ring",
                polygon_index + 1
            ));
            continue;
        }
        for (ring_index, ring) in polygon.iter().enumerate() {
            if !ring_is_closed(ring) {
                reasons.push(format!(
                    "Boundary polygon {} ring {} is not closed",
                    polygon_index + 1,
                    ring_index + 1
                ));
                continue;
            }
            let unique_count = unique_ring_vertex_count(ring);
            if unique_count < 3 {
                reasons.push(format!(
                    "Boundary polygon {} ring {} has fewer than three vertices",
                    polygon_index + 1,
                    ring_index + 1
                ));
                continue;
            }
            if let Some(invalid) = ring
                .iter()
                .copied()
                .find(|coordinate| validate_coordinate(*coordinate).is_err())
            {
                reasons.push(format!(
                    "Boundary polygon {} ring {} contains invalid coordinate [{}, {}]",
                    polygon_index + 1,
                    ring_index + 1,
                    invalid[0],
                    invalid[1]
                ));
            }
            if signed_ring_area(&ring[..unique_count]).abs() <= f64::EPSILON {
                reasons.push(format!(
                    "Boundary polygon {} ring {} has zero area",
                    polygon_index + 1,
                    ring_index + 1
                ));
            }
            if ring_self_intersects(&ring[..unique_count]) {
                reasons.push(format!(
                    "Boundary polygon {} ring {} self-intersects",
                    polygon_index + 1,
                    ring_index + 1
                ));
            }
        }
    }
    BoundaryGeometryValidity {
        valid: reasons.is_empty(),
        reasons,
    }
}

fn signed_ring_area(points: &[[f64; 2]]) -> f64 {
    (0..points.len())
        .map(|index| {
            let next = (index + 1) % points.len();
            points[index][0] * points[next][1] - points[next][0] * points[index][1]
        })
        .sum::<f64>()
        / 2.0
}

fn ring_self_intersects(points: &[[f64; 2]]) -> bool {
    let edge_count = points.len();
    for first in 0..edge_count {
        for second in (first + 1)..edge_count {
            if first == second
                || (first + 1) % edge_count == second
                || (second + 1) % edge_count == first
            {
                continue;
            }
            if segments_intersect(
                points[first],
                points[(first + 1) % edge_count],
                points[second],
                points[(second + 1) % edge_count],
            ) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    fn on_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> bool {
        point[0] >= a[0].min(b[0])
            && point[0] <= a[0].max(b[0])
            && point[1] >= a[1].min(b[1])
            && point[1] <= a[1].max(b[1])
    }
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);
    if ((ab_c > 0.0 && ab_d < 0.0) || (ab_c < 0.0 && ab_d > 0.0))
        && ((cd_a > 0.0 && cd_b < 0.0) || (cd_a < 0.0 && cd_b > 0.0))
    {
        return true;
    }
    (ab_c.abs() <= f64::EPSILON && on_segment(a, b, c))
        || (ab_d.abs() <= f64::EPSILON && on_segment(a, b, d))
        || (cd_a.abs() <= f64::EPSILON && on_segment(c, d, a))
        || (cd_b.abs() <= f64::EPSILON && on_segment(c, d, b))
}
