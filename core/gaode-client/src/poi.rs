//! 高德地点搜索响应解析与学校类筛选
//!
//! ADR-0008 第 1-3 条落地：
//! - 解析高德地点搜索（PlaceSearch）返回的 POI 列表；
//! - **结果筛选**：只保留学校类地点（typecode 141xx 教育类目），过滤掉
//!   以学校命名的公交站、地铁站、银行网点等干扰项；
//! - 同名/近名结果去重（同一 POI id 或同名同地址只留一条）；
//! - 候选列表每项展示：学校名称 + 所在地址（含行政区），多校区大学
//!   （如"华东师范大学(普陀校区)"与"(闵行校区)"）作为独立候选并列展示。

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// 学校类 POI 的高德类型码前缀（141200 学校 / 141201 高等院校 /
/// 141202 中学 / 141203 小学 / 141206 职业技术学校 等，同属 1412 科教类目）
pub const SCHOOL_TYPECODE_PREFIX: &str = "1412";

/// 一条学校类候选 POI（候选列表的一行：名称 + 地址 + 坐标锚点）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchoolPoi {
    /// 高德地点标识（POI identity，持久化字段之一）
    pub poi_id: String,
    /// 学校名称（校区名称直接取自高德，用户不手动输入）
    pub name: String,
    /// 所在地址（含行政区，用于区分同名/多校区）
    pub address: String,
    /// 坐标锚点：经度（GCJ-02）
    pub longitude: f64,
    /// 坐标锚点：纬度（GCJ-02）
    pub latitude: f64,
    /// 高德类型码（education 类目校验凭据）
    pub typecode: String,
}

impl SchoolPoi {
    /// 是否属于学校类地点（教育类目 1412 前缀）
    pub fn is_school(&self) -> bool {
        self.typecode.starts_with(SCHOOL_TYPECODE_PREFIX)
    }
}

/// 高德地点搜索响应中的一条原始 POI（JS 桥回传的 REST 风格 JSON）
#[derive(Debug, Deserialize)]
struct RawPoi {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: serde_json::Value,
    /// "经度,纬度" 文本（高德 location 字段格式）
    #[serde(default)]
    location: String,
    #[serde(default)]
    typecode: String,
}

/// 高德地点搜索响应外层结构
#[derive(Debug, Deserialize)]
struct RawResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    info: String,
    #[serde(default)]
    pois: Vec<RawPoi>,
}

/// 解析高德地点搜索响应 JSON，产出筛选、去重后的学校类候选列表
///
/// - `status != "1"` → [`Error::ServiceRejected`]（失败策略由上层另行决策，
///   本函数只负责如实上报）；
/// - 非学校类 POI（公交站、校门、银行网点等）被静默过滤；
/// - 坐标无法解析的条目跳过（脏数据不进候选列表）；
/// - 去重：同一 POI id 或同名同地址只保留先出现的一条。
pub fn parse_place_search_response(json: &str) -> Result<Vec<SchoolPoi>> {
    let response: RawResponse =
        serde_json::from_str(json).map_err(|e| Error::MalformedResponse(e.to_string()))?;
    if response.status != "1" {
        return Err(Error::ServiceRejected {
            info: response.info,
        });
    }

    let mut seen_ids = Vec::new();
    let mut seen_name_address = Vec::new();
    let mut candidates = Vec::new();
    for raw in response.pois {
        let Some((longitude, latitude)) = parse_location(&raw.location) else {
            continue;
        };
        let poi = SchoolPoi {
            poi_id: raw.id,
            name: raw.name,
            address: flatten_address(&raw.address),
            longitude,
            latitude,
            typecode: raw.typecode,
        };
        if !poi.is_school() {
            continue;
        }
        // 去重：同 POI id / 同名同地址只留先出现的一条（ADR-0008 第 2 条）
        let name_address = (poi.name.clone(), poi.address.clone());
        if seen_ids.contains(&poi.poi_id) || seen_name_address.contains(&name_address) {
            continue;
        }
        seen_ids.push(poi.poi_id.clone());
        seen_name_address.push(name_address);
        candidates.push(poi);
    }
    Ok(candidates)
}

/// 解析高德 "经度,纬度" 文本，非法格式返回 None
fn parse_location(location: &str) -> Option<(f64, f64)> {
    let (lng_text, lat_text) = location.split_once(',')?;
    let longitude: f64 = lng_text.trim().parse().ok()?;
    let latitude: f64 = lat_text.trim().parse().ok()?;
    // 合法经纬度范围粗校验（高德为 GCJ-02，数值范围同 WGS-84）
    if !(-180.0..=180.0).contains(&longitude) || !(-90.0..=90.0).contains(&latitude) {
        return None;
    }
    Some((longitude, latitude))
}

/// 高德 address 字段可能是字符串，也可能是空数组 `[]`（无地址时的怪癖）
fn flatten_address(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条响应 JSON 的辅助函数
    fn response_with(pois: &str) -> String {
        format!(r#"{{"status":"1","info":"OK","pois":[{pois}]}}"#)
    }

    #[test]
    fn keeps_only_school_pois() {
        let json = response_with(
            r#"{"id":"B01","name":"华东师范大学(普陀校区)","address":"中山北路3663号","location":"121.406,31.228","typecode":"141201"},
               {"id":"B02","name":"华东师大站(公交站)","address":"中山北路","location":"121.407,31.229","typecode":"150700"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert_eq!(pois.len(), 1);
        assert_eq!(pois[0].name, "华东师范大学(普陀校区)");
        assert!(pois[0].is_school());
    }

    #[test]
    fn multi_campus_universities_stay_as_separate_candidates() {
        let json = response_with(
            r#"{"id":"B01","name":"华东师范大学(普陀校区)","address":"中山北路3663号","location":"121.406,31.228","typecode":"141201"},
               {"id":"B02","name":"华东师范大学(闵行校区)","address":"东川路500号","location":"121.456,31.033","typecode":"141201"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert_eq!(pois.len(), 2, "多校区作为独立候选并列展示");
    }

    #[test]
    fn duplicate_id_and_same_name_address_are_deduplicated() {
        let json = response_with(
            r#"{"id":"B01","name":"第一中学","address":"人民路1号","location":"121.4,31.2","typecode":"141202"},
               {"id":"B01","name":"第一中学","address":"人民路1号","location":"121.4,31.2","typecode":"141202"},
               {"id":"B03","name":"第一中学","address":"人民路1号","location":"121.4001,31.2001","typecode":"141202"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert_eq!(pois.len(), 1, "同 id 与同名同地址都只留一条");
    }

    #[test]
    fn bad_location_entries_are_skipped() {
        let json = response_with(
            r#"{"id":"B01","name":"某小学","address":[],"location":"","typecode":"141203"},
               {"id":"B02","name":"某中学","address":"文化路9号","location":"200.0,95.0","typecode":"141202"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert!(pois.is_empty(), "空坐标与越界坐标都不进候选列表");
    }

    #[test]
    fn service_failure_is_reported_with_info() {
        let json = r#"{"status":"0","info":"INVALID_USER_KEY","pois":[]}"#;
        let err = parse_place_search_response(json).unwrap_err();
        assert!(matches!(err, Error::ServiceRejected { .. }));
        assert!(err.to_string().contains("INVALID_USER_KEY"));
    }

    #[test]
    fn malformed_json_is_reported() {
        let err = parse_place_search_response("not json").unwrap_err();
        assert!(matches!(err, Error::MalformedResponse(_)));
    }
}

/// T23/T24: IPC 消息类型（取点页/边界编辑页回传）
#[derive(Debug, Clone, PartialEq)]
pub enum IpcMessage {
    /// 坐标："经度，纬度" 字符串（T23 pick point / T24 manual_point）
    Coordinate { longitude: f64, latitude: f64 },
    /// 错误：结构化 JSON
    Error { message: String },
    // T24: OSM 边界编辑相关
    /// OSM Overpass 返回的原始要素列表 (osm_elements)
    OsmElements { elements: Vec<OsmElement> },
    /// 编辑后的多边形坐标 (boundary_update: GCJ-02 坐标数组)
    BoundaryUpdate { coords: Vec<[f64; 2]> },
    /// 人工圈画落点 (manual_point: WGS-84 单个点)
    ManualPoint { lon: f64, lat: f64 },
    /// 撤销上一个点 (manual_cancel)
    ManualCancel,
    /// 清空重画 (manual_clear)
    ManualClear,
    /// 确认最终边界 (confirm_boundary: GCJ-02 or WGS-84 TBD)
    ConfirmBoundary { coords: Vec<[f64; 2]> },
}

/// T24: OSM 元素结构 (Overpass JSON)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OsmElement {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub geometry: Option<Vec<[f64; 2]>>,
    #[serde(default)]
    pub members: Vec<OsmMember>,
    #[serde(default)]
    pub tags: std::collections::HashMap<String, String>,
}

/// T24: OSM 成员结构 (relation member)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OsmMember {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub reference: i64,
    #[serde(default)]
    pub role: String,
}

/// 解析来自 WebView IPC 的消息（多分支：坐标/错误/OSM/编辑/手动）
///
/// - 纯文本 → 尝试解析为 "经度，纬度" (pick point / manual_point)
/// - JSON 含 `type="error"` → [`IpcMessage::Error`]
/// - JSON 含 `type="osm_elements"` → [`IpcMessage::OsmElements`]
/// - JSON 含 `type="boundary_update"` → [`IpcMessage::BoundaryUpdate`]
/// - JSON 含 `type="manual_point"` → [`IpcMessage::ManualPoint`]
/// - JSON 含 `type="manual_cancel"` → [`IpcMessage::ManualCancel`]
/// - JSON 含 `type="manual_clear"` → [`IpcMessage::ManualClear`]
/// - JSON 含 `type="confirm_boundary"` → [`IpcMessage::ConfirmBoundary`]
/// - 其他 → [`Error::UnsupportedIpcMessage`]
pub fn parse_ipc_message(msg: &str) -> Result<IpcMessage> {
    // 先尝试直接当作 "经度，纬度" (pick point / manual_point)
    if let Some((longitude, latitude)) = try_parse_coordinate(msg) {
        return Ok(IpcMessage::Coordinate {
            longitude,
            latitude,
        });
    }

    // JSON 载荷解析
    if msg.starts_with('{') {
        #[derive(Deserialize)]
        struct TypePayload {
            #[serde(default)]
            r#type: String,
        }
        if let Ok(type_payload) = serde_json::from_str::<TypePayload>(msg) {
            match type_payload.r#type.as_str() {
                "error" => {
                    #[derive(Deserialize)]
                    struct ErrorPayload {
                        #[serde(default)]
                        message: String,
                    }
                    if let Ok(payload) = serde_json::from_str::<ErrorPayload>(msg) {
                        return Ok(IpcMessage::Error {
                            message: payload.message,
                        });
                    }
                }
                "osm_elements" => {
                    #[derive(Deserialize)]
                    struct OsmPayload {
                        #[serde(default)]
                        elements: Vec<OsmElement>,
                    }
                    if let Ok(payload) = serde_json::from_str::<OsmPayload>(msg) {
                        return Ok(IpcMessage::OsmElements {
                            elements: payload.elements,
                        });
                    }
                }
                "boundary_update" | "confirm_boundary" => {
                    #[derive(Deserialize)]
                    struct BoundaryPayload {
                        #[serde(default)]
                        coords: Vec<[f64; 2]>,
                    }
                    if let Ok(payload) = serde_json::from_str::<BoundaryPayload>(msg) {
                        if type_payload.r#type == "boundary_update" {
                            return Ok(IpcMessage::BoundaryUpdate {
                                coords: payload.coords,
                            });
                        }
                        return Ok(IpcMessage::ConfirmBoundary {
                            coords: payload.coords,
                        });
                    }
                }
                "manual_point" => {
                    #[derive(Deserialize)]
                    struct ManualPointPayload {
                        #[serde(default)]
                        point: Option<[f64; 2]>,
                    }
                    if let Ok(payload) = serde_json::from_str::<ManualPointPayload>(msg) {
                        if let Some([lon, lat]) = payload.point {
                            return Ok(IpcMessage::ManualPoint { lon, lat });
                        }
                    }
                }
                "manual_cancel" => {
                    return Ok(IpcMessage::ManualCancel);
                }
                "manual_clear" => {
                    return Ok(IpcMessage::ManualClear);
                }
                _ => {}
            }
        }
    }

    Err(Error::UnsupportedIpcMessage(msg.to_owned()))
}

fn try_parse_coordinate(s: &str) -> Option<(f64, f64)> {
    let (lng_text, lat_text) = s.split_once(',')?;
    let longitude: f64 = lng_text.trim().parse().ok()?;
    let latitude: f64 = lat_text.trim().parse().ok()?;
    // 合法范围粗校验
    if !(-180.0..=180.0).contains(&longitude) || !(-90.0..=90.0).contains(&latitude) {
        return None;
    }
    Some((longitude, latitude))
}

#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[test]
    fn coordinate_payload_parsed_correctly() {
        let result = parse_ipc_message("121.456,31.033").unwrap();
        assert!(
            matches!(result, IpcMessage::Coordinate { longitude, latitude }
            if longitude == 121.456 && latitude == 31.033)
        );
    }

    #[test]
    fn error_payload_parsed_correctly() {
        let json = r#"{"type":"error","message":"SDK 加载超时"}"#;
        let result = parse_ipc_message(json).unwrap();
        assert!(matches!(result, IpcMessage::Error { message } if message == "SDK 加载超时"));
    }

    #[test]
    fn malformed_payload_rejected() {
        let bad_msg = "not_a_coordinate";
        assert!(matches!(
            parse_ipc_message(bad_msg),
            Err(Error::UnsupportedIpcMessage(_))
        ));

        let bad_json = r#"{"type":"unknown"}"#;
        assert!(matches!(
            parse_ipc_message(bad_json),
            Err(Error::UnsupportedIpcMessage(_))
        ));
    }

    // T24: 新 IPC 类型测试
    #[test]
    fn osm_elements_parsed_correctly() {
        let json = r#"{
            "type": "osm_elements",
            "elements": [
                {"type": "way", "id": 123, "tags": {"amenity": "university"}, "geometry": [[116.4, 39.9], [116.5, 39.9], [116.5, 40.0], [116.4, 40.0]]}
            ]
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::OsmElements { elements } = result {
            assert_eq!(elements.len(), 1);
            assert_eq!(
                elements[0].tags.get("amenity"),
                Some(&"university".to_string())
            );
        } else {
            panic!("Expected OsmElements variant");
        }
    }

    #[test]
    fn boundary_update_parsed_correctly() {
        let json = r#"{
            "type": "boundary_update",
            "coords": [[116.4, 39.9], [116.5, 39.9], [116.5, 40.0]]
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::BoundaryUpdate { coords } = result {
            assert_eq!(coords.len(), 3);
            assert_eq!(coords[0], [116.4, 39.9]);
        } else {
            panic!("Expected BoundaryUpdate variant");
        }
    }

    #[test]
    fn confirm_boundary_parsed_correctly() {
        // confirm_boundary 与 boundary_update 载荷同形但语义不同——
        // 映射必须严格区分（回归锁定：曾共用分支导致映射混淆）
        let json = r#"{
            "type": "confirm_boundary",
            "coords": [[116.4, 39.9], [116.5, 39.9], [116.5, 40.0]]
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::ConfirmBoundary { coords } = result {
            assert_eq!(coords.len(), 3);
        } else {
            panic!("Expected ConfirmBoundary variant");
        }
    }

    #[test]
    fn manual_point_parsed_correctly() {
        let json = r#"{
            "type": "manual_point",
            "point": [116.456, 39.876]
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::ManualPoint { lon, lat } = result {
            assert_eq!(lon, 116.456);
            assert_eq!(lat, 39.876);
        } else {
            panic!("Expected ManualPoint variant");
        }
    }

    #[test]
    fn manual_cancel_parsed_correctly() {
        let json = r#"{
            "type": "manual_cancel"
        }"#;
        let result = parse_ipc_message(json).unwrap();
        matches!(result, IpcMessage::ManualCancel);
    }

    #[test]
    fn manual_clear_parsed_correctly() {
        let json = r#"{
            "type": "manual_clear"
        }"#;
        let result = parse_ipc_message(json).unwrap();
        matches!(result, IpcMessage::ManualClear);
    }
}
