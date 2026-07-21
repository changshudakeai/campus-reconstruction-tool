mod foundation_generators;
mod schema2_foundation;

pub use schema2_foundation::*;

use campus_state::{CampusProject, FeatureKind, FoundationStylePack, GeoPoint, MapFeature};
use fastnbt::{ByteArray, IntArray};
use flate2::{write::GzEncoder, Compression};
use foundation_generators::{
    FoundationFeatureGeneratorRegistry, FoundationFeatureRender, FoundationRenderTarget,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SPAN: usize = 2048;
static EXPORT_STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxelModel {
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub palette: Vec<String>,
    pub blocks: Vec<u16>,
}

/// The reviewed Foundation input required by the compiler. It deliberately
/// excludes project persistence, provider caches, candidate queues, and UI
/// state so the compiler can be exercised from a stable seam.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewedCampusModel {
    pub boundary: Vec<GeoPoint>,
    pub features: Vec<MapFeature>,
    pub orientation_degrees: f64,
    pub blocks_per_meter: f64,
    pub style_pack: FoundationStylePack,
    pub road_width_blocks: i32,
}

impl From<&CampusProject> for ReviewedCampusModel {
    fn from(project: &CampusProject) -> Self {
        Self {
            boundary: project.boundary.clone(),
            features: project.features.clone(),
            orientation_degrees: project.orientation_degrees,
            blocks_per_meter: project.blocks_per_meter,
            style_pack: project.foundation_style_pack.clone(),
            road_width_blocks: project.foundation_road_width_blocks(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Root {
    schematic: Schematic,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Schematic {
    version: i32,
    data_version: i32,
    metadata: Metadata,
    width: i16,
    height: i16,
    length: i16,
    offset: IntArray,
    blocks: Blocks,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Metadata {
    name: String,
    author: String,
    date: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Blocks {
    palette: BTreeMap<String, i32>,
    data: ByteArray,
}

pub fn foundation_model(project: &CampusProject) -> Result<VoxelModel, String> {
    foundation_model_from_reviewed(&ReviewedCampusModel::from(project))
}

pub fn foundation_model_from_reviewed(
    reviewed: &ReviewedCampusModel,
) -> Result<VoxelModel, String> {
    let mut all_points = reviewed.boundary.clone();
    for feature in &reviewed.features {
        all_points.extend_from_slice(&feature.points);
    }
    if all_points.len() < 3 {
        return Err("校区边界或已接受地物不足，无法导出".into());
    }
    let origin = all_points[0];
    let angle = -reviewed.orientation_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    let lat_scale = 111_320.0;
    let lng_scale = lat_scale * origin.lat.to_radians().cos();
    let convert = |point: GeoPoint| {
        let east = (point.lng - origin.lng) * lng_scale * reviewed.blocks_per_meter;
        let north = (point.lat - origin.lat) * lat_scale * reviewed.blocks_per_meter;
        (east * cos - north * sin, east * sin + north * cos)
    };
    let local = all_points.iter().copied().map(convert).collect::<Vec<_>>();
    let min_x = local
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let min_z = local
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_x = local
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_z = local
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let width = (max_x - min_x).ceil() as usize + 9;
    let length = (max_z - min_z).ceil() as usize + 9;
    if width == 0 || length == 0 || width > MAX_SPAN || length > MAX_SPAN {
        return Err(format!(
            "地基范围 {}×{} 超出 {} 方块限制，请降低比例",
            width, length, MAX_SPAN
        ));
    }
    let to_grid = |point: GeoPoint| {
        let (x, z) = convert(point);
        (
            (x - min_x).round() as i32 + 4,
            (z - min_z).round() as i32 + 4,
        )
    };
    let campus_block = reviewed
        .style_pack
        .features
        .get("campus")
        .and_then(|style| style.blocks.first())
        .map(|block| normalize_block(block))
        .unwrap_or_else(|| "minecraft:grass_block".into());
    let mut palette = vec!["minecraft:air".into(), campus_block];
    for style in reviewed.style_pack.features.values() {
        for block in &style.blocks {
            let block = normalize_block(block);
            if !palette.contains(&block) {
                palette.push(block);
            }
        }
    }
    for feature in &reviewed.features {
        let block = normalize_block(&feature.block);
        if !palette.contains(&block) {
            palette.push(block);
        }
    }
    let generated_trees = reviewed
        .style_pack
        .style(FeatureKind::Vegetation)
        .is_some_and(|style| style.generator == "arnis:vegetation/v1");
    let height = if generated_trees { 8 } else { 2 };
    let mut blocks = vec![0u16; width * height * length];
    let boundary = reviewed
        .boundary
        .iter()
        .copied()
        .map(to_grid)
        .collect::<Vec<_>>();
    if boundary.len() >= 3 {
        fill_polygon(&mut blocks, width, length, 0, &boundary, 1);
    }
    for feature in &reviewed.features {
        let points = feature
            .points
            .iter()
            .copied()
            .map(to_grid)
            .collect::<Vec<_>>();
        let block = normalize_block(&feature.block);
        let palette_index = palette
            .iter()
            .position(|entry| *entry == block)
            .ok_or_else(|| format!("missing Foundation palette entry for {}", feature.name))?
            as u16;
        let generator_style = reviewed.style_pack.style(feature.kind);
        FoundationFeatureGeneratorRegistry::render(
            &mut FoundationRenderTarget {
                blocks: &mut blocks,
                width,
                height,
                length,
                palette: &palette,
            },
            FoundationFeatureRender {
                feature,
                points: &points,
                style: generator_style,
                palette_index,
                road_width_blocks: reviewed.road_width_blocks,
            },
        )?;
    }
    Ok(VoxelModel {
        width,
        height,
        length,
        palette,
        blocks,
    })
}

fn secondary_palette_index(
    style: Option<&campus_state::FoundationGeneratorStyle>,
    palette: &[String],
    fallback: u16,
    index: usize,
) -> u16 {
    style
        .and_then(|style| style.blocks.get(index))
        .map(|block| normalize_block(block))
        .and_then(|block| palette.iter().position(|entry| *entry == block))
        .map(|index| index as u16)
        .unwrap_or(fallback)
}

fn normalize_block(block: &str) -> String {
    if block.starts_with("minecraft:") {
        block.to_string()
    } else {
        format!("minecraft:{block}")
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewModel<'a> {
    width: usize,
    height: usize,
    length: usize,
    palette: &'a [String],
    block_runs: Vec<PreviewBlockRun>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBlockRun {
    palette_index: u16,
    run_length: u32,
}

pub fn write_preview_model(path: &Path, model: &VoxelModel) -> Result<(), String> {
    if model.blocks.len() != model.width * model.height * model.length {
        return Err("方块数组尺寸与模型尺寸不一致".into());
    }
    let mut block_runs: Vec<PreviewBlockRun> = Vec::new();
    for palette_index in &model.blocks {
        if let Some(run) = block_runs.last_mut() {
            if run.palette_index == *palette_index && run.run_length < u32::MAX {
                run.run_length += 1;
                continue;
            }
        }
        block_runs.push(PreviewBlockRun {
            palette_index: *palette_index,
            run_length: 1,
        });
    }
    let preview = PreviewModel {
        width: model.width,
        height: model.height,
        length: model.length,
        palette: &model.palette,
        block_runs,
    };
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&preview).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFaultPoint {
    BeforeEncode,
    AfterEncode,
    AfterStageWrite,
    BeforePublish,
    AfterPublish,
}

pub fn write_schematic(path: &Path, name: &str, model: &VoxelModel) -> Result<(), String> {
    write_schematic_with_fault(path, name, model, None)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchematicInspection {
    pub sponge_version: i32,
    pub data_version: i32,
    pub dimensions: [usize; 3],
    pub offset: [i32; 3],
    pub palette_entries: usize,
    pub total_voxels: usize,
    pub non_air_voxels: usize,
    pub content_sha256: String,
}

pub fn inspect_schematic(path: &Path) -> Result<SchematicInspection, String> {
    use fastnbt::Value;
    use std::io::Read;

    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(file)
        .read_to_end(&mut decoded)
        .map_err(|error| format!("schematic is not valid gzip: {error}"))?;
    let Value::Compound(mut root) = fastnbt::from_bytes::<Value>(&decoded)
        .map_err(|error| format!("invalid Sponge NBT: {error}"))?
    else {
        return Err("schematic root must be an NBT compound".into());
    };
    let Value::Compound(mut schematic) = root
        .remove("Schematic")
        .ok_or("schematic is missing the Sponge Schematic compound")?
    else {
        return Err("Schematic tag must be a compound".into());
    };
    let integer =
        |map: &mut std::collections::HashMap<String, Value>, name: &str| match map.remove(name) {
            Some(Value::Int(value)) => Ok(value),
            Some(Value::Short(value)) => Ok(i32::from(value)),
            _ => Err(format!("schematic is missing integer {name}")),
        };
    let sponge_version = integer(&mut schematic, "Version")?;
    let data_version = integer(&mut schematic, "DataVersion")?;
    let width = integer(&mut schematic, "Width")?;
    let height = integer(&mut schematic, "Height")?;
    let length = integer(&mut schematic, "Length")?;
    if width <= 0 || height <= 0 || length <= 0 {
        return Err("schematic dimensions must be positive".into());
    }
    let Value::IntArray(offset) = schematic
        .remove("Offset")
        .ok_or("schematic is missing Offset")?
    else {
        return Err("schematic Offset must be an int array".into());
    };
    let offset = offset.into_inner();
    if offset.len() != 3 {
        return Err("schematic Offset must contain three coordinates".into());
    }
    let Value::Compound(mut blocks) = schematic
        .remove("Blocks")
        .ok_or("schematic is missing Blocks")?
    else {
        return Err("schematic Blocks must be a compound".into());
    };
    let Value::Compound(palette) = blocks
        .remove("Palette")
        .ok_or("schematic is missing Blocks.Palette")?
    else {
        return Err("schematic Palette must be a compound".into());
    };
    if palette.is_empty() || palette.keys().any(|name| !supported_v11_block(name)) {
        return Err("schematic palette contains a block outside the pinned V1.1 catalog".into());
    }
    let air_index = match palette.get("minecraft:air") {
        Some(Value::Int(value)) => Some(*value as u32),
        _ => None,
    };
    let Value::ByteArray(data) = blocks
        .remove("Data")
        .ok_or("schematic is missing Blocks.Data")?
    else {
        return Err("schematic block data must be a byte array".into());
    };
    let data = data.into_inner();
    let mut values = Vec::new();
    let mut current = 0u32;
    let mut shift = 0;
    for byte in data.iter().map(|byte| *byte as u8) {
        current |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            values.push(current);
            current = 0;
            shift = 0;
        } else {
            shift += 7;
            if shift >= 35 {
                return Err("schematic block data contains an invalid VarInt".into());
            }
        }
    }
    if shift != 0 {
        return Err("schematic block data ends inside a VarInt".into());
    }
    let total_voxels = width as usize * height as usize * length as usize;
    if values.len() != total_voxels {
        return Err("schematic block count does not match its dimensions".into());
    }
    let non_air_voxels = air_index
        .map(|air| values.iter().filter(|value| **value != air).count())
        .unwrap_or(values.len());
    if non_air_voxels == 0 {
        return Err("schematic contains no non-air voxels".into());
    }
    let stable_palette = palette.iter().collect::<BTreeMap<_, _>>();
    let stable = serde_json::to_vec(&(
        sponge_version,
        data_version,
        width,
        height,
        length,
        &offset,
        stable_palette,
        &values,
    ))
    .map_err(|error| error.to_string())?;
    Ok(SchematicInspection {
        sponge_version,
        data_version,
        dimensions: [width as usize, height as usize, length as usize],
        offset: [offset[0], offset[1], offset[2]],
        palette_entries: palette.len(),
        total_voxels,
        non_air_voxels,
        content_sha256: format!("{:x}", Sha256::digest(stable)),
    })
}

fn supported_v11_block(block: &str) -> bool {
    let base = block.split_once('[').map_or(block, |(base, _)| base);
    matches!(
        base,
        "minecraft:air"
            | "minecraft:birch_leaves"
            | "minecraft:birch_log"
            | "minecraft:black_concrete"
            | "minecraft:black_stained_glass"
            | "minecraft:blue_stained_glass"
            | "minecraft:bricks"
            | "minecraft:brown_stained_glass"
            | "minecraft:chiseled_deepslate"
            | "minecraft:chiseled_stone_bricks"
            | "minecraft:cobblestone"
            | "minecraft:cut_copper"
            | "minecraft:cyan_stained_glass"
            | "minecraft:cyan_terracotta"
            | "minecraft:dark_oak_door"
            | "minecraft:dark_oak_leaves"
            | "minecraft:dark_oak_log"
            | "minecraft:dark_oak_planks"
            | "minecraft:dark_oak_slab"
            | "minecraft:deepslate_brick_slab"
            | "minecraft:deepslate_bricks"
            | "minecraft:deepslate_tile_slab"
            | "minecraft:deepslate_tiles"
            | "minecraft:diamond_block"
            | "minecraft:exposed_copper"
            | "minecraft:glass"
            | "minecraft:glass_pane"
            | "minecraft:grass_block"
            | "minecraft:gray_concrete"
            | "minecraft:gray_stained_glass"
            | "minecraft:green_concrete"
            | "minecraft:green_stained_glass"
            | "minecraft:iron_bars"
            | "minecraft:iron_door"
            | "minecraft:light_blue_stained_glass"
            | "minecraft:light_gray_concrete"
            | "minecraft:moss_block"
            | "minecraft:mud_bricks"
            | "minecraft:oak_door"
            | "minecraft:oak_leaves"
            | "minecraft:oak_log"
            | "minecraft:oak_planks"
            | "minecraft:polished_andesite"
            | "minecraft:polished_andesite_slab"
            | "minecraft:polished_blackstone"
            | "minecraft:polished_granite"
            | "minecraft:purple_concrete"
            | "minecraft:quartz_block"
            | "minecraft:quartz_bricks"
            | "minecraft:sand"
            | "minecraft:sandstone"
            | "minecraft:smooth_quartz"
            | "minecraft:smooth_quartz_slab"
            | "minecraft:smooth_sandstone"
            | "minecraft:smooth_stone"
            | "minecraft:smooth_stone_slab"
            | "minecraft:spruce_door"
            | "minecraft:spruce_planks"
            | "minecraft:spruce_slab"
            | "minecraft:stone"
            | "minecraft:stone_brick_slab"
            | "minecraft:stone_bricks"
            | "minecraft:stone_slab"
            | "minecraft:stripped_oak_log"
            | "minecraft:stripped_spruce_log"
            | "minecraft:terracotta"
            | "minecraft:tinted_glass"
            | "minecraft:water"
            | "minecraft:white_concrete"
            | "minecraft:yellow_concrete"
            | "minecraft:yellow_stained_glass"
    )
}

pub fn write_schematic_with_fault(
    path: &Path,
    name: &str,
    model: &VoxelModel,
    fault: Option<ExportFaultPoint>,
) -> Result<(), String> {
    fail_export_at(fault, ExportFaultPoint::BeforeEncode)?;
    if model.blocks.len() != model.width * model.height * model.length {
        return Err("方块数组尺寸与模型尺寸不一致".into());
    }
    if model.width > i16::MAX as usize
        || model.height > i16::MAX as usize
        || model.length > i16::MAX as usize
    {
        return Err("Schematic 尺寸超出格式限制".into());
    }
    let palette = model
        .palette
        .iter()
        .enumerate()
        .map(|(index, block)| (block.clone(), index as i32))
        .collect();
    let mut varints = Vec::with_capacity(model.blocks.len());
    for value in &model.blocks {
        encode_varint(*value as u32, &mut varints);
    }
    let root = Root {
        schematic: Schematic {
            version: 3,
            // Minecraft Java 1.21.1; block IDs used here are stable across supported V1 targets.
            data_version: 3955,
            metadata: Metadata {
                name: name.into(),
                author: "Campus Reconstruction Tool".into(),
                date: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            },
            width: model.width as i16,
            height: model.height as i16,
            length: model.length as i16,
            offset: IntArray::new(vec![0, 0, 0]),
            blocks: Blocks {
                palette,
                data: ByteArray::new(varints.into_iter().map(|byte| byte as i8).collect()),
            },
        },
    };
    let nbt = fastnbt::to_bytes(&root).map_err(|error| error.to_string())?;
    fail_export_at(fault, ExportFaultPoint::AfterEncode)?;
    let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
    gzip.write_all(&nbt).map_err(|error| error.to_string())?;
    let bytes = gzip.finish().map_err(|error| error.to_string())?;
    publish_export_atomically(path, &bytes, fault)
}

fn fail_export_at(
    injected: Option<ExportFaultPoint>,
    point: ExportFaultPoint,
) -> Result<(), String> {
    if injected == Some(point) {
        Err(format!("injected export failure at {point:?}"))
    } else {
        Ok(())
    }
}

fn publish_export_atomically(
    path: &Path,
    bytes: &[u8],
    fault: Option<ExportFaultPoint>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Schematic destination has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Schematic destination has no file name")?;
    let sequence = EXPORT_STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stage = parent.join(format!(
        ".{file_name}.stage-{}-{sequence}",
        std::process::id()
    ));
    let backup = parent.join(format!(
        ".{file_name}.backup-{}-{sequence}",
        std::process::id()
    ));
    let mut moved_previous = false;
    let mut published = false;
    let result = (|| {
        let mut file = File::create(&stage).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fail_export_at(fault, ExportFaultPoint::AfterStageWrite)?;
        fail_export_at(fault, ExportFaultPoint::BeforePublish)?;
        if path.exists() {
            fs::rename(path, &backup).map_err(|error| error.to_string())?;
            moved_previous = true;
        }
        if let Err(error) = fs::rename(&stage, path) {
            if moved_previous {
                let _ = fs::rename(&backup, path);
                moved_previous = false;
            }
            return Err(error.to_string());
        }
        published = true;
        fail_export_at(fault, ExportFaultPoint::AfterPublish)?;
        if moved_previous {
            fs::remove_file(&backup).map_err(|error| error.to_string())?;
            moved_previous = false;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&stage);
        if published {
            let _ = fs::remove_file(path);
        }
        if moved_previous {
            let _ = fs::rename(&backup, path);
        } else {
            let _ = fs::remove_file(&backup);
        }
    }
    result
}

pub fn model_from_runs(
    width: usize,
    height: usize,
    length: usize,
    palette: Vec<String>,
    runs: impl IntoIterator<Item = (u16, u32)>,
) -> Result<VoxelModel, String> {
    let mut blocks = Vec::with_capacity(width * height * length);
    for (palette_index, length) in runs {
        blocks.extend(std::iter::repeat_n(palette_index, length as usize));
    }
    if blocks.len() != width * height * length {
        return Err("RLE 方块数量与模型尺寸不一致".into());
    }
    Ok(VoxelModel {
        width,
        height,
        length,
        palette,
        blocks,
    })
}

fn encode_varint(mut value: u32, output: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn fill_polygon(
    blocks: &mut [u16],
    width: usize,
    length: usize,
    y: usize,
    polygon: &[(i32, i32)],
    value: u16,
) {
    let min_x = polygon
        .iter()
        .map(|point| point.0)
        .min()
        .unwrap_or(0)
        .max(0) as usize;
    let max_x = polygon
        .iter()
        .map(|point| point.0)
        .max()
        .unwrap_or(0)
        .min(width as i32 - 1) as usize;
    let min_z = polygon
        .iter()
        .map(|point| point.1)
        .min()
        .unwrap_or(0)
        .max(0) as usize;
    let max_z = polygon
        .iter()
        .map(|point| point.1)
        .max()
        .unwrap_or(0)
        .min(length as i32 - 1) as usize;
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            if point_in_polygon(x as f64 + 0.5, z as f64 + 0.5, polygon) {
                blocks[x + z * width + y * width * length] = value;
            }
        }
    }
}

fn point_in_polygon(x: f64, z: f64, polygon: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (ax, az) = polygon[current];
        let (bx, bz) = polygon[previous];
        if ((az as f64 > z) != (bz as f64 > z))
            && x < (bx - ax) as f64 * (z - az as f64) / (bz - az) as f64 + ax as f64
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn draw_polyline(
    blocks: &mut [u16],
    width: usize,
    length: usize,
    y: usize,
    points: &[(i32, i32)],
    radius: i32,
    value: u16,
) {
    for pair in points.windows(2) {
        let (mut x, mut z) = pair[0];
        let (end_x, end_z) = pair[1];
        let dx = (end_x - x).abs();
        let dz = -(end_z - z).abs();
        let step_x = if x < end_x { 1 } else { -1 };
        let step_z = if z < end_z { 1 } else { -1 };
        let mut error = dx + dz;
        loop {
            for oz in -radius..=radius {
                for ox in -radius..=radius {
                    if ox * ox + oz * oz <= radius * radius {
                        let px = x + ox;
                        let pz = z + oz;
                        if px >= 0 && pz >= 0 && px < width as i32 && pz < length as i32 {
                            blocks[px as usize + pz as usize * width + y * width * length] = value;
                        }
                    }
                }
            }
            if x == end_x && z == end_z {
                break;
            }
            let doubled = 2 * error;
            if doubled >= dz {
                error += dz;
                x += step_x;
            }
            if doubled <= dx {
                error += dx;
                z += step_z;
            }
        }
    }
}

fn draw_polygon_outline(
    blocks: &mut [u16],
    width: usize,
    length: usize,
    y: usize,
    polygon: &[(i32, i32)],
    value: u16,
) {
    if polygon.len() < 3 {
        return;
    }
    let mut closed = polygon.to_vec();
    closed.push(polygon[0]);
    draw_polyline(blocks, width, length, y, &closed, 1, value);
}

#[allow(clippy::too_many_arguments)]
fn draw_vegetation_trees(
    blocks: &mut [u16],
    width: usize,
    height: usize,
    length: usize,
    polygon: &[(i32, i32)],
    log: u16,
    leaves: u16,
    density: f64,
    seed: u64,
) {
    let min_x = polygon.iter().map(|point| point.0).min().unwrap_or(0);
    let max_x = polygon.iter().map(|point| point.0).max().unwrap_or(0);
    let min_z = polygon.iter().map(|point| point.1).min().unwrap_or(0);
    let max_z = polygon.iter().map(|point| point.1).max().unwrap_or(0);
    let step = (1.0 / density.max(0.005).sqrt()).round().max(4.0) as i32;
    let start_x = min_x + ((seed >> 3) % step as u64) as i32;
    let start_z = min_z + (seed % step as u64) as i32;
    for z in (start_z..=max_z).step_by(step as usize) {
        for x in (start_x..=max_x).step_by(step as usize) {
            if !point_in_polygon(x as f64 + 0.5, z as f64 + 0.5, polygon) {
                continue;
            }
            for y in 2..=5 {
                set_voxel(blocks, width, height, length, x, y, z, log);
            }
            for y in 5..=7 {
                for dz in -2i32..=2 {
                    for dx in -2i32..=2 {
                        if dx.abs() + dz.abs() + (y as i32 - 6).abs() <= 4 {
                            set_voxel(blocks, width, height, length, x + dx, y, z + dz, leaves);
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn set_voxel(
    blocks: &mut [u16],
    width: usize,
    height: usize,
    length: usize,
    x: i32,
    y: usize,
    z: i32,
    value: u16,
) {
    if x >= 0 && z >= 0 && x < width as i32 && z < length as i32 && y < height {
        blocks[x as usize + z as usize * width + y * width * length] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use campus_state::{FeatureKind, MapFeature};

    #[test]
    fn exports_nonempty_sponge_v3() {
        let output = tempfile::tempdir().unwrap();
        let mut project = CampusProject::new("test", "campus");
        project.boundary = vec![
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
        ];
        project.features.push(MapFeature {
            id: "building".into(),
            name: "building".into(),
            kind: FeatureKind::Building,
            points: project.boundary.clone(),
            block: "minecraft:stone_bricks".into(),
            source_id: None,
        });
        let model = foundation_model(&project).unwrap();
        let stone_bricks = model
            .palette
            .iter()
            .position(|block| block == "minecraft:stone_bricks")
            .unwrap() as u16;
        assert!(model.blocks.contains(&stone_bricks));
        assert!(model
            .palette
            .iter()
            .any(|block| block == "minecraft:stone_bricks"));
        let preview_path = output.path().join("foundation.preview.json");
        write_preview_model(&preview_path, &model).unwrap();
        let preview: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&preview_path).unwrap()).unwrap();
        assert_eq!(preview["width"], model.width);
        assert!(preview["blockRuns"].as_array().unwrap().len() > 1);
        let path = output.path().join("foundation.schem");
        write_schematic(&path, "test", &model).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 64);
        let mut decoded = Vec::new();
        let file = std::fs::File::open(&path).unwrap();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(file), &mut decoded).unwrap();
        let value: fastnbt::Value = fastnbt::from_bytes(&decoded).unwrap();
        let fastnbt::Value::Compound(root) = value else {
            panic!("schematic root must be an NBT compound");
        };
        assert!(root.contains_key("Schematic"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(preview_path);
    }

    #[test]
    fn inspects_the_public_sponge_schematic_contract() {
        let output = tempfile::tempdir().unwrap();
        let path = output.path().join("inspection.schem");
        let model = VoxelModel {
            width: 2,
            height: 1,
            length: 2,
            palette: vec!["minecraft:air".into(), "minecraft:stone".into()],
            blocks: vec![0, 1, 1, 0],
        };
        write_schematic(&path, "inspection", &model).unwrap();

        let inspection = inspect_schematic(&path).unwrap();

        assert_eq!(inspection.sponge_version, 3);
        assert_eq!(inspection.data_version, 3955);
        assert_eq!(inspection.dimensions, [2, 1, 2]);
        assert_eq!(inspection.offset, [0, 0, 0]);
        assert_eq!(inspection.palette_entries, 2);
        assert_eq!(inspection.non_air_voxels, 2);
        assert_eq!(inspection.total_voxels, 4);
        assert_eq!(inspection.content_sha256.len(), 64);
    }

    #[test]
    fn inspection_rejects_a_namespaced_block_outside_the_pinned_catalog() {
        let output = tempfile::tempdir().unwrap();
        let path = output.path().join("unknown.schem");
        write_schematic(
            &path,
            "unknown",
            &VoxelModel {
                width: 1,
                height: 1,
                length: 1,
                palette: vec!["minecraft:not_a_real_block".into()],
                blocks: vec![0],
            },
        )
        .unwrap();

        assert!(inspect_schematic(&path)
            .unwrap_err()
            .contains("pinned V1.1 catalog"));
    }

    #[test]
    fn reviewed_campus_seam_preserves_legacy_foundation_output() {
        let mut project = CampusProject::new("test", "campus");
        project.boundary = vec![
            GeoPoint {
                lng: 121.0,
                lat: 31.0,
            },
            GeoPoint {
                lng: 121.0002,
                lat: 31.0,
            },
            GeoPoint {
                lng: 121.0002,
                lat: 30.9998,
            },
            GeoPoint {
                lng: 121.0,
                lat: 30.9998,
            },
        ];
        project.features.push(MapFeature {
            id: "road".into(),
            name: "path".into(),
            kind: FeatureKind::Road,
            points: vec![project.boundary[0], project.boundary[2]],
            block: project
                .foundation_style_pack
                .primary_block(FeatureKind::Road),
            source_id: None,
        });

        let legacy = foundation_model(&project).unwrap();
        let reviewed = ReviewedCampusModel::from(&project);
        let compiled = foundation_model_from_reviewed(&reviewed).unwrap();

        assert_eq!(compiled, legacy);
    }

    #[test]
    fn arnis_foundation_generators_add_edges_and_trees() {
        let mut project = CampusProject::new("test", "campus");
        project.boundary = vec![
            GeoPoint {
                lng: 121.0,
                lat: 31.0,
            },
            GeoPoint {
                lng: 121.001,
                lat: 31.0,
            },
            GeoPoint {
                lng: 121.001,
                lat: 30.999,
            },
            GeoPoint {
                lng: 121.0,
                lat: 30.999,
            },
        ];
        project.features.push(MapFeature {
            id: "vegetation".into(),
            name: "grove".into(),
            kind: FeatureKind::Vegetation,
            points: project.boundary.clone(),
            block: project
                .foundation_style_pack
                .primary_block(FeatureKind::Vegetation),
            source_id: None,
        });
        let model = foundation_model(&project).unwrap();
        assert_eq!(model.height, 8);
        let log = model
            .palette
            .iter()
            .position(|block| block == "minecraft:oak_log")
            .unwrap() as u16;
        let leaves = model
            .palette
            .iter()
            .position(|block| block == "minecraft:oak_leaves")
            .unwrap() as u16;
        assert!(model.blocks.contains(&log));
        assert!(model.blocks.contains(&leaves));
    }

    #[test]
    fn every_export_fault_preserves_the_previous_destination_without_stage_files() {
        let output = tempfile::tempdir().unwrap();
        let destination = output.path().join("campus.schem");
        let previous = b"previous-confirmed-export";
        let model = VoxelModel {
            width: 1,
            height: 1,
            length: 1,
            palette: vec!["minecraft:stone".into()],
            blocks: vec![0],
        };

        for point in [
            ExportFaultPoint::BeforeEncode,
            ExportFaultPoint::AfterEncode,
            ExportFaultPoint::AfterStageWrite,
            ExportFaultPoint::BeforePublish,
            ExportFaultPoint::AfterPublish,
        ] {
            std::fs::write(&destination, previous).unwrap();
            let error = write_schematic_with_fault(&destination, "campus", &model, Some(point))
                .unwrap_err();
            assert!(error.contains(&format!("{point:?}")));
            assert_eq!(std::fs::read(&destination).unwrap(), previous);
            assert_eq!(
                std::fs::read_dir(output.path()).unwrap().count(),
                1,
                "{point:?} left a stage or backup file"
            );
        }
    }
}
