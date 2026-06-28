//! Adapted from Arnis 2.9.0 (commit 7d2a0ebe), Apache-2.0.
//! Modified by Campus Reconstruction Tool contributors: extracted single-building
//! generation from the world writer and replaced it with an in-memory BlockSink.

#[allow(dead_code)]
mod bresenham;
pub mod deterministic_rng;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

const MAX_SPAN: usize = 512;
const MAX_BLOCKS: usize = 32_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeoPoint {
    pub lng: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FootprintComponent {
    pub exterior: Vec<GeoPoint>,
    #[serde(default)]
    pub interior_rings: Vec<Vec<GeoPoint>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildingPart {
    pub id: String,
    pub component: FootprintComponent,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    pub height_m: Option<f64>,
    pub min_height_m: Option<f64>,
    pub floors: Option<u32>,
    pub min_level: Option<u32>,
    pub roof_shape: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildingCandidate {
    pub id: String,
    pub source: String,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    pub components: Vec<FootprintComponent>,
    pub height_m: Option<f64>,
    pub floors: Option<u32>,
    pub roof_shape: Option<String>,
    pub identity_confidence: String,
    pub distance_m: f64,
    pub width_m: f64,
    pub length_m: f64,
    #[serde(default)]
    pub parts: Vec<BuildingPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MaterialOverrides {
    pub foundation: Option<String>,
    pub wall: Option<String>,
    pub window: Option<String>,
    pub floor: Option<String>,
    pub roof: Option<String>,
    pub entrance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateBuildingRequest {
    pub candidate_id: String,
    pub source: String,
    pub components: Vec<FootprintComponent>,
    pub height_m: Option<f64>,
    pub floors: Option<u32>,
    pub roof_shape: Option<String>,
    #[serde(default = "default_scale")]
    pub blocks_per_meter: f64,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub materials: MaterialOverrides,
    #[serde(default)]
    pub correction_notes: Vec<String>,
    #[serde(default)]
    pub parts: Vec<BuildingPart>,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockRun {
    pub palette_index: u16,
    pub run_length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationReport {
    pub candidate_id: String,
    pub source: String,
    pub generator: String,
    pub blocks_per_meter: f64,
    pub floor_count: u32,
    pub roof_shape: String,
    pub non_air_blocks: usize,
    pub deterministic_seed: u64,
    pub correction_notes: Vec<String>,
    pub building_part_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedBuilding {
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub palette: Vec<String>,
    pub block_runs: Vec<BlockRun>,
    pub report: GenerationReport,
}

#[derive(Default)]
pub struct BlockSink {
    blocks: BTreeMap<(i32, i32, i32), String>,
}

impl BlockSink {
    pub fn set_block_absolute(&mut self, block: &str, x: i32, y: i32, z: i32) {
        self.blocks.insert((x, y, z), block.to_string());
    }

    fn finish(self, report: GenerationReport) -> Result<GeneratedBuilding, String> {
        if self.blocks.is_empty() {
            return Err("Arnis building generation produced no blocks".into());
        }
        let min_x = self.blocks.keys().map(|v| v.0).min().unwrap();
        let min_y = self.blocks.keys().map(|v| v.1).min().unwrap();
        let min_z = self.blocks.keys().map(|v| v.2).min().unwrap();
        let max_x = self.blocks.keys().map(|v| v.0).max().unwrap();
        let max_y = self.blocks.keys().map(|v| v.1).max().unwrap();
        let max_z = self.blocks.keys().map(|v| v.2).max().unwrap();
        let width = (max_x - min_x + 1) as usize;
        let height = (max_y - min_y + 1) as usize;
        let length = (max_z - min_z + 1) as usize;
        if width > MAX_SPAN
            || height > MAX_SPAN
            || length > MAX_SPAN
            || width * height * length > MAX_BLOCKS
        {
            return Err("Generated building exceeds the configured size limit".into());
        }
        let mut palette = vec!["minecraft:air".to_string()];
        let mut palette_lookup = HashMap::from([("minecraft:air".to_string(), 0u16)]);
        for block in self.blocks.values() {
            if !palette_lookup.contains_key(block) {
                let index = palette.len() as u16;
                palette_lookup.insert(block.clone(), index);
                palette.push(block.clone());
            }
        }
        let mut runs = Vec::new();
        let mut last = u16::MAX;
        let mut count = 0u32;
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    let index = self
                        .blocks
                        .get(&(x, y, z))
                        .and_then(|b| palette_lookup.get(b))
                        .copied()
                        .unwrap_or(0);
                    if index == last {
                        count += 1;
                    } else {
                        if count > 0 {
                            runs.push(BlockRun {
                                palette_index: last,
                                run_length: count,
                            });
                        }
                        last = index;
                        count = 1;
                    }
                }
            }
        }
        if count > 0 {
            runs.push(BlockRun {
                palette_index: last,
                run_length: count,
            });
        }
        Ok(GeneratedBuilding {
            width,
            height,
            length,
            palette,
            block_runs: runs,
            report,
        })
    }
}

#[derive(Clone, Copy)]
struct P {
    x: f64,
    z: f64,
}
struct LocalComponent {
    exterior: Vec<P>,
    holes: Vec<Vec<P>>,
}

struct LocalPart {
    component: LocalComponent,
    height: i32,
    min_height: i32,
    floors: u32,
    roof_shape: String,
}

pub fn generate_building(request: GenerateBuildingRequest) -> Result<GeneratedBuilding, String> {
    validate_request(&request)?;
    let scale = request.blocks_per_meter;
    let origin = request.components[0].exterior[0].clone();
    let lat_scale = 111_320.0;
    let lng_scale = lat_scale * origin.lat.to_radians().cos();
    let mut components: Vec<LocalComponent> = request
        .components
        .iter()
        .map(|component| local_component(component, &origin, lng_scale, lat_scale, scale))
        .collect();
    let fallback_floors = request.floors.unwrap_or(3).clamp(1, 64);
    let fallback_height = request
        .height_m
        .map(|height| (height * scale).ceil() as i32)
        .unwrap_or((fallback_floors * 4) as i32)
        .max(4);
    let mut parts: Vec<LocalPart> = request
        .parts
        .iter()
        .map(|part| {
            let floors = part.floors.unwrap_or(fallback_floors).clamp(1, 64);
            let min_height = part
                .min_height_m
                .map(|height| (height * scale).round() as i32)
                .or_else(|| {
                    part.min_level
                        .map(|level| (level as f64 * 4.0 * scale).round() as i32)
                })
                .unwrap_or(0)
                .max(0);
            let height = part
                .height_m
                .map(|height| (height * scale).ceil() as i32)
                .unwrap_or_else(|| (min_height + (floors * 4) as i32).max(fallback_height))
                .max(min_height + 1);
            LocalPart {
                component: local_component(&part.component, &origin, lng_scale, lat_scale, scale),
                height,
                min_height,
                floors,
                roof_shape: normalize_roof(part.roof_shape.as_deref()),
            }
        })
        .collect();
    let min_x = components
        .iter()
        .flat_map(|c| c.exterior.iter())
        .map(|p| p.x)
        .fold(f64::INFINITY, f64::min);
    let min_z = components
        .iter()
        .flat_map(|c| c.exterior.iter())
        .map(|p| p.z)
        .fold(f64::INFINITY, f64::min);
    for c in &mut components {
        shift_component(c, min_x, min_z);
    }
    for part in &mut parts {
        shift_component(&mut part.component, min_x, min_z);
    }
    let max_x = components
        .iter()
        .flat_map(|c| c.exterior.iter())
        .map(|p| p.x)
        .fold(0.0, f64::max)
        .ceil() as i32
        + 2;
    let max_z = components
        .iter()
        .flat_map(|c| c.exterior.iter())
        .map(|p| p.z)
        .fold(0.0, f64::max)
        .ceil() as i32
        + 2;
    if max_x as usize > MAX_SPAN || max_z as usize > MAX_SPAN {
        return Err("Footprint exceeds the configured size limit".into());
    }
    let mask = mask_for_components(&components, max_x, max_z);
    if !mask.iter().flatten().any(|v| *v) {
        return Err("Accepted footprint did not occupy any block cells".into());
    }

    let roof_shape = normalize_roof(request.roof_shape.as_deref());
    let m = materials(&request.materials);
    let mut sink = BlockSink::default();
    for z in 0..=max_z {
        for x in 0..=max_x {
            if mask[z as usize][x as usize] {
                sink.set_block_absolute(&m.foundation, x, 0, z);
            }
        }
    }
    if parts.is_empty() {
        render_volume(
            &mut sink,
            &mask,
            0,
            fallback_height,
            fallback_floors,
            request.seed,
            &m,
        );
        add_roof(&mut sink, &mask, fallback_height, &roof_shape, &m.roof);
    } else {
        parts.sort_by_key(|part| part.height);
        for (index, part) in parts.iter().enumerate() {
            let part_mask =
                mask_for_components(std::slice::from_ref(&part.component), max_x, max_z);
            if !part_mask.iter().flatten().any(|value| *value) {
                continue;
            }
            render_volume(
                &mut sink,
                &part_mask,
                part.min_height,
                part.height,
                part.floors,
                request.seed.wrapping_add(index as u64),
                &m,
            );
            add_roof(
                &mut sink,
                &part_mask,
                part.height,
                &part.roof_shape,
                &m.roof,
            );
        }
    }
    add_entrance(&mut sink, &mask, fallback_height, &m.entrance);
    let non_air_blocks = sink.blocks.len();
    sink.finish(GenerationReport {
        candidate_id: request.candidate_id,
        source: request.source,
        generator: "arnis-core-2.9.0-campus".into(),
        blocks_per_meter: scale,
        floor_count: parts
            .iter()
            .map(|part| part.floors)
            .max()
            .unwrap_or(fallback_floors),
        roof_shape: if parts.is_empty() {
            roof_shape
        } else {
            "building-parts".into()
        },
        non_air_blocks,
        deterministic_seed: request.seed,
        correction_notes: request.correction_notes,
        building_part_count: parts.len(),
    })
}

fn local_component(
    component: &FootprintComponent,
    origin: &GeoPoint,
    lng_scale: f64,
    lat_scale: f64,
    scale: f64,
) -> LocalComponent {
    let convert = |point: &GeoPoint| P {
        x: (point.lng - origin.lng) * lng_scale * scale,
        z: (origin.lat - point.lat) * lat_scale * scale,
    };
    LocalComponent {
        exterior: component.exterior.iter().map(convert).collect(),
        holes: component
            .interior_rings
            .iter()
            .map(|ring| ring.iter().map(convert).collect())
            .collect(),
    }
}

fn shift_component(component: &mut LocalComponent, min_x: f64, min_z: f64) {
    for point in component
        .exterior
        .iter_mut()
        .chain(component.holes.iter_mut().flatten())
    {
        point.x -= min_x - 2.0;
        point.z -= min_z - 2.0;
    }
}

fn mask_for_components(components: &[LocalComponent], max_x: i32, max_z: i32) -> Vec<Vec<bool>> {
    let mut mask = vec![vec![false; max_x as usize + 1]; max_z as usize + 1];
    for z in 0..=max_z {
        for x in 0..=max_x {
            mask[z as usize][x as usize] = inside_components(
                P {
                    x: x as f64 + 0.5,
                    z: z as f64 + 0.5,
                },
                components,
            );
        }
    }
    mask
}

fn render_volume(
    sink: &mut BlockSink,
    mask: &[Vec<bool>],
    min_height: i32,
    height: i32,
    floors: u32,
    seed: u64,
    materials: &Materials,
) {
    let span = (height - min_height).max(1);
    let spacing = (span / floors.max(1) as i32).max(3);
    for z in 0..mask.len() as i32 {
        for x in 0..mask[0].len() as i32 {
            if !mask[z as usize][x as usize] {
                continue;
            }
            sink.set_block_absolute(&materials.floor, x, min_height, z);
            for y in min_height + 1..height {
                if (y - min_height) % spacing == 0 {
                    sink.set_block_absolute(&materials.floor, x, y, z);
                }
                if boundary(mask, x, z) {
                    let block = if y > min_height + 1
                        && (y - min_height) % spacing != 0
                        && (x + z + seed as i32).rem_euclid(3) != 0
                    {
                        &materials.window
                    } else {
                        &materials.wall
                    };
                    sink.set_block_absolute(block, x, y, z);
                }
            }
        }
    }
}

struct Materials {
    foundation: String,
    wall: String,
    window: String,
    floor: String,
    roof: String,
    entrance: String,
}
fn materials(m: &MaterialOverrides) -> Materials {
    Materials {
        foundation: m
            .foundation
            .clone()
            .unwrap_or("minecraft:smooth_stone".into()),
        wall: m.wall.clone().unwrap_or("minecraft:stone_bricks".into()),
        window: m.window.clone().unwrap_or("minecraft:glass".into()),
        floor: m.floor.clone().unwrap_or("minecraft:oak_planks".into()),
        roof: m.roof.clone().unwrap_or("minecraft:dark_oak_slab".into()),
        entrance: m
            .entrance
            .clone()
            .unwrap_or("minecraft:dark_oak_door".into()),
    }
}
fn validate_request(r: &GenerateBuildingRequest) -> Result<(), String> {
    if r.candidate_id.trim().is_empty() {
        return Err("candidateId is required".into());
    }
    if !(0.25..=4.0).contains(&r.blocks_per_meter) {
        return Err("blocksPerMeter must be between 0.25 and 4".into());
    }
    if r.components.is_empty() || r.components.iter().any(|c| c.exterior.len() < 3) {
        return Err("At least one closed footprint component is required".into());
    }
    for p in r.components.iter().flat_map(|c| c.exterior.iter()) {
        if !p.lng.is_finite() || !p.lat.is_finite() {
            return Err("Footprint coordinates must be finite".into());
        }
    }
    for part in &r.parts {
        if part.component.exterior.len() < 3
            || part
                .component
                .exterior
                .iter()
                .any(|point| !point.lng.is_finite() || !point.lat.is_finite())
        {
            return Err("Building part coordinates must be finite polygons".into());
        }
    }
    Ok(())
}
fn inside_components(p: P, cs: &[LocalComponent]) -> bool {
    cs.iter()
        .any(|c| point_in_ring(p, &c.exterior) && !c.holes.iter().any(|h| point_in_ring(p, h)))
}
fn point_in_ring(p: P, ring: &[P]) -> bool {
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[j];
        if ((a.z > p.z) != (b.z > p.z)) && (p.x < (b.x - a.x) * (p.z - a.z) / (b.z - a.z) + a.x) {
            inside = !inside
        }
        j = i;
    }
    inside
}
fn boundary(mask: &[Vec<bool>], x: i32, z: i32) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| {
        let nx = x + dx;
        let nz = z + dz;
        nz < 0
            || nx < 0
            || nz as usize >= mask.len()
            || nx as usize >= mask[0].len()
            || !mask[nz as usize][nx as usize]
    })
}
fn add_entrance(s: &mut BlockSink, mask: &[Vec<bool>], h: i32, block: &str) {
    let south = (0..mask.len()).rev().find(|z| mask[*z].iter().any(|v| *v));
    if let Some(z) = south {
        let xs: Vec<_> = mask[z]
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(x, _)| x as i32)
            .collect();
        if !xs.is_empty() {
            let c = xs[xs.len() / 2];
            for x in c - 1..=c + 1 {
                for y in 1..=h.min(4) {
                    s.set_block_absolute(block, x, y, z as i32)
                }
            }
        }
    }
}
fn normalize_roof(v: Option<&str>) -> String {
    match v.unwrap_or("flat").to_ascii_lowercase().as_str() {
        "gabled" | "hipped" | "skillion" | "pyramidal" | "dome" | "cone" | "onion" => {
            v.unwrap().to_ascii_lowercase()
        }
        _ => "flat".into(),
    }
}
fn add_roof(s: &mut BlockSink, mask: &[Vec<bool>], base: i32, shape: &str, block: &str) {
    let w = mask[0].len() as f64;
    let l = mask.len() as f64;
    let cx = (w - 1.0) / 2.0;
    let cz = (l - 1.0) / 2.0;
    let radius = w.min(l) / 2.0;
    for (z, row) in mask.iter().enumerate() {
        for (x, on) in row.iter().enumerate() {
            if !on {
                continue;
            }
            let dx = (x as f64 - cx).abs();
            let dz = (z as f64 - cz).abs();
            let layer = match shape {
                "gabled" => ((radius - dx).max(0.0) * 0.45) as i32,
                "hipped" | "pyramidal" => ((radius - dx.max(dz)).max(0.0) * 0.55) as i32,
                "skillion" => ((x as f64 / w) * 4.0) as i32,
                "dome" => ((radius * radius - dx * dx - dz * dz).max(0.0).sqrt() * 0.65) as i32,
                "cone" => ((radius - (dx * dx + dz * dz).sqrt()).max(0.0) * 0.8) as i32,
                "onion" => {
                    let d = (dx * dx + dz * dz).sqrt() / radius;
                    ((1.0 - d).max(0.0) * radius * 1.1) as i32
                }
                _ => 0,
            };
            for y in base..=base + layer.max(1) {
                s.set_block_absolute(block, x as i32, y, z as i32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> Vec<FootprintComponent> {
        vec![FootprintComponent {
            exterior: vec![
                GeoPoint {
                    lng: 121.0,
                    lat: 31.0,
                },
                GeoPoint {
                    lng: 121.0001,
                    lat: 31.0,
                },
                GeoPoint {
                    lng: 121.0001,
                    lat: 30.9999,
                },
                GeoPoint {
                    lng: 121.0,
                    lat: 30.9999,
                },
            ],
            interior_rings: vec![],
        }]
    }
    fn request(shape: &str) -> GenerateBuildingRequest {
        GenerateBuildingRequest {
            candidate_id: "osm:1".into(),
            source: "osm".into(),
            components: square(),
            height_m: Some(15.0),
            floors: Some(4),
            roof_shape: Some(shape.into()),
            blocks_per_meter: 1.0,
            seed: 42,
            materials: Default::default(),
            correction_notes: vec![],
            parts: vec![],
        }
    }
    #[test]
    fn deterministic_block_snapshot() {
        let a = generate_building(request("hipped")).unwrap();
        let b = generate_building(request("hipped")).unwrap();
        assert_eq!(a.block_runs, b.block_runs);
        assert!(a.report.non_air_blocks > 100)
    }
    #[test]
    fn preserves_hole() {
        let mut r = request("flat");
        r.components[0].interior_rings = vec![vec![
            GeoPoint {
                lng: 121.00003,
                lat: 30.99997,
            },
            GeoPoint {
                lng: 121.00007,
                lat: 30.99997,
            },
            GeoPoint {
                lng: 121.00007,
                lat: 30.99993,
            },
            GeoPoint {
                lng: 121.00003,
                lat: 30.99993,
            },
        ]];
        assert!(generate_building(r).is_ok())
    }
    #[test]
    fn supports_roof_shapes() {
        for shape in [
            "flat",
            "gabled",
            "hipped",
            "skillion",
            "pyramidal",
            "dome",
            "cone",
            "onion",
        ] {
            let out = generate_building(request(shape)).unwrap();
            assert_eq!(out.report.roof_shape, shape)
        }
    }

    #[test]
    fn building_parts_drive_observed_massing() {
        let mut r = request("flat");
        r.parts = vec![BuildingPart {
            id: "osm:part:1".into(),
            component: square().into_iter().next().unwrap(),
            tags: HashMap::from([("building:part".into(), "yes".into())]),
            height_m: Some(28.0),
            min_height_m: Some(4.0),
            floors: Some(7),
            min_level: Some(1),
            roof_shape: Some("flat".into()),
        }];
        let generated = generate_building(r).unwrap();
        assert_eq!(generated.report.building_part_count, 1);
        assert_eq!(generated.report.roof_shape, "building-parts");
        assert!(generated.height >= 25);
    }
    #[test]
    fn rejects_invalid_scale() {
        let mut r = request("flat");
        r.blocks_per_meter = 9.0;
        assert!(generate_building(r).is_err())
    }
}
