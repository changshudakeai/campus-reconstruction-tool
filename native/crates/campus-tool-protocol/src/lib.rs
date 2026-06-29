use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

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
    CampusReview,
    FoundationFeatureDrawing,
    BuildingEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapOverlay {
    pub label: String,
    pub points: Vec<MapCoordinate>,
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
    },
    OpenPreview {
        model_path: String,
        title: String,
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
    MapBoundaryChanged {
        points: Vec<MapCoordinate>,
    },
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
}
