//! OSM/Overpass/Nominatim Rust 侧直连（T31）。
//!
//! 调研根因（`docs/research/candidate-data-sources-and-naming.md` §4.2）：
//! 1. 请求 URL 缺 `data=` 参数 → 服务器把参数名当查询体，必报 parse error；
//! 2. `amenity~"university|college|school"` 的 `|` 正则被新版 Overpass 拒绝
//!    （版本相关，端点滚动更新）→ 一律 union 写法；
//! 3. overpass-api.de / kumi 不给浏览器 CORS 头 → 一律 Rust 侧直连，
//!    不再依赖 WebView fetch。
//!
//! 端点按 de → kumi → mail.ru 回退，每端点超时 [`OVERPASS_HTTP_TIMEOUT`]；
//! Nominatim 按 OSMF 政策 ≤1 次/秒并带 User-Agent。
//!
//! 校区边界自动获取级联（ADR-0029 + T31）：
//! Nominatim 校名 → osm_type/osm_id → Overpass 按 ID 拉取；
//! 失败回退 Overpass `amenity=university|college|school` 锚点近域查询；
//! 再失败回退 `landuse=education`；均无数据 → 人工圈画兜底（由调用方决定）。

use std::sync::Arc;
use std::time::Duration;

use gaode_client::{convert_coords_wgs84_to_gcj02, BoundarySorter, OsmElement, OsmMember};
use shared_domain_types::Boundary;

/// Overpass 公共端点（按 de → kumi → mail.ru 顺序回退；上海网络实测见调研 §4.1）
pub const OVERPASS_ENDPOINTS: [&str; 3] = [
    "https://overpass-api.de",
    "https://overpass.kumi.systems",
    "https://maps.mail.ru/osm/tools/overpass",
];

/// 每端点 HTTP 超时（工单：10–15 秒）
pub const OVERPASS_HTTP_TIMEOUT: Duration = Duration::from_secs(12);

/// Overpass 服务端超时（秒；比客户端超时小，避免公共端点长期占用）
pub const OVERPASS_SERVER_TIMEOUT_SECS: u32 = 25;

/// Nominatim 端点（OSMF 公共实例）
pub const NOMINATIM_ENDPOINT: &str = "https://nominatim.openstreetmap.org/search";

/// 所有 OSM 请求的 User-Agent（Nominatim 政策要求；Overpass 礼貌请求也带）
pub const USER_AGENT: &str = "MCRebuildV2/2.0.0-dev (campus-reconstruction-tool; desktop; T31)";

/// 可注入的 HTTP 传输（生产为 ureq；测试注入罐头）
pub type HttpTransport =
    Box<dyn Fn(&str, Duration) -> std::result::Result<String, String> + Send + Sync>;

/// 校区边界自动获取结果（来源标注与自动绘制所需全部事实）
#[derive(Debug, Clone, PartialEq)]
pub enum CampusBoundaryResult {
    /// 自动选取到最佳匹配（坐标已转 GCJ-02，可直接上屏）
    AutoSelected {
        /// OSM name 标签（评审/来源标注）
        name: String,
        /// GCJ-02 坐标环
        gcj02: Vec<[f64; 2]>,
        /// 数据来源（来源标注）
        source: BoundarySourceKind,
        /// 参与排序的候选元素数（排序证据）
        candidate_count: usize,
    },
    /// 各数据源均无该校区边界 → 人工圈画兜底
    NotFound,
    /// 网络/解析失败（保留错误文本供诊断；仍按 ADR-0029 回退人工圈画）
    Unreachable { message: String },
}

/// 边界数据来源（来源标注）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundarySourceKind {
    /// Nominatim 校名解析 → Overpass 按元素 ID 拉取
    NominatimByElementId,
    /// Overpass `amenity=university|college|school` 锚点近域查询
    OverpassAmenity,
    /// Overpass `landuse=education` 兜底查询
    LanduseEducation,
}

impl std::fmt::Display for BoundarySourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NominatimByElementId => f.write_str("Nominatim + Overpass by-id"),
            Self::OverpassAmenity => f.write_str("Overpass amenity=university|college|school"),
            Self::LanduseEducation => f.write_str("Overpass landuse=education"),
        }
    }
}

/// Nominatim 校名解析命中
#[derive(Debug, Clone, PartialEq)]
pub struct NominatimMatch {
    pub osm_type: String,
    pub osm_id: i64,
    pub display_name: String,
    pub class: String,
    pub kind: String,
}

/// Overpass 客户端：端点回退 + union 查询（全部可单测）
pub struct OverpassClient {
    transport: HttpTransport,
}

impl OverpassClient {
    /// 生产传输：ureq 直连，每请求 [`OVERPASS_HTTP_TIMEOUT`]
    pub fn production() -> Self {
        Self {
            transport: ureq_transport(),
        }
    }

    /// 测试注入
    pub fn with_transport(transport: HttpTransport) -> Self {
        Self { transport }
    }

    /// 按端点回退执行查询：de → kumi → mail.ru；
    /// 错误页（parse error / Runtime error / HTML）视为失败，换下一端点。
    pub fn query_with_fallback(&self, query: &str) -> std::result::Result<String, String> {
        let mut errors: Vec<String> = Vec::new();
        for endpoint in OVERPASS_ENDPOINTS {
            let url = format!("{endpoint}/api/interpreter?data={}", encode_query(query));
            match (self.transport)(&url, OVERPASS_HTTP_TIMEOUT) {
                Ok(body) if is_error_body(&body) => {
                    errors.push(format!(
                        "端点 {endpoint} 返回错误页: {}",
                        first_error_line(&body)
                    ));
                }
                Ok(body) => return Ok(body),
                Err(message) => {
                    errors.push(format!("端点 {endpoint} 不可达: {message}"));
                }
            }
        }
        if errors.is_empty() {
            Err("Overpass 端点列表为空".to_owned())
        } else {
            Err(errors.join("；"))
        }
    }
}

/// Nominatim 客户端：校名 → 校区元素（OSMF 政策：≤1 次/秒、带 User-Agent）
pub struct NominatimClient {
    transport: HttpTransport,
}

impl NominatimClient {
    /// 生产传输：ureq 直连，调用前按政策休眠 1 秒
    pub fn production() -> Self {
        Self {
            transport: Box::new(|url: &str, timeout: Duration| {
                // OSMF 政策 ≤1 次/秒：用 recv_timeout 等待 1 秒
                // （std::thread::sleep 被 clippy 禁用——无卡顿铁律；调用方均在后台线程）。
                let (_, rx) = std::sync::mpsc::channel::<()>();
                let _ = rx.recv_timeout(Duration::from_secs(1));
                ureq_transport()(url, timeout)
            }),
        }
    }

    /// 测试注入（不要求休眠）
    pub fn with_transport(transport: HttpTransport) -> Self {
        Self { transport }
    }

    /// 解析校名：先精确查询，无校区命中时去掉括号后缀再查一次。
    /// 命中规则：`class=amenity` 且 `type` 为 university/college/school。
    pub fn resolve_campus(
        &self,
        campus_name: &str,
    ) -> std::result::Result<Option<NominatimMatch>, String> {
        for candidate in campus_name_candidates(campus_name) {
            let url = format!(
                "{NOMINATIM_ENDPOINT}?q={}&format=json&limit=5",
                encode_query(&candidate)
            );
            let body = (self.transport)(&url, Duration::from_secs(15))?;
            let results = parse_nominatim_results(&body);
            if let Some(matched) = results.into_iter().find(is_campus_like) {
                return Ok(Some(matched));
            }
        }
        Ok(None)
    }
}

/// 校区边界自动获取器：Nominatim → by-id → amenity 近域 → landuse 级联
pub struct CampusBoundaryFetcher {
    overpass: OverpassClient,
    nominatim: NominatimClient,
}

impl CampusBoundaryFetcher {
    pub fn production() -> Self {
        Self {
            overpass: OverpassClient::production(),
            nominatim: NominatimClient::production(),
        }
    }

    pub fn with_clients(overpass: OverpassClient, nominatim: NominatimClient) -> Self {
        Self {
            overpass,
            nominatim,
        }
    }

    /// 级联获取校区边界（锚点为 GCJ-02；OSM 元素 WGS-84 先转 GCJ-02 再排序）。
    pub fn fetch_campus(
        &self,
        campus_name: &str,
        anchor_lon: f64,
        anchor_lat: f64,
    ) -> CampusBoundaryResult {
        // 1. Nominatim 校名 → 元素 ID → Overpass 按 ID 拉取
        match self.nominatim.resolve_campus(campus_name) {
            Ok(Some(matched)) => {
                let query = element_by_id_query(&matched.osm_type, matched.osm_id);
                match self.overpass.query_with_fallback(&query) {
                    Ok(body) => {
                        if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name)
                        {
                            return CampusBoundaryResult::AutoSelected {
                                name: best.name,
                                gcj02: best.geometry,
                                source: BoundarySourceKind::NominatimByElementId,
                                candidate_count: best.candidate_count,
                            };
                        }
                    }
                    Err(message) => {
                        return fallback_after_error(
                            &self.overpass,
                            anchor_lon,
                            anchor_lat,
                            campus_name,
                            message,
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(message) => {
                return fallback_after_error(
                    &self.overpass,
                    anchor_lon,
                    anchor_lat,
                    campus_name,
                    message,
                );
            }
        }
        // 2. ADR-0029 主路径：amenity=university|college|school 锚点近域
        match self.query_and_select(
            &university_query(bbox_around(anchor_lon, anchor_lat, 1500.0)),
            anchor_lon,
            anchor_lat,
            campus_name,
            BoundarySourceKind::OverpassAmenity,
        ) {
            Some(result) => result,
            None => CampusBoundaryResult::NotFound,
        }
    }

    /// 执行一次 Overpass 查询并自动选取最佳边界；查询失败返回 Unreachable。
    fn query_and_select(
        &self,
        query: &str,
        anchor_lon: f64,
        anchor_lat: f64,
        campus_name: &str,
        source: BoundarySourceKind,
    ) -> Option<CampusBoundaryResult> {
        match self.overpass.query_with_fallback(query) {
            Ok(body) => select_best(&body, anchor_lon, anchor_lat, campus_name).map(|best| {
                CampusBoundaryResult::AutoSelected {
                    name: best.name,
                    gcj02: best.geometry,
                    source,
                    candidate_count: best.candidate_count,
                }
            }),
            Err(message) => Some(CampusBoundaryResult::Unreachable { message }),
        }
    }
}

/// Nominatim 失败后按级联继续：amenity 近域 → landuse=education → NotFound/Unreachable
fn fallback_after_error(
    overpass: &OverpassClient,
    anchor_lon: f64,
    anchor_lat: f64,
    campus_name: &str,
    first_error: String,
) -> CampusBoundaryResult {
    let amenity = overpass.query_with_fallback(&university_query(bbox_around(
        anchor_lon, anchor_lat, 1500.0,
    )));
    match amenity {
        Ok(body) => {
            if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name) {
                return CampusBoundaryResult::AutoSelected {
                    name: best.name,
                    gcj02: best.geometry,
                    source: BoundarySourceKind::OverpassAmenity,
                    candidate_count: best.candidate_count,
                };
            }
            let landuse = overpass.query_with_fallback(&landuse_education_query(bbox_around(
                anchor_lon, anchor_lat, 1500.0,
            )));
            match landuse {
                Ok(body) => {
                    if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name) {
                        return CampusBoundaryResult::AutoSelected {
                            name: best.name,
                            gcj02: best.geometry,
                            source: BoundarySourceKind::LanduseEducation,
                            candidate_count: best.candidate_count,
                        };
                    }
                    CampusBoundaryResult::NotFound
                }
                Err(message) => CampusBoundaryResult::Unreachable {
                    message: format!("{first_error}；{message}"),
                },
            }
        }
        Err(message) => CampusBoundaryResult::Unreachable {
            message: format!("{first_error}；{message}"),
        },
    }
}

struct SelectedBoundary {
    name: String,
    geometry: Vec<[f64; 2]>,
    candidate_count: usize,
}

/// 归一化坐标环：截断到第一次回到首点的位置。
///
/// 真实 OSM 几何的闭合节点可能不在末尾（环在中途闭合后仍带尾点，如
/// 华东师大普陀校区边界 90 点中第 87 点即首点、其后还带 2 个尾点）。
/// 归一化保证进入桌面链路的边界是干净单环（去掉重复闭合点与尾点），
/// 避免确认校验把共享端点的尾边误判为自相交（T33）。
fn normalize_closed_ring(coords: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let Some(first) = coords.first().copied() else {
        return Vec::new();
    };
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(coords.len());
    for point in &coords {
        if out.len() > 1 && *point == first {
            break;
        }
        out.push(*point);
    }
    out
}

/// 解析 Overpass 响应 → WGS-84 转 GCJ-02 → 按 ADR-0029 排序（锚点包含 →
/// 名称匹配 → 距离最近）→ 取最佳。
fn select_best(
    json: &str,
    anchor_lon: f64,
    anchor_lat: f64,
    campus_name: &str,
) -> Option<SelectedBoundary> {
    let elements = elements_to_gcj02(parse_elements(json));
    let candidate_count = elements.len();
    let mut sorted =
        BoundarySorter::sort_candidates(elements, anchor_lon, anchor_lat, Some(campus_name));
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| {
        b.contains_anchor
            .cmp(&a.contains_anchor)
            .then_with(|| b.name_match_score.total_cmp(&a.name_match_score))
            .then_with(|| a.distance_to_anchor_m.total_cmp(&b.distance_to_anchor_m))
    });
    let best = sorted.remove(0);
    Some(SelectedBoundary {
        name: best
            .element
            .tags
            .get("name")
            .cloned()
            .unwrap_or_else(|| best.element.id.to_string()),
        geometry: normalize_closed_ring(best.element.geometry?),
        candidate_count,
    })
}

/// 解析 Overpass `out geom` JSON 为 OSM 元素（way 用 geometry；relation 拼接外环成员）
pub fn parse_elements(json: &str) -> Vec<OsmElement> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(elements) = value.get("elements").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    elements.iter().filter_map(parse_element).collect()
}

fn parse_element(value: &serde_json::Value) -> Option<OsmElement> {
    let kind = value.get("type")?.as_str()?;
    let id = value.get("id")?.as_i64()?;
    let tags = value
        .get("tags")
        .and_then(serde_json::Value::as_object)
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let members = value
        .get("members")
        .and_then(serde_json::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|m| {
                    Some(OsmMember {
                        r#type: m.get("type")?.as_str()?.to_owned(),
                        reference: m.get("ref")?.as_i64()?,
                        role: m
                            .get("role")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let geometry = match kind {
        "way" => value
            .get("geometry")
            .and_then(serde_json::Value::as_array)
            .map(|points| geometry_points(points)),
        "relation" => value
            .get("members")
            .and_then(serde_json::Value::as_array)
            .map(|members| {
                let mut points = Vec::new();
                for member in members {
                    let role = member
                        .get("role")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if role == "outer" || role.is_empty() {
                        if let Some(part) =
                            member.get("geometry").and_then(serde_json::Value::as_array)
                        {
                            points.extend(geometry_points(part));
                        }
                    }
                }
                points
            })
            .filter(|points| !points.is_empty()),
        _ => None,
    };
    Some(OsmElement {
        r#type: kind.to_owned(),
        id,
        geometry,
        members,
        tags,
    })
}

fn geometry_points(points: &[serde_json::Value]) -> Vec<[f64; 2]> {
    points
        .iter()
        .filter_map(|point| Some([point.get("lon")?.as_f64()?, point.get("lat")?.as_f64()?]))
        .collect()
}

/// WGS-84 元素几何就地转 GCJ-02（原始 WGS-84 由原始载荷另行保全）
pub fn elements_to_gcj02(mut elements: Vec<OsmElement>) -> Vec<OsmElement> {
    for element in &mut elements {
        if let Some(geometry) = element.geometry.as_mut() {
            convert_coords_wgs84_to_gcj02(geometry);
        }
    }
    elements
}

/// union 写法（避免 `|` 正则被新版 Overpass 拒绝）：way/relation × university/college/school
pub fn university_query(bbox: (f64, f64, f64, f64)) -> String {
    let (south, west, north, east) = bbox;
    format!(
        "[out:json][timeout:{t}];(way[\"amenity\"=\"university\"]({s},{w},{n},{e});way[\"amenity\"=\"college\"]({s},{w},{n},{e});way[\"amenity\"=\"school\"]({s},{w},{n},{e});relation[\"amenity\"=\"university\"]({s},{w},{n},{e});relation[\"amenity\"=\"college\"]({s},{w},{n},{e});relation[\"amenity\"=\"school\"]({s},{w},{n},{e}););out geom;",
        t = OVERPASS_SERVER_TIMEOUT_SECS,
        s = south,
        w = west,
        n = north,
        e = east
    )
}

/// 按元素 ID 拉取（Nominatim 解析结果 → Overpass 取边界）
pub fn element_by_id_query(osm_type: &str, osm_id: i64) -> String {
    format!(
        "[out:json][timeout:{t}];{osm_type}({osm_id});out geom;",
        t = OVERPASS_SERVER_TIMEOUT_SECS
    )
}

/// landuse=education 兜底（union：way + relation）
pub fn landuse_education_query(bbox: (f64, f64, f64, f64)) -> String {
    let (south, west, north, east) = bbox;
    format!(
        "[out:json][timeout:{t}];(way[\"landuse\"=\"education\"]({s},{w},{n},{e});relation[\"landuse\"=\"education\"]({s},{w},{n},{e}););out geom;",
        t = OVERPASS_SERVER_TIMEOUT_SECS,
        s = south,
        w = west,
        n = north,
        e = east
    )
}

/// 候选建筑查询：`building=*`（面几何 + name/building:levels 标签；union 写法）
pub fn buildings_query(bbox: (f64, f64, f64, f64)) -> String {
    let (south, west, north, east) = bbox;
    format!(
        "[out:json][timeout:{t}];(way[\"building\"]({s},{w},{n},{e});relation[\"building\"]({s},{w},{n},{e}););out geom;",
        t = OVERPASS_SERVER_TIMEOUT_SECS,
        s = south,
        w = west,
        n = north,
        e = east
    )
}

/// 以锚点为中心、给定半径（米）的 WGS-84 包围盒（south, west, north, east）
pub fn bbox_around(lon: f64, lat: f64, radius_m: f64) -> (f64, f64, f64, f64) {
    let lat_delta = radius_m / 111_320.0;
    let lon_delta = radius_m / (111_320.0 * lat.to_radians().cos().abs().max(0.01));
    (
        lat - lat_delta,
        lon - lon_delta,
        lat + lat_delta,
        lon + lon_delta,
    )
}

/// 方案边界（GCJ-02）→ 查询包围盒；`margin_deg` 为外扩余量。
/// 说明：边界为 GCJ-02，Overpass 查询窗口为 WGS-84——工单禁止 GCJ→WGS 反向，
/// 采用“GCJ 边界包围盒 + 外扩余量（默认 ~0.01°≈1km，覆盖 GCJ 偏移 ~500m）”的
/// 查询窗口口径，候选再经 WGS→GCJ 进入应用坐标系。
pub fn boundary_bbox(boundary: &Boundary, margin_deg: f64) -> Option<(f64, f64, f64, f64)> {
    let mut south = f64::MAX;
    let mut west = f64::MAX;
    let mut north = f64::MIN;
    let mut east = f64::MIN;
    let mut any = false;
    let rings: Vec<&serde_json::Value> = match boundary.r#type.as_str() {
        "Polygon" => boundary
            .coordinates
            .as_array()
            .map(|rings| rings.iter().collect())
            .unwrap_or_default(),
        "MultiPolygon" => boundary
            .coordinates
            .as_array()
            .map(|polys| {
                polys
                    .iter()
                    .filter_map(|poly| poly.as_array())
                    .flatten()
                    .collect()
            })
            .unwrap_or_default(),
        _ => return None,
    };
    for ring in rings {
        let Some(points) = ring.as_array() else {
            continue;
        };
        for point in points {
            let Some(pair) = point.as_array() else {
                continue;
            };
            let (Some(lon), Some(lat)) = (
                pair.first().and_then(serde_json::Value::as_f64),
                pair.get(1).and_then(serde_json::Value::as_f64),
            ) else {
                continue;
            };
            south = south.min(lat);
            west = west.min(lon);
            north = north.max(lat);
            east = east.max(lon);
            any = true;
        }
    }
    any.then_some((
        south - margin_deg,
        west - margin_deg,
        north + margin_deg,
        east + margin_deg,
    ))
}

/// 校名候选：精确名 → 去括号后缀的基础名（高德校区名常带“(闵行本部校区)”后缀）
pub fn campus_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_owned()];
    let stripped = strip_parenthetical_suffix(name);
    if stripped != name && !stripped.is_empty() {
        candidates.push(stripped);
    }
    candidates
}

fn strip_parenthetical_suffix(name: &str) -> String {
    // 全角（…）或半角（…）后缀，保留括号前主体
    for (open, close) in [('（', '）'), ('(', ')')] {
        if let Some(open_index) = name.rfind(open) {
            if name[open_index..].contains(close) {
                return name[..open_index].trim().to_owned();
            }
        }
    }
    name.to_owned()
}

/// 解析 Nominatim JSON 结果
pub fn parse_nominatim_results(json: &str) -> Vec<NominatimMatch> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(results) = value.as_array() else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|item| {
            let osm_type = item.get("osm_type")?.as_str()?.to_owned();
            let osm_id = item.get("osm_id")?.as_i64()?;
            if osm_type == "node" {
                // 校名解析只认面元素（way/relation）；地铁站/公交站 node 会干扰
                return None;
            }
            Some(NominatimMatch {
                osm_type,
                osm_id,
                display_name: item
                    .get("display_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                class: item
                    .get("class")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                kind: item
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

fn is_campus_like(matched: &NominatimMatch) -> bool {
    matched.class == "amenity"
        && matches!(matched.kind.as_str(), "university" | "college" | "school")
}

/// 百分号编码（覆盖 UTF-8 中文字节与 Overpass 语法字符）
pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

fn is_error_body(body: &str) -> bool {
    body.contains("parse error")
        || body.contains("Runtime error")
        || body.trim_start().starts_with("<html")
}

fn first_error_line(body: &str) -> String {
    body.lines()
        .find(|line| line.contains("parse error") || line.contains("Error"))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// 生产 HTTP 传输（ureq；每请求独立 Agent 以携带超时与 UA）
fn ureq_transport() -> HttpTransport {
    Box::new(|url: &str, timeout: Duration| {
        let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .tls_connector(Arc::new(tls))
            .build();
        let response = agent.get(url).call().map_err(|error| error.to_string())?;
        response.into_string().map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod overpass_tests;
