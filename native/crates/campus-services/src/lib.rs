use base64::Engine;
use campus_state::{FeatureKind, GeoPoint, MapCandidate, ReviewDecision};
#[cfg(debug_assertions)]
use serde::Deserialize;
#[cfg(debug_assertions)]
use std::collections::HashMap;
use std::f64::consts::PI;

pub mod acquisition;

#[derive(Debug, Clone, Copy)]
pub struct GeoBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

const EARTH_SEMI_MAJOR_AXIS: f64 = 6_378_245.0;
const ECCENTRICITY_SQUARED: f64 = 0.006_693_421_622_965_943;

pub fn wgs84_to_gcj02(point: GeoPoint) -> GeoPoint {
    if outside_china(point) {
        return point;
    }
    let mut lat_offset = transform_latitude(point.lng - 105.0, point.lat - 35.0);
    let mut lng_offset = transform_longitude(point.lng - 105.0, point.lat - 35.0);
    let latitude_radians = point.lat / 180.0 * PI;
    let magic_sine = latitude_radians.sin();
    let magic = 1.0 - ECCENTRICITY_SQUARED * magic_sine * magic_sine;
    let sqrt_magic = magic.sqrt();
    lat_offset = lat_offset * 180.0
        / ((EARTH_SEMI_MAJOR_AXIS * (1.0 - ECCENTRICITY_SQUARED)) / (magic * sqrt_magic) * PI);
    lng_offset =
        lng_offset * 180.0 / (EARTH_SEMI_MAJOR_AXIS / sqrt_magic * latitude_radians.cos() * PI);
    GeoPoint {
        lng: point.lng + lng_offset,
        lat: point.lat + lat_offset,
    }
}

pub fn gcj02_to_wgs84(point: GeoPoint) -> GeoPoint {
    if outside_china(point) {
        return point;
    }
    let mut result = point;
    for _ in 0..8 {
        let projected = wgs84_to_gcj02(result);
        result.lng += point.lng - projected.lng;
        result.lat += point.lat - projected.lat;
    }
    result
}

fn outside_china(point: GeoPoint) -> bool {
    point.lng < 72.004 || point.lng > 137.8347 || point.lat < 0.8293 || point.lat > 55.8271
}

fn transform_latitude(x: f64, y: f64) -> f64 {
    let mut result = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y * PI / 30.0).sin()) * 2.0 / 3.0;
    result
}

fn transform_longitude(x: f64, y: f64) -> f64 {
    let mut result = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    result += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    result += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * 2.0 / 3.0;
    result += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * 2.0 / 3.0;
    result
}

pub fn analyze_visual_capture(
    image_data_url: &str,
    bounds: GeoBounds,
    campus: &str,
) -> Result<(Vec<u8>, Vec<MapCandidate>), String> {
    let bounds = bounds.validate()?;
    let encoded = image_data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("视觉截图不是 PNG data URL")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("视觉截图 Base64 无效：{error}"))?;
    let decoder = png::Decoder::new(bytes.as_slice());
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("视觉截图 PNG 无效：{error}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("视觉截图解码失败：{error}"))?;
    let pixels = rgba_pixels(&buffer[..info.buffer_size()], info.color_type)?;
    let candidates = extract_visual_features(
        &pixels,
        info.width as usize,
        info.height as usize,
        bounds,
        campus,
    )?;
    Ok((bytes, candidates))
}

fn rgba_pixels(bytes: &[u8], color_type: png::ColorType) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(bytes.len().saturating_mul(4));
    match color_type {
        png::ColorType::Rgba => output.extend_from_slice(bytes),
        png::ColorType::Rgb => {
            for pixel in bytes.chunks_exact(3) {
                output.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        _ => return Err("视觉截图必须是 RGB/RGBA PNG".into()),
    }
    Ok(output)
}

#[derive(Clone, Copy)]
struct PixelPoint {
    x: usize,
    y: usize,
}

struct ColorRegion {
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
    pixels: usize,
    boundary: Vec<PixelPoint>,
}

impl ColorRegion {
    fn width(&self) -> usize {
        self.max_x - self.min_x + 1
    }

    fn height(&self) -> usize {
        self.max_y - self.min_y + 1
    }

    fn fill_ratio(&self) -> f64 {
        self.pixels as f64 / (self.width() * self.height()) as f64
    }
}

type ColorRule = fn(u8, u8, u8) -> bool;

pub fn extract_visual_features(
    pixels: &[u8],
    width: usize,
    height: usize,
    bounds: GeoBounds,
    campus: &str,
) -> Result<Vec<MapCandidate>, String> {
    if width == 0 || height == 0 || pixels.len() != width * height * 4 {
        return Err("视觉截图像素尺寸不匹配".into());
    }
    let minimum_area = 45usize.max(((width * height) as f64 * 0.000_045).round() as usize);
    let rules: [(FeatureKind, ColorRule); 4] = [
        (FeatureKind::Water, is_water),
        (FeatureKind::Vegetation, is_vegetation),
        (FeatureKind::Sports, is_sports),
        (FeatureKind::Road, is_road),
    ];
    let mut result = Vec::new();
    for (kind, rule) in rules {
        let mut regions = connected_regions(pixels, width, height, rule, minimum_area);
        regions.retain(|region| accepts_region(kind, region));
        for (index, region) in regions.into_iter().take(60).enumerate() {
            let mut contour = convex_hull(&region.boundary);
            if contour.len() < 3 {
                continue;
            }
            if contour.len() > 80 {
                let step = contour.len().div_ceil(80);
                contour = contour.into_iter().step_by(step).collect();
            }
            let points = contour
                .into_iter()
                .map(|point| pixel_to_geo(point, width, height, bounds))
                .collect();
            let area_share = region.pixels as f64 / (width * height) as f64;
            let touches_edge = region.min_x == 0
                || region.min_y == 0
                || region.max_x + 1 == width
                || region.max_y + 1 == height;
            let confidence = if area_share >= 0.012 && !touches_edge {
                "中"
            } else {
                "低"
            };
            let kind_slug = feature_slug(kind);
            let signature = format!(
                "{kind_slug}:{}:{}:{}:{}:{}",
                region.min_x, region.min_y, region.max_x, region.max_y, region.pixels
            );
            let mut tags = std::collections::BTreeMap::new();
            tags.insert("analysis".into(), "deterministic-label-free-v2".into());
            tags.insert("pixels".into(), region.pixels.to_string());
            tags.insert("fill_ratio".into(), format!("{:.2}", region.fill_ratio()));
            tags.insert("review_required".into(), "true".into());
            result.push(MapCandidate {
                id: format!("visual-rule:{signature}"),
                name: format!("视觉识别 {} {}", feature_label(kind), index + 1),
                kind,
                source: "截图规则分割（确定性 v2）".into(),
                confidence: confidence.into(),
                points,
                height_m: None,
                floors: None,
                roof_shape: None,
                tags,
                review: ReviewDecision::Pending,
            });
        }
    }
    if result.is_empty() {
        return Err(format!("{campus} 当前截图未识别到可复核的视觉地物"));
    }
    Ok(result)
}

fn connected_regions(
    pixels: &[u8],
    width: usize,
    height: usize,
    rule: ColorRule,
    minimum_area: usize,
) -> Vec<ColorRegion> {
    let mut mask = vec![false; width * height];
    for (index, selected) in mask.iter_mut().enumerate() {
        let offset = index * 4;
        *selected =
            pixels[offset + 3] > 20 && rule(pixels[offset], pixels[offset + 1], pixels[offset + 2]);
    }
    mask = denoise(mask, width, height);
    let mut visited = vec![false; mask.len()];
    let mut regions = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = vec![start];
        let mut cursor = 0;
        let mut cells = Vec::new();
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0;
        let mut max_y = 0;
        while cursor < queue.len() {
            let cell = queue[cursor];
            cursor += 1;
            let x = cell % width;
            let y = cell / width;
            cells.push(cell);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            for (dx, dy) in [
                (-1, -1),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                    continue;
                }
                let next = ny as usize * width + nx as usize;
                if mask[next] && !visited[next] {
                    visited[next] = true;
                    queue.push(next);
                }
            }
        }
        if cells.len() < minimum_area {
            continue;
        }
        let boundary = cells
            .iter()
            .filter_map(|cell| {
                let x = cell % width;
                let y = cell / width;
                let edge = x == 0
                    || y == 0
                    || x + 1 == width
                    || y + 1 == height
                    || !mask[cell - usize::from(x > 0)]
                    || (x + 1 < width && !mask[cell + 1])
                    || (y > 0 && !mask[cell - width])
                    || (y + 1 < height && !mask[cell + width]);
                edge.then_some(PixelPoint { x, y })
            })
            .collect();
        regions.push(ColorRegion {
            min_x,
            min_y,
            max_x,
            max_y,
            pixels: cells.len(),
            boundary,
        });
    }
    regions.sort_by_key(|region| std::cmp::Reverse(region.pixels));
    regions
}

fn denoise(mask: Vec<bool>, width: usize, height: usize) -> Vec<bool> {
    if width < 3 || height < 3 {
        return mask;
    }
    let mut opened = vec![false; mask.len()];
    let mut closed = vec![false; mask.len()];
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let index = y * width + x;
            opened[index] = mask[index] && neighbor_count(&mask, width, x, y) >= 3;
        }
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let index = y * width + x;
            closed[index] = opened[index] || neighbor_count(&opened, width, x, y) >= 5;
        }
    }
    closed
}

fn neighbor_count(mask: &[bool], width: usize, x: usize, y: usize) -> usize {
    let mut count = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            count +=
                usize::from(mask[(y as isize + dy) as usize * width + (x as isize + dx) as usize]);
        }
    }
    count
}

fn convex_hull(points: &[PixelPoint]) -> Vec<PixelPoint> {
    let mut points = points.to_vec();
    points.sort_by_key(|point| (point.x, point.y));
    points.dedup_by_key(|point| (point.x, point.y));
    if points.len() <= 3 {
        return points;
    }
    fn cross(origin: PixelPoint, a: PixelPoint, b: PixelPoint) -> i64 {
        (a.x as i64 - origin.x as i64) * (b.y as i64 - origin.y as i64)
            - (a.y as i64 - origin.y as i64) * (b.x as i64 - origin.x as i64)
    }
    let mut lower = Vec::new();
    for point in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *point) <= 0
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *point) <= 0
        {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn pixel_to_geo(point: PixelPoint, width: usize, height: usize, bounds: GeoBounds) -> GeoPoint {
    GeoPoint {
        lng: bounds.west + point.x as f64 / width.max(1) as f64 * (bounds.east - bounds.west),
        lat: bounds.north - point.y as f64 / height.max(1) as f64 * (bounds.north - bounds.south),
    }
}

fn color_metrics(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    let saturation = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * lightness - 1.0).abs())
    };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (hue, saturation, lightness)
}

fn is_water(r: u8, g: u8, b: u8) -> bool {
    let (hue, saturation, _) = color_metrics(r, g, b);
    hue > 195.0 && hue <= 235.0 && saturation >= 0.12 && b as i16 > r as i16 + 10
}

fn is_vegetation(r: u8, g: u8, b: u8) -> bool {
    let (hue, saturation, lightness) = color_metrics(r, g, b);
    (70.0..=165.0).contains(&hue)
        && saturation >= 0.1
        && g as i16 > r as i16 + 7
        && g as i16 > b as i16 + 4
        && lightness < 0.86
}

fn is_sports(r: u8, g: u8, b: u8) -> bool {
    let (hue, saturation, lightness) = color_metrics(r, g, b);
    (155.0..=195.0).contains(&hue)
        && saturation >= 0.16
        && g as i16 > r as i16 + 12
        && b as i16 > r as i16 + 8
        && lightness < 0.82
}

fn is_road(r: u8, g: u8, b: u8) -> bool {
    let (_, saturation, lightness) = color_metrics(r, g, b);
    saturation < 0.09 && (0.72..=0.97).contains(&lightness)
}

fn accepts_region(kind: FeatureKind, region: &ColorRegion) -> bool {
    match kind {
        FeatureKind::Water | FeatureKind::Vegetation => {
            region.pixels >= 70 && region.width() >= 5 && region.height() >= 5
        }
        FeatureKind::Sports => {
            region.pixels >= 110
                && region.width() >= 8
                && region.height() >= 8
                && region.fill_ratio() >= 0.35
        }
        FeatureKind::Road => {
            let long = region.width().max(region.height()) as f64;
            let short = region.width().min(region.height()).max(1) as f64;
            region.pixels >= 120 && (long / short >= 1.8 || region.fill_ratio() < 0.62)
        }
        FeatureKind::Building => false,
    }
}

fn feature_slug(kind: FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Building => "building",
        FeatureKind::Road => "road",
        FeatureKind::Water => "water",
        FeatureKind::Vegetation => "vegetation",
        FeatureKind::Sports => "sports",
    }
}

fn feature_label(kind: FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Building => "建筑",
        FeatureKind::Road => "道路",
        FeatureKind::Water => "水域",
        FeatureKind::Vegetation => "植被",
        FeatureKind::Sports => "体育设施",
    }
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

// BEGIN DEBUG-ONLY LEGACY ACQUISITION
#[cfg(debug_assertions)]
mod legacy_provider {
    use super::*;

    #[derive(Deserialize)]
    pub(super) struct OverpassResponse {
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

    pub(super) fn largest_geojson_ring(
        geometry: Option<&serde_json::Value>,
    ) -> Option<Vec<GeoPoint>> {
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

    pub(super) fn parse_overpass(response: OverpassResponse) -> Vec<MapCandidate> {
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
}

#[cfg(debug_assertions)]
pub use legacy_provider::{query_campus_data, query_open_map_data};
// END DEBUG-ONLY LEGACY ACQUISITION

#[cfg(test)]
mod tests {
    use super::legacy_provider::{largest_geojson_ring, parse_overpass, OverpassResponse};
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

    #[test]
    fn gaode_conversion_round_trips_in_shanghai() {
        let wgs = GeoPoint {
            lng: 121.402_112,
            lat: 31.225_711,
        };
        let gcj = wgs84_to_gcj02(wgs);
        assert!((gcj.lng - wgs.lng).abs() > 0.001);
        let restored = gcj02_to_wgs84(gcj);
        assert!((restored.lng - wgs.lng).abs() < 1e-7);
        assert!((restored.lat - wgs.lat).abs() < 1e-7);
    }

    #[test]
    fn deterministic_visual_recovery_finds_colored_regions() {
        let width = 100;
        let height = 100;
        let mut pixels = vec![255u8; width * height * 4];
        for y in 20..60 {
            for x in 15..55 {
                let offset = (y * width + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[50, 150, 70, 255]);
            }
        }
        let candidates = extract_visual_features(
            &pixels,
            width,
            height,
            GeoBounds {
                west: 121.40,
                south: 31.22,
                east: 121.41,
                north: 31.23,
            },
            "测试校区",
        )
        .unwrap();
        assert!(candidates
            .iter()
            .any(|candidate| candidate.kind == FeatureKind::Vegetation));
        assert!(candidates.iter().all(|candidate| candidate
            .points
            .iter()
            .all(|point| (121.40..=121.41).contains(&point.lng)
                && (31.22..=31.23).contains(&point.lat))));
    }

    #[test]
    fn png_data_url_is_decoded_and_analyzed() {
        let width = 32;
        let height = 32;
        let mut pixels = vec![255u8; width * height * 4];
        for y in 5..27 {
            for x in 5..27 {
                let offset = (y * width + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[45, 145, 65, 255]);
            }
        }
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png_bytes)
        );
        let (restored, candidates) = analyze_visual_capture(
            &data_url,
            GeoBounds {
                west: 121.40,
                south: 31.22,
                east: 121.41,
                north: 31.23,
            },
            "测试校区",
        )
        .unwrap();
        assert_eq!(restored, png_bytes);
        assert!(candidates
            .iter()
            .any(|candidate| candidate.kind == FeatureKind::Vegetation));
    }
}
