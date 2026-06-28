use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::Manager;

const MIN_RADIUS_M: u32 = 20;
const MAX_RADIUS_M: u32 = 250;
const MAX_FEATURES: usize = 200;
const MAX_RESPONSE_BYTES: u64 = 5 * 1024 * 1024;
static OVERPASS_CANDIDATE_CACHE: OnceLock<
    Mutex<HashMap<String, Vec<arnis_core::BuildingCandidate>>>,
> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OvertureQueryRequest {
    lng: f64,
    lat: f64,
    radius_m: u32,
    name: String,
    release_id: Option<String>,
    limit: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeographicBounds {
    min_lng: f64,
    min_lat: f64,
    max_lng: f64,
    max_lat: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildingCandidateQuery {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    lng: f64,
    lat: f64,
    radius_m: u32,
    scale: f64,
    coordinate_system: String,
    gaode_poi_id: String,
    gaode_lng: f64,
    gaode_lat: f64,
    transformation: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildingCandidateQueryResult {
    candidates: Vec<arnis_core::BuildingCandidate>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    file_name: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedExportBundle {
    directory: String,
    paths: Vec<String>,
}

#[tauri::command]
fn save_export_bundle(
    directory: String,
    files: Vec<ExportFile>,
) -> Result<SavedExportBundle, String> {
    let directory_path = std::path::PathBuf::from(&directory);
    if !directory_path.is_dir() {
        return Err("The selected export directory does not exist".to_string());
    }
    if files.is_empty() {
        return Err("No export files were supplied".to_string());
    }
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        let safe_name = std::path::Path::new(&file.file_name)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| *name == file.file_name && !name.is_empty())
            .ok_or_else(|| "An export filename was invalid".to_string())?;
        let destination = directory_path.join(safe_name);
        std::fs::write(&destination, file.bytes)
            .map_err(|error| format!("Could not write {}: {error}", destination.display()))?;
        paths.push(destination.to_string_lossy().to_string());
    }
    Ok(SavedExportBundle { directory, paths })
}

#[tauri::command]
fn save_local_campus_annotations(
    app: tauri::AppHandle,
    campus_key: String,
    json: String,
) -> Result<String, String> {
    if json.len() > 2 * 1024 * 1024
        || !campus_key.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err("Campus annotation payload or key was invalid".to_string());
    }
    serde_json::from_str::<Value>(&json)
        .map_err(|error| format!("Campus annotation JSON was invalid: {error}"))?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("campus-building-annotations");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{campus_key}.json"));
    std::fs::write(&path, json)
        .map_err(|error| format!("Could not save campus annotations: {error}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn load_local_campus_annotations(
    app: tauri::AppHandle,
    campus_key: String,
) -> Result<Option<String>, String> {
    if !campus_key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err("Campus annotation key was invalid".to_string());
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("campus-building-annotations")
        .join(format!("{campus_key}.json"));
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn query_building_candidates(
    request: BuildingCandidateQuery,
) -> Result<BuildingCandidateQueryResult, String> {
    validate_candidate_query(&request)?;
    let radius_m = request.radius_m.clamp(MIN_RADIUS_M, MAX_RADIUS_M);
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    match query_overpass_candidates(&request, radius_m).await {
        Ok(mut items) => candidates.append(&mut items),
        Err(error) => warnings.push(format!("OSM/Overpass: {error}")),
    }

    let overture_request = OvertureQueryRequest {
        lng: request.lng,
        lat: request.lat,
        radius_m,
        name: request.name.clone(),
        release_id: None,
        limit: MAX_FEATURES,
    };
    match query_overture_buildings(overture_request).await {
        Ok(payload) => candidates.extend(overture_candidates(&payload, &request)),
        Err(error) => warnings.push(format!("Overture: {error}")),
    }

    if promote_nearest_location_match(&mut candidates) {
        warnings.push("No source name matched; promoted the nearest footprint as a location-based review candidate.".to_string());
    }

    candidates.sort_by(|left, right| {
        confidence_rank(&right.identity_confidence)
            .cmp(&confidence_rank(&left.identity_confidence))
            .then_with(|| left.distance_m.total_cmp(&right.distance_m))
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates = deduplicate_source_candidates(candidates);
    candidates.truncate(MAX_FEATURES);
    if candidates.is_empty() {
        return Err(format!(
            "No live building candidates were available. {}",
            warnings.join(" | ")
        ));
    }
    Ok(BuildingCandidateQueryResult {
        candidates,
        warnings,
    })
}

fn deduplicate_source_candidates(
    candidates: Vec<arnis_core::BuildingCandidate>,
) -> Vec<arnis_core::BuildingCandidate> {
    let mut unique: Vec<arnis_core::BuildingCandidate> = Vec::new();
    for candidate in candidates {
        let (candidate_lng, candidate_lat, _, _) = component_metrics(&candidate.components);
        let duplicate = unique.iter().any(|existing| {
            let (existing_lng, existing_lat, _, _) = component_metrics(&existing.components);
            haversine_m(candidate_lng, candidate_lat, existing_lng, existing_lat) < 2.0
                && (candidate.width_m - existing.width_m).abs() < 1.0
                && (candidate.length_m - existing.length_m).abs() < 1.0
        });
        if !duplicate {
            unique.push(candidate);
        }
    }
    unique
}

fn promote_nearest_location_match(candidates: &mut [arnis_core::BuildingCandidate]) -> bool {
    if candidates
        .iter()
        .any(|candidate| candidate.identity_confidence != "low")
    {
        return false;
    }
    let nearest = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.distance_m <= 120.0)
        .min_by(|(_, left), (_, right)| left.distance_m.total_cmp(&right.distance_m))
        .map(|(index, _)| index);
    if let Some(index) = nearest {
        candidates[index].identity_confidence = "medium".to_string();
        true
    } else {
        false
    }
}

fn validate_candidate_query(request: &BuildingCandidateQuery) -> Result<(), String> {
    validate_coordinates(request.lng, request.lat)?;
    validate_coordinates(request.gaode_lng, request.gaode_lat)?;
    if !(0.25..=4.0).contains(&request.scale) {
        return Err("scale must be between 0.25 and 4".to_string());
    }
    if request.coordinate_system != "WGS-84" {
        return Err("Arnis candidate queries require an explicit WGS-84 anchor".to_string());
    }
    if request.transformation != "gcj02-to-wgs84-iterative-v1"
        || request.gaode_poi_id.trim().is_empty()
    {
        return Err("Gaode-to-open-geodata coordinate lineage is incomplete".to_string());
    }
    Ok(())
}

#[tauri::command]
fn generate_building(
    request: arnis_core::GenerateBuildingRequest,
) -> Result<arnis_core::GeneratedBuilding, String> {
    arnis_core::generate_building(request)
}

async fn query_overpass_candidates(
    request: &BuildingCandidateQuery,
    radius_m: u32,
) -> Result<Vec<arnis_core::BuildingCandidate>, String> {
    let configured_endpoint = std::env::var("OVERPASS_ENDPOINT")
        .or_else(|_| std::env::var("VITE_OVERPASS_ENDPOINT"))
        .unwrap_or_else(|_| "https://overpass-api.de/api/interpreter".to_string());
    let cache_key = format!(
        "{:.6}:{:.6}:{radius_m}:{}",
        request.lng, request.lat, request.name
    );
    if let Some(cached) = OVERPASS_CANDIDATE_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Overpass cache lock was poisoned".to_string())?
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }
    let mut endpoints = vec![configured_endpoint];
    for fallback in [
        "https://overpass.kumi.systems/api/interpreter",
        "https://overpass.nchc.org.tw/api/interpreter",
    ] {
        if !endpoints.iter().any(|endpoint| endpoint == fallback) {
            endpoints.push(fallback.to_string());
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    for (attempt, endpoint) in endpoints.iter().enumerate() {
        let attempt_radius = match attempt {
            0 => radius_m,
            1 => radius_m * 7 / 10,
            _ => radius_m * 45 / 100,
        }
        .max(MIN_RADIUS_M);
        let query = format!(
            "[out:json][timeout:25];(way(around:{attempt_radius},{lat},{lng})[\"building\"];way(around:{attempt_radius},{lat},{lng})[\"building:part\"];relation(around:{attempt_radius},{lat},{lng})[\"building\"];relation(around:{attempt_radius},{lat},{lng})[\"type\"=\"building\"];);out body geom;",
            lat = request.lat, lng = request.lng
        );
        let response = match client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(
                reqwest::header::USER_AGENT,
                "Campus-Reconstruction-Tool/0.1 arnis-core",
            )
            .form(&[("data", query)])
            .send()
            .await
        {
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
        if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES {
            errors.push(format!(
                "{endpoint}: response exceeded the 5 MiB safety limit"
            ));
            continue;
        }
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                errors.push(format!("{endpoint}: could not read response: {error}"));
                continue;
            }
        };
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            errors.push(format!(
                "{endpoint}: response exceeded the 5 MiB safety limit"
            ));
            continue;
        }
        let payload: Value = match serde_json::from_slice(&bytes) {
            Ok(payload) => payload,
            Err(error) => {
                errors.push(format!("{endpoint}: invalid JSON: {error}"));
                continue;
            }
        };
        let candidates = overpass_candidates(&payload, request);
        OVERPASS_CANDIDATE_CACHE
            .get()
            .expect("cache initialized")
            .lock()
            .map_err(|_| "Overpass cache lock was poisoned".to_string())?
            .insert(cache_key.clone(), candidates.clone());
        return Ok(candidates);
    }
    Err(format!("all endpoints failed: {}", errors.join(" | ")))
}

fn overpass_candidates(
    payload: &Value,
    query: &BuildingCandidateQuery,
) -> Vec<arnis_core::BuildingCandidate> {
    let mut candidates: Vec<_> = payload
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|element| {
            let id = element.get("id")?.as_u64()?;
            let tags = string_map(element.get("tags"));
            let components = if element.get("type").and_then(Value::as_str) == Some("relation") {
                relation_components(element.get("members")?)
            } else {
                let exterior = geometry_points(element.get("geometry")?);
                if exterior.len() < 3 {
                    vec![]
                } else {
                    vec![arnis_core::FootprintComponent {
                        exterior,
                        interior_rings: vec![],
                    }]
                }
            };
            if components.is_empty() {
                return None;
            }
            Some(candidate_from_parts(
                format!("osm:{id}"),
                "osm_overpass",
                tags,
                components,
                query,
            ))
        })
        .collect();
    let parts: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.tags.contains_key("building:part"))
        .filter_map(building_part_from_candidate)
        .collect();
    for candidate in candidates
        .iter_mut()
        .filter(|candidate| candidate.tags.contains_key("building"))
    {
        candidate.parts = parts
            .iter()
            .filter(|part| part_belongs_to_candidate(part, candidate))
            .cloned()
            .collect();
    }
    candidates
        .into_iter()
        .filter(|candidate| !candidate.tags.contains_key("building:part"))
        .collect()
}

fn building_part_from_candidate(
    candidate: &arnis_core::BuildingCandidate,
) -> Option<arnis_core::BuildingPart> {
    let component = candidate.components.first()?.clone();
    Some(arnis_core::BuildingPart {
        id: candidate.id.clone(),
        component,
        tags: candidate.tags.clone(),
        height_m: candidate.height_m,
        min_height_m: candidate
            .tags
            .get("min_height")
            .and_then(|value| value.trim_end_matches('m').trim().parse().ok()),
        floors: candidate.floors,
        min_level: candidate
            .tags
            .get("building:min_level")
            .and_then(|value| value.parse().ok()),
        roof_shape: candidate.roof_shape.clone(),
    })
}

fn part_belongs_to_candidate(
    part: &arnis_core::BuildingPart,
    candidate: &arnis_core::BuildingCandidate,
) -> bool {
    let Some(center) = component_center(&part.component) else {
        return false;
    };
    candidate.components.iter().any(|component| {
        geo_point_in_ring(&center, &component.exterior)
            && !component
                .interior_rings
                .iter()
                .any(|ring| geo_point_in_ring(&center, ring))
    })
}

fn component_center(component: &arnis_core::FootprintComponent) -> Option<arnis_core::GeoPoint> {
    let count = component.exterior.len();
    (count > 0).then(|| arnis_core::GeoPoint {
        lng: component
            .exterior
            .iter()
            .map(|point| point.lng)
            .sum::<f64>()
            / count as f64,
        lat: component
            .exterior
            .iter()
            .map(|point| point.lat)
            .sum::<f64>()
            / count as f64,
    })
}

fn relation_components(members: &Value) -> Vec<arnis_core::FootprintComponent> {
    let Some(members) = members.as_array() else {
        return vec![];
    };
    let outers: Vec<_> = members
        .iter()
        .filter(|m| {
            matches!(
                m.get("role").and_then(Value::as_str),
                Some("outer") | Some("part")
            )
        })
        .map(|m| geometry_points(&m["geometry"]))
        .filter(|r| r.len() >= 3)
        .collect();
    let inners: Vec<_> = members
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("inner"))
        .map(|m| geometry_points(&m["geometry"]))
        .filter(|r| r.len() >= 3)
        .collect();
    outers
        .into_iter()
        .map(|exterior| arnis_core::FootprintComponent {
            interior_rings: inners
                .iter()
                .filter(|ring| {
                    ring.first()
                        .is_some_and(|point| geo_point_in_ring(point, &exterior))
                })
                .cloned()
                .collect(),
            exterior,
        })
        .collect()
}

fn geo_point_in_ring(point: &arnis_core::GeoPoint, ring: &[arnis_core::GeoPoint]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = ring.len() - 1;
    for current in 0..ring.len() {
        let a = &ring[current];
        let b = &ring[previous];
        if ((a.lat > point.lat) != (b.lat > point.lat))
            && point.lng < (b.lng - a.lng) * (point.lat - a.lat) / (b.lat - a.lat) + a.lng
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn overture_candidates(
    payload: &Value,
    query: &BuildingCandidateQuery,
) -> Vec<arnis_core::BuildingCandidate> {
    payload
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .flat_map(|(index, feature)| {
            let id = feature
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("feature-{index}"));
            let tags = string_map(feature.get("properties"));
            geojson_components(feature.get("geometry"))
                .into_iter()
                .map(move |components| {
                    candidate_from_parts(
                        format!("overture:{id}"),
                        "overture",
                        tags.clone(),
                        components,
                        query,
                    )
                })
        })
        .collect()
}

fn geojson_components(geometry: Option<&Value>) -> Option<Vec<arnis_core::FootprintComponent>> {
    let geometry = geometry?;
    let coordinates = geometry.get("coordinates")?;
    let polygons: Vec<&Value> = match geometry.get("type")?.as_str()? {
        "Polygon" => vec![coordinates],
        "MultiPolygon" => coordinates.as_array()?.iter().collect(),
        _ => return None,
    };
    let components = polygons
        .into_iter()
        .filter_map(|polygon| {
            let rings = polygon.as_array()?;
            let exterior = lng_lat_points(rings.first()?);
            if exterior.len() < 3 {
                return None;
            }
            Some(arnis_core::FootprintComponent {
                exterior,
                interior_rings: rings
                    .iter()
                    .skip(1)
                    .map(lng_lat_points)
                    .filter(|r| r.len() >= 3)
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    (!components.is_empty()).then_some(components)
}

fn geometry_points(value: &Value) -> Vec<arnis_core::GeoPoint> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|point| {
            Some(arnis_core::GeoPoint {
                lng: point.get("lon")?.as_f64()?,
                lat: point.get("lat")?.as_f64()?,
            })
        })
        .collect()
}
fn lng_lat_points(value: &Value) -> Vec<arnis_core::GeoPoint> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|point| {
            Some(arnis_core::GeoPoint {
                lng: point.get(0)?.as_f64()?,
                lat: point.get(1)?.as_f64()?,
            })
        })
        .collect()
}
fn string_map(value: Option<&Value>) -> HashMap<String, String> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            v.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .or_else(|| v.as_f64().map(|n| (k.clone(), n.to_string())))
        })
        .collect()
}
fn candidate_from_parts(
    id: String,
    source: &str,
    tags: HashMap<String, String>,
    components: Vec<arnis_core::FootprintComponent>,
    query: &BuildingCandidateQuery,
) -> arnis_core::BuildingCandidate {
    let name = tags
        .get("name")
        .or_else(|| tags.get("name:en"))
        .or_else(|| tags.get("name:zh"))
        .cloned();
    let target_names: Vec<_> = std::iter::once(query.name.as_str())
        .chain(query.aliases.iter().map(String::as_str))
        .map(normalize_identity_name)
        .filter(|value| !value.is_empty())
        .collect();
    let candidate_name = name.as_deref().map(normalize_identity_name);
    let exact = candidate_name
        .as_ref()
        .is_some_and(|candidate| target_names.iter().any(|target| candidate == target));
    let partial = candidate_name.as_ref().is_some_and(|candidate| {
        target_names.iter().any(|target| {
            candidate.chars().count().min(target.chars().count()) >= 2
                && (candidate.contains(target) || target.contains(candidate))
        })
    });
    let identity_confidence = if exact {
        "high"
    } else if partial {
        "medium"
    } else {
        "low"
    }
    .to_string();
    let height_m = tags
        .get("height")
        .and_then(|v| v.trim_end_matches('m').trim().parse().ok());
    let floors = tags.get("building:levels").and_then(|v| v.parse().ok());
    let roof_shape = tags.get("roof:shape").cloned();
    let (center_lng, center_lat, width_m, length_m) = component_metrics(&components);
    let distance_m = haversine_m(query.lng, query.lat, center_lng, center_lat);
    arnis_core::BuildingCandidate {
        id,
        source: source.into(),
        name,
        tags,
        components,
        height_m,
        floors,
        roof_shape,
        identity_confidence,
        distance_m,
        width_m,
        length_m,
        parts: vec![],
    }
}

fn normalize_identity_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn component_metrics(components: &[arnis_core::FootprintComponent]) -> (f64, f64, f64, f64) {
    let mut min_lng = f64::INFINITY;
    let mut max_lng = f64::NEG_INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    for point in components
        .iter()
        .flat_map(|component| component.exterior.iter())
    {
        min_lng = min_lng.min(point.lng);
        max_lng = max_lng.max(point.lng);
        min_lat = min_lat.min(point.lat);
        max_lat = max_lat.max(point.lat);
    }
    let center_lng = (min_lng + max_lng) / 2.0;
    let center_lat = (min_lat + max_lat) / 2.0;
    let width_m = haversine_m(min_lng, center_lat, max_lng, center_lat);
    let length_m = haversine_m(center_lng, min_lat, center_lng, max_lat);
    (center_lng, center_lat, width_m, length_m)
}

fn haversine_m(lng_a: f64, lat_a: f64, lng_b: f64, lat_b: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let d_lat = (lat_b - lat_a).to_radians();
    let d_lng = (lng_b - lng_a).to_radians();
    let lat_a = lat_a.to_radians();
    let lat_b = lat_b.to_radians();
    let h = (d_lat / 2.0).sin().powi(2) + lat_a.cos() * lat_b.cos() * (d_lng / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().asin()
}
fn confidence_rank(value: &str) -> u8 {
    match value {
        "high" => 2,
        "medium" => 1,
        _ => 0,
    }
}

#[tauri::command]
async fn query_overture_buildings(request: OvertureQueryRequest) -> Result<Value, String> {
    validate_coordinates(request.lng, request.lat)?;
    let endpoint = std::env::var("OVERTURE_BUILDING_ENDPOINT").map_err(|_| {
        "OVERTURE_BUILDING_ENDPOINT is not configured for the Tauri backend".to_string()
    })?;
    let radius_m = request.radius_m.clamp(MIN_RADIUS_M, MAX_RADIUS_M);
    let limit = request.limit.clamp(1, MAX_FEATURES);
    let bounds = bounds_around(request.lng, request.lat, radius_m);
    let release_id = request
        .release_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("OVERTURE_RELEASE_ID").ok());
    let bbox = format!(
        "{},{},{},{}",
        bounds.min_lng, bounds.min_lat, bounds.max_lng, bounds.max_lat
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| format!("Could not create Overture client: {error}"))?;
    let mut query = vec![
        ("lng", request.lng.to_string()),
        ("lat", request.lat.to_string()),
        ("radius_m", radius_m.to_string()),
        ("name", request.name),
        ("theme", "buildings".to_string()),
        ("type", "building".to_string()),
        ("bbox", bbox),
        ("limit", limit.to_string()),
    ];
    if let Some(release) = &release_id {
        query.push(("release", release.clone()));
    }

    let response = client
        .get(endpoint)
        .query(&query)
        .header(
            reqwest::header::ACCEPT,
            "application/geo+json, application/json",
        )
        .send()
        .await
        .map_err(|error| format!("Overture query failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Overture query returned HTTP {}",
            response.status()
        ));
    }
    if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES {
        return Err("Overture response exceeded the 5 MiB safety limit".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read Overture response: {error}"))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("Overture response exceeded the 5 MiB safety limit".to_string());
    }
    let mut payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Overture response was not valid JSON: {error}"))?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "Overture response must be a GeoJSON object".to_string())?;
    let features = object
        .get_mut("features")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Overture response must contain a features array".to_string())?;
    features.truncate(limit);
    object.insert(
        "metadata".to_string(),
        json!({
            "releaseId": release_id,
            "queryBounds": bounds,
            "queryLimit": limit
        }),
    );
    Ok(payload)
}

fn validate_coordinates(lng: f64, lat: f64) -> Result<(), String> {
    if !lng.is_finite()
        || !lat.is_finite()
        || !(-180.0..=180.0).contains(&lng)
        || !(-90.0..=90.0).contains(&lat)
    {
        return Err("Overture query coordinates are invalid".to_string());
    }
    Ok(())
}

fn bounds_around(lng: f64, lat: f64, radius_m: u32) -> GeographicBounds {
    let radius = radius_m as f64;
    let lat_delta = radius / 111_320.0;
    let lng_scale = (lat.to_radians().cos()).max(0.1);
    let lng_delta = radius / (111_320.0 * lng_scale);
    GeographicBounds {
        min_lng: lng - lng_delta,
        min_lat: lat - lat_delta,
        max_lng: lng + lng_delta,
        max_lat: lat + lat_delta,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            query_overture_buildings,
            query_building_candidates,
            generate_building,
            save_export_bundle,
            save_local_campus_annotations,
            load_local_campus_annotations
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_coordinates() {
        assert!(validate_coordinates(181.0, 31.0).is_err());
        assert!(validate_coordinates(121.0, 91.0).is_err());
        assert!(validate_coordinates(f64::NAN, 31.0).is_err());
    }

    #[test]
    fn creates_bounded_query_extent() {
        let bounds = bounds_around(121.409, 31.228, 120);
        assert!(bounds.min_lng < 121.409 && bounds.max_lng > 121.409);
        assert!(bounds.min_lat < 31.228 && bounds.max_lat > 31.228);
        assert!((bounds.max_lat - bounds.min_lat) < 0.003);
    }

    #[test]
    fn parses_named_overpass_candidate_without_placeholder_fallback() {
        let payload = json!({"elements":[{
            "type":"way","id":42,
            "tags":{"building":"university","name":"Putuo Campus Library","building:levels":"5"},
            "geometry":[
                {"lon":121.0,"lat":31.0},{"lon":121.001,"lat":31.0},
                {"lon":121.001,"lat":30.999},{"lon":121.0,"lat":30.999}
            ]
        }]});
        let query = BuildingCandidateQuery {
            name: "Putuo Campus Library".into(),
            aliases: vec!["图书馆".into()],
            lng: 121.0,
            lat: 31.0,
            radius_m: 250,
            scale: 1.0,
            coordinate_system: "WGS-84".into(),
            gaode_poi_id: "gaode:test-library".into(),
            gaode_lng: 121.00465,
            gaode_lat: 30.99816,
            transformation: "gcj02-to-wgs84-iterative-v1".into(),
        };
        let candidates = overpass_candidates(&payload, &query);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "osm:42");
        assert_eq!(candidates[0].identity_confidence, "high");
        assert_eq!(candidates[0].floors, Some(5));
        assert!(candidates[0].distance_m.is_finite());
        assert!(candidates[0].width_m > 0.0);
    }

    #[test]
    fn identity_confidence_follows_the_current_target_instead_of_library_tags() {
        let components = vec![arnis_core::FootprintComponent {
            exterior: vec![
                arnis_core::GeoPoint {
                    lng: 121.0,
                    lat: 31.0,
                },
                arnis_core::GeoPoint {
                    lng: 121.001,
                    lat: 31.0,
                },
                arnis_core::GeoPoint {
                    lng: 121.001,
                    lat: 30.999,
                },
            ],
            interior_rings: vec![],
        }];
        let query = BuildingCandidateQuery {
            name: "华东师范大学普陀校区体育馆".into(),
            aliases: vec!["体育馆".into()],
            lng: 121.0,
            lat: 31.0,
            radius_m: 250,
            scale: 1.0,
            coordinate_system: "WGS-84".into(),
            gaode_poi_id: "gaode:test-gym".into(),
            gaode_lng: 121.00465,
            gaode_lat: 30.99816,
            transformation: "gcj02-to-wgs84-iterative-v1".into(),
        };
        let library = candidate_from_parts(
            "osm:library".into(),
            "osm_overpass",
            HashMap::from([
                ("building".into(), "yes".into()),
                ("amenity".into(), "library".into()),
                ("name".into(), "图书馆".into()),
            ]),
            components.clone(),
            &query,
        );
        let gym = candidate_from_parts(
            "osm:gym".into(),
            "osm_overpass",
            HashMap::from([
                ("building".into(), "sports_hall".into()),
                ("name".into(), "体育馆".into()),
            ]),
            components,
            &query,
        );
        assert_eq!(library.identity_confidence, "low");
        assert_eq!(gym.identity_confidence, "high");
    }

    #[test]
    fn nearest_unnamed_candidate_becomes_location_match_when_names_are_missing() {
        let mut candidates = vec![
            arnis_core::BuildingCandidate {
                id: "near".into(),
                source: "overture".into(),
                name: None,
                tags: HashMap::new(),
                components: vec![],
                height_m: None,
                floors: None,
                roof_shape: None,
                identity_confidence: "low".into(),
                distance_m: 18.0,
                width_m: 20.0,
                length_m: 30.0,
                parts: vec![],
            },
            arnis_core::BuildingCandidate {
                id: "far".into(),
                source: "overture".into(),
                name: None,
                tags: HashMap::new(),
                components: vec![],
                height_m: None,
                floors: None,
                roof_shape: None,
                identity_confidence: "low".into(),
                distance_m: 120.0,
                width_m: 20.0,
                length_m: 30.0,
                parts: vec![],
            },
        ];
        assert!(promote_nearest_location_match(&mut candidates));
        assert_eq!(candidates[0].identity_confidence, "medium");
        assert_eq!(candidates[1].identity_confidence, "low");
    }

    #[test]
    fn saves_export_bundle_to_the_selected_directory() {
        let directory =
            std::env::temp_dir().join(format!("campus-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let result = save_export_bundle(
            directory.to_string_lossy().to_string(),
            vec![ExportFile {
                file_name: "library.schem".into(),
                bytes: vec![1, 2, 3, 4],
            }],
        )
        .unwrap();
        assert_eq!(
            std::fs::read(directory.join("library.schem")).unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(result.paths.len(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn attached_building_parts_are_not_returned_as_standalone_candidates() {
        let payload = json!({"elements":[
            {"type":"way","id":1,"tags":{"building":"yes","name":"体育馆"},"geometry":[
                {"lon":121.0,"lat":31.0},{"lon":121.001,"lat":31.0},{"lon":121.001,"lat":30.999},{"lon":121.0,"lat":30.999}
            ]},
            {"type":"way","id":2,"tags":{"building:part":"yes","height":"12"},"geometry":[
                {"lon":121.0002,"lat":30.9998},{"lon":121.0008,"lat":30.9998},{"lon":121.0008,"lat":30.9992},{"lon":121.0002,"lat":30.9992}
            ]}
        ]});
        let query = BuildingCandidateQuery {
            name: "体育馆".into(),
            aliases: vec![],
            lng: 121.0,
            lat: 31.0,
            radius_m: 250,
            scale: 1.0,
            coordinate_system: "WGS-84".into(),
            gaode_poi_id: "gaode:test-gym".into(),
            gaode_lng: 121.00465,
            gaode_lat: 30.99816,
            transformation: "gcj02-to-wgs84-iterative-v1".into(),
        };
        let candidates = overpass_candidates(&payload, &query);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].parts.len(), 1);
    }

    #[test]
    fn rejects_raw_gaode_candidate_query() {
        let query = BuildingCandidateQuery {
            name: "Library".into(),
            aliases: vec![],
            lng: 121.409,
            lat: 31.2282,
            radius_m: 250,
            scale: 1.0,
            coordinate_system: "GCJ-02".into(),
            gaode_poi_id: "gaode:test-library".into(),
            gaode_lng: 121.409,
            gaode_lat: 31.2282,
            transformation: "gcj02-to-wgs84-iterative-v1".into(),
        };
        assert!(validate_candidate_query(&query).is_err());
    }

    #[test]
    fn preserves_relation_parts_and_inner_rings() {
        let members = json!([
            {"role":"outer","geometry":[{"lon":0.0,"lat":1.0},{"lon":1.0,"lat":1.0},{"lon":1.0,"lat":0.0},{"lon":0.0,"lat":0.0}]},
            {"role":"inner","geometry":[{"lon":0.2,"lat":0.8},{"lon":0.8,"lat":0.8},{"lon":0.8,"lat":0.2},{"lon":0.2,"lat":0.2}]},
            {"role":"part","geometry":[{"lon":2.0,"lat":1.0},{"lon":3.0,"lat":1.0},{"lon":3.0,"lat":0.0},{"lon":2.0,"lat":0.0}]}
        ]);
        let components = relation_components(&members);
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].interior_rings.len(), 1);
        assert!(components[1].interior_rings.is_empty());
    }

    #[tokio::test]
    #[ignore = "live Overpass/Overture smoke test"]
    async fn live_putuo_library_query_uses_confirmed_gaode_anchor() {
        let request = BuildingCandidateQuery {
            name: "华东师范大学普陀校区图书馆".into(),
            aliases: vec!["Putuo Campus Library".into(), "图书馆".into()],
            lng: 121.40193058322627,
            lat: 31.230153283602437,
            radius_m: 250,
            scale: 1.0,
            coordinate_system: "WGS-84".into(),
            gaode_poi_id: "gaode:js-poi:B00156-library".into(),
            gaode_lng: 121.406582,
            gaode_lat: 31.228318,
            transformation: "gcj02-to-wgs84-iterative-v1".into(),
        };
        let result = query_building_candidates(request).await.unwrap();
        assert!(!result.candidates.is_empty());
        assert!(
            result
                .candidates
                .iter()
                .any(|candidate| candidate.id == "osm:5699952"),
            "expected the named Putuo library multipolygon relation"
        );
        let library = result
            .candidates
            .iter()
            .find(|candidate| candidate.id == "osm:5699952")
            .unwrap();
        assert!(
            library.parts.len() >= 20,
            "expected detailed library building parts"
        );
        let generated = arnis_core::generate_building(arnis_core::GenerateBuildingRequest {
            candidate_id: library.id.clone(),
            source: library.source.clone(),
            components: library.components.clone(),
            height_m: library.height_m,
            floors: library.floors,
            roof_shape: library.roof_shape.clone(),
            blocks_per_meter: 1.0,
            seed: 42,
            materials: arnis_core::MaterialOverrides::default(),
            correction_notes: vec!["live Putuo library validation".into()],
            parts: library.parts.clone(),
        })
        .unwrap();
        assert_eq!(generated.report.building_part_count, library.parts.len());
        assert!(
            generated.height >= 35,
            "expected the observed tower massing"
        );
        assert!(generated.width >= 80 && generated.length >= 60);
        println!(
            "generated={}x{}x{} blocks={} building_parts={}",
            generated.width,
            generated.height,
            generated.length,
            generated.report.non_air_blocks,
            generated.report.building_part_count
        );
        assert!(result
            .candidates
            .iter()
            .any(|candidate| candidate.distance_m < 100.0));
        for candidate in result.candidates.iter().take(10) {
            println!(
                "{} {} distance={:.1}m size={:.1}x{:.1}m parts={}",
                candidate.id,
                candidate.name.as_deref().unwrap_or("unnamed"),
                candidate.distance_m,
                candidate.width_m,
                candidate.length_m,
                candidate.parts.len()
            );
        }
    }
}
