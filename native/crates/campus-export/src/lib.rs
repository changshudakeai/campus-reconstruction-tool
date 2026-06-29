use campus_state::{CampusProject, FeatureKind, GeoPoint};
use fastnbt::{ByteArray, IntArray};
use flate2::{write::GzEncoder, Compression};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

const MAX_SPAN: usize = 2048;

#[derive(Debug, Clone)]
pub struct VoxelModel {
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub palette: Vec<String>,
    pub blocks: Vec<u16>,
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
    let mut all_points = project.boundary.clone();
    for feature in &project.features {
        all_points.extend_from_slice(&feature.points);
    }
    if all_points.len() < 3 {
        return Err("校区边界或已接受地物不足，无法导出".into());
    }
    let origin = all_points[0];
    let angle = -project.orientation_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    let lat_scale = 111_320.0;
    let lng_scale = lat_scale * origin.lat.to_radians().cos();
    let convert = |point: GeoPoint| {
        let east = (point.lng - origin.lng) * lng_scale * project.blocks_per_meter;
        let north = (point.lat - origin.lat) * lat_scale * project.blocks_per_meter;
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
    let mut palette = vec!["minecraft:air".into(), "minecraft:grass_block".into()];
    for feature in &project.features {
        let block = normalize_block(&feature.block);
        if !palette.contains(&block) {
            palette.push(block);
        }
    }
    let height = 2;
    let mut blocks = vec![0u16; width * height * length];
    let boundary = project
        .boundary
        .iter()
        .copied()
        .map(to_grid)
        .collect::<Vec<_>>();
    if boundary.len() >= 3 {
        fill_polygon(&mut blocks, width, length, 0, &boundary, 1);
    }
    for feature in &project.features {
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
        if feature.kind == FeatureKind::Road {
            let radius = (project.foundation_style_preset.road_width_blocks() / 2).max(1);
            draw_polyline(
                &mut blocks,
                width,
                length,
                1,
                &points,
                radius,
                palette_index,
            );
        } else if points.len() >= 3 {
            fill_polygon(&mut blocks, width, length, 1, &points, palette_index);
        }
    }
    Ok(VoxelModel {
        width,
        height,
        length,
        palette,
        blocks,
    })
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

pub fn write_schematic(path: &Path, name: &str, model: &VoxelModel) -> Result<(), String> {
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
    let file = File::create(path).map_err(|error| error.to_string())?;
    let mut gzip = GzEncoder::new(file, Compression::best());
    gzip.write_all(&nbt).map_err(|error| error.to_string())?;
    gzip.finish().map_err(|error| error.to_string())?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use campus_state::{FeatureKind, MapFeature};

    #[test]
    fn exports_nonempty_sponge_v3() {
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
        assert!(model.blocks.contains(&2));
        assert!(model
            .palette
            .iter()
            .any(|block| block == "minecraft:stone_bricks"));
        let preview_path = std::env::temp_dir().join("campus-export-test.preview.json");
        write_preview_model(&preview_path, &model).unwrap();
        let preview: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&preview_path).unwrap()).unwrap();
        assert_eq!(preview["width"], model.width);
        assert!(preview["blockRuns"].as_array().unwrap().len() > 1);
        let path = std::env::temp_dir().join("campus-export-test.schem");
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
}
