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
    /// 高德分类文本（JS API v2.0 的 `type` 字段，如 "科教文化服务;学校;高等院校"；
    /// REST/旧版无该字段时为空）。与 `typecode` 互补：JS API v2.0 的 POI 常无
    /// `typecode`，只有该文本分类，二者任一命中学校类目即保留。
    pub category: String,
}

impl SchoolPoi {
    /// 是否属于学校类地点（教育类目 1412 前缀；或 JS API v2.0 分类文本含"学校"）
    pub fn is_school(&self) -> bool {
        self.typecode.starts_with(SCHOOL_TYPECODE_PREFIX)
            || (self.category.contains('学')
                && (self.category.contains("学校") || self.category.contains("大学")))
    }
}

/// 高德地点搜索响应中的一条原始 POI（JS 桥回传的 JSON）
///
/// D-1：`location` 兼容三种来源格式——REST 风格文本 `"经度,纬度"`、
/// JS API v2.0 对象 `{"lng":..,"lat":..}` 与数组 `[lng, lat]`。
/// 坐标解析失败按"脏数据跳过"处理，不得因格式差异让整包反序列化失败。
#[derive(Debug, Deserialize)]
struct RawPoi {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    address: serde_json::Value,
    #[serde(default)]
    location: serde_json::Value,
    #[serde(default)]
    typecode: String,
    /// JS API v2.0 的分类文本字段名为 `type`（Rust 关键字，用 rename 绑定）
    #[serde(default, rename = "type")]
    category: String,
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
        let Some((longitude, latitude)) = parse_location_value(&raw.location) else {
            continue;
        };
        let poi = SchoolPoi {
            poi_id: raw.id,
            name: raw.name,
            address: flatten_address(&raw.address),
            longitude,
            latitude,
            typecode: raw.typecode,
            category: raw.category,
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

/// 解析高德 POI 坐标（三种格式：文本 / 对象 / 数组），非法格式返回 None。
///
/// - 文本：`"经度,纬度"`（REST 风格）；
/// - 对象：`{"lng":..,"lat":..}`（JS API v2.0 序列化形式）；
/// - 数组：`[经度, 纬度]`（JS API v2.0 `LngLat.toJSON()` 形式）。
///
/// 坐标必须有限且在合法范围内（高德为 GCJ-02，数值范围同 WGS-84）。
pub fn parse_location_value(location: &serde_json::Value) -> Option<(f64, f64)> {
    match location {
        serde_json::Value::String(text) => parse_location_text(text),
        serde_json::Value::Array(pair) => {
            let longitude = pair.first()?.as_f64()?;
            let latitude = pair.get(1)?.as_f64()?;
            validate_location(longitude, latitude)
        }
        serde_json::Value::Object(map) => {
            let longitude = map.get("lng").and_then(serde_json::Value::as_f64)?;
            let latitude = map.get("lat").and_then(serde_json::Value::as_f64)?;
            validate_location(longitude, latitude)
        }
        _ => None,
    }
}

/// 解析高德 "经度,纬度" 文本，非法格式返回 None
fn parse_location_text(location: &str) -> Option<(f64, f64)> {
    let (lng_text, lat_text) = location.split_once(',')?;
    let longitude: f64 = lng_text.trim().parse().ok()?;
    let latitude: f64 = lat_text.trim().parse().ok()?;
    validate_location(longitude, latitude)
}

/// 合法经纬度范围粗校验（高德为 GCJ-02，数值范围同 WGS-84）
fn validate_location(longitude: f64, latitude: f64) -> Option<(f64, f64)> {
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
    fn js_api_v2_object_and_array_locations_are_accepted() {
        // D-1：真实 JS API v2.0 响应中 POI location 可能序列化为
        // 对象 {"lng":..,"lat":..} 或数组 [lng, lat]，必须与 REST 风格
        // "经度,纬度" 文本并存，坐标真实进入候选，不得静默丢弃。
        let json = response_with(
            r#"{"id":"B01","name":"华东师范大学(普陀校区)","address":"中山北路3663号","location":{"lng":121.406,"lat":31.228},"typecode":"141201"},
               {"id":"B02","name":"华东师范大学(闵行校区)","address":"东川路500号","location":[121.456,31.033],"typecode":"141201"},
               {"id":"B03","name":"第三中学","address":"学院路1号","location":"121.4,31.2","typecode":"141202"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert_eq!(pois.len(), 3, "三种 location 格式都必须解析，不得静默丢弃");
        assert_eq!((pois[0].longitude, pois[0].latitude), (121.406, 31.228));
        assert_eq!((pois[1].longitude, pois[1].latitude), (121.456, 31.033));
        assert_eq!((pois[2].longitude, pois[2].latitude), (121.4, 31.2));
    }

    #[test]
    fn js_api_v2_type_text_classifies_school_without_typecode() {
        // Real JS API v2.0 PlaceSearch POIs have no typecode/typeCode/type_code;
        // only the `type` classification text (e.g. "科教文化服务;学校;高等院校").
        // typecode-prefix-only filtering would drop every campus (T30 D-3 probe).
        let json = response_with(
            r#"{"id":"B00155R1D5","name":"上海交通大学(闵行本部校区)","address":"东川路800号","location":{"lng":121.436882,"lat":31.025626},"type":"科教文化服务;学校;高等院校"},
               {"id":"B00155L3CA","name":"上海交通大学徐汇校区","address":"华山路1954号","location":{"lng":121.433095,"lat":31.199005},"type":"科教文化服务;学校;高等院校"},
               {"id":"B09","name":"闵行路公交站","address":"闵行路","location":{"lng":121.45,"lat":31.02},"type":"科教文化服务;文化科技;通讯信号"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert_eq!(
            pois.len(),
            2,
            "type text with school keeps campus, traffic signal dropped"
        );
        assert_eq!(pois[0].poi_id, "B00155R1D5");
        assert_eq!(pois[0].category, "科教文化服务;学校;高等院校");
        assert!(pois[0].is_school());
        assert!(pois[1].is_school());
    }

    #[test]
    fn vertex_editing_ipc_messages_parse_correctly() {
        let selected =
            parse_ipc_message(r#"{"type":"vertex_selected","index":2,"count":5}"#).unwrap();
        assert_eq!(selected, IpcMessage::VertexSelected { index: 2, count: 5 });
        assert_eq!(
            parse_ipc_message(r#"{"type":"vertex_deselected"}"#).unwrap(),
            IpcMessage::VertexDeselected
        );
        let rejected =
            parse_ipc_message(r#"{"type":"delete_vertex_rejected","reason":"too_few_points"}"#)
                .unwrap();
        assert_eq!(
            rejected,
            IpcMessage::DeleteVertexRejected {
                reason: "too_few_points".to_owned()
            }
        );
    }

    #[test]
    fn invalid_object_and_array_locations_are_skipped() {
        // 对象/数组格式同样执行范围校验与缺失校验，坏数据不进候选。
        let json = response_with(
            r#"{"id":"B01","name":"某小学","address":"路1号","location":{"lng":200.0,"lat":31.2},"typecode":"141203"},
               {"id":"B02","name":"某中学","address":"路2号","location":[121.4],"typecode":"141202"},
               {"id":"B03","name":"某大学","address":"路3号","location":{},"typecode":"141201"}"#,
        );
        let pois = parse_place_search_response(&json).unwrap();
        assert!(pois.is_empty(), "越界/缺维/空对象坐标都不进候选列表");
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

/// T23/T24/T25: IPC 消息类型（取点页/边界编辑页/朝向页面回传）
#[derive(Debug, Clone, PartialEq)]
pub enum IpcMessage {
    /// 坐标："经度，纬度" 字符串（T23 pick point / T24 manual_point）
    Coordinate { longitude: f64, latitude: f64 },
    /// 错误：结构化 JSON
    Error { message: String },
    /// T31: 地图就绪 → Rust 侧发起 OSM 边界自动获取（绕开 WebView CORS）
    MapReady,
    // T24: OSM 边界编辑相关
    /// OSM Overpass 返回的原始要素列表 (osm_elements)
    OsmElements { elements: Vec<OsmElement> },
    /// 编辑后的多边形坐标 (boundary_update: GCJ-02 坐标数组)
    BoundaryUpdate { coords: Vec<[f64; 2]> },
    /// 用户选中一个边界顶点 (vertex_selected: index + 当前点数)
    VertexSelected { index: u32, count: u32 },
    /// 用户取消选中 (vertex_deselected)
    VertexDeselected,
    /// 删除选中顶点被拒绝（剩余点数不足）(delete_vertex_rejected)
    DeleteVertexRejected { reason: String },
    /// 人工圈画落点 (manual_point: WGS-84 单个点；total 供抽屉 ① 显示点数)
    ManualPoint { lon: f64, lat: f64, total: u32 },
    /// 撤销上一个点 (manual_cancel)
    ManualCancel,
    /// 清空重画 (manual_clear)
    ManualClear,
    /// 确认最终边界 (confirm_boundary: GCJ-02 or WGS-84 TBD)
    ConfirmBoundary { coords: Vec<[f64; 2]> },
    /// GeoJSON Polygon/MultiPolygon boundary submitted by the map seam.
    BoundaryGeometryUpdate {
        r#type: String,
        coordinates: serde_json::Value,
    },
    /// Confirmed GeoJSON Polygon/MultiPolygon boundary submitted by the map seam.
    ConfirmBoundaryGeometry {
        r#type: String,
        coordinates: serde_json::Value,
    },
    // T25: 朝向模式
    /// 朝向点击两点 [(lng,lat), (lng,lat)]
    OrientationPoints { points: [[f64; 2]; 2] },
    /// 确认朝向并请求计算角度 (同上)
    ConfirmOrientation { points: [[f64; 2]; 2] },
    /// 清除朝向点
    OrientationClear,
    // T38: 评审地图
    /// 点击评审地图上的候选对象 → 高亮对应卡片（双向联动）
    ReviewObjectClicked { candidate_id: String },
    /// 评审地图“显示地图文字”开关切换（true = 恢复地标/POI 文字）。
    ReviewMapTextToggled { visible: bool },
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
/// - JSON 含 `type="vertex_selected"` → [`IpcMessage::VertexSelected`]
/// - JSON 含 `type="vertex_deselected"` → [`IpcMessage::VertexDeselected`]
/// - JSON 含 `type="delete_vertex_rejected"` → [`IpcMessage::DeleteVertexRejected`]
/// - JSON 含 `type="manual_point"` → [`IpcMessage::ManualPoint`]
/// - JSON 含 `type="manual_cancel"` → [`IpcMessage::ManualCancel`]
/// - JSON 含 `type="manual_clear"` → [`IpcMessage::ManualClear`]
/// - JSON 含 `type="confirm_boundary"` → [`IpcMessage::ConfirmBoundary`]
/// - JSON 含 `type="orientation_points"` → [`IpcMessage::OrientationPoints`]
/// - JSON 含 `type="confirm_orientation"` → [`IpcMessage::ConfirmOrientation`]
/// - JSON 含 `type="orientation_clear"` → [`IpcMessage::OrientationClear`]
/// - JSON 含 `type="review_object_clicked"` → [`IpcMessage::ReviewObjectClicked`]
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
                "map_ready" => {
                    return Ok(IpcMessage::MapReady);
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
                        #[serde(default)]
                        geometry: Option<BoundaryGeometryPayload>,
                    }
                    #[derive(Deserialize)]
                    struct BoundaryGeometryPayload {
                        #[serde(rename = "type")]
                        r#type: String,
                        coordinates: serde_json::Value,
                    }
                    if let Ok(payload) = serde_json::from_str::<BoundaryPayload>(msg) {
                        if let Some(geometry) = payload.geometry {
                            if type_payload.r#type == "boundary_update" {
                                return Ok(IpcMessage::BoundaryGeometryUpdate {
                                    r#type: geometry.r#type,
                                    coordinates: geometry.coordinates,
                                });
                            }
                            return Ok(IpcMessage::ConfirmBoundaryGeometry {
                                r#type: geometry.r#type,
                                coordinates: geometry.coordinates,
                            });
                        }
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
                "vertex_selected" => {
                    #[derive(Deserialize)]
                    struct VertexSelectedPayload {
                        #[serde(default)]
                        index: Option<u32>,
                        #[serde(default)]
                        count: Option<u32>,
                    }
                    if let Ok(payload) = serde_json::from_str::<VertexSelectedPayload>(msg) {
                        if let Some(index) = payload.index {
                            return Ok(IpcMessage::VertexSelected {
                                index,
                                count: payload.count.unwrap_or(0),
                            });
                        }
                    }
                }
                "vertex_deselected" => {
                    return Ok(IpcMessage::VertexDeselected);
                }
                "delete_vertex_rejected" => {
                    #[derive(Deserialize)]
                    struct DeleteVertexRejectedPayload {
                        #[serde(default)]
                        reason: String,
                    }
                    if let Ok(payload) = serde_json::from_str::<DeleteVertexRejectedPayload>(msg) {
                        return Ok(IpcMessage::DeleteVertexRejected {
                            reason: payload.reason,
                        });
                    }
                }
                "manual_point" => {
                    #[derive(Deserialize)]
                    struct ManualPointPayload {
                        #[serde(default)]
                        point: Option<[f64; 2]>,
                        #[serde(default)]
                        total: Option<u32>,
                    }
                    if let Ok(payload) = serde_json::from_str::<ManualPointPayload>(msg) {
                        if let Some([lon, lat]) = payload.point {
                            return Ok(IpcMessage::ManualPoint {
                                lon,
                                lat,
                                total: payload.total.unwrap_or(0),
                            });
                        }
                    }
                }
                "manual_cancel" => {
                    return Ok(IpcMessage::ManualCancel);
                }
                "manual_clear" => {
                    return Ok(IpcMessage::ManualClear);
                }
                "orientation_points" | "confirm_orientation" => {
                    #[derive(Deserialize)]
                    struct OrientationPayload {
                        #[serde(default)]
                        points: Option<[[f64; 2]; 2]>,
                    }
                    if let Ok(payload) = serde_json::from_str::<OrientationPayload>(msg) {
                        if let Some(points) = payload.points {
                            if type_payload.r#type == "orientation_points" {
                                return Ok(IpcMessage::OrientationPoints { points });
                            }
                            return Ok(IpcMessage::ConfirmOrientation { points });
                        }
                    }
                }
                "orientation_clear" => {
                    return Ok(IpcMessage::OrientationClear);
                }
                "review_object_clicked" => {
                    #[derive(Deserialize)]
                    struct ReviewClickPayload {
                        #[serde(default)]
                        candidate_id: String,
                    }
                    if let Ok(payload) = serde_json::from_str::<ReviewClickPayload>(msg) {
                        if !payload.candidate_id.is_empty() {
                            return Ok(IpcMessage::ReviewObjectClicked {
                                candidate_id: payload.candidate_id,
                            });
                        }
                    }
                }
                "review_map_text_toggled" => {
                    #[derive(Deserialize)]
                    struct ReviewMapTextPayload {
                        #[serde(default)]
                        visible: bool,
                    }
                    if let Ok(payload) = serde_json::from_str::<ReviewMapTextPayload>(msg) {
                        return Ok(IpcMessage::ReviewMapTextToggled {
                            visible: payload.visible,
                        });
                    }
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
    fn map_ready_payload_triggers_rust_side_fetch() {
        // T31：JS 只发就绪信号，Overpass 查询由 Rust 侧执行
        let result = parse_ipc_message(r#"{"type":"map_ready"}"#).unwrap();
        assert!(matches!(result, IpcMessage::MapReady));
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
    fn confirmed_multipolygon_geometry_is_preserved_for_f9() {
        let json = r#"{
            "type": "confirm_boundary",
            "geometry": {
                "type": "MultiPolygon",
                "coordinates": [[[[116.4, 39.9], [116.5, 39.9], [116.5, 40.0], [116.4, 39.9]]]]
            }
        }"#;
        let result = parse_ipc_message(json).unwrap();
        match result {
            IpcMessage::ConfirmBoundaryGeometry {
                r#type,
                coordinates,
            } => {
                assert_eq!(r#type, "MultiPolygon");
                assert_eq!(coordinates[0][0][0][0], 116.4);
            }
            other => panic!("expected geometry-preserving confirmation, got {other:?}"),
        }
    }

    #[test]
    fn manual_point_parsed_correctly() {
        let json = r#"{
            "type": "manual_point",
            "point": [116.456, 39.876],
            "total": 3
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::ManualPoint { lon, lat, total } = result {
            assert_eq!(lon, 116.456);
            assert_eq!(lat, 39.876);
            assert_eq!(total, 3);
        } else {
            panic!("Expected ManualPoint variant");
        }
    }

    #[test]
    fn manual_point_without_total_defaults_to_zero() {
        let json = r#"{
            "type": "manual_point",
            "point": [116.456, 39.876]
        }"#;
        let result = parse_ipc_message(json).unwrap();
        if let IpcMessage::ManualPoint { total, .. } = result {
            assert_eq!(total, 0);
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

    #[test]
    fn review_object_clicked_payload_parsed_correctly() {
        // T38：点击评审地图对象 → 高亮对应卡片
        let result = parse_ipc_message(
            r#"{"type":"review_object_clicked","candidate_id":"overpass:way/1:outer"}"#,
        )
        .unwrap();
        assert!(matches!(
            result,
            IpcMessage::ReviewObjectClicked { candidate_id } if candidate_id == "overpass:way/1:outer"
        ));

        // 空 candidate_id 视为不支持的消息（不产生无主高亮）
        assert!(
            parse_ipc_message(r#"{"type":"review_object_clicked","candidate_id":""}"#).is_err(),
            "空候选 ID 不得产生高亮请求"
        );
    }

    #[test]
    fn review_map_text_toggled_payload_parsed_correctly() {
        let result =
            parse_ipc_message(r#"{"type":"review_map_text_toggled","visible":true}"#).unwrap();
        assert!(matches!(
            result,
            IpcMessage::ReviewMapTextToggled { visible: true }
        ));

        let result =
            parse_ipc_message(r#"{"type":"review_map_text_toggled","visible":false}"#).unwrap();
        assert!(matches!(
            result,
            IpcMessage::ReviewMapTextToggled { visible: false }
        ));
    }
}
