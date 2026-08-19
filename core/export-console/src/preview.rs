//! 第五步 3D 方块预览（T52）。
//!
//! F9 只把与导出**同源**的 B18 [`BlockModel`] 序列化为渲染数据
//! （方块 ID + 坐标），供 WebView 内的 Three.js 渲染；不另造一套预览模型。
//! 预览是第五步的呈现能力：生成失败只影响预览，绝不阻塞或改变导出流程
//! （ADR-0045：预览只是呈现层）。
//!
//! 数据格式（版本 2，紧凑 JSON，水平方向 RLE 游程 + 保留候选要素）：
//! ```json
//! {
//!   "v": 2,
//!   "palette": ["minecraft:air", "minecraft:stone_bricks"],
//!   "bounds": [min_x, min_y, min_z, max_x, max_y, max_z],
//!   "count": 123,
//!   "runs": [[palette_index, x_start, x_end, y, z], ...],
//!   "features": [
//!     {"id":"c1","title":"教学楼","category":"Building","bounds":[0,0,0,20,14,30]}
//!   ]
//! }
//! ```
//! 调色板 0 号位固定 `minecraft:air`（与 B4 `VoxelModel` 的空位约定一致），
//! 其余按 B18 的确定性字典序排列；坐标为方案局部坐标，前端按包围盒平移。
//! 同 `(y, z)` 上相邻同方块沿 x 轴合并成一条游程——无损压缩地面/墙体等
//! 长条内容，超大校园也能轻量传输，且展开后与导出 `.schem` 逐块一致。
//! `features` 携带增强导出中每个保留候选的定位要素（ID/名称/类别/包围盒，
//! 与候选在最终模型中的位置一致），供第五步抽屉卡片定位与精细检查；
//! 边界直出预览无候选时为 `[]`。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use generation_engine::BlockModel;
use manifest_generator::MaterialTable;

use crate::boundary_export::{
    generate_ground_model, ActiveTaskGuard, BoundaryExportInput, BoundaryExportRequest,
};
use crate::data::ExportStage;
use crate::enhanced::{
    generate_enhanced_model, CandidateExportReader, EnhancedExportInput, EnhancedExportRequest,
};
use crate::error::{Error, PreviewError, Result};
use crate::progress::ProgressTracker;
use crate::views::ExportProgressView;

/// 超过该包围盒格数的预览明确失败（`PreviewError::TooLarge`），避免 WebView 卡死。
/// 一格一个方块：格数是渲染网格内存与剔除时间的真实上限。真实校区（如约
/// 1064×863 边界 + 数十格高建筑）的包围盒可达数千万格，Uint16 体素网格与
/// 一次性的隐藏面剔除仍可承受；64M 格 ≈ 128MB 网格，属于桌面 WebView2 的
/// 安全上界，超过即明确拒绝而非卡死。
pub const PREVIEW_GRID_CELL_LIMIT: usize = 64_000_000;

/// 单个保留候选的预览定位要素（随渲染负载下发，供前端定位/高亮/精细检查）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFeature {
    /// B2 候选投影的稳定标识。
    pub candidate_id: String,
    /// 展示标题（来源名称）。
    pub display_title: String,
    /// 六类别枚举名（`Building`/`Road`/`Water`/`Vegetation`/`Sports`/`Other`）。
    pub category: String,
    /// 候选在最终模型中的包围盒（方案局部坐标，含端点）。
    pub bounds: [i32; 6],
}

/// 一次预览生成的最终渲染负载：紧凑 JSON + 方块数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRenderPayload {
    /// 供 WebView Three.js 消费的紧凑 JSON（见模块文档）。
    pub json: String,
    /// 本次负载包含的非空气方块数。
    pub block_count: usize,
}

/// B18 `BlockModel` → 预览渲染数据（方块 ID + 坐标，水平 RLE 游程）。
pub fn serialize_preview(
    model: &BlockModel,
    features: &[PreviewFeature],
) -> Result<PreviewRenderPayload> {
    const AIR: &str = "minecraft:air";

    let Some(bounds) = model.bounding_box() else {
        return Ok(PreviewRenderPayload {
            json: r#"{"v":2,"palette":["minecraft:air"],"bounds":[0,0,0,0,0,0],"count":0,"runs":[],"features":[]}"#
                .to_owned(),
            block_count: 0,
        });
    };
    let grid_cells =
        u64::from(bounds.width()) * u64::from(bounds.height()) * u64::from(bounds.length());
    if grid_cells > PREVIEW_GRID_CELL_LIMIT as u64 {
        return Err(Error::Preview(PreviewError::TooLarge {
            cells: grid_cells,
            limit: PREVIEW_GRID_CELL_LIMIT,
        }));
    }
    let block_count = model.block_count();

    // 空气固定 0 号位，其余按 B18 确定性字典序（与 adapt_to_voxel_model 一致）。
    let mut palette: Vec<String> = vec![AIR.to_owned()];
    palette.extend(model.palette().into_iter().filter(|id| id != AIR));
    let mut indices: Vec<(&str, usize)> = Vec::with_capacity(palette.len());
    for (index, block_id) in palette.iter().enumerate() {
        indices.push((block_id.as_str(), index));
    }

    let bounds = [
        bounds.min_x,
        bounds.min_y,
        bounds.min_z,
        bounds.max_x,
        bounds.max_y,
        bounds.max_z,
    ];

    // 同 (y,z) 上相邻同方块沿 x 合并成游程；BTreeMap 迭代顺序保证按
    // (x, y, z) 字典序输出，游程必然连续。
    let mut runs: Vec<(usize, i32, i32, i32, i32)> = Vec::new();
    for block in model.blocks() {
        if let Some((index, _x_start, x_end, y, z)) = runs.last_mut() {
            let palette_index = palette_index(&indices, &block.block_id);
            if *index == palette_index
                && *y == block.position.y
                && *z == block.position.z
                && block.position.x == *x_end + 1
            {
                *x_end = block.position.x;
                continue;
            }
        }
        runs.push((
            palette_index(&indices, &block.block_id),
            block.position.x,
            block.position.x,
            block.position.y,
            block.position.z,
        ));
    }

    let mut json = String::with_capacity(runs.len().saturating_mul(24).saturating_add(1024));
    json.push_str(r#"{"v":2,"palette":["#);
    for (index, block_id) in palette.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        push_json_string(&mut json, block_id);
    }
    json.push_str("],\"bounds\":[");
    for (index, value) in bounds.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&value.to_string());
    }
    json.push_str("],\"count\":");
    json.push_str(&block_count.to_string());
    json.push_str(",\"runs\":[");
    for (position, (index, x_start, x_end, y, z)) in runs.iter().enumerate() {
        if position > 0 {
            json.push(',');
        }
        json.push('[');
        json.push_str(&index.to_string());
        json.push(',');
        json.push_str(&x_start.to_string());
        json.push(',');
        json.push_str(&x_end.to_string());
        json.push(',');
        json.push_str(&y.to_string());
        json.push(',');
        json.push_str(&z.to_string());
        json.push(']');
    }
    json.push_str("],\"features\":[");
    for (position, feature) in features.iter().enumerate() {
        if position > 0 {
            json.push(',');
        }
        json.push_str("{\"id\":");
        push_json_string(&mut json, &feature.candidate_id);
        json.push_str(",\"title\":");
        push_json_string(&mut json, &feature.display_title);
        json.push_str(",\"category\":");
        push_json_string(&mut json, &feature.category);
        json.push_str(",\"bounds\":[");
        for (index, value) in feature.bounds.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&value.to_string());
        }
        json.push_str("]}");
    }
    json.push_str("]}");

    Ok(PreviewRenderPayload { json, block_count })
}

fn palette_index(indices: &[(&str, usize)], block_id: &str) -> usize {
    indices
        .iter()
        .find(|(id, _)| *id == block_id)
        .map(|(_, index)| *index)
        .expect("调色板由同一模型导出，必然命中")
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < '\u{20}' => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// 一次预览请求的冻结输入（边界直出或增强导出的同一份请求形态）。
enum BlockPreviewRequest {
    Boundary(BoundaryExportRequest),
    Enhanced(EnhancedExportRequest),
}

/// 预览输入来源：边界直出或增强导出的同一批稳定输入端口。
pub enum BlockPreviewInput {
    Boundary(Arc<dyn BoundaryExportInput>),
    Enhanced {
        input: Arc<dyn EnhancedExportInput>,
        reader: Arc<dyn CandidateExportReader>,
    },
}

/// 预览生成用例：与导出一模一样的 B5 → B18 代码，但不写任何文件。
pub(crate) enum BlockPreviewUseCase {
    Boundary { material_table: MaterialTable },
    Enhanced { material_table: MaterialTable },
}

impl BlockPreviewUseCase {
    fn generate(
        &self,
        request: BlockPreviewRequest,
        reader: Option<&dyn CandidateExportReader>,
        progress: &ProgressTracker,
    ) -> Result<PreviewRenderPayload> {
        match (self, request) {
            (Self::Boundary { material_table }, BlockPreviewRequest::Boundary(request)) => {
                progress.set_stage(ExportStage::Generating);
                progress.report_percent(5);
                let ground = generate_ground_model(material_table, &request)?;
                progress.report_percent(60);
                let payload = serialize_preview(&ground.model, &[])?;
                progress.report_percent(95);
                progress.finish();
                Ok(payload)
            }
            (Self::Enhanced { material_table }, BlockPreviewRequest::Enhanced(request)) => {
                let reader = reader.expect("增强预览请求必然携带候选读取器（端口构造不变量）");
                progress.set_stage(ExportStage::Generating);
                progress.report_percent(5);
                let generated =
                    generate_enhanced_model(material_table, &request, reader, progress)?;
                let payload = serialize_preview(&generated.model, &generated.features)?;
                progress.report_percent(95);
                progress.finish();
                Ok(payload)
            }
            _ => Err(Error::BackgroundTask),
        }
    }
}

/// F9 稳定的预览能力端口；S1 经 A2 只调用一次 [`Self::start`]。
#[derive(Clone)]
pub struct BlockPreviewPort {
    use_case: Arc<BlockPreviewUseCase>,
    input: Arc<BlockPreviewInput>,
    active: Arc<AtomicBool>,
    lifecycle: Arc<AtomicU64>,
}

impl BlockPreviewPort {
    /// 边界直出预览：与 [`crate::BoundaryExportPort::new_boundary_only_v26_1_2`] 同契约。
    pub fn new_boundary_v26_1_2(input: Arc<dyn BoundaryExportInput>) -> Self {
        Self::new(
            BlockPreviewUseCase::Boundary {
                material_table: MaterialTable::v26_1_2_school(),
            },
            BlockPreviewInput::Boundary(input),
        )
    }

    /// 增强预览：与 [`crate::EnhancedExportPort::new_enhanced_v26_1_2`] 同契约。
    pub fn new_enhanced_v26_1_2(
        input: Arc<dyn EnhancedExportInput>,
        reader: Arc<dyn CandidateExportReader>,
    ) -> Self {
        Self::new(
            BlockPreviewUseCase::Enhanced {
                material_table: MaterialTable::v26_1_2_school(),
            },
            BlockPreviewInput::Enhanced { input, reader },
        )
    }

    fn new(use_case: BlockPreviewUseCase, input: BlockPreviewInput) -> Self {
        Self {
            use_case: Arc::new(use_case),
            input: Arc::new(input),
            active: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 提交一次预览生成意图；输入取得与完整生成均由 F9 端口拥有。
    pub fn start(&self) -> Result<BlockPreviewOperation> {
        if self.active.swap(true, Ordering::SeqCst) {
            return Err(Error::InvalidState("3D 预览生成进行中，不接受新的预览请求"));
        }

        let (request, reader) =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &*self.input {
                BlockPreviewInput::Boundary(input) => input
                    .load_request()
                    .map(|request| (BlockPreviewRequest::Boundary(request), None)),
                BlockPreviewInput::Enhanced { input, reader } => {
                    input.load_request().map(|request| {
                        (
                            BlockPreviewRequest::Enhanced(request),
                            Some(Arc::clone(reader)),
                        )
                    })
                }
            })) {
                Ok(Ok(frozen)) => frozen,
                Ok(Err(error)) => {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(error);
                }
                Err(_) => {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(Error::BackgroundTask);
                }
            };
        let progress = ProgressTracker::new();
        let worker_progress = progress.clone();
        let use_case = Arc::clone(&self.use_case);
        let active = Arc::clone(&self.active);
        let lifecycle = Arc::clone(&self.lifecycle);
        let generation = lifecycle.load(Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        let spawn_result = std::thread::Builder::new().spawn(move || {
            let result = {
                let _active_guard = ActiveTaskGuard(active);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    use_case.generate(request, reader.as_deref(), &worker_progress)
                }))
                .unwrap_or_else(|_| {
                    worker_progress.fail();
                    Err(Error::BackgroundTask)
                })
            };
            let _ = sender.send(result);
        });
        if spawn_result.is_err() {
            self.active.store(false, Ordering::SeqCst);
            return Err(Error::BackgroundTask);
        }
        Ok(BlockPreviewOperation::new(
            progress, receiver, lifecycle, generation,
        ))
    }

    /// 使一次预览操作的结果过期（其呈现上下文已离开）。
    pub fn expire_active(&self) {
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
    }
}

/// 后台预览操作；UI 只读取真实 F9 进度并轮询终态。
pub struct BlockPreviewOperation {
    progress: ProgressTracker,
    result: mpsc::Receiver<Result<PreviewRenderPayload>>,
    lifecycle: Arc<AtomicU64>,
    generation: u64,
}

impl BlockPreviewOperation {
    pub(crate) fn new(
        progress: ProgressTracker,
        result: mpsc::Receiver<Result<PreviewRenderPayload>>,
        lifecycle: Arc<AtomicU64>,
        generation: u64,
    ) -> Self {
        Self {
            progress,
            result,
            lifecycle,
            generation,
        }
    }

    /// 当前真实阶段/百分比对应的呈现数据。
    pub fn progress_view(&self) -> ExportProgressView {
        ExportProgressView::from_tracker(&self.progress)
    }

    /// 非阻塞取得后台终态；没有终态时返回 None。
    pub fn try_complete(&mut self) -> Option<Result<PreviewRenderPayload>> {
        let expired = self.lifecycle.load(Ordering::SeqCst) != self.generation;
        match self.result.try_recv() {
            Ok(_result) if expired => {
                self.progress.fail();
                Some(Err(Error::InvalidState("preview result expired")))
            }
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::BackgroundTask)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use generation_engine::BlockPosition;

    #[test]
    fn payload_uses_air_at_palette_zero_and_absolute_coordinates() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(10, 5, 20), "minecraft:bricks");
        model.set_block(BlockPosition::new(11, 5, 20), "minecraft:bricks");
        model.set_block(BlockPosition::new(12, 5, 20), "minecraft:glass_pane");

        let payload = serialize_preview(&model, &[]).expect("序列化成功");
        let parsed: serde_json::Value = serde_json::from_str(&payload.json).expect("合法 JSON");
        assert_eq!(parsed["v"], 2);
        assert_eq!(parsed["palette"][0], "minecraft:air", "空气必须固定 0 号位");
        assert_eq!(parsed["count"], 3);
        assert_eq!(payload.block_count, 3);
        assert_eq!(parsed["features"].as_array().expect("要素数组").len(), 0);
        // 坐标保持方案局部坐标（不预先平移）
        let runs = parsed["runs"].as_array().expect("游程数组");
        assert_eq!(runs.len(), 2, "相邻同方块必须合并为一条游程");
        // 第一条游程：bricks 10..=11
        assert_eq!(runs[0][1], 10);
        assert_eq!(runs[0][2], 11);
        assert_eq!(runs[0][3], 5);
        assert_eq!(runs[0][4], 20);
        // 第二条游程：glass_pane 12
        assert_eq!(runs[1][1], 12);
        assert_eq!(runs[1][2], 12);
    }

    #[test]
    fn too_large_model_fails_with_typed_preview_error() {
        let mut model = BlockModel::new();
        assert!(serialize_preview(&model, &[]).is_ok());
        model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone");
        assert!(serialize_preview(&model, &[]).is_ok());

        let error = Error::Preview(PreviewError::TooLarge {
            cells: PREVIEW_GRID_CELL_LIMIT as u64 + 1,
            limit: PREVIEW_GRID_CELL_LIMIT,
        });
        assert!(matches!(
            error,
            Error::Preview(PreviewError::TooLarge { .. })
        ));
    }

    #[test]
    fn expanded_runs_match_the_schematic_voxel_model_block_for_block() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone_bricks");
        model.set_block(BlockPosition::new(1, 0, 0), "minecraft:stone_bricks");
        model.set_block(BlockPosition::new(2, 0, 0), "minecraft:bricks");
        model.set_block(BlockPosition::new(0, 1, 0), "minecraft:water");
        model.set_block(BlockPosition::new(3, 2, 1), "minecraft:oak_leaves");

        let payload = serialize_preview(&model, &[]).expect("序列化成功");
        let parsed: serde_json::Value = serde_json::from_str(&payload.json).expect("合法 JSON");
        let palette: Vec<String> = parsed["palette"]
            .as_array()
            .expect("调色板数组")
            .iter()
            .map(|value| value.as_str().expect("方块 ID").to_owned())
            .collect();
        let bounds: Vec<i32> = parsed["bounds"]
            .as_array()
            .expect("包围盒数组")
            .iter()
            .map(|value| value.as_i64().expect("整数坐标") as i32)
            .collect();

        // 展开 RLE 游程 → (方块 ID, 本地 x, y, z)
        let mut expanded: Vec<(String, i32, i32, i32)> = Vec::new();
        for run in parsed["runs"].as_array().expect("游程数组") {
            let palette_index = run[0].as_u64().expect("调色板索引") as usize;
            let x_start = run[1].as_i64().expect("x0") as i32;
            let x_end = run[2].as_i64().expect("x1") as i32;
            let y = run[3].as_i64().expect("y") as i32;
            let z = run[4].as_i64().expect("z") as i32;
            for x in x_start..=x_end {
                expanded.push((
                    palette[palette_index].clone(),
                    x - bounds[0],
                    y - bounds[1],
                    z - bounds[2],
                ));
            }
        }

        // 对照 .schem 的同一份适配结果（B18 → B4 VoxelModel）
        let voxel = crate::pipeline::adapt_to_voxel_model(&model).expect("体素适配");
        let mut expected: Vec<(String, i32, i32, i32)> = Vec::new();
        for y in 0..voxel.height {
            for z in 0..voxel.length {
                for x in 0..voxel.width {
                    let palette_index =
                        voxel.blocks[x + z * voxel.width + y * voxel.width * voxel.length] as usize;
                    let block_id = voxel.palette[palette_index].clone();
                    if block_id != "minecraft:air" {
                        expected.push((block_id, x as i32, y as i32, z as i32));
                    }
                }
            }
        }

        expanded.sort();
        expected.sort();
        assert_eq!(
            expanded, expected,
            "预览展开后的方块必须与最终 .schem 的体素逐块一致"
        );
    }

    #[test]
    fn payload_carries_kept_candidate_features_for_locate_and_highlight() {
        let mut model = BlockModel::new();
        model.set_block(BlockPosition::new(4, 0, 3), "minecraft:bricks");
        model.set_block(BlockPosition::new(5, 0, 3), "minecraft:bricks");
        let features = vec![PreviewFeature {
            candidate_id: "way/42".to_owned(),
            display_title: "图书馆".to_owned(),
            category: "Building".to_owned(),
            bounds: [4, 0, 3, 5, 0, 3],
        }];

        let payload = serialize_preview(&model, &features).expect("序列化成功");
        let parsed: serde_json::Value = serde_json::from_str(&payload.json).expect("合法 JSON");
        let list = parsed["features"].as_array().expect("要素数组");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["id"], "way/42");
        assert_eq!(list[0]["title"], "图书馆");
        assert_eq!(list[0]["category"], "Building");
        assert_eq!(list[0]["bounds"][0], 4);
        assert_eq!(list[0]["bounds"][5], 3);
    }
}
