//! Adapted from Arnis 2.9.0 (commit 7d2a0ebe), Apache-2.0.
//! Modified by Campus Reconstruction Tool contributors: extracted single-building
//! generation from the world writer and replaced it with an in-memory BlockSink.

#[allow(dead_code)]
mod bresenham;
pub mod deterministic_rng;
mod foundation_tracer;

pub use foundation_tracer::*;

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
    pub accent: Option<String>,
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
    #[serde(default = "default_style_preset")]
    pub style_preset: String,
    #[serde(default = "default_window_density")]
    pub window_density: u8,
    #[serde(default = "default_wall_depth")]
    pub wall_depth: u8,
}

fn default_scale() -> f64 {
    1.0
}

fn default_style_preset() -> String {
    "school".into()
}

fn default_window_density() -> u8 {
    55
}

fn default_wall_depth() -> u8 {
    40
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
    let m = materials(&request.style_preset, &request.materials);
    let behavior = style_behavior(&request.style_preset);
    let mut sink = BlockSink::default();
    for z in 0..=max_z {
        for x in 0..=max_x {
            if mask[z as usize][x as usize] {
                sink.set_block_absolute(&m.foundation, x, 0, z);
            }
        }
    }
    if parts.is_empty() {
        let roof_base = roof_base_height(0, fallback_height, &roof_shape);
        render_volume(
            &mut sink,
            &mask,
            VolumeSpec {
                min_height: 0,
                height: roof_base,
                floors: fallback_floors,
                seed: request.seed,
                window_density: request.window_density,
                wall_depth: request.wall_depth,
            },
            &behavior,
            &m,
        );
        add_roof(&mut sink, &mask, roof_base, &roof_shape, &m.roof);
        add_style_details(&mut sink, &mask, roof_base, &roof_shape, &behavior, &m);
    } else {
        parts.sort_by_key(|part| part.height);
        for (index, part) in parts.iter().enumerate() {
            let part_mask =
                mask_for_components(std::slice::from_ref(&part.component), max_x, max_z);
            if !part_mask.iter().flatten().any(|value| *value) {
                continue;
            }
            let roof_base = roof_base_height(part.min_height, part.height, &part.roof_shape);
            render_volume(
                &mut sink,
                &part_mask,
                VolumeSpec {
                    min_height: part.min_height,
                    height: roof_base,
                    floors: part.floors,
                    seed: request.seed.wrapping_add(index as u64),
                    window_density: request.window_density,
                    wall_depth: request.wall_depth,
                },
                &behavior,
                &m,
            );
            add_roof(&mut sink, &part_mask, roof_base, &part.roof_shape, &m.roof);
            add_style_details(
                &mut sink,
                &part_mask,
                roof_base,
                &part.roof_shape,
                &behavior,
                &m,
            );
        }
    }
    add_entrance(&mut sink, &mask, fallback_height, &m.entrance);
    let measured_height = parts
        .iter()
        .map(|part| part.height)
        .max()
        .unwrap_or(fallback_height);
    sink.blocks
        .retain(|(_, y, _), _| *y >= 0 && *y < measured_height);
    let non_air_blocks = sink.blocks.len();
    sink.finish(GenerationReport {
        candidate_id: request.candidate_id,
        source: request.source,
        generator: format!(
            "arnis-core-2.9.0-campus-exterior-v1/{}",
            request.style_preset
        ),
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

fn roof_base_height(min_height: i32, measured_height: i32, shape: &str) -> i32 {
    let span = (measured_height - min_height).max(2);
    let reserve = match shape {
        "flat" => 2,
        "dome" | "cone" | "onion" => (span / 3).clamp(3, 8),
        _ => (span / 4).clamp(2, 6),
    };
    (measured_height - reserve).max(min_height + 1)
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
    spec: VolumeSpec,
    behavior: &StyleBehavior,
    materials: &Materials,
) {
    let span = (spec.height - spec.min_height).max(1);
    let spacing = (span / spec.floors.max(1) as i32).max(3);
    for z in 0..mask.len() as i32 {
        for x in 0..mask[0].len() as i32 {
            if !mask[z as usize][x as usize] {
                continue;
            }
            sink.set_block_absolute(&materials.floor, x, spec.min_height, z);
            for y in spec.min_height + 1..spec.height {
                if (y - spec.min_height) % spacing == 0 {
                    sink.set_block_absolute(&materials.floor, x, y, z);
                }
                if boundary(mask, x, z) {
                    let facade_cell = (x * 17 + z * 31 + spec.seed as i32).rem_euclid(100);
                    let within_floor = (y - spec.min_height).rem_euclid(spacing);
                    let pattern_window = if behavior.horizontal_windows {
                        within_floor >= 2
                    } else if behavior.vertical_windows {
                        facade_cell.rem_euclid(5) <= 2
                    } else {
                        facade_cell.rem_euclid(4) <= 1 || facade_cell.rem_euclid(7) == 0
                    };
                    let is_window = behavior.has_windows
                        && y > spec.min_height + 1
                        && (y - spec.min_height) % spacing != 0
                        && facade_cell < spec.window_density.clamp(5, 95) as i32
                        && within_floor > 1
                        && pattern_window;
                    let floor_band =
                        behavior.accent_lines && within_floor == 0 && y > spec.min_height + 1;
                    let block = if is_window {
                        &materials.window
                    } else if floor_band {
                        &materials.accent
                    } else {
                        &materials.wall
                    };
                    sink.set_block_absolute(block, x, y, z);
                    if spec.wall_depth >= 25
                        && !is_window
                        && (y - spec.min_height) % spacing != 0
                        && depth_feature(
                            behavior.depth,
                            facade_cell,
                            within_floor,
                            spacing,
                            spec.wall_depth,
                        )
                    {
                        if let Some((outside_x, outside_z)) = outward_cell(mask, x, z) {
                            sink.set_block_absolute(&materials.accent, outside_x, y, outside_z);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct VolumeSpec {
    min_height: i32,
    height: i32,
    floors: u32,
    seed: u64,
    window_density: u8,
    wall_depth: u8,
}

struct Materials {
    foundation: String,
    wall: String,
    window: String,
    floor: String,
    roof: String,
    entrance: String,
    accent: String,
}

#[derive(Clone, Copy)]
enum FacadeDepth {
    None,
    SubtlePilasters,
    ModernPillars,
    InstitutionalBands,
    IndustrialBeams,
    HistoricOrnate,
    ReligiousButtress,
    SkyscraperFins,
    GlassCurtain,
}

#[derive(Clone, Copy)]
struct StyleBehavior {
    vertical_windows: bool,
    horizontal_windows: bool,
    has_windows: bool,
    accent_roof_line: bool,
    accent_lines: bool,
    chimney: bool,
    parapet: bool,
    depth: FacadeDepth,
}

fn style_behavior(style: &str) -> StyleBehavior {
    let mut result = StyleBehavior {
        vertical_windows: false,
        horizontal_windows: false,
        has_windows: true,
        accent_roof_line: false,
        accent_lines: false,
        chimney: false,
        parapet: false,
        depth: FacadeDepth::None,
    };
    match style {
        "house" => {
            result.chimney = true;
            result.accent_roof_line = true;
            result.depth = FacadeDepth::SubtlePilasters;
        }
        "residential" => result.depth = FacadeDepth::SubtlePilasters,
        "commercial" => {
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::ModernPillars;
        }
        "office" => {
            result.vertical_windows = true;
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::ModernPillars;
        }
        "hotel" => {
            result.vertical_windows = true;
            result.accent_roof_line = true;
            result.accent_lines = true;
            result.parapet = true;
            result.depth = FacadeDepth::ModernPillars;
        }
        "industrial" | "warehouse" => result.depth = FacadeDepth::IndustrialBeams,
        "school" => {
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::InstitutionalBands;
        }
        "hospital" => {
            result.vertical_windows = true;
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::InstitutionalBands;
        }
        "religious" => {
            result.vertical_windows = true;
            result.accent_roof_line = true;
            result.depth = FacadeDepth::ReligiousButtress;
        }
        "historic" => result.depth = FacadeDepth::HistoricOrnate,
        "tower" => {
            result.vertical_windows = true;
            result.accent_lines = true;
            result.accent_roof_line = true;
        }
        "garage" => {
            result.has_windows = false;
            result.accent_roof_line = true;
        }
        "shed" | "greenhouse" => result.has_windows = false,
        "tall_building" => {
            result.vertical_windows = true;
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::SkyscraperFins;
        }
        "glassy_skyscraper" => {
            result.has_windows = false;
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::GlassCurtain;
        }
        "modern_skyscraper" => {
            result.horizontal_windows = true;
            result.accent_roof_line = true;
            result.parapet = true;
            result.depth = FacadeDepth::SkyscraperFins;
        }
        _ => {}
    }
    result
}

fn depth_feature(
    style: FacadeDepth,
    facade_cell: i32,
    within_floor: i32,
    spacing: i32,
    wall_depth: u8,
) -> bool {
    let strength = (wall_depth as i32 / 12).clamp(1, 8);
    match style {
        FacadeDepth::None => false,
        FacadeDepth::SubtlePilasters => facade_cell.rem_euclid(8) < strength.min(2),
        FacadeDepth::ModernPillars => {
            facade_cell.rem_euclid(7) < strength.min(2) || within_floor == spacing - 1
        }
        FacadeDepth::InstitutionalBands => {
            facade_cell.rem_euclid(9) < strength.min(2) || within_floor == spacing - 1
        }
        FacadeDepth::IndustrialBeams => facade_cell.rem_euclid(17) < strength,
        FacadeDepth::HistoricOrnate => {
            facade_cell.rem_euclid(6) < strength.min(2) || within_floor >= spacing - 2
        }
        FacadeDepth::ReligiousButtress => facade_cell.rem_euclid(11) < strength.min(3),
        FacadeDepth::SkyscraperFins => facade_cell.rem_euclid(5) < strength.min(2),
        FacadeDepth::GlassCurtain => facade_cell.rem_euclid(23) < strength.min(2),
    }
}
fn materials(style: &str, m: &MaterialOverrides) -> Materials {
    let preset = preset_materials(style);
    Materials {
        foundation: m
            .foundation
            .clone()
            .unwrap_or_else(|| preset.foundation.into()),
        wall: m.wall.clone().unwrap_or_else(|| preset.wall.into()),
        window: m.window.clone().unwrap_or_else(|| preset.window.into()),
        floor: m.floor.clone().unwrap_or_else(|| preset.floor.into()),
        roof: m.roof.clone().unwrap_or_else(|| preset.roof.into()),
        entrance: m.entrance.clone().unwrap_or_else(|| preset.entrance.into()),
        accent: m.accent.clone().unwrap_or_else(|| preset.accent.into()),
    }
}

struct MaterialPreset {
    foundation: &'static str,
    wall: &'static str,
    window: &'static str,
    floor: &'static str,
    roof: &'static str,
    entrance: &'static str,
    accent: &'static str,
}

fn preset_materials(style: &str) -> MaterialPreset {
    let values = match style {
        "house" => (
            "cobblestone",
            "bricks",
            "glass_pane",
            "oak_planks",
            "dark_oak_slab",
            "oak_door",
            "stripped_oak_log",
        ),
        "residential" => (
            "smooth_stone",
            "white_concrete",
            "glass_pane",
            "spruce_planks",
            "stone_slab",
            "spruce_door",
            "light_gray_concrete",
        ),
        "farm" => (
            "cobblestone",
            "oak_planks",
            "glass_pane",
            "spruce_planks",
            "spruce_slab",
            "oak_door",
            "stripped_spruce_log",
        ),
        "commercial" => (
            "smooth_stone",
            "smooth_quartz",
            "light_blue_stained_glass",
            "polished_andesite",
            "smooth_stone_slab",
            "iron_door",
            "gray_concrete",
        ),
        "office" => (
            "smooth_stone",
            "light_gray_concrete",
            "gray_stained_glass",
            "smooth_stone",
            "polished_andesite_slab",
            "iron_door",
            "white_concrete",
        ),
        "hotel" => (
            "stone_bricks",
            "quartz_bricks",
            "black_stained_glass",
            "dark_oak_planks",
            "dark_oak_slab",
            "dark_oak_door",
            "cut_copper",
        ),
        "industrial" => (
            "deepslate_tiles",
            "gray_concrete",
            "iron_bars",
            "smooth_stone",
            "deepslate_tile_slab",
            "iron_door",
            "exposed_copper",
        ),
        "warehouse" => (
            "stone",
            "light_gray_concrete",
            "glass_pane",
            "smooth_stone",
            "stone_slab",
            "iron_door",
            "gray_concrete",
        ),
        "hospital" => (
            "smooth_stone",
            "white_concrete",
            "light_blue_stained_glass",
            "quartz_block",
            "smooth_quartz_slab",
            "iron_door",
            "cyan_terracotta",
        ),
        "religious" => (
            "stone_bricks",
            "sandstone",
            "yellow_stained_glass",
            "smooth_stone",
            "stone_brick_slab",
            "dark_oak_door",
            "chiseled_stone_bricks",
        ),
        "historic" => (
            "cobblestone",
            "bricks",
            "brown_stained_glass",
            "dark_oak_planks",
            "deepslate_tile_slab",
            "dark_oak_door",
            "polished_granite",
        ),
        "tower" => (
            "deepslate_bricks",
            "stone_bricks",
            "tinted_glass",
            "smooth_stone",
            "deepslate_brick_slab",
            "iron_door",
            "chiseled_deepslate",
        ),
        "garage" => (
            "stone",
            "gray_concrete",
            "iron_bars",
            "smooth_stone",
            "stone_slab",
            "iron_door",
            "yellow_concrete",
        ),
        "shed" => (
            "cobblestone",
            "spruce_planks",
            "glass_pane",
            "oak_planks",
            "spruce_slab",
            "spruce_door",
            "stripped_spruce_log",
        ),
        "greenhouse" => (
            "stone",
            "glass",
            "glass",
            "moss_block",
            "glass",
            "oak_door",
            "green_stained_glass",
        ),
        "tall_building" => (
            "deepslate_tiles",
            "light_gray_concrete",
            "blue_stained_glass",
            "smooth_stone",
            "smooth_stone_slab",
            "iron_door",
            "polished_blackstone",
        ),
        "glassy_skyscraper" => (
            "deepslate_tiles",
            "cyan_stained_glass",
            "light_blue_stained_glass",
            "smooth_stone",
            "smooth_quartz_slab",
            "iron_door",
            "black_concrete",
        ),
        "modern_skyscraper" => (
            "deepslate_tiles",
            "white_concrete",
            "black_stained_glass",
            "smooth_stone",
            "smooth_quartz_slab",
            "iron_door",
            "gray_concrete",
        ),
        _ => (
            "stone_bricks",
            "bricks",
            "glass_pane",
            "oak_planks",
            "dark_oak_slab",
            "dark_oak_door",
            "smooth_sandstone",
        ),
    };
    MaterialPreset {
        foundation: block(values.0),
        wall: block(values.1),
        window: block(values.2),
        floor: block(values.3),
        roof: block(values.4),
        entrance: block(values.5),
        accent: block(values.6),
    }
}

fn block(value: &'static str) -> &'static str {
    match value.strip_prefix("minecraft:") {
        Some(_) => value,
        None => {
            // All built-in values are literals, so their prefixed forms are listed here.
            match value {
                "cobblestone" => "minecraft:cobblestone",
                "bricks" => "minecraft:bricks",
                "glass_pane" => "minecraft:glass_pane",
                "oak_planks" => "minecraft:oak_planks",
                "dark_oak_slab" => "minecraft:dark_oak_slab",
                "oak_door" => "minecraft:oak_door",
                "stripped_oak_log" => "minecraft:stripped_oak_log",
                "smooth_stone" => "minecraft:smooth_stone",
                "white_concrete" => "minecraft:white_concrete",
                "spruce_planks" => "minecraft:spruce_planks",
                "stone_slab" => "minecraft:stone_slab",
                "spruce_door" => "minecraft:spruce_door",
                "light_gray_concrete" => "minecraft:light_gray_concrete",
                "spruce_slab" => "minecraft:spruce_slab",
                "stripped_spruce_log" => "minecraft:stripped_spruce_log",
                "smooth_quartz" => "minecraft:smooth_quartz",
                "light_blue_stained_glass" => "minecraft:light_blue_stained_glass",
                "polished_andesite" => "minecraft:polished_andesite",
                "smooth_stone_slab" => "minecraft:smooth_stone_slab",
                "iron_door" => "minecraft:iron_door",
                "gray_concrete" => "minecraft:gray_concrete",
                "gray_stained_glass" => "minecraft:gray_stained_glass",
                "polished_andesite_slab" => "minecraft:polished_andesite_slab",
                "quartz_bricks" => "minecraft:quartz_bricks",
                "black_stained_glass" => "minecraft:black_stained_glass",
                "dark_oak_planks" => "minecraft:dark_oak_planks",
                "dark_oak_door" => "minecraft:dark_oak_door",
                "cut_copper" => "minecraft:cut_copper",
                "deepslate_tiles" => "minecraft:deepslate_tiles",
                "iron_bars" => "minecraft:iron_bars",
                "deepslate_tile_slab" => "minecraft:deepslate_tile_slab",
                "exposed_copper" => "minecraft:exposed_copper",
                "stone" => "minecraft:stone",
                "quartz_block" => "minecraft:quartz_block",
                "smooth_quartz_slab" => "minecraft:smooth_quartz_slab",
                "cyan_terracotta" => "minecraft:cyan_terracotta",
                "stone_bricks" => "minecraft:stone_bricks",
                "sandstone" => "minecraft:sandstone",
                "yellow_stained_glass" => "minecraft:yellow_stained_glass",
                "stone_brick_slab" => "minecraft:stone_brick_slab",
                "chiseled_stone_bricks" => "minecraft:chiseled_stone_bricks",
                "brown_stained_glass" => "minecraft:brown_stained_glass",
                "polished_granite" => "minecraft:polished_granite",
                "deepslate_bricks" => "minecraft:deepslate_bricks",
                "tinted_glass" => "minecraft:tinted_glass",
                "deepslate_brick_slab" => "minecraft:deepslate_brick_slab",
                "chiseled_deepslate" => "minecraft:chiseled_deepslate",
                "yellow_concrete" => "minecraft:yellow_concrete",
                "glass" => "minecraft:glass",
                "moss_block" => "minecraft:moss_block",
                "green_stained_glass" => "minecraft:green_stained_glass",
                "blue_stained_glass" => "minecraft:blue_stained_glass",
                "polished_blackstone" => "minecraft:polished_blackstone",
                "cyan_stained_glass" => "minecraft:cyan_stained_glass",
                "black_concrete" => "minecraft:black_concrete",
                "smooth_sandstone" => "minecraft:smooth_sandstone",
                _ => "minecraft:stone_bricks",
            }
        }
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

fn outward_cell(mask: &[Vec<bool>], x: i32, z: i32) -> Option<(i32, i32)> {
    [(0, 1), (1, 0), (0, -1), (-1, 0)]
        .into_iter()
        .find_map(|(dx, dz)| {
            let nx = x + dx;
            let nz = z + dz;
            let outside = nz < 0
                || nx < 0
                || nz as usize >= mask.len()
                || nx as usize >= mask[0].len()
                || !mask[nz as usize][nx as usize];
            outside.then_some((nx, nz))
        })
}

fn add_style_details(
    sink: &mut BlockSink,
    mask: &[Vec<bool>],
    height: i32,
    roof_shape: &str,
    behavior: &StyleBehavior,
    materials: &Materials,
) {
    if behavior.accent_roof_line {
        for z in 0..mask.len() as i32 {
            for x in 0..mask[0].len() as i32 {
                if mask[z as usize][x as usize] && boundary(mask, x, z) {
                    sink.set_block_absolute(&materials.accent, x, height, z);
                }
            }
        }
    }
    if behavior.parapet && roof_shape == "flat" {
        for z in 0..mask.len() as i32 {
            for x in 0..mask[0].len() as i32 {
                if mask[z as usize][x as usize] && boundary(mask, x, z) {
                    sink.set_block_absolute(&materials.wall, x, height + 2, z);
                }
            }
        }
    }
    if behavior.chimney && matches!(roof_shape, "gabled" | "hipped") {
        let center_x = mask[0].len() as i32 / 2;
        let center_z = mask.len() as i32 / 2;
        for y in height + 1..=height + 4 {
            sink.set_block_absolute(&materials.accent, center_x, y, center_z);
        }
    }
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
            style_preset: "school".into(),
            window_density: 55,
            wall_depth: 40,
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

    #[test]
    fn measured_height_is_not_changed_by_style_details() {
        let output = generate_building(request("gabled")).unwrap();
        assert_eq!(output.height, 15);
    }

    #[test]
    fn upstream_style_categories_produce_distinct_facades() {
        let mut school = request("flat");
        school.style_preset = "school".into();
        let mut glassy = request("flat");
        glassy.style_preset = "glassy_skyscraper".into();
        let school = generate_building(school).unwrap();
        let glassy = generate_building(glassy).unwrap();
        assert_ne!(school.palette, glassy.palette);
        assert_ne!(school.block_runs, glassy.block_runs);
    }
}
