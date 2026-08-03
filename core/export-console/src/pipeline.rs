//! 缝 6 生成流水线（F9 → B18 → B4 的居中适配）
//!
//! B18 递回纯内存方块模型（`BlockModel`，不碰文件 IO）；B4 只认自家的
//! `VoxelModel`（扁平三维数组 + 调色板）。两基础模块互不依赖——
//! 本模块就是把前者翻译成后者、再落成 .schem 的那只手，
//! 全程通过 [`ProgressTracker`] 报进度（非阻塞进度条）。

use std::path::Path;

use generation_engine::BlockModel;
use sponge_export::VoxelModel;

use crate::boundary_export::ExportFileSystem;
use crate::data::ExportStage;
use crate::error::{Error, Result};
use crate::progress::ProgressTracker;

/// 把 B18 的方块模型适配成 B4 的体素模型。
///
/// - 坐标平移：包围盒最小角对齐到 (0,0,0)（.schem 局部坐标）；
/// - 调色板：`minecraft:air` 固定占 0 号位（B4 稀疏转稠密的空位填充）；
/// - 空模型（最小路径：一块平整空地都没有）适配为 1×1×1 纯空气模型，
///   保证 .schem 结构合法。
pub fn adapt_to_voxel_model(model: &BlockModel) -> Result<VoxelModel> {
    const AIR: &str = "minecraft:air";

    let Some(bounds) = model.bounding_box() else {
        // 空模型：合法的最小 .schem（1×1×1 空气）
        return Ok(VoxelModel::empty(1, 1, 1));
    };

    let width = bounds.width() as usize;
    let height = bounds.height() as usize;
    let length = bounds.length() as usize;

    // 调色板：空气固定 0 号位，其余按 B18 的确定性字典序排列
    let mut palette: Vec<String> = vec![AIR.to_owned()];
    palette.extend(model.palette().into_iter().filter(|id| id != AIR));

    let palette_index = |block_id: &str| -> u16 {
        palette
            .iter()
            .position(|id| id == block_id)
            .expect("调色板由同一模型导出，必然命中") as u16
    };

    // 稀疏 BTreeMap → 稠密扁平数组 [x + z*width + y*width*length]
    let mut blocks = vec![0u16; width * height * length];
    for block in model.blocks() {
        let x = (block.position.x - bounds.min_x) as usize;
        let y = (block.position.y - bounds.min_y) as usize;
        let z = (block.position.z - bounds.min_z) as usize;
        blocks[x + z * width + y * width * length] = palette_index(&block.block_id);
    }

    let voxel = VoxelModel {
        width,
        height,
        length,
        palette,
        blocks,
    };
    voxel.validate().map_err(Error::SchematicWrite)?;
    Ok(voxel)
}

/// 把方块模型落成 .schem 文件，全程向进度追踪器报进度。
///
/// 进度分账：适配 0→50%，落盘 50→100%（B4 写文件是原子发布，
/// 中途失败不留半截文件）。
pub fn export_schematic(
    model: &BlockModel,
    output_path: &Path,
    schematic_name: &str,
    progress: &ProgressTracker,
) -> Result<()> {
    export_schematic_inner(model, output_path, schematic_name, progress, true)
}

/// F9 双文件事务使用的 B4 staging 写入：复用 B4 编码器，文件写入经
/// 注入的窄端口完成，不让 B4 先发布一个可被误认的最终产物。
pub(crate) fn export_schematic_staged_with_file_system(
    model: &BlockModel,
    output_path: &Path,
    schematic_name: &str,
    profile: sponge_export::SchematicProfile,
    file_system: &dyn ExportFileSystem,
    progress: &ProgressTracker,
) -> Result<()> {
    progress.set_stage(ExportStage::Generating);
    let voxel = adapt_to_voxel_model(model)?;
    progress.report_percent(80);
    progress.set_stage(ExportStage::Writing);
    let bytes = sponge_export::encode_schematic(schematic_name, &voxel, profile)
        .map_err(|error| Error::SchematicWrite(error.to_string()))?;
    file_system
        .write(output_path, &bytes)
        .map_err(|error| Error::SchematicWrite(error.to_string()))?;
    Ok(())
}

fn export_schematic_inner(
    model: &BlockModel,
    output_path: &Path,
    schematic_name: &str,
    progress: &ProgressTracker,
    finish: bool,
) -> Result<()> {
    progress.set_stage(ExportStage::Generating);
    progress.report_percent(0);

    let voxel = adapt_to_voxel_model(model)?;
    progress.report_percent(50);

    progress.set_stage(ExportStage::Writing);
    sponge_export::write_schematic(output_path, schematic_name, &voxel)
        .map_err(|err| Error::SchematicWrite(err.to_string()))?;
    if finish {
        progress.finish();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use generation_engine::BlockPosition;

    #[test]
    fn empty_model_adapts_to_minimal_air_schematic() {
        let voxel = adapt_to_voxel_model(&BlockModel::new()).unwrap();
        assert_eq!((voxel.width, voxel.height, voxel.length), (1, 1, 1));
        assert_eq!(voxel.palette, vec!["minecraft:air".to_owned()]);
    }

    #[test]
    fn blocks_are_translated_to_local_origin() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(10, 5, 20), "minecraft:bricks");
        model.set_block(BlockPosition::new(11, 5, 20), "minecraft:glass_pane");

        let voxel = adapt_to_voxel_model(&model).unwrap();
        assert_eq!((voxel.width, voxel.height, voxel.length), (2, 1, 1));
        // 空气必须固定 0 号位
        assert_eq!(voxel.palette[0], "minecraft:air");
        // (10,5,20) 平移到 (0,0,0)
        let bricks_index = voxel
            .palette
            .iter()
            .position(|id| id == "minecraft:bricks")
            .unwrap() as u16;
        assert_eq!(voxel.blocks[0], bricks_index);
    }

    #[test]
    fn export_reports_progress_and_writes_valid_schematic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("campus.schem");

        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone");

        let progress = ProgressTracker::new();
        export_schematic(&model, &path, "plan-1", &progress).unwrap();

        assert_eq!(progress.percent(), 100);
        assert_eq!(progress.stage(), ExportStage::Done);

        let inspection = sponge_export::inspect_schematic(&path).unwrap();
        assert_eq!(inspection.sponge_version, 3);
        assert_eq!(inspection.non_air_voxels, 1);
    }

    #[test]
    fn write_failure_surfaces_as_schematic_write_error() {
        let dir = tempfile::tempdir().unwrap();
        // 目标路径是一个已存在的目录：B4 原子发布必然失败
        let path = dir.path().join("campus.schem");
        std::fs::create_dir(&path).unwrap();

        let progress = ProgressTracker::new();
        let err = export_schematic(&BlockModel::new(), &path, "plan-1", &progress).unwrap_err();
        assert!(matches!(err, Error::SchematicWrite(_)));
    }
}
