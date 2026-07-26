//! B4 Sponge 导出引擎（.schem 落盘）
//!
//! **职责：**输入方块模型 → 输出 Sponge V3 `.schem` 文件（NBT 编码 + gzip 压缩）。
//!
//! # 架构边界
//!
//! - ✅ B4 是"打印机"：只管文件格式，零业务逻辑、零生成规则、零数据库
//! - ❌ 禁止依赖 `core/generation-engine`（两模块互不依赖，缝 6 规定）
//! - ✅ 复用 T02 shared-domain-types 的类别枚举（当前接口无需类别，预留）
//!
//! # 数据结构
//!
//! ## VoxelModel（方块模型）
//!
//! - **width/height/length**：模型尺寸（方块单位）
//! - **palette**：方块 ID 列表（Minecraft namespace:path），索引为 u16
//! - **blocks**：扁平化三维数组 `[x + z*width + y*width*length]`，值为 palette 索引
//!
//! 高度（Y 维）是一等公民字段：v2.0.0 平地导出时由调用方铺同一水平面，
//! 将来做真实起伏只需在 Y 维填充不同高度，数据结构无须翻修（ADR-0024）。
//!
//! # 导出流程（WorldEdit //schem load 接受）
//!
//! 1. 构建 VoxelModel（F9 将来负责把生成引擎输出适配为本结构）
//! 2. `write_schematic(path, name, &model)` → 写入 .schem 文件
//! 3. `inspect_schematic(path)` → 解析验证格式合法性（往返测试用）
//! 4. `verify_worldedit_import_contract(path)` → 断言兼容固定 MC 档案
//!
//! # Sponge V3 格式要点
//!
//! - **版本**: `SPONGE_SCHEMATIC_VERSION = 3`
//! - **DataVersion**: `PINNED_DATA_VERSION = 3955`（MC Java 26.1.2 固定值）
//! - **压缩**: NBT → Gzip 包裹
//! - **根节点**: Compound{ "Schematic": {...} }
//!
//! # 示例
//!
//! ```no_run
//! use std::path::Path;
//! use sponge_export::{VoxelModel, write_schematic, inspect_schematic};
//!
//! let model = VoxelModel {
//!     width: 2,
//!     height: 1,
//!     length: 2,
//!     palette: vec!["minecraft:air".into(), "minecraft:stone".into()],
//!     blocks: vec![0, 1, 1, 0],
//! };
//!
//! write_schematic(Path::new("campus.schem"), "初始校园", &model).unwrap();
//! let inspection = inspect_schematic(Path::new("campus.schem")).unwrap();
//! assert_eq!(inspection.sponge_version, 3);
//! ```

use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

/// Sponge Schematic specification version emitted by every export.
pub const SPONGE_SCHEMATIC_VERSION: i32 = 3;
/// NBT data version pinned by the Minecraft Java Edition 26.1.2 compatibility profile.
pub const PINNED_DATA_VERSION: i32 = 3955;

static EXPORT_STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A 3D voxel model suitable for Sponge Schematic export.
///
/// This is the seam between the generation pipeline and this printer crate:
/// callers (F9) fill in block coordinates + palette; this crate only encodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoxelModel {
    /// Width in blocks (X dimension).
    pub width: usize,
    /// Height in blocks (Y dimension). Reserved for future terrain relief;
    /// flat-ground exports use a single shared level (ADR-0024).
    pub height: usize,
    /// Length in blocks (Z dimension).
    pub length: usize,
    /// Palette of block identifiers (e.g., "minecraft:stone").
    pub palette: Vec<String>,
    /// Flat array of palette indices, ordered as `[x + z*width + y*width*length]`.
    pub blocks: Vec<u16>,
}

impl VoxelModel {
    /// Validates dimensions and block count consistency.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 || self.length == 0 {
            return Err("dimensions must be positive".into());
        }
        if self.width > i16::MAX as usize
            || self.height > i16::MAX as usize
            || self.length > i16::MAX as usize
        {
            return Err("dimensions exceed i16::MAX limit".into());
        }
        let expected_len = self.width * self.height * self.length;
        if self.blocks.len() != expected_len {
            return Err(format!(
                "block count {} does not match dimensions {}x{}x{}",
                self.blocks.len(),
                self.width,
                self.height,
                self.length
            ));
        }
        Ok(())
    }

    /// Creates a minimal air-filled model.
    pub fn empty(width: usize, height: usize, length: usize) -> Self {
        Self {
            width,
            height,
            length,
            palette: vec!["minecraft:air".into()],
            blocks: vec![0; width * height * length],
        }
    }

    /// Builds a flat-ground campus canvas: one uniform ground layer at Y=0
    /// filled with `ground_block`, with `height - 1` empty layers above it
    /// reserved for future structures. This realizes the ADR-0024 flat-ground
    /// decision while keeping the Y dimension extensible.
    pub fn flat_ground(
        width: usize,
        length: usize,
        height: usize,
        ground_block: &str,
    ) -> Result<Self, String> {
        if height == 0 {
            return Err("height must be at least 1 for the ground layer".into());
        }
        let mut model = Self {
            width,
            length,
            height,
            palette: vec!["minecraft:air".into(), ground_block.to_string()],
            blocks: vec![0; width * height * length],
        };
        // Y=0 layer is the flat ground plane
        for z in 0..length {
            for x in 0..width {
                model.blocks[x + z * width] = 1;
            }
        }
        model.validate()?;
        Ok(model)
    }
}

/// Encodes a palette index as a VarInt into the output buffer.
fn encode_varint(mut value: u32, output: &mut Vec<i8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        output.push(byte as i8);
        if value == 0 {
            break;
        }
    }
}

/// Writes a Sponge V3 .schem file to the given path.
pub fn write_schematic(path: &Path, name: &str, model: &VoxelModel) -> Result<(), anyhow::Error> {
    model.validate().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Build the Sponge V3 structure with fastnbt::Value compounds
    let mut palette_map = HashMap::new();
    for (idx, block) in model.palette.iter().enumerate() {
        palette_map.insert(block.clone(), fastnbt::Value::Int(idx as i32));
    }

    // Encode blocks as VarInts
    let mut varints = Vec::with_capacity(model.blocks.len());
    for &index in &model.blocks {
        encode_varint(u32::from(index), &mut varints);
    }

    let mut blocks_compound = HashMap::new();
    blocks_compound.insert("Palette".to_string(), fastnbt::Value::Compound(palette_map));
    blocks_compound.insert(
        "Data".to_string(),
        fastnbt::Value::ByteArray(fastnbt::ByteArray::new(varints)),
    );

    let mut metadata = HashMap::new();
    metadata.insert("Name".to_string(), fastnbt::Value::String(name.to_string()));
    metadata.insert(
        "Author".to_string(),
        fastnbt::Value::String("Campus Reconstruction Tool".to_string()),
    );
    metadata.insert(
        "Date".to_string(),
        fastnbt::Value::Long(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        ),
    );

    let mut schematic = HashMap::new();
    schematic.insert(
        "Version".to_string(),
        fastnbt::Value::Int(SPONGE_SCHEMATIC_VERSION),
    );
    schematic.insert(
        "DataVersion".to_string(),
        fastnbt::Value::Int(PINNED_DATA_VERSION),
    );
    schematic.insert("Metadata".to_string(), fastnbt::Value::Compound(metadata));
    schematic.insert(
        "Width".to_string(),
        fastnbt::Value::Short(model.width as i16),
    );
    schematic.insert(
        "Height".to_string(),
        fastnbt::Value::Short(model.height as i16),
    );
    schematic.insert(
        "Length".to_string(),
        fastnbt::Value::Short(model.length as i16),
    );
    schematic.insert(
        "Offset".to_string(),
        fastnbt::Value::IntArray(fastnbt::IntArray::new(vec![0, 0, 0])),
    );
    schematic.insert(
        "Blocks".to_string(),
        fastnbt::Value::Compound(blocks_compound),
    );

    let mut root = HashMap::new();
    root.insert("Schematic".to_string(), fastnbt::Value::Compound(schematic));

    // Serialize NBT and gzip
    let nbt = fastnbt::to_bytes(&fastnbt::Value::Compound(root))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&nbt)?;
    let compressed = encoder.finish()?;

    publish_atomically(path, &compressed)
}

/// Inspection result after reading a .schem file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchematicInspection {
    /// Sponge format version.
    pub sponge_version: i32,
    /// Minecraft DataVersion.
    pub data_version: i32,
    /// Dimensions [width, height, length].
    pub dimensions: [usize; 3],
    /// Offset vector [x, y, z].
    pub offset: [i32; 3],
    /// Number of palette entries.
    pub palette_entries: usize,
    /// Total voxels.
    pub total_voxels: usize,
    /// Non-air voxels.
    pub non_air_voxels: usize,
    /// Decoded voxel palette indices in storage order (round-trip checks).
    pub voxels: Vec<u32>,
    /// Palette as block id → palette index (round-trip checks).
    pub palette: HashMap<String, i32>,
    /// SHA256 hash of the canonical representation.
    pub content_sha256: String,
}

/// Inspects a .schem file, decoding and validating its full structure.
pub fn inspect_schematic(path: &Path) -> Result<SchematicInspection, anyhow::Error> {
    let mut file = File::open(path)?;
    let mut decoded = Vec::new();
    GzDecoder::new(&mut file).read_to_end(&mut decoded)?;

    let root = match fastnbt::from_bytes::<fastnbt::Value>(&decoded)? {
        fastnbt::Value::Compound(c) => c,
        _ => return Err(anyhow::anyhow!("root must be NBT compound")),
    };

    let schematic = get_compound(&root, "Schematic")?;

    let version = get_int(schematic, "Version")?;
    let data_version = get_int(schematic, "DataVersion")?;
    let width = get_int(schematic, "Width")?;
    let height = get_int(schematic, "Height")?;
    let length = get_int(schematic, "Length")?;

    if width <= 0 || height <= 0 || length <= 0 {
        return Err(anyhow::anyhow!("dimensions must be positive"));
    }

    let offset_vec = get_int_array(schematic, "Offset")?;
    if offset_vec.len() != 3 {
        return Err(anyhow::anyhow!("offset must have 3 coordinates"));
    }
    let offset: [i32; 3] = [offset_vec[0], offset_vec[1], offset_vec[2]];

    let blocks_compound = get_compound(schematic, "Blocks")?;
    let palette_map = get_compound(blocks_compound, "Palette")?;
    if palette_map.is_empty() {
        return Err(anyhow::anyhow!("palette must not be empty"));
    }

    let mut palette = HashMap::new();
    for (block, value) in palette_map {
        match value {
            fastnbt::Value::Int(idx) => {
                palette.insert(block.clone(), *idx);
            }
            _ => return Err(anyhow::anyhow!("palette entry {block} must be an int")),
        }
    }
    let air_index = palette.get("minecraft:air").map(|idx| *idx as u32);

    let raw_data = get_byte_array(blocks_compound, "Data")?;

    // Decode VarInts
    let mut voxels = Vec::new();
    let mut current = 0u32;
    let mut shift = 0;
    for byte in raw_data.iter().map(|b| *b as u8) {
        current |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            voxels.push(current);
            current = 0;
            shift = 0;
        } else {
            shift += 7;
            if shift >= 35 {
                return Err(anyhow::anyhow!("invalid VarInt chain"));
            }
        }
    }
    if shift != 0 {
        return Err(anyhow::anyhow!("block data ends inside a VarInt"));
    }

    let total = width as usize * height as usize * length as usize;
    if voxels.len() != total {
        return Err(anyhow::anyhow!(
            "voxel count mismatch: expected {}, got {}",
            total,
            voxels.len()
        ));
    }

    let non_air = air_index.map_or(total, |air| voxels.iter().filter(|&&v| v != air).count());

    // Canonical hash over an order-stable representation
    let stable_palette: std::collections::BTreeMap<&String, &i32> = palette.iter().collect();
    let stable = serde_json::to_vec(&(
        version,
        data_version,
        width,
        height,
        length,
        &offset,
        stable_palette,
        &voxels,
    ))?;

    Ok(SchematicInspection {
        sponge_version: version,
        data_version,
        dimensions: [width as usize, height as usize, length as usize],
        offset,
        palette_entries: palette.len(),
        total_voxels: total,
        non_air_voxels: non_air,
        voxels,
        palette,
        content_sha256: format!("{:x}", Sha256::digest(stable)),
    })
}

fn get_int(map: &HashMap<String, fastnbt::Value>, name: &str) -> Result<i32, anyhow::Error> {
    match map.get(name) {
        Some(fastnbt::Value::Int(v)) => Ok(*v),
        Some(fastnbt::Value::Short(v)) => Ok(i32::from(*v)),
        _ => Err(anyhow::anyhow!("missing or wrong type for {name}")),
    }
}

fn get_compound<'a>(
    map: &'a HashMap<String, fastnbt::Value>,
    name: &str,
) -> Result<&'a HashMap<String, fastnbt::Value>, anyhow::Error> {
    match map.get(name) {
        Some(fastnbt::Value::Compound(c)) => Ok(c),
        _ => Err(anyhow::anyhow!("{name} must be compound")),
    }
}

fn get_int_array(
    map: &HashMap<String, fastnbt::Value>,
    name: &str,
) -> Result<Vec<i32>, anyhow::Error> {
    match map.get(name) {
        Some(fastnbt::Value::IntArray(arr)) => Ok(arr.clone().into_inner()),
        _ => Err(anyhow::anyhow!("{name} must be int array")),
    }
}

fn get_byte_array(
    map: &HashMap<String, fastnbt::Value>,
    name: &str,
) -> Result<Vec<i8>, anyhow::Error> {
    match map.get(name) {
        Some(fastnbt::Value::ByteArray(arr)) => Ok(arr.clone().into_inner()),
        _ => Err(anyhow::anyhow!("{name} must be byte array")),
    }
}

/// Verifies the file is valid for WorldEdit `//schem load` against the pinned profile.
pub fn verify_worldedit_import_contract(path: &Path) -> Result<SchematicInspection, anyhow::Error> {
    if path.extension().is_none_or(|ext| ext != "schem") {
        return Err(anyhow::anyhow!(".schem extension required"));
    }
    let inspection = inspect_schematic(path)?;
    if inspection.sponge_version != SPONGE_SCHEMATIC_VERSION {
        return Err(anyhow::anyhow!(
            "expected Sponge v{}, found v{}",
            SPONGE_SCHEMATIC_VERSION,
            inspection.sponge_version
        ));
    }
    if inspection.data_version != PINNED_DATA_VERSION {
        return Err(anyhow::anyhow!(
            "expected DataVersion {}, found {}",
            PINNED_DATA_VERSION,
            inspection.data_version
        ));
    }
    Ok(inspection)
}

/// Publishes bytes atomically (stage + rename pattern, migrated from v1.x).
fn publish_atomically(path: &Path, bytes: &[u8]) -> Result<(), anyhow::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory"))?;
    fs::create_dir_all(parent)?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("destination has no valid file name"))?;
    let sequence = EXPORT_STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stage = parent.join(format!(
        ".{filename}.stage-{}-{sequence}",
        std::process::id()
    ));
    let backup = parent.join(format!(
        ".{filename}.backup-{}-{sequence}",
        std::process::id()
    ));

    let mut moved_previous = false;
    let mut published = false;

    let result = (|| {
        let mut file = File::create(&stage)?;
        file.write_all(bytes)?;
        file.sync_all()?;

        if path.exists() {
            fs::rename(path, &backup)?;
            moved_previous = true;
        }

        if let Err(error) = fs::rename(&stage, path) {
            if moved_previous {
                let _ = fs::rename(&backup, path);
                moved_previous = false;
            }
            return Err(error.into());
        }
        published = true;

        if moved_previous {
            fs::remove_file(&backup)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_minimal_empty_model() {
        let model = VoxelModel::empty(2, 1, 2);
        assert_eq!(model.width, 2);
        assert_eq!(model.height, 1);
        assert_eq!(model.length, 2);
        assert_eq!(model.blocks.len(), 4);
        assert_eq!(model.blocks, vec![0, 0, 0, 0]);
    }

    #[test]
    fn flat_ground_paints_one_uniform_level_with_headroom() {
        let model = VoxelModel::flat_ground(3, 2, 4, "minecraft:grass_block").unwrap();
        assert_eq!(model.height, 4);
        // The whole Y=0 plane is ground
        for z in 0..2 {
            for x in 0..3 {
                assert_eq!(model.blocks[x + z * 3], 1, "ground missing at ({x},{z})");
            }
        }
        // Everything above is air (extension slots for future relief)
        assert!(model.blocks[3 * 2..].iter().all(|&b| b == 0));
    }

    #[test]
    fn flat_ground_requires_a_ground_layer() {
        assert!(VoxelModel::flat_ground(3, 2, 0, "minecraft:grass_block").is_err());
    }

    #[test]
    fn writes_and_reads_schematic_validly() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test.schem");

        let model = VoxelModel {
            width: 2,
            height: 1,
            length: 2,
            palette: vec!["minecraft:air".into(), "minecraft:stone".into()],
            blocks: vec![0, 1, 1, 0],
        };

        write_schematic(&path, "test", &model).unwrap();
        assert!(path.exists());
        assert!(path.metadata().unwrap().len() > 10);

        let inspection = inspect_schematic(&path).unwrap();
        assert_eq!(inspection.sponge_version, 3);
        assert_eq!(inspection.data_version, 3955);
        assert_eq!(inspection.dimensions, [2, 1, 2]);
        assert_eq!(inspection.offset, [0, 0, 0]);
        assert_eq!(inspection.palette_entries, 2);
        assert_eq!(inspection.total_voxels, 4);
        assert_eq!(inspection.non_air_voxels, 2);
        assert_eq!(inspection.content_sha256.len(), 64);
    }

    #[test]
    fn round_trip_preserves_exact_blocks_and_palette() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("roundtrip.schem");

        let original = VoxelModel {
            width: 4,
            height: 2,
            length: 3,
            palette: vec![
                "minecraft:air".into(),
                "minecraft:dirt".into(),
                "minecraft:grass_block".into(),
            ],
            blocks: vec![
                // Layer Y=0 (4x3 = 12 voxels)
                0, 1, 0, 1, //
                0, 1, 0, 1, //
                0, 1, 0, 1, //
                // Layer Y=1 (4x3 = 12 voxels)
                2, 0, 2, 0, //
                2, 0, 2, 0, //
                2, 0, 2, 0, //
            ],
        };

        write_schematic(&path, "roundtrip", &original).unwrap();
        let inspection = inspect_schematic(&path).unwrap();

        assert_eq!(inspection.dimensions, [4, 2, 3]);
        assert_eq!(inspection.total_voxels, 24);
        assert_eq!(inspection.non_air_voxels, 12);
        assert_eq!(inspection.palette_entries, 3);

        // Exact voxel-by-voxel equality: decoded indices must map back to the
        // same block ids as the input model.
        let mut reverse = HashMap::new();
        for (block, idx) in &inspection.palette {
            reverse.insert(*idx as u32, block.clone());
        }
        assert_eq!(inspection.voxels.len(), original.blocks.len());
        for (position, (decoded, expected)) in inspection
            .voxels
            .iter()
            .zip(original.blocks.iter())
            .enumerate()
        {
            assert_eq!(
                reverse.get(decoded),
                Some(&original.palette[*expected as usize]),
                "voxel mismatch at position {position}"
            );
        }
    }

    #[test]
    fn worldedit_contract_accepts_pinned_profile_and_rejects_other_packaging() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("campus.schem");
        let model = VoxelModel {
            width: 1,
            height: 1,
            length: 1,
            palette: vec!["minecraft:stone".into()],
            blocks: vec![0],
        };
        write_schematic(&path, "campus", &model).unwrap();

        let inspection = verify_worldedit_import_contract(&path).unwrap();
        assert_eq!(inspection.sponge_version, SPONGE_SCHEMATIC_VERSION);
        assert_eq!(inspection.data_version, PINNED_DATA_VERSION);

        let zip = tempdir.path().join("campus.zip");
        std::fs::write(&zip, b"not a schematic").unwrap();
        assert!(verify_worldedit_import_contract(&zip)
            .unwrap_err()
            .to_string()
            .contains(".schem"));
    }

    #[test]
    fn overwrite_replaces_previous_export_without_leftover_stage_files() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("campus.schem");
        let first = VoxelModel::flat_ground(2, 2, 1, "minecraft:grass_block").unwrap();
        let second = VoxelModel::flat_ground(3, 3, 1, "minecraft:stone").unwrap();

        write_schematic(&path, "first", &first).unwrap();
        write_schematic(&path, "second", &second).unwrap();

        let inspection = inspect_schematic(&path).unwrap();
        assert_eq!(inspection.dimensions, [3, 1, 3]);
        // Only the destination file remains — no stage/backup leftovers.
        assert_eq!(std::fs::read_dir(tempdir.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_invalid_dimensions() {
        let model = VoxelModel {
            width: 0,
            height: 1,
            length: 1,
            palette: vec!["minecraft:air".into()],
            blocks: vec![0],
        };

        assert!(model.validate().is_err());
    }

    #[test]
    fn rejects_mismatched_block_count() {
        let model = VoxelModel {
            width: 2,
            height: 2,
            length: 2,
            palette: vec!["minecraft:air".into()],
            blocks: vec![0, 0, 0], // should be 8
        };

        assert!(model.validate().is_err());
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("invalid.schem");
        assert!(write_schematic(&path, "invalid", &model).is_err());
        assert!(!path.exists());
    }
}
