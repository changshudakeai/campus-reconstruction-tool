use super::{tr, DesktopLocale, GeoPoint, MapLaunchRequest, ToolSupervisor, ToolUpdate};
use campus_tool_protocol::{MapCoordinate, MapOverlay, ToolCommand, ToolEvent, ToolKind};
use std::path::PathBuf;

pub(crate) fn gaode_search_failure_message(code: &str, locale: DesktopLocale) -> String {
    let code = code.trim();
    let (zh, en) = match code.to_ascii_uppercase().as_str() {
        "INVALID_USER_SCODE" => (
            "高德安全密钥验证失败。请在“更多 → 地图设置”中填写与当前 Web JS API Key 同一条配置下生成的 securityJsCode。",
            "Gaode security-code verification failed. In More → Map settings, enter the securityJsCode generated for the same Web JS API Key entry.",
        ),
        "INVALID_USER_KEY" => (
            "高德 Web JS API Key 无效或已失效，请在“更多 → 地图设置”中更新。",
            "The Gaode Web JS API Key is invalid or expired. Update it in More → Map settings.",
        ),
        "USERKEY_PLAT_NOMATCH" => (
            "高德 Key 平台不匹配；这里必须使用 Web（JS API）类型的 Key。",
            "The Gaode key platform does not match; this tool requires a Web (JS API) key.",
        ),
        "INVALID_USER_DOMAIN" => (
            "当前应用域名不在高德 Key 的允许范围内，请检查该 Web（JS API）Key 的域名配置。",
            "This application domain is not allowed by the Gaode key. Check the Web (JS API) key's domain settings.",
        ),
        "SERVICE_NOT_AVAILABLE" => (
            "当前高德 Key 未开通地点搜索服务，或该服务暂不可用。",
            "Place search is not enabled for this Gaode key, or the service is temporarily unavailable.",
        ),
        "NO_DATA" => (
            "高德没有返回校园候选。请缩短为“学校名 + 校区名”后重试。",
            "Gaode returned no campus candidates. Retry with a shorter school and campus name.",
        ),
        _ => (
            "高德校园搜索失败，请根据错误码检查地图配置。",
            "Gaode campus search failed. Check the map configuration using the error code.",
        ),
    };
    format!(
        "{}（{}）",
        tr(locale, zh, en),
        if code.is_empty() { "UNKNOWN" } else { code }
    )
}

impl ToolSupervisor {
    pub(super) fn launch_map_supervised(&self, request: MapLaunchRequest) -> Result<(), String> {
        let locale = if request.english {
            DesktopLocale::En
        } else {
            DesktopLocale::ZhCn
        };
        let command = ToolCommand::OpenMap {
            campus_name: request.title,
            center_lng: request.view.center.lng,
            center_lat: request.view.center.lat,
            zoom: request.view.zoom,
            pitch: request.view.pitch,
            rotation: request.view.rotation,
            js_api_key: request.js_api_key,
            security_code: request.security_code,
            boundary: request
                .boundary
                .into_iter()
                .map(campus_services::wgs84_to_gcj02)
                .map(|point| MapCoordinate {
                    lng: point.lng,
                    lat: point.lat,
                })
                .collect(),
            purpose: request.purpose,
            overlays: request
                .overlays
                .into_iter()
                .map(|overlay| MapOverlay {
                    label: overlay.label,
                    points: overlay
                        .points
                        .into_iter()
                        .map(|point| {
                            let point = campus_services::wgs84_to_gcj02(GeoPoint {
                                lng: point.lng,
                                lat: point.lat,
                            });
                            MapCoordinate {
                                lng: point.lng,
                                lat: point.lat,
                            }
                        })
                        .collect(),
                })
                .collect(),
            feature_kind: request.feature_kind,
            english: request.english,
        };
        let updates = self.updates.clone();
        self.processes
            .launch("campus-map", ToolKind::Map, command, move |event| {
                let updates = updates.clone();
                async move {
                    let message = match event {
                        ToolEvent::Ready { .. } => {
                            tr(locale, "高德工具已连接", "Gaode tool connected").to_string()
                        }
                        ToolEvent::MapCamera {
                            center_lng,
                            center_lat,
                            zoom,
                            pitch,
                            rotation,
                        } => {
                            let _ = updates.send(ToolUpdate::MapCamera {
                                center: GeoPoint {
                                    lng: center_lng,
                                    lat: center_lat,
                                },
                                zoom,
                                pitch,
                                rotation,
                            });
                            format!("Map view recorded · zoom {zoom:.1} · pitch {pitch:.0}")
                        }
                        ToolEvent::MapPointSelected { lng, lat } => {
                            let _ = updates.send(ToolUpdate::MapPoint(GeoPoint { lng, lat }));
                            format!("Selected location {lng:.6}, {lat:.6}")
                        }
                        ToolEvent::MapCampusSelected {
                            poi_id,
                            name,
                            lng,
                            lat,
                        } => {
                            let gcj02 = GeoPoint { lng, lat };
                            let target = campus_state::CampusTargetEvidence {
                                poi_id,
                                name: name.clone(),
                                gcj02,
                                wgs84: campus_services::gcj02_to_wgs84(gcj02),
                                acquisition: "gaode_poi_search".into(),
                            };
                            let _ = updates.send(ToolUpdate::MapCampusTarget(target));
                            format!("Campus confirmed: {name}")
                        }
                        ToolEvent::MapSearchFailed { code } => {
                            let _ = updates.send(ToolUpdate::Error {
                                event: "map.campus-search".into(),
                                message: gaode_search_failure_message(&code, locale),
                            });
                            return;
                        }
                        ToolEvent::MapBoundaryChanged { points } => {
                            let count = points.len();
                            let points = points
                                .into_iter()
                                .map(|point| {
                                    campus_services::gcj02_to_wgs84(GeoPoint {
                                        lng: point.lng,
                                        lat: point.lat,
                                    })
                                })
                                .collect();
                            let _ = updates.send(ToolUpdate::MapBoundary(points));
                            format!("Campus boundary saved · {count} nodes")
                        }
                        ToolEvent::Error { message } => {
                            let _ = updates.send(ToolUpdate::Error {
                                event: "map.tool".into(),
                                message,
                            });
                            return;
                        }
                        ToolEvent::Closed { .. } => "Gaode tool closed".into(),
                        _ => return,
                    };
                    let _ = updates.send(ToolUpdate::Status(message));
                }
            })
    }

    pub(super) fn launch_preview_supervised(
        &self,
        model_path: PathBuf,
        title: String,
        english: bool,
    ) -> Result<(), String> {
        let command = ToolCommand::OpenPreview {
            model_path: model_path.to_string_lossy().into_owned(),
            title,
            english,
        };
        let updates = self.updates.clone();
        self.processes
            .launch("campus-preview", ToolKind::Preview, command, move |event| {
                let updates = updates.clone();
                async move {
                    let update = match event {
                        ToolEvent::Ready { .. } => {
                            ToolUpdate::Status("Native preview connected".into())
                        }
                        ToolEvent::PreviewBlockSelected { x, y, z, block } => {
                            ToolUpdate::PreviewBlockSelected { x, y, z, block }
                        }
                        ToolEvent::Error { message } => ToolUpdate::Error {
                            event: "preview.tool".into(),
                            message,
                        },
                        ToolEvent::Closed { .. } => {
                            ToolUpdate::Status("Native preview closed".into())
                        }
                        _ => return,
                    };
                    let _ = updates.send(update);
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::gaode_search_failure_message;
    use crate::DesktopLocale;

    #[test]
    fn gaode_security_failure_is_actionable_and_keeps_the_original_code() {
        let message = gaode_search_failure_message("INVALID_USER_SCODE", DesktopLocale::ZhCn);
        assert!(message.contains("同一条配置"));
        assert!(message.contains("securityJsCode"));
        assert!(message.contains("INVALID_USER_SCODE"));
    }

    #[test]
    fn gaode_no_data_is_not_reported_as_a_credentials_error() {
        let message = gaode_search_failure_message("no_data", DesktopLocale::ZhCn);
        assert!(message.contains("缩短"));
        assert!(message.contains("no_data"));
        assert!(!message.contains("安全密钥验证失败"));
    }
}
