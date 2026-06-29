use campus_state::{FeatureKind, GeoPoint, MapCandidate, ReviewDecision};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct GeoBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl GeoBounds {
    fn validate(self) -> Result<Self, String> {
        if !self.west.is_finite()
            || !self.south.is_finite()
            || !self.east.is_finite()
            || !self.north.is_finite()
            || self.west >= self.east
            || self.south >= self.north
        {
            return Err("地图视野范围无效".into());
        }
        if self.east - self.west > 0.08 || self.north - self.south > 0.08 {
            return Err("当前视野过大，请放大到单个校区后重试".into());
        }
        Ok(self)
    }
}

#[derive(Deserialize)]
struct OverpassResponse {
    #[serde(default)]
    elements: Vec<OverpassElement>,
}

#[derive(Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    element_type: String,
    id: i64,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    geometry: Vec<OverpassPoint>,
    #[serde(default)]
    members: Vec<OverpassMember>,
}

#[derive(Deserialize)]
struct OverpassMember {
    #[serde(default)]
    geometry: Vec<OverpassPoint>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct OverpassPoint {
    lon: f64,
    lat: f64,
}

pub async fn query_open_map_data(bounds: GeoBounds) -> Result<Vec<MapCandidate>, String> {
    let bounds = bounds.validate()?;
    let query = format!(
        r#"[out:json][timeout:45];
(
way["building"]({s},{w},{n},{e});
relation["building"]({s},{w},{n},{e});
way["highway"]({s},{w},{n},{e});
way["natural"="water"]({s},{w},{n},{e});
way["waterway"]({s},{w},{n},{e});
way["landuse"~"forest|grass|meadow|recreation_ground"]({s},{w},{n},{e});
way["leisure"~"park|garden|pitch|stadium|track|sports_centre"]({s},{w},{n},{e});
relation["leisure"~"pitch|stadium|track|sports_centre"]({s},{w},{n},{e});
);
out tags geom;"#,
        s = bounds.south,
        w = bounds.west,
        n = bounds.north,
        e = bounds.east
    );
    let endpoints = [
        "https://overpass-api.de/api/interpreter",
        "https://overpass.kumi.systems/api/interpreter",
    ];
    let client = reqwest::Client::builder()
        .user_agent("CampusReconstructionTool/0.1")
        .timeout(std::time::Duration::from_secs(70))
        .build()
        .map_err(|error| error.to_string())?;
    let mut errors = Vec::new();
    for endpoint in endpoints {
        let response = match client.post(endpoint).form(&[("data", &query)]).send().await {
            Ok(response) => response,
            Err(error) => {
                errors.push(format!("{endpoint}: {error}"));
                continue;
            }
        };
        if !response.status().is_success() {
            errors.push(format!("{endpoint}: HTTP {}", response.status()));
            continue;
        }
        let payload: OverpassResponse = response
            .json()
            .await
            .map_err(|error| format!("解析 OpenStreetMap 响应失败：{error}"))?;
        return Ok(parse_overpass(payload));
    }
    Err(format!("开放地图查询失败：{}", errors.join(" | ")))
}

pub async fn query_campus_data(
    bounds: GeoBounds,
    overture_endpoint: Option<&str>,
) -> Result<Vec<MapCandidate>, String> {
    let osm = query_open_map_data(bounds).await;
    let overture = match overture_endpoint.filter(|value| !value.trim().is_empty()) {
        Some(endpoint) => query_overture_buildings(bounds, endpoint).await,
        None => Ok(Vec::new()),
    };
    match (osm, overture) {
        (Ok(mut osm), Ok(mut overture)) => {
            overture.append(&mut osm);
            Ok(overture)
        }
        (Ok(osm), Err(_)) => Ok(osm),
        (Err(_), Ok(overture)) if !overture.is_empty() => Ok(overture),
        (Err(osm_error), Err(overture_error)) => {
            Err(format!("{osm_error} | Overture: {overture_error}"))
        }
        (Err(osm_error), Ok(_)) => Err(osm_error),
    }
}

async fn query_overture_buildings(
    bounds: GeoBounds,
    endpoint: &str,
) -> Result<Vec<MapCandidate>, String> {
    let bounds = bounds.validate()?;
    let mut url = reqwest::Url::parse(endpoint).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair(
            "bbox",
            &format!(
                "{},{},{},{}",
                bounds.west, bounds.south, bounds.east, bounds.north
            ),
        )
        .append_pair("limit", "500");
    let payload: serde_json::Value = reqwest::Client::builder()
        .user_agent("CampusReconstructionTool/0.1")
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;
    let mut result = Vec::new();
    for (index, feature) in payload
        .get("features")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .enumerate()
    {
        let Some(points) = largest_geojson_ring(feature.get("geometry")) else {
            continue;
        };
        let properties = feature
            .get("properties")
            .unwrap_or(&serde_json::Value::Null);
        let raw_id = feature
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("feature-{index}"));
        let name = properties
            .get("name")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Overture 建筑 {}", &raw_id[..raw_id.len().min(8)]));
        result.push(MapCandidate {
            id: format!("overture:{raw_id}"),
            name,
            kind: FeatureKind::Building,
            source: "Overture Maps（云端查询）".into(),
            confidence: "较高".into(),
            points,
            height_m: properties.get("height").and_then(json_number),
            floors: properties
                .get("num_floors")
                .or_else(|| properties.get("floors"))
                .and_then(json_number)
                .map(|value| value.max(1.0) as u32),
            roof_shape: properties
                .get("roof_shape")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
            tags: properties
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect(),
            review: ReviewDecision::Pending,
        });
    }
    Ok(result)
}

fn largest_geojson_ring(geometry: Option<&serde_json::Value>) -> Option<Vec<GeoPoint>> {
    let geometry = geometry?;
    let coordinates = geometry.get("coordinates")?;
    let rings = match geometry.get("type").and_then(|value| value.as_str())? {
        "Polygon" => coordinates
            .as_array()?
            .first()?
            .as_array()
            .into_iter()
            .collect::<Vec<_>>(),
        "MultiPolygon" => coordinates
            .as_array()?
            .iter()
            .filter_map(|polygon| polygon.as_array()?.first()?.as_array())
            .collect(),
        _ => return None,
    };
    rings
        .into_iter()
        .max_by_key(|ring| ring.len())
        .map(|ring| {
            ring.iter()
                .filter_map(|coordinate| {
                    let coordinate = coordinate.as_array()?;
                    Some(GeoPoint {
                        lng: coordinate.first()?.as_f64()?,
                        lat: coordinate.get(1)?.as_f64()?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|ring| ring.len() >= 3)
}

fn parse_overpass(response: OverpassResponse) -> Vec<MapCandidate> {
    let mut candidates = Vec::new();
    for element in response.elements {
        let Some(kind) = classify(&element.tags) else {
            continue;
        };
        let geometry = if element.geometry.len() >= 2 {
            element.geometry.clone()
        } else {
            element
                .members
                .iter()
                .max_by_key(|member| member.geometry.len())
                .map(|member| member.geometry.clone())
                .unwrap_or_default()
        };
        let minimum = if kind == FeatureKind::Road { 2 } else { 3 };
        if geometry.len() < minimum {
            continue;
        }
        let type_name = match kind {
            FeatureKind::Building => "未命名建筑",
            FeatureKind::Road => "未命名道路",
            FeatureKind::Water => "未命名水域",
            FeatureKind::Vegetation => "未命名绿化",
            FeatureKind::Sports => "未命名体育设施",
        };
        let name = element
            .tags
            .get("name:zh")
            .or_else(|| element.tags.get("name"))
            .cloned()
            .unwrap_or_else(|| format!("{type_name} {}", element.id));
        candidates.push(MapCandidate {
            id: format!("osm:{}:{}", element.element_type, element.id),
            name,
            kind,
            source: "OpenStreetMap / Overpass".into(),
            confidence: if element.tags.contains_key("name") {
                "较高"
            } else {
                "中等"
            }
            .into(),
            points: geometry
                .into_iter()
                .map(|point| GeoPoint {
                    lng: point.lon,
                    lat: point.lat,
                })
                .collect(),
            height_m: element
                .tags
                .get("height")
                .and_then(|value| parse_number(value)),
            floors: element
                .tags
                .get("building:levels")
                .and_then(|value| parse_number(value))
                .map(|value| value.max(1.0) as u32),
            roof_shape: element.tags.get("roof:shape").cloned(),
            tags: element.tags.into_iter().collect(),
            review: ReviewDecision::Pending,
        });
    }
    candidates.sort_by(|left, right| {
        kind_order(left.kind)
            .cmp(&kind_order(right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
    candidates
}

fn parse_number(value: &str) -> Option<f64> {
    value
        .trim()
        .trim_end_matches('m')
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(parse_number))
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn classify(tags: &HashMap<String, String>) -> Option<FeatureKind> {
    if tags.contains_key("building") {
        Some(FeatureKind::Building)
    } else if tags.contains_key("highway") {
        Some(FeatureKind::Road)
    } else if tags.contains_key("waterway")
        || tags.get("natural").is_some_and(|value| value == "water")
    {
        Some(FeatureKind::Water)
    } else if tags.get("leisure").is_some_and(|value| {
        matches!(
            value.as_str(),
            "pitch" | "stadium" | "track" | "sports_centre"
        )
    }) {
        Some(FeatureKind::Sports)
    } else if tags.contains_key("landuse") || tags.contains_key("leisure") {
        Some(FeatureKind::Vegetation)
    } else {
        None
    }
}

fn kind_order(kind: FeatureKind) -> u8 {
    match kind {
        FeatureKind::Building => 0,
        FeatureKind::Road => 1,
        FeatureKind::Water => 2,
        FeatureKind::Vegetation => 3,
        FeatureKind::Sports => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_foundation_categories() {
        let payload: OverpassResponse = serde_json::from_str(
            r#"{"elements":[
              {"type":"way","id":1,"tags":{"building":"yes","name":"教学楼"},"geometry":[{"lon":121.0,"lat":31.0},{"lon":121.1,"lat":31.0},{"lon":121.1,"lat":30.9}]},
              {"type":"way","id":2,"tags":{"highway":"footway"},"geometry":[{"lon":121.0,"lat":31.0},{"lon":121.1,"lat":31.0}]},
              {"type":"way","id":3,"tags":{"leisure":"pitch"},"geometry":[{"lon":121.0,"lat":31.0},{"lon":121.1,"lat":31.0},{"lon":121.1,"lat":30.9}]}
            ]}"#,
        )
        .unwrap();
        let candidates = parse_overpass(payload);
        assert_eq!(candidates.len(), 3);
        assert!(candidates
            .iter()
            .any(|item| item.kind == FeatureKind::Building));
        assert!(candidates.iter().any(|item| item.kind == FeatureKind::Road));
        assert!(candidates
            .iter()
            .any(|item| item.kind == FeatureKind::Sports));
    }

    #[test]
    fn parses_overture_polygon_and_multipolygon() {
        let polygon: serde_json::Value = serde_json::from_str(
            r#"{"type":"Polygon","coordinates":[[[121.0,31.0],[121.1,31.0],[121.1,30.9],[121.0,31.0]]]}"#,
        )
        .unwrap();
        let multi: serde_json::Value = serde_json::from_str(
            r#"{"type":"MultiPolygon","coordinates":[[[[121.0,31.0],[121.1,31.0],[121.1,30.9],[121.0,31.0]]],[[[122.0,32.0],[122.2,32.0],[122.2,31.8],[122.0,31.8],[122.0,32.0]]]]}"#,
        )
        .unwrap();
        assert_eq!(largest_geojson_ring(Some(&polygon)).unwrap().len(), 4);
        assert_eq!(largest_geojson_ring(Some(&multi)).unwrap().len(), 5);
    }
}
