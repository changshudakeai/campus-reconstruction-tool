//! 数据源适配器缝（ADR-0013：数据源可插拔，同一 trait 多实现）。
//!
//! 每个数据源实现为一个独立适配器，只负责"查询指定边界 → 返回带标签的
//! 地理对象"这一件事；返回结果统一进入 B13 标签映射与后续评审流程。
//! 新增数据源 = 新增一个 [`DataSource`] 实现，不改动采集、评审、生成、导出。
//!
//! 默认实现 [`GaodeDataSource`]：复用 B3 高德客户端的 POI 搜索解析
//! （信封校验、学校类筛选、坐标校验、去重），并以适配器自己的
//! "typecode → 标签"翻译词典把高德类型码译成映射表词汇——翻译是
//! 数据源方言转换，归类仍完全由 B13 引擎裁决（ADR-0011）。

use data_transformers::TagMap;
use gaode_client::{parse_place_search_response, SCHOOL_TYPECODE_PREFIX};
use shared_domain_types::Boundary;

use crate::error::{AcquisitionError, Result};

/// 数据源明确提供的几何，不由 F4 根据标签或类别推测。
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SourceGeometry {
    /// 单一位置点。
    Point((f64, f64)),
    /// 有序折线。
    LineString(Vec<(f64, f64)>),
    /// 来源提供的面环。
    Polygon(Vec<(f64, f64)>),
}

/// 一个从数据源拉回的原始对象（带完整标签，等待 B13 归类）
#[derive(Debug, Clone, PartialEq)]
pub struct RawEntity {
    /// 真实世界对象 ID（来自数据源，落库主键之一）
    pub entity_id: String,
    /// 对象名称（评审列表展示用；可为空）
    pub name: String,
    /// 标签集（B13 归类的唯一输入）
    pub tags: TagMap,
    /// 数据源原始载荷（原样保全，数据粮仓的"完整原始标签"要求）
    pub source_payload: serde_json::Value,
    /// 来源几何；缺失时仍保留该原始对象，交由后续流程显式隔离。
    pub source_geometry: Option<SourceGeometry>,
    /// 同一来源对象的稳定几何分片标识。
    pub geometry_part_id: String,
}

impl RawEntity {
    /// 构造原始对象
    pub fn new(
        entity_id: impl Into<String>,
        name: impl Into<String>,
        tags: TagMap,
        source_payload: serde_json::Value,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            name: name.into(),
            tags,
            source_payload,
            source_geometry: None,
            geometry_part_id: "default".to_owned(),
        }
    }

    /// 用来源明确提供的几何建立原始对象。
    pub fn with_geometry(
        entity_id: impl Into<String>,
        name: impl Into<String>,
        tags: TagMap,
        source_payload: serde_json::Value,
        source_geometry: Option<SourceGeometry>,
        geometry_part_id: impl Into<String>,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            name: name.into(),
            tags,
            source_payload,
            source_geometry,
            geometry_part_id: geometry_part_id.into(),
        }
    }

    /// 组装写入数据粮仓的 source_data：名称 + 标签 + 原始载荷全量保全，
    /// 未来精细建筑模式可凭 tags 重新归类、凭 payload 还原现场。
    pub fn to_source_data(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "tags": self.tags,
            "payload": self.source_payload,
            "source_geometry": self.source_geometry.as_ref().map(source_geometry_json),
            "geometry_part_id": self.geometry_part_id,
        })
    }
}

/// 数据源适配器接口（ADR-0013 可插拔缝）
///
/// 对象安全：采集流水线以 `&dyn DataSource` 消费，换源零改动。
pub trait DataSource {
    /// 数据源标识（写入 raw_observations.data_source_tag，来源全程可见）
    fn source_tag(&self) -> &str;

    /// 查询指定边界内的原始对象（带完整标签）
    fn fetch_raw_entities(&self, boundary: &Boundary) -> Result<Vec<RawEntity>>;
}

/// 桥接传输：边界 → 高德地点搜索响应 JSON（REST 风格信封）。
///
/// 实际网络请求由壳层 WebView 的 JS 桥完成（B3 是纯逻辑层，不发请求）；
/// 壳层注入真实桥接闭包，测试注入罐头 JSON——离线可测。
pub type BridgeTransport =
    Box<dyn Fn(&Boundary) -> std::result::Result<String, String> + Send + Sync>;

/// 可注入的 Overpass `out geom` 传输；生产网络由外部适配器提供。
pub type OverpassTransport = BridgeTransport;

/// OSM/Overpass 来源适配器，保留 node/way 的原始几何，不拼接 relation。
pub struct OverpassDataSource {
    transport: OverpassTransport,
}

impl OverpassDataSource {
    pub const SOURCE_TAG: &'static str = "overpass";
    pub fn new(transport: OverpassTransport) -> Self {
        Self { transport }
    }
}

impl DataSource for OverpassDataSource {
    fn source_tag(&self) -> &str {
        Self::SOURCE_TAG
    }
    fn fetch_raw_entities(&self, boundary: &Boundary) -> Result<Vec<RawEntity>> {
        let payload =
            (self.transport)(boundary).map_err(|message| AcquisitionError::SourceUnreachable {
                source_tag: Self::SOURCE_TAG.to_owned(),
                message,
            })?;
        let value: serde_json::Value = serde_json::from_str(&payload).map_err(|error| {
            AcquisitionError::Source(gaode_client::Error::MalformedResponse(error.to_string()))
        })?;
        let elements = value
            .get("elements")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(elements.into_iter().filter_map(overpass_entity).collect())
    }
}

fn overpass_entity(value: serde_json::Value) -> Option<RawEntity> {
    let kind = value.get("type")?.as_str()?;
    let id = value.get("id")?.as_i64()?;
    let entity_id = format!("{kind}/{id}");
    let tags: TagMap = value
        .get("tags")
        .and_then(serde_json::Value::as_object)
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let name = tags
        .get("name")
        .cloned()
        .unwrap_or_else(|| entity_id.clone());
    let geometry = match kind {
        "node" => Some(SourceGeometry::Point((
            value.get("lon")?.as_f64()?,
            value.get("lat")?.as_f64()?,
        ))),
        "way" => value
            .get("geometry")
            .and_then(serde_json::Value::as_array)
            .map(|points| {
                points
                    .iter()
                    .filter_map(|point| {
                        Some((point.get("lon")?.as_f64()?, point.get("lat")?.as_f64()?))
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|points| !points.is_empty())
            .map(|points| {
                if points.len() > 2 && points.first() == points.last() {
                    SourceGeometry::Polygon(points)
                } else {
                    SourceGeometry::LineString(points)
                }
            }),
        _ => None,
    };
    Some(RawEntity::with_geometry(
        entity_id, name, tags, value, geometry, "default",
    ))
}

/// 高德 typecode 前缀 → 标签的翻译词典（适配器方言转换，非归类逻辑）。
///
/// 高德 POI 没有 OSM 式标签，只有类型码；此表把类型码族译成
/// B13 映射表（tag-rules.json）的同源词汇，归类裁决权仍在 B13。
/// 词典没收录的类型码给空标签集——B13 按纪律兜底进"其他"，禁止静默丢弃。
const GAODE_TYPECODE_TAG_DICT: &[(&str, &str, &str)] = &[
    // 体育休闲服务（0800xx 运动场馆族）→ 体育
    ("08", "leisure", "sports_centre"),
    // 公园广场（110100 公园/绿地）→ 植被
    ("1101", "leisure", "garden"),
    // 停车场（150900）→ 停车设施（映射表暂无规则，兜底进"其他"，评审可见）
    ("1509", "amenity", "parking"),
    // 通行设施（9900xx 门/出入口，校门在此族）→ 其他
    ("99", "barrier", "gate"),
];

/// 高德数据源适配器 —— F4 的默认数据源实现
pub struct GaodeDataSource {
    transport: BridgeTransport,
}

impl std::fmt::Debug for GaodeDataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GaodeDataSource").finish_non_exhaustive()
    }
}

impl GaodeDataSource {
    /// 高德数据来源标识（与 B3 校区记录的 data_source 同源）
    pub const SOURCE_TAG: &'static str = "gaode";

    /// 用桥接传输闭包构造（壳层注入 WebView 桥，测试注入罐头 JSON）
    pub fn new(transport: BridgeTransport) -> Self {
        Self { transport }
    }

    /// typecode → 标签集（词典查前缀；未收录返回空集，交 B13 兜底）
    fn typecode_to_tags(typecode: &str) -> TagMap {
        let mut tags = TagMap::new();
        if typecode.starts_with(SCHOOL_TYPECODE_PREFIX) {
            // 学校类（B3 同源前缀 1412）→ 建筑
            tags.insert("building".to_owned(), "school".to_owned());
            return tags;
        }
        for (prefix, key, value) in GAODE_TYPECODE_TAG_DICT {
            if typecode.starts_with(prefix) {
                tags.insert((*key).to_owned(), (*value).to_owned());
                return tags;
            }
        }
        tags
    }

    /// 解析响应中的全部 POI 为原始对象（信封已由 B3 校验通过）。
    ///
    /// 学校类以外的 POI 由 B3 静默过滤，故此处对全量 pois 逐条翻译；
    /// 同 id 去重；无 id 的条目无法作为实体主键，跳过（脏数据）。
    fn parse_all_pois(json: &str) -> Result<Vec<RawEntity>> {
        let value: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| gaode_client::Error::MalformedResponse(e.to_string()))?;
        let pois = value
            .get("pois")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut seen_ids: Vec<String> = Vec::new();
        let mut entities = Vec::new();
        for poi in pois {
            let id = poi
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            if id.is_empty() || seen_ids.contains(&id) {
                continue;
            }
            let name = poi
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let typecode = poi
                .get("typecode")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let tags = Self::typecode_to_tags(typecode);
            seen_ids.push(id.clone());
            let geometry = poi
                .get("location")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_gaode_location)
                .map(SourceGeometry::Point);
            entities.push(RawEntity::with_geometry(
                id, name, tags, poi, geometry, "point",
            ));
        }
        Ok(entities)
    }
}

fn parse_gaode_location(text: &str) -> Option<(f64, f64)> {
    let (longitude, latitude) = text.split_once(',')?;
    Some((
        longitude.trim().parse().ok()?,
        latitude.trim().parse().ok()?,
    ))
}

fn source_geometry_json(geometry: &SourceGeometry) -> serde_json::Value {
    match geometry {
        SourceGeometry::Point(point) => {
            serde_json::json!({"kind":"point","coordinates":[point.0, point.1]})
        }
        SourceGeometry::LineString(points) => {
            serde_json::json!({"kind":"line_string","coordinates":points})
        }
        SourceGeometry::Polygon(points) => {
            serde_json::json!({"kind":"polygon","coordinates":points})
        }
    }
}

impl DataSource for GaodeDataSource {
    fn source_tag(&self) -> &str {
        Self::SOURCE_TAG
    }

    fn fetch_raw_entities(&self, boundary: &Boundary) -> Result<Vec<RawEntity>> {
        let json =
            (self.transport)(boundary).map_err(|message| AcquisitionError::SourceUnreachable {
                source_tag: Self::SOURCE_TAG.to_owned(),
                message,
            })?;
        // 信封校验复用 B3：status != "1" / 畸形 JSON 由 B3 带类型上报
        parse_place_search_response(&json)?;
        Self::parse_all_pois(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary() -> Boundary {
        Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [121.40, 31.20],
                [121.41, 31.20],
                [121.41, 31.21],
                [121.40, 31.20]
            ]]),
        }
    }

    fn canned_source(json: &str) -> GaodeDataSource {
        let payload = json.to_owned();
        GaodeDataSource::new(Box::new(move |_| Ok(payload.clone())))
    }

    const CAMPUS_POIS: &str = r#"{"status":"1","info":"OK","pois":[
        {"id":"B01","name":"第一教学楼","address":"校内","location":"121.401,31.201","typecode":"141201"},
        {"id":"B02","name":"体育馆","address":"校内","location":"121.402,31.202","typecode":"080300"},
        {"id":"B03","name":"南校门","address":"校内","location":"121.403,31.203","typecode":"991400"},
        {"id":"B04","name":"神秘设施","address":"校内","location":"121.404,31.204","typecode":"070000"}
    ]}"#;

    #[test]
    fn fetch_translates_typecodes_to_tags() {
        let source = canned_source(CAMPUS_POIS);
        let entities = source.fetch_raw_entities(&boundary()).unwrap();
        assert_eq!(entities.len(), 4, "全部对象进流水线，禁止静默丢弃");

        let tag_of = |id: &str| {
            entities
                .iter()
                .find(|e| e.entity_id == id)
                .map(|e| e.tags.clone())
                .unwrap()
        };
        assert_eq!(tag_of("B01").get("building").unwrap(), "school");
        assert_eq!(tag_of("B02").get("leisure").unwrap(), "sports_centre");
        assert_eq!(tag_of("B03").get("barrier").unwrap(), "gate");
        assert!(
            tag_of("B04").is_empty(),
            "未收录类型码给空标签，交 B13 兜底"
        );
    }

    #[test]
    fn source_payload_is_preserved_verbatim() {
        let source = canned_source(CAMPUS_POIS);
        let entities = source.fetch_raw_entities(&boundary()).unwrap();
        let gym = entities.iter().find(|e| e.entity_id == "B02").unwrap();
        assert_eq!(gym.source_payload["typecode"], "080300");
        let source_data = gym.to_source_data();
        assert_eq!(source_data["tags"]["leisure"], "sports_centre");
        assert_eq!(source_data["payload"]["name"], "体育馆");
    }

    #[test]
    fn duplicated_and_id_less_pois_are_skipped() {
        let json = r#"{"status":"1","info":"OK","pois":[
            {"id":"B01","name":"甲","typecode":"080300"},
            {"id":"B01","name":"甲重复","typecode":"080300"},
            {"id":"","name":"无主数据","typecode":"080300"}
        ]}"#;
        let entities = canned_source(json).fetch_raw_entities(&boundary()).unwrap();
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn service_rejection_is_reported_via_b3() {
        let source = canned_source(r#"{"status":"0","info":"INVALID_USER_KEY","pois":[]}"#);
        let err = source.fetch_raw_entities(&boundary()).unwrap_err();
        assert!(matches!(err, AcquisitionError::Source(_)));
    }

    #[test]
    fn transport_failure_is_reported_as_unreachable() {
        let source = GaodeDataSource::new(Box::new(|_| Err("网络超时".to_owned())));
        let err = source.fetch_raw_entities(&boundary()).unwrap_err();
        assert!(matches!(
            err,
            AcquisitionError::SourceUnreachable { source_tag, .. } if source_tag == "gaode"
        ));
    }
}
