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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use data_persistence::CandidateNameSource;
use data_transformers::TagMap;
use gaode_client::{
    convert_pairs_wgs84_to_gcj02, parse_location_value, parse_place_search_response,
    wgs84_to_gcj02, SCHOOL_TYPECODE_PREFIX,
};
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

impl SourceGeometry {
    /// 从数据粮仓 `source_data.source_geometry`（本 crate 序列化格式）读回来源几何。
    ///
    /// 原始观测只封存来源证据（ADR-0040），重验证在本地从该字段还原几何，
    /// 不需要重新联网拉取。
    pub fn from_source_data(source_data: &serde_json::Value) -> Option<Self> {
        let value = source_data.get("source_geometry")?;
        match value.get("kind")?.as_str()? {
            "point" => {
                let pair = value.get("coordinates")?.as_array()?;
                Some(Self::Point((
                    pair.first()?.as_f64()?,
                    pair.get(1)?.as_f64()?,
                )))
            }
            "line_string" => Some(Self::LineString(parse_coordinate_pairs(
                value.get("coordinates")?,
            )?)),
            "polygon" => Some(Self::Polygon(parse_coordinate_pairs(
                value.get("coordinates")?,
            )?)),
            _ => None,
        }
    }
}

fn parse_coordinate_pairs(value: &serde_json::Value) -> Option<Vec<(f64, f64)>> {
    value
        .as_array()?
        .iter()
        .map(|point| {
            let pair = point.as_array()?;
            Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?))
        })
        .collect()
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

    /// 仅供命名资格门构造最小命名目标（无标签；几何只用于计算补名质心）。
    pub fn for_naming(
        entity_id: impl Into<String>,
        source_geometry: Option<SourceGeometry>,
        geometry_part_id: impl Into<String>,
    ) -> Self {
        let entity_id = entity_id.into();
        Self::with_geometry(
            entity_id.clone(),
            entity_id,
            TagMap::new(),
            serde_json::json!({}),
            source_geometry,
            geometry_part_id,
        )
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

    /// 批量补名（T36）：默认不补名；有补名能力的数据源覆写。
    ///
    /// `deadline` 为本次采集运行的整体截止时刻：有界并发补名必须在该时刻
    /// 前停止派发新调用，超限立即结束并如实标记“部分建筑未命名”。
    fn enrich(&self, entities: Vec<RawEntity>, _deadline: Instant) -> Result<EnrichedEntities> {
        Ok(EnrichedEntities {
            name_sources: entities.iter().map(entity_name_source).collect(),
            entities,
            partial: false,
            attempted: 0,
            key_missing: false,
            skipped_count: 0,
        })
    }

    /// 本次补名是否“部分建筑未命名”（截止 / 上限 / 调用失败导致）。
    fn enrichment_partial(&self) -> bool {
        false
    }
}

/// 一次批量补名后的完整事实（实体 + 是否部分未命名 + 实际调用数）。
#[derive(Debug, Clone, PartialEq)]
pub struct EnrichedEntities {
    /// 补名后的实体（未命名项保持来源 #id 名称）
    pub entities: Vec<RawEntity>,
    /// 本次是否“部分建筑未命名”
    pub partial: bool,
    /// 本次实际发出的补名调用数
    pub attempted: usize,
    /// 与 `entities` 对齐的每个实体名称来源。
    pub name_sources: Vec<CandidateNameSource>,
    /// 是否因未配置 Web 服务 Key 而未执行补名。
    pub key_missing: bool,
    /// 因缺 Key 而未执行补名的目标数。
    pub skipped_count: usize,
}

/// 桥接传输：边界 → 高德地点搜索响应 JSON（REST 风格信封）。
///
/// 实际网络请求由壳层 WebView 的 JS 桥完成（B3 是纯逻辑层，不发请求）；
/// 壳层注入真实桥接闭包，测试注入罐头 JSON——离线可测。
pub type BridgeTransport =
    Box<dyn Fn(&Boundary) -> std::result::Result<String, String> + Send + Sync>;

/// 可注入的 Overpass `out geom` 传输；生产网络由外部适配器提供。
pub type OverpassTransport = BridgeTransport;

/// 批量补名结果（与入参实体对齐）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchEnrichment {
    /// 每个入参实体的补名结果；`None` 表示保持来源 #id 名称
    pub names: Vec<Option<String>>,
    /// 每个入参实体的名称来源（与 `names` 对齐）。
    pub name_sources: Vec<CandidateNameSource>,
    /// 本次是否“部分建筑未命名”（截止 / 上限 / 调用失败导致）
    pub partial: bool,
    /// 本次实际发出的补名调用数
    pub attempted: usize,
    /// 是否因未配置 Web 服务 Key 而未执行补名。
    pub key_missing: bool,
    /// 因缺 Key 而未执行补名的目标数。
    pub skipped_count: usize,
}

/// 命名补强器（T36）：OSM name 缺失时批量补名（regeo 等）。
///
/// 实现必须遵守：有界并发（默认 8 路）、单次调用超时（默认 5s）、
/// 总体截止（`deadline`）与调用上限，禁止无限等待。
pub trait NameEnricher: Send + Sync {
    fn enrich_batch(&self, entities: &[RawEntity], deadline: Instant) -> BatchEnrichment;
}

/// OSM/Overpass 来源适配器，保留 node/way 的原始几何，不拼接 relation。
pub struct OverpassDataSource {
    transport: OverpassTransport,
    name_enricher: Option<Arc<dyn NameEnricher>>,
    last_enrichment_partial: AtomicBool,
}

impl OverpassDataSource {
    pub const SOURCE_TAG: &'static str = "overpass";
    pub fn new(transport: OverpassTransport) -> Self {
        Self {
            transport,
            name_enricher: None,
            last_enrichment_partial: AtomicBool::new(false),
        }
    }

    /// 注入命名补强器（仅 OSM name 缺失的面几何调用；点位不参与补名主体）
    pub fn with_name_enricher(mut self, enricher: Option<Arc<dyn NameEnricher>>) -> Self {
        self.name_enricher = enricher;
        self
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
        let mut entities: Vec<RawEntity> =
            elements.into_iter().filter_map(overpass_entity).collect();
        // 采集入口坐标转换：OSM 的 WGS-84 面数据转 GCJ-02（应用工作坐标系），
        // 原始 WGS-84 载荷仍完整保留在 source_payload 备查（T31，不做反向转换）。
        for entity in &mut entities {
            if let Some(geometry) = entity.source_geometry.as_mut() {
                convert_geometry_to_gcj02(geometry);
            }
        }
        Ok(entities)
    }

    fn enrich(&self, mut entities: Vec<RawEntity>, deadline: Instant) -> Result<EnrichedEntities> {
        let Some(enricher) = &self.name_enricher else {
            self.last_enrichment_partial.store(false, Ordering::SeqCst);
            return Ok(EnrichedEntities {
                name_sources: entities.iter().map(entity_name_source).collect(),
                entities,
                partial: false,
                attempted: 0,
                key_missing: false,
                skipped_count: 0,
            });
        };
        // 命名两级：OSM name 优先（RawEntity::name 已取 tags.name）；
        // 缺名的关键建筑（教学楼/图书馆/宿舍等）由补强器（regeo）批量补名。
        // 点位只作证据，不参与补名主体（ADR-0040：点位不得扩面/冒充建筑）。
        let unnamed: Vec<(usize, RawEntity)> = entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| {
                entity.name == entity.entity_id
                    && matches!(entity.source_geometry, Some(SourceGeometry::Polygon(_)))
            })
            .map(|(index, entity)| (index, entity.clone()))
            .collect();
        if unnamed.is_empty() {
            self.last_enrichment_partial.store(false, Ordering::SeqCst);
            return Ok(EnrichedEntities {
                name_sources: entities.iter().map(entity_name_source).collect(),
                entities,
                partial: false,
                attempted: 0,
                key_missing: false,
                skipped_count: 0,
            });
        }
        let unnamed_entities: Vec<RawEntity> =
            unnamed.iter().map(|(_, entity)| entity.clone()).collect();
        let batch = enricher.enrich_batch(&unnamed_entities, deadline);
        let mut name_sources: Vec<CandidateNameSource> =
            entities.iter().map(entity_name_source).collect();
        for (position, (index, _)) in unnamed.iter().enumerate() {
            name_sources[*index] = batch
                .name_sources
                .get(position)
                .copied()
                .unwrap_or_default();
        }
        for (position, name) in batch.names.into_iter().enumerate() {
            if let Some(name) = name {
                let (index, _) = &unnamed[position];
                entities[*index].name = name;
            }
        }
        self.last_enrichment_partial
            .store(batch.partial, Ordering::SeqCst);
        Ok(EnrichedEntities {
            entities,
            partial: batch.partial,
            attempted: batch.attempted,
            name_sources,
            key_missing: batch.key_missing,
            skipped_count: batch.skipped_count,
        })
    }

    fn enrichment_partial(&self) -> bool {
        self.last_enrichment_partial.load(Ordering::SeqCst)
    }
}

/// 未补名实体的当前来源：有 OSM 名称 → OSM；否则仍以回退标识占位 → 仍未命名。
fn entity_name_source(entity: &RawEntity) -> CandidateNameSource {
    if entity.name == entity.entity_id {
        CandidateNameSource::Unnamed
    } else {
        CandidateNameSource::Osm
    }
}

/// WGS-84 → GCJ-02 几何就地转换（点/线/面统一）
fn convert_geometry_to_gcj02(geometry: &mut SourceGeometry) {
    match geometry {
        SourceGeometry::Point((lon, lat)) => {
            let (converted_lon, converted_lat) = wgs84_to_gcj02(*lon, *lat);
            *lon = converted_lon;
            *lat = converted_lat;
        }
        SourceGeometry::LineString(points) | SourceGeometry::Polygon(points) => {
            convert_pairs_wgs84_to_gcj02(points);
        }
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
        "node" => parse_overpass_coordinate(&value).map(SourceGeometry::Point),
        "way" => value
            .get("geometry")
            .and_then(serde_json::Value::as_array)
            .and_then(|points| {
                points
                    .iter()
                    .map(parse_overpass_coordinate)
                    .collect::<Option<Vec<(f64, f64)>>>()
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

fn parse_overpass_coordinate(value: &serde_json::Value) -> Option<(f64, f64)> {
    let lon = value.get("lon")?.as_f64()?;
    let lat = value.get("lat")?.as_f64()?;
    (lon.is_finite()
        && lat.is_finite()
        && (-180.0..=180.0).contains(&lon)
        && (-90.0..=90.0).contains(&lat))
    .then_some((lon, lat))
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
                .or_else(|| poi.get("typeCode").and_then(|v| v.as_str()))
                .or_else(|| poi.get("type_code").and_then(|v| v.as_str()))
                .unwrap_or_default();
            // JS API v2.0 的 POI 常无 typecode，只有 `type` 分类文本（如
            // "科教文化服务;学校;高等院校"）。缺失类型码时按文本兜底，
            // 学校类映射回 1412 前缀，保证采集候选带真实类别标签而非静默置空。
            let effective_typecode = if typecode.is_empty() {
                let category = poi.get("type").and_then(|v| v.as_str()).unwrap_or_default();
                if category.contains("学校") || category.contains("大学") {
                    SCHOOL_TYPECODE_PREFIX
                } else {
                    typecode
                }
            } else {
                typecode
            };
            let tags = Self::typecode_to_tags(effective_typecode);
            seen_ids.push(id.clone());
            let geometry = poi
                .get("location")
                .and_then(parse_location_value)
                .map(SourceGeometry::Point);
            entities.push(RawEntity::with_geometry(
                id, name, tags, poi, geometry, "point",
            ));
        }
        Ok(entities)
    }
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
    fn js_api_v2_type_text_falls_back_to_school_tag() {
        // 真实 JS API v2.0 searchNearBy 的 POI 无 typecode，只有 `type` 分类文本；
        // 学校类文本必须映射回 school 标签，坐标真实进入候选，不得静默丢弃。
        let json = r#"{"status":"1","info":"OK","pois":[
            {"id":"B01","name":"第一教学楼","address":"校内","location":{"lng":121.401,"lat":31.201},"type":"科教文化服务;学校;高等院校"},
            {"id":"B02","name":"学生宿舍","address":"校内","location":[121.402,31.202],"type":"科教文化服务;学校;高等院校"},
            {"id":"B03","name":"南校门","address":"校内","location":"121.403,31.203","type":"科教文化服务;交通设施;出入口"}
        ]}"#;
        let source = canned_source(json);
        let entities = source.fetch_raw_entities(&boundary()).unwrap();
        assert_eq!(entities.len(), 3, "全部对象进流水线，禁止静默丢弃");
        let school = entities.iter().find(|e| e.entity_id == "B01").unwrap();
        assert_eq!(school.tags.get("building").unwrap(), "school");
        let gate = entities.iter().find(|e| e.entity_id == "B03").unwrap();
        assert!(
            gate.tags.is_empty(),
            "非学校类文本不伪造 school 标签，交 B13 兜底"
        );
        let dorm = entities.iter().find(|e| e.entity_id == "B02").unwrap();
        assert!(
            matches!(dorm.source_geometry, Some(SourceGeometry::Point(_))),
            "数组 location 必须解析为真实点几何"
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

    #[test]
    fn js_api_v2_object_and_array_locations_become_point_geometry() {
        // D-1：真实 JS API v2.0 响应的 location 为对象/数组时，坐标必须真实
        // 进入候选（Point 几何），不得静默丢弃。
        let json = r#"{"status":"1","info":"OK","pois":[
            {"id":"B01","name":"第一教学楼","address":"校内","location":{"lng":121.401,"lat":31.201},"typecode":"141201"},
            {"id":"B02","name":"体育馆","address":"校内","location":[121.402,31.202],"typecode":"080300"},
            {"id":"B03","name":"南校门","address":"校内","location":"121.403,31.203","typecode":"991400"}
        ]}"#;
        let entities = canned_source(json).fetch_raw_entities(&boundary()).unwrap();
        assert_eq!(entities.len(), 3, "全部对象进流水线，禁止静默丢弃");

        let geometry_of = |id: &str| {
            entities
                .iter()
                .find(|e| e.entity_id == id)
                .expect("实体存在")
                .source_geometry
                .clone()
                .expect("来源几何必须存在")
        };
        assert_eq!(geometry_of("B01"), SourceGeometry::Point((121.401, 31.201)));
        assert_eq!(geometry_of("B02"), SourceGeometry::Point((121.402, 31.202)));
        assert_eq!(geometry_of("B03"), SourceGeometry::Point((121.403, 31.203)));
    }

    #[test]
    fn js_api_v2_type_code_field_name_is_accepted() {
        // JS API v2.0 可能返回 typeCode（驼峰）或 type_code；与 REST 风格
        // typecode 并存，归类不得静默退化为空标签。
        let json = r#"{"status":"1","info":"OK","pois":[
            {"id":"B01","name":"第一教学楼","address":"校内","location":"121.401,31.201","typeCode":"141201"},
            {"id":"B02","name":"体育馆","address":"校内","location":"121.402,31.202","type_code":"080300"}
        ]}"#;
        let entities = canned_source(json).fetch_raw_entities(&boundary()).unwrap();
        let tag_of = |id: &str| {
            entities
                .iter()
                .find(|e| e.entity_id == id)
                .expect("实体存在")
                .tags
                .clone()
        };
        assert_eq!(tag_of("B01").get("building").unwrap(), "school");
        assert_eq!(tag_of("B02").get("leisure").unwrap(), "sports_centre");
    }

    // ── T31：Overpass 数据源（生产采集源）───────────────────────────

    fn overpass_source(json: &str) -> OverpassDataSource {
        let payload = json.to_owned();
        OverpassDataSource::new(Box::new(move |_| Ok(payload.clone())))
    }

    const OVERPASS_BUILDINGS: &str = r#"{"elements":[
        {"type":"way","id":154427164,"tags":{"building":"yes","name":"第一餐饮大楼"},"geometry":[
            {"lat":31.0295,"lon":121.4184},{"lat":31.03,"lon":121.42},{"lat":31.028,"lon":121.421},{"lat":31.0295,"lon":121.4184}]},
        {"type":"way","id":160634093,"tags":{"building":"university","building:levels":"6"},"geometry":[
            {"lat":31.03,"lon":121.43},{"lat":31.031,"lon":121.431},{"lat":31.03,"lon":121.432},{"lat":31.03,"lon":121.43}]},
        {"type":"node","id":999,"tags":{"building":"yes"},"lat":31.03,"lon":121.42},
        {"type":"relation","id":777,"tags":{"building":"yes"}}
    ]}"#;

    #[test]
    fn overpass_building_ways_keep_name_levels_and_polygon_geometry() {
        let entities = overpass_source(OVERPASS_BUILDINGS)
            .fetch_raw_entities(&boundary())
            .unwrap();
        let dining = entities
            .iter()
            .find(|e| e.entity_id == "way/154427164")
            .unwrap();
        assert_eq!(dining.name, "第一餐饮大楼", "OSM name 优先");
        assert_eq!(dining.tags.get("building").unwrap(), "yes");
        let hospital = entities
            .iter()
            .find(|e| e.entity_id == "way/160634093")
            .unwrap();
        assert_eq!(hospital.tags.get("building:levels").unwrap(), "6");
        assert!(
            matches!(hospital.source_geometry, Some(SourceGeometry::Polygon(ref p)) if p.len() == 4),
            "way 面几何必须保留"
        );
    }

    #[test]
    fn overpass_wgs84_geometry_is_converted_to_gcj02_but_payload_keeps_wgs84() {
        let entities = overpass_source(OVERPASS_BUILDINGS)
            .fetch_raw_entities(&boundary())
            .unwrap();
        let dining = entities
            .iter()
            .find(|e| e.entity_id == "way/154427164")
            .unwrap();
        let SourceGeometry::Polygon(points) = dining.source_geometry.as_ref().unwrap() else {
            panic!("面几何");
        };
        // WGS-84 (121.4184, 31.0295) → GCJ-02 应发生偏移（>0.0005°，≈50m）
        assert!(
            (points[0].0 - 121.4184).abs() > 0.0005,
            "几何必须已转 GCJ-02: {:?}",
            points[0]
        );
        // 原始 WGS-84 载荷原样保全
        assert_eq!(dining.source_payload["geometry"][0]["lon"], 121.4184);
        assert_eq!(dining.source_payload["geometry"][0]["lat"], 31.0295);
    }

    #[test]
    fn overpass_node_stays_a_point_never_expanded_to_polygon() {
        // ADR-0040 红线：点位只作名称/位置/来源证据，禁止固定半径/包围盒/模板扩面。
        let entities = overpass_source(OVERPASS_BUILDINGS)
            .fetch_raw_entities(&boundary())
            .unwrap();
        let node = entities.iter().find(|e| e.entity_id == "node/999").unwrap();
        assert!(
            matches!(node.source_geometry, Some(SourceGeometry::Point(_))),
            "node 必须保持 Point，不得扩面: {:?}",
            node.source_geometry
        );
        assert!(
            !matches!(node.source_geometry, Some(SourceGeometry::Polygon(_))),
            "点位禁止扩成面"
        );
    }

    #[test]
    fn overpass_batch_enricher_fills_missing_names_with_cache() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        struct CountingEnricher {
            calls: Arc<AtomicUsize>,
        }
        impl NameEnricher for CountingEnricher {
            fn enrich_batch(&self, entities: &[RawEntity], _deadline: Instant) -> BatchEnrichment {
                self.calls.fetch_add(entities.len(), Ordering::SeqCst);
                BatchEnrichment {
                    names: entities
                        .iter()
                        .map(|_| Some("未命名建筑".to_owned()))
                        .collect(),
                    name_sources: entities
                        .iter()
                        .map(|_| CandidateNameSource::Gaode)
                        .collect(),
                    partial: false,
                    attempted: entities.len(),
                    key_missing: false,
                    skipped_count: 0,
                }
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let enricher = Arc::new(CountingEnricher {
            calls: Arc::clone(&calls),
        });
        let source = overpass_source(OVERPASS_BUILDINGS).with_name_enricher(Some(enricher));
        let fetched = source.fetch_raw_entities(&boundary()).unwrap();
        // T36：补名从拉取关键路径拆出，由流水线按阶段调用
        let enriched = source
            .enrich(fetched, Instant::now() + Duration::from_secs(60))
            .unwrap();
        let hospital = enriched
            .entities
            .iter()
            .find(|e| e.entity_id == "way/160634093")
            .unwrap();
        assert_eq!(hospital.name, "未命名建筑");
        // node 是点位：补名器不参与（语义：点位只作证据）
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // 带 OSM name 的建筑不触发补名
        let dining = enriched
            .entities
            .iter()
            .find(|e| e.entity_id == "way/154427164")
            .unwrap();
        assert_eq!(dining.name, "第一餐饮大楼");
        assert!(!enriched.partial, "全部缺名建筑补名成功时不标记部分未命名");
    }

    #[test]
    fn overpass_transport_failure_is_structured_unreachable() {
        let source = OverpassDataSource::new(Box::new(|_| Err("端点全部不可达".to_owned())));
        let error = source.fetch_raw_entities(&boundary()).unwrap_err();
        assert!(matches!(
            error,
            AcquisitionError::SourceUnreachable { source_tag, .. } if source_tag == "overpass"
        ));
    }
}
