use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 8;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapCoordinate {
    pub lng: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapPurpose {
    #[default]
    CampusSelection,
    CampusBoundary,
    FoundationReview,
    BuildingEvidence,
}

impl MapPurpose {
    #[allow(non_upper_case_globals)]
    pub const CampusReview: Self = Self::CampusSelection;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapOverlay {
    pub label: String,
    pub points: Vec<MapCoordinate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapBoundaryCandidate {
    pub id: String,
    pub rank: u32,
    pub label: String,
    pub valid: bool,
    #[serde(default)]
    pub invalid_reasons: Vec<String>,
    pub points: Vec<MapCoordinate>,
    pub source_summary: String,
    pub ranking_summary: String,
    pub lineage_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapBoundaryDesk {
    pub candidates: Vec<MapBoundaryCandidate>,
    #[serde(default)]
    pub selected_candidate_id: Option<String>,
    #[serde(default)]
    pub working_points: Vec<MapCoordinate>,
    #[serde(default)]
    pub can_undo: bool,
    pub dataset_bundle_summary: String,
    pub coverage_summary: String,
    #[serde(default)]
    pub confirmation_blocked_reason: Option<String>,
    #[serde(default)]
    pub recovery_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapBoundaryDeskRequest {
    pub campus_name: String,
    pub center_lng: f64,
    pub center_lat: f64,
    pub zoom: f64,
    pub pitch: f64,
    pub rotation: f64,
    pub js_api_key: String,
    pub security_code: String,
    pub desk: MapBoundaryDesk,
    #[serde(default)]
    pub english: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapFoundationReviewCategory {
    pub id: String,
    pub label: String,
    pub acquisition_state: String,
    pub disposed: usize,
    pub total: usize,
    pub pending: usize,
    pub blockers: usize,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapEvidenceAssessment {
    pub geometry: String,
    pub semantics: String,
    pub entity_match: String,
    pub name_match: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapFoundationReviewCandidate {
    pub id: String,
    pub label: String,
    pub disposition: String,
    pub priority: String,
    pub source_summary: String,
    pub lineage_summary: String,
    pub provenance_summary: String,
    pub geometry_form: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub width_summary: Option<String>,
    pub assessment: MapEvidenceAssessment,
    #[serde(default)]
    pub geometry: Vec<Vec<MapCoordinate>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapProviderOutcome {
    pub provider: String,
    pub tile_id: String,
    pub state: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapKnownFeatureGap {
    pub id: String,
    pub location_summary: String,
    pub attempted_evidence: String,
    pub generation_impact: String,
    pub status: String,
    pub history_summary: String,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum MapAreaGeometry {
    Polygon {
        rings: Vec<Vec<MapCoordinate>>,
    },
    MultiPolygon {
        polygons: Vec<Vec<Vec<MapCoordinate>>>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapCoarseRasterEvidence {
    pub id: String,
    pub linked_gap_id: String,
    pub label: String,
    pub decision: String,
    pub dataset_summary: String,
    pub resolution_class_summary: String,
    pub lineage_summary: String,
    pub exclusion_summary: String,
    pub assessment: MapEvidenceAssessment,
    pub approximate_geometry: MapAreaGeometry,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapReviewConflict {
    pub id: String,
    pub kind: String,
    pub explanation: String,
    pub subject_ids: Vec<String>,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapFoundationReviewDesk {
    pub categories: Vec<MapFoundationReviewCategory>,
    pub active_category: String,
    pub candidates: Vec<MapFoundationReviewCandidate>,
    #[serde(default)]
    pub selected_candidate_id: Option<String>,
    pub provider_outcomes: Vec<MapProviderOutcome>,
    pub known_gaps: Vec<MapKnownFeatureGap>,
    #[serde(default)]
    pub coarse_raster_evidence: Vec<MapCoarseRasterEvidence>,
    pub conflicts: Vec<MapReviewConflict>,
    pub basis_token: String,
    pub ledger_sequence: u64,
    #[serde(default)]
    pub completion_blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapFoundationReviewDeskRequest {
    pub campus_name: String,
    pub center_lng: f64,
    pub center_lat: f64,
    pub zoom: f64,
    pub pitch: f64,
    pub rotation: f64,
    pub js_api_key: String,
    pub security_code: String,
    pub boundary: Vec<MapCoordinate>,
    pub desk: MapFoundationReviewDesk,
    #[serde(default)]
    pub english: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapFoundationCandidateDecision {
    Accept,
    Reject,
    Revoke,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapCoarseRasterDecision {
    Accept,
    Reject,
    LeaveUnresolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MapFoundationBatchDecision {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "resolution")]
pub enum MapFoundationConflictResolution {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MapBoundaryEditOperation {
    MoveVertex {
        vertex_index: u32,
        coordinate: MapCoordinate,
    },
    InsertVertex {
        edge_index: u32,
    },
    DeleteVertex {
        vertex_index: u32,
    },
    Undo,
    RestoreCandidateOriginal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MapBoundaryHandleSelection {
    Vertex { vertex_index: u32 },
    Edge { edge_index: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ToolCommand {
    Hello {
        protocol_version: u32,
        session_token: String,
        tool: ToolKind,
    },
    OpenMap {
        campus_name: String,
        center_lng: f64,
        center_lat: f64,
        zoom: f64,
        pitch: f64,
        rotation: f64,
        js_api_key: String,
        security_code: String,
        boundary: Vec<MapCoordinate>,
        #[serde(default)]
        purpose: MapPurpose,
        #[serde(default)]
        overlays: Vec<MapOverlay>,
        #[serde(default)]
        feature_kind: Option<String>,
        #[serde(default)]
        english: bool,
    },
    OpenBoundaryDesk {
        request: Box<MapBoundaryDeskRequest>,
    },
    UpdateBoundaryDesk {
        desk: MapBoundaryDesk,
    },
    OpenFoundationReviewDesk {
        request: Box<MapFoundationReviewDeskRequest>,
    },
    UpdateFoundationReviewDesk {
        desk: MapFoundationReviewDesk,
    },
    ShowTaskError {
        message: String,
    },
    OpenPreview {
        model_path: String,
        title: String,
        #[serde(default)]
        english: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Map,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ToolEvent {
    Ready {
        protocol_version: u32,
        tool: ToolKind,
    },
    MapCamera {
        center_lng: f64,
        center_lat: f64,
        zoom: f64,
        pitch: f64,
        rotation: f64,
    },
    MapPointSelected {
        lng: f64,
        lat: f64,
    },
    MapCampusSelected {
        poi_id: String,
        name: String,
        lng: f64,
        lat: f64,
    },
    MapSearchFailed {
        code: String,
    },
    MapBoundaryChanged {
        points: Vec<MapCoordinate>,
    },
    MapBoundaryCandidateSelected {
        candidate_id: String,
    },
    MapBoundaryAdjustmentChanged {
        candidate_id: String,
        enabled: bool,
    },
    MapBoundaryHandleSelected {
        candidate_id: String,
        selection: MapBoundaryHandleSelection,
    },
    MapBoundaryOperation {
        candidate_id: String,
        operation: MapBoundaryEditOperation,
    },
    MapBoundaryConfirmed {
        candidate_id: String,
    },
    MapBoundaryRetryRequested,
    MapBoundaryReturnToCampusRequested,
    MapFoundationReviewCategorySelected {
        category: String,
    },
    MapFoundationReviewCandidateSelected {
        category: String,
        subject_id: String,
    },
    MapFoundationReviewDecisionRequested {
        category: String,
        subject_id: String,
        decision: MapFoundationCandidateDecision,
    },
    MapFoundationReviewDeferredRequested {
        category: String,
        subject_id: String,
        structured_reason: String,
        acknowledged_gap_id: String,
    },
    MapFoundationBatchReviewRequested {
        category: String,
        exact_subject_ids: Vec<String>,
        basis_token: String,
        expected_ledger_sequence: u64,
        decision: MapFoundationBatchDecision,
    },
    MapKnownFeatureGapAcknowledgementRequested {
        category: String,
        gap_id: String,
        acknowledged: bool,
    },
    MapCoarseRasterSupplementRequested {
        category: String,
        gap_id: String,
    },
    MapCoarseRasterDecisionRequested {
        category: String,
        observation_id: String,
        decision: MapCoarseRasterDecision,
    },
    MapFoundationConflictResolutionRequested {
        category: String,
        conflict_id: String,
        resolution: MapFoundationConflictResolution,
    },
    MapFoundationCategoryCompletionRequested {
        category: String,
    },
    MapFoundationRefreshRequested,
    PreviewBlockSelected {
        x: i32,
        y: i32,
        z: i32,
        block: String,
    },
    Error {
        message: String,
    },
    Closed {
        tool: ToolKind,
    },
}

pub async fn write_message<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err("tool message exceeded size limit".into());
    }
    writer
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|error| error.to_string())?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())
}

pub async fn read_message<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    reader: &mut R,
) -> Result<T, String> {
    let length = reader
        .read_u32_le()
        .await
        .map_err(|error| error.to_string())? as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err("invalid tool message length".into());
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

pub async fn forward_tool_events<W: AsyncWrite + Unpin>(
    mut writer: W,
    receiver: std::sync::mpsc::Receiver<ToolEvent>,
) -> Result<(), String> {
    loop {
        let event = receiver
            .recv()
            .map_err(|_| "tool event channel closed before Closed".to_string())?;
        let closed = matches!(event, ToolEvent::Closed { .. });
        write_message(&mut writer, &event).await?;
        if closed {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn protocol_round_trip() {
        let command = ToolCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
            session_token: "secret".into(),
            tool: ToolKind::Map,
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &command).await.unwrap();
        let restored: ToolCommand = read_message(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(restored, command);
    }

    #[tokio::test]
    async fn task_error_round_trip() {
        let command = ToolCommand::ShowTaskError {
            message: "Select a boundary handle before moving it".into(),
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &command).await.unwrap();
        let restored: ToolCommand = read_message(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(restored, command);
    }

    #[tokio::test]
    async fn boundary_edit_round_trip_is_exhaustively_typed() {
        let event = ToolEvent::MapBoundaryOperation {
            candidate_id: "boundary-1".into(),
            operation: MapBoundaryEditOperation::MoveVertex {
                vertex_index: 2,
                coordinate: MapCoordinate {
                    lng: 121.4,
                    lat: 31.2,
                },
            },
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &event).await.unwrap();
        let restored: ToolEvent = read_message(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(restored, event);

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["operation"]["type"], "move_vertex");
        assert_eq!(json["operation"]["vertexIndex"], 2);
        assert!(json["operation"].get("points").is_none());
    }

    #[tokio::test]
    async fn list_first_foundation_review_actions_round_trip_without_drawing_commands() {
        let event = ToolEvent::MapFoundationBatchReviewRequested {
            category: "vegetation".into(),
            exact_subject_ids: vec!["tree-row".into(), "tree-area".into()],
            basis_token: "basis-v7".into(),
            expected_ledger_sequence: 12,
            decision: MapFoundationBatchDecision::Reject,
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &event).await.unwrap();
        let restored: ToolEvent = read_message(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(restored, event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "mapFoundationBatchReviewRequested");
        assert_eq!(json["decision"], "reject");
        assert_eq!(json["exactSubjectIds"].as_array().unwrap().len(), 2);

        let deferred = ToolEvent::MapFoundationReviewDeferredRequested {
            category: "water".into(),
            subject_id: "water-centreline".into(),
            structured_reason: "source coverage ended before the full channel".into(),
            acknowledged_gap_id: "gap:water:overture:tile-7".into(),
        };
        let json = serde_json::to_value(&deferred).unwrap();
        assert_eq!(json["type"], "mapFoundationReviewDeferredRequested");
        assert_eq!(json["subjectId"], "water-centreline");
        assert_eq!(json["acknowledgedGapId"], "gap:water:overture:tile-7");

        let refresh = serde_json::to_value(ToolEvent::MapFoundationRefreshRequested).unwrap();
        assert_eq!(refresh["type"], "mapFoundationRefreshRequested");
    }

    #[tokio::test]
    async fn map_search_failure_round_trip_preserves_gaode_code() {
        let event = ToolEvent::MapSearchFailed {
            code: "INVALID_USER_SCODE".into(),
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &event).await.unwrap();
        let restored: ToolEvent = read_message(&mut bytes.as_slice()).await.unwrap();
        assert_eq!(restored, event);
    }

    #[tokio::test]
    async fn event_forwarder_flushes_closed_before_stopping() {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(ToolEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
                tool: ToolKind::Map,
            })
            .unwrap();
        sender
            .send(ToolEvent::Closed {
                tool: ToolKind::Map,
            })
            .unwrap();
        let (writer, mut reader) = tokio::io::duplex(4096);

        forward_tool_events(writer, receiver).await.unwrap();

        assert!(matches!(
            read_message::<_, ToolEvent>(&mut reader).await.unwrap(),
            ToolEvent::Ready {
                tool: ToolKind::Map,
                ..
            }
        ));
        assert_eq!(
            read_message::<_, ToolEvent>(&mut reader).await.unwrap(),
            ToolEvent::Closed {
                tool: ToolKind::Map
            }
        );
        assert!(sender
            .send(ToolEvent::Error {
                message: "must not be forwarded after close".into()
            })
            .is_err());
    }
}
