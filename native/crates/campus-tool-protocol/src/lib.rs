use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 6;
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
    FoundationFeatureDrawing,
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
    MapBoundaryOperation {
        candidate_id: String,
        operation: String,
        points: Vec<MapCoordinate>,
    },
    MapBoundaryConfirmed {
        candidate_id: String,
        points: Vec<MapCoordinate>,
    },
    MapBoundaryRetryRequested,
    MapBoundaryReturnToCampusRequested,
    MapFeatureDrawn {
        kind: String,
        points: Vec<MapCoordinate>,
    },
    MapCaptureRequested {
        south_west_lng: f64,
        south_west_lat: f64,
        north_east_lng: f64,
        north_east_lat: f64,
    },
    MapVisualCapture {
        image_data_url: String,
        south_west_lng: f64,
        south_west_lat: f64,
        north_east_lng: f64,
        north_east_lat: f64,
    },
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
