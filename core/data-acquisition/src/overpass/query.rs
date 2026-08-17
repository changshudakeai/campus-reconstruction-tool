//! Overpass 查询规划：边界查询与六类候选的集中 selector 分片。

use std::collections::{BTreeMap, BTreeSet};

use data_transformers::{ClassifyEngine, TransformError};
use shared_domain_types::Boundary;

use super::OVERPASS_SERVER_TIMEOUT_SECS;

/// union 写法：way/relation × university/college/school。
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

/// 按元素 ID 拉取（Nominatim 解析结果 → Overpass 取边界）。
pub fn element_by_id_query(osm_type: &str, osm_id: i64) -> String {
    format!(
        "[out:json][timeout:{t}];{osm_type}({osm_id});out geom;",
        t = OVERPASS_SERVER_TIMEOUT_SECS
    )
}

/// landuse=education 兜底（union：way + relation）。
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

/// 兼容的单体六类查询；生产候选采集使用同一规划器生成的有界分片。
pub fn campus_objects_query(bbox: (f64, f64, f64, f64)) -> Result<String, TransformError> {
    let clauses = campus_object_clauses(bbox)?;
    Ok(overpass_query(
        clauses.into_values().flatten().collect::<BTreeSet<_>>(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CampusObjectShardKind {
    Structures,
    Networks,
    Grounds,
    Points,
}

pub(super) struct CampusObjectQueryShard {
    pub(super) query: String,
}

pub(super) fn campus_object_query_shards(
    bbox: (f64, f64, f64, f64),
) -> Result<Vec<CampusObjectQueryShard>, TransformError> {
    Ok(campus_object_clauses(bbox)?
        .into_values()
        .filter(|clauses| !clauses.is_empty())
        .map(|clauses| CampusObjectQueryShard {
            query: overpass_query(clauses),
        })
        .collect())
}

fn campus_object_clauses(
    bbox: (f64, f64, f64, f64),
) -> Result<BTreeMap<CampusObjectShardKind, BTreeSet<String>>, TransformError> {
    let engine = ClassifyEngine::with_default_mapping()?;
    let mut shards = BTreeMap::<CampusObjectShardKind, BTreeSet<String>>::new();
    for rule in &engine.config().rules {
        // building=* 粗族由集中“建筑”规则条目识别；最终类别仍只由 B13 裁决。
        if rule.category_tkey == "collection.category_building" {
            for element_kind in ["way", "relation"] {
                insert_campus_clause(
                    &mut shards,
                    CampusObjectShardKind::Structures,
                    element_kind,
                    "[\"building\"]",
                    bbox,
                );
            }
        }
        for pattern in &rule.tags {
            let Some((key, value)) = pattern.as_str().split_once('=') else {
                continue;
            };
            let selector = overpass_tag_selector(key, value);
            for element_kind in overpass_element_kinds(key, value) {
                insert_campus_clause(
                    &mut shards,
                    campus_object_shard_kind(key, element_kind),
                    element_kind,
                    &selector,
                    bbox,
                );
            }
        }
    }
    Ok(shards)
}

fn insert_campus_clause(
    shards: &mut BTreeMap<CampusObjectShardKind, BTreeSet<String>>,
    shard: CampusObjectShardKind,
    element_kind: &str,
    selector: &str,
    bbox: (f64, f64, f64, f64),
) {
    let (south, west, north, east) = bbox;
    shards.entry(shard).or_default().insert(format!(
        "{element_kind}{selector}({south},{west},{north},{east});"
    ));
}

fn campus_object_shard_kind(key: &str, element_kind: &str) -> CampusObjectShardKind {
    if element_kind == "node" {
        CampusObjectShardKind::Points
    } else {
        match key {
            "building" => CampusObjectShardKind::Structures,
            "highway" | "railway" | "waterway" | "barrier" => CampusObjectShardKind::Networks,
            _ => CampusObjectShardKind::Grounds,
        }
    }
}

fn overpass_query(clauses: BTreeSet<String>) -> String {
    format!(
        "[out:json][timeout:{OVERPASS_SERVER_TIMEOUT_SECS}];({});out geom;",
        clauses.into_iter().collect::<String>()
    )
}

fn overpass_tag_selector(key: &str, value: &str) -> String {
    if value == "*" {
        format!("[\"{key}\"]")
    } else if let Some(prefix) = value.strip_suffix('*') {
        format!("[\"{key}\"~\"^{prefix}\"]")
    } else {
        format!("[\"{key}\"=\"{value}\"]")
    }
}

fn overpass_element_kinds(key: &str, value: &str) -> &'static [&'static str] {
    match (key, value) {
        ("natural", "tree") | ("barrier", "gate") => &["node"],
        ("amenity", "fountain") => &["node", "way", "relation"],
        ("highway" | "railway" | "waterway", _) => &["way"],
        ("barrier", "wall" | "fence") | ("natural", "tree_row") => &["way"],
        ("building" | "landuse" | "leisure" | "sport" | "water", _) => &["way", "relation"],
        ("natural", "water" | "wood" | "scrub") => &["way", "relation"],
        ("historic" | "power" | "man_made", _) => &[],
        _ => &[],
    }
}

/// 以锚点为中心、给定半径（米）的 WGS-84 包围盒。
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

/// 方案边界（GCJ-02）→ 查询包围盒；bbox 只缩小传输范围，不授予候选资格。
pub fn boundary_bbox(boundary: &Boundary, margin_deg: f64) -> Option<(f64, f64, f64, f64)> {
    let mut bounds = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
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
    for point in rings
        .into_iter()
        .filter_map(serde_json::Value::as_array)
        .flatten()
    {
        let Some(pair) = point.as_array() else {
            continue;
        };
        let (Some(lon), Some(lat)) = (
            pair.first().and_then(serde_json::Value::as_f64),
            pair.get(1).and_then(serde_json::Value::as_f64),
        ) else {
            continue;
        };
        bounds.0 = bounds.0.min(lat);
        bounds.1 = bounds.1.min(lon);
        bounds.2 = bounds.2.max(lat);
        bounds.3 = bounds.3.max(lon);
        any = true;
    }
    any.then_some((
        bounds.0 - margin_deg,
        bounds.1 - margin_deg,
        bounds.2 + margin_deg,
        bounds.3 + margin_deg,
    ))
}
