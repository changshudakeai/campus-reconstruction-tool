//! F9 增强导出完整用例（ADR-0040/0041/0043）。
//!
//! 边界直出（`boundary_export`）保持不变；本模块是独立的增强导出入口：
//! 只消费应用流程从 B2 读取的“封账后状态为保留”的稳定候选标识，并按同一份
//! 规范化候选投影生成初始校园内容。F9 不依赖 F5，也不查询原始观测。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use chrono::Utc;
use foundation_mode::{BoundaryProjector, CandidateBlockBounds};
use generation_engine::{
    rules::{
        generate_other, generate_road, generate_sports_court, generate_vegetation, generate_water,
        OtherCandidate,
    },
    BlockModel, BlockPosition, BuildingCandidate, GenerationEngine,
};
use manifest_generator::{
    CandidateFacts, CategoryCount, ExportKind, ManifestGenerator, ManifestOrientation,
    ManifestOrientationSource, MaterialTable, PlanInfo,
};
use shared_domain_types::{CandidateCategory, Orientation};

use crate::boundary_export::{
    guarded_export, staged_path, validate_targets, version_contract, write_and_publish,
    ActiveTaskGuard, BoundaryExportOperation, BoundaryExportResult, ExportArtifactTargets,
    ExportFileSystem, ExportPlanContext, ExportPlanState, ExportVersionContract,
    PublicationPayload, PublicationTargets,
};
use crate::data::ExportStage;
use crate::error::{BoundaryError, Error, Result, VersionError};
use crate::preview::PreviewFeature;
use crate::progress::ProgressTracker;

/// 增强导出与增强预览共用的一次完整“边界 + 保留候选 → 校园模型”生成结果。
///
/// 预览与导出消费同一个函数，保证预览内容与最终 `.schem` 同源（T52）。
pub(crate) struct EnhancedGeneration {
    pub(crate) contract: ExportVersionContract,
    pub(crate) degree: f32,
    pub(crate) source: ManifestOrientationSource,
    pub(crate) model: BlockModel,
    pub(crate) dimensions: [usize; 3],
    /// 保留候选的预览定位要素（与最终模型坐标一致；导出流程不消费）。
    pub(crate) features: Vec<crate::preview::PreviewFeature>,
}

/// 校验边界/候选资格并按保留候选生成完整初始校园模型（B5 → B18）。
///
/// 不写任何文件；候选读取、投影复核与合并规则与增强导出完全一致。
pub(crate) fn generate_enhanced_model(
    material_table: &MaterialTable,
    request: &EnhancedExportRequest,
    reader: &dyn CandidateExportReader,
    progress: &ProgressTracker,
) -> Result<EnhancedGeneration> {
    let boundary = request
        .state
        .boundary
        .as_ref()
        .ok_or(Error::Boundary(BoundaryError::Missing))?;
    if !request.state.boundary_confirmed {
        return Err(Error::Boundary(BoundaryError::NotConfirmed));
    }
    let contract = version_contract(
        request.context.plan.minecraft_version.as_str(),
        material_table,
    )?;
    material_table
        .validate_configured_blocks()
        .map_err(|error| {
            Error::Version(VersionError::InvalidMaterialTable {
                version: material_table.minecraft_version.to_string(),
                detail: error.to_string(),
            })
        })?;
    if request.summary.keep_total != request.kept_candidate_ids.len() {
        return Err(Error::CandidateFactsMismatch(format!(
            "摘要保留数 {} 与候选标识数 {} 不一致",
            request.summary.keep_total,
            request.kept_candidate_ids.len()
        )));
    }

    let (orientation, degree, source) = match request.state.orientation {
        Some(orientation) => (
            orientation,
            orientation.degree(),
            ManifestOrientationSource::Custom,
        ),
        None => (
            Orientation::new(0.0).expect("地图正北是合法的完整用例默认值"),
            0.0,
            ManifestOrientationSource::MapNorth,
        ),
    };

    let projector = BoundaryProjector::for_boundary_with_orientation(boundary, orientation)
        .map_err(|error| Error::Boundary(BoundaryError::Invalid(error.to_string())))?;
    let footprint = projector.footprint();
    let bounds = projector.bounds();

    progress.set_stage(ExportStage::Generating);
    progress.report_percent(5);
    let engine = GenerationEngine::new(material_table.clone());
    let mut model = engine.generate_flat_ground_with_polygon(
        footprint.width_blocks,
        footprint.length_blocks,
        projector.boundary_polygons_local(),
    )?;
    progress.report_percent(20);

    let plan_key = request.context.plan.plan_id.to_string();
    let mut generated = 0usize;
    let mut features = Vec::with_capacity(request.kept_candidate_ids.len());
    for candidate_id in &request.kept_candidate_ids {
        let projection = reader
            .kept_projection(&plan_key, candidate_id)?
            .ok_or_else(|| {
                Error::CandidateEligibility(format!("候选 {candidate_id} 的规范化投影缺失"))
            })?;
        if !projection.reviewable {
            return Err(Error::CandidateEligibility(format!(
                "候选 {} 的当前投影不再满足 Reviewable 资格",
                projection.candidate_id
            )));
        }
        let candidate_bounds = projector
            .candidate_bounds(&projection.coordinates)
            .map_err(|error| {
                Error::CandidateEligibility(format!(
                    "候选 {} 几何投影失败：{error}",
                    projection.candidate_id
                ))
            })?;
        let candidate_model = generate_candidate_model(&projection, candidate_bounds, &engine)?;
        let offset_x = (candidate_bounds.min_x - bounds.min_block_x).max(0);
        let offset_z = (candidate_bounds.min_z - bounds.min_block_z).max(0);
        let feature_bounds =
            feature_bounds_after_merge(&candidate_model, offset_x, offset_z, candidate_bounds);
        merge_models(&mut model, &candidate_model, offset_x, offset_z);
        features.push(PreviewFeature {
            candidate_id: projection.candidate_id.clone(),
            display_title: projection.display_title.clone(),
            category: serde_json::to_string(&projection.category)
                .unwrap_or_else(|_| "Other".to_owned())
                .trim_matches('"')
                .to_owned(),
            bounds: feature_bounds,
        });
        generated += 1;
        let percent = 20 + (generated * 45 / request.kept_candidate_ids.len().max(1));
        progress.report_percent(percent as u32);
    }
    if generated != request.summary.keep_total {
        return Err(Error::CandidateFactsMismatch(format!(
            "实际生成 {} 项，摘要保留 {} 项",
            generated, request.summary.keep_total
        )));
    }

    let dimensions = model
        .bounding_box()
        .map(|bbox| {
            [
                bbox.width() as usize,
                bbox.height() as usize,
                bbox.length() as usize,
            ]
        })
        .unwrap_or([footprint.width_blocks, 1, footprint.length_blocks]);
    Ok(EnhancedGeneration {
        contract,
        degree,
        source,
        model,
        dimensions,
        features,
    })
}

/// 增强导出内部完整用例；不向 S1 暴露 B18/B17/B4 的分段接口。
pub(crate) struct EnhancedExportUseCase {
    material_table: MaterialTable,
    file_system: Arc<dyn ExportFileSystem>,
}

impl EnhancedExportUseCase {
    pub(crate) fn new(
        material_table: MaterialTable,
        file_system: Arc<dyn ExportFileSystem>,
    ) -> Self {
        Self {
            material_table,
            file_system,
        }
    }

    pub(crate) fn export(
        &self,
        request: &EnhancedExportRequest,
        reader: &dyn CandidateExportReader,
        progress: &ProgressTracker,
    ) -> Result<BoundaryExportResult> {
        let staged_schematic = staged_path(&request.targets.schematic_path, "schem")?;
        let staged_manifest = staged_path(&request.targets.manifest_path, "manifest")?;
        guarded_export(
            self.file_system.as_ref(),
            progress,
            &staged_schematic,
            &staged_manifest,
            |staged_schematic, staged_manifest| {
                self.export_inner(request, reader, progress, staged_schematic, staged_manifest)
            },
        )
    }

    fn export_inner(
        &self,
        request: &EnhancedExportRequest,
        reader: &dyn CandidateExportReader,
        progress: &ProgressTracker,
        staged_schematic: &std::path::Path,
        staged_manifest: &std::path::Path,
    ) -> Result<BoundaryExportResult> {
        validate_targets(
            &request.targets.schematic_path,
            &request.targets.manifest_path,
            self.file_system.as_ref(),
        )?;
        let generated = generate_enhanced_model(&self.material_table, request, reader, progress)?;

        let actual_plan = PlanInfo::new(
            request.context.plan.campus_name.clone(),
            request.context.plan.plan_id,
            request.context.plan.plan_name.clone(),
            generated.contract.version,
        );
        let keep_decisions: Vec<(CandidateCategory, bool)> = request
            .summary
            .keep_by_category
            .iter()
            .map(|(category, _)| (*category, true))
            .collect();
        let candidate_facts = facts_from_summary(&request.summary);
        let mut manifest = ManifestGenerator::new()
            .generate_manifest_with_facts(
                &actual_plan,
                &keep_decisions,
                uuid::Uuid::new_v4().to_string(),
                Utc::now().to_rfc3339(),
                Some(ManifestOrientation {
                    degree: generated.degree,
                    source: generated.source,
                }),
                candidate_facts,
            )
            .map_err(|error| Error::ManifestWrite(error.to_string()))?;
        manifest.export_kind = ExportKind::Enhanced;
        progress.report_percent(70);

        write_and_publish(
            self.file_system.as_ref(),
            PublicationTargets {
                schematic: &request.targets.schematic_path,
                manifest: &request.targets.manifest_path,
                staged_schematic,
                staged_manifest,
            },
            PublicationPayload {
                manifest: &manifest,
                model: &generated.model,
                contract: generated.contract,
                schematic_dimensions: generated.dimensions,
            },
            progress,
        )
    }
}

/// 封账摘要：应用流程从 B2 读取后传入 F9（F9 对 F5 零依赖）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateExportSummary {
    /// 当前已发布候选投影数量（Reviewable）。
    pub candidate_projection_count: usize,
    /// 已存在的评审决定数量（真实封账写回条数）。
    pub review_decision_count: usize,
    /// 本次实际保留候选数量。
    pub keep_total: usize,
    /// 保留候选按类别计数（仅列出保留数 > 0 的类别）。
    pub keep_by_category: Vec<(CandidateCategory, usize)>,
    /// 待定项数（如实报数，不导出）。
    pub pending_count: usize,
    /// 剔除项数（如实报数，不导出）。
    pub remove_count: usize,
}

/// 单个保留候选的规范化投影值（F9 消费的同一份投影；不含原始观测）。
#[derive(Debug, Clone)]
pub struct KeptCandidateProjection {
    /// B2 候选投影的稳定标识。
    pub candidate_id: String,
    /// 六类别。
    pub category: CandidateCategory,
    /// 展示标题（来源名称）。
    pub display_title: String,
    /// 展示标签（高度/层数/屋顶/标签家族等）。
    pub tags: Vec<(String, String)>,
    /// 形状种类（point/line_string/polygon）。
    pub shape_kind: String,
    /// 规范化投影几何（GeoJSON coordinates）。
    pub coordinates: serde_json::Value,
    /// 当前投影是否仍具备 Reviewable 资格（ADR-0040 复核）。
    pub reviewable: bool,
}

/// 窄读者 seam：只按稳定候选标识读取规范化候选投影。
///
/// 生产实现由 A2 经 B2 `CandidateProjectionsApi` 读取当前已发布批次；
/// F9 借此“只读 B2 保留标识与规范化投影”，不接触原始观测。
pub trait CandidateExportReader: Send + Sync {
    /// 读取一个保留候选的当前规范化投影；无投影时返回 `Ok(None)`。
    fn kept_projection(
        &self,
        plan_id: &str,
        candidate_id: &str,
    ) -> Result<Option<KeptCandidateProjection>>;
}

/// 增强导出的完整输入（Start 返回前冻结）。
#[derive(Debug, Clone)]
pub struct EnhancedExportRequest {
    pub(crate) context: ExportPlanContext,
    pub(crate) state: ExportPlanState,
    pub(crate) targets: ExportArtifactTargets,
    /// 封账摘要（应用流程从 B2 读取后传入）。
    pub summary: CandidateExportSummary,
    /// 封账后状态为保留的稳定候选标识。
    pub kept_candidate_ids: Vec<String>,
}

impl EnhancedExportRequest {
    /// 创建一次增强导出请求。
    pub fn new(
        context: ExportPlanContext,
        state: ExportPlanState,
        targets: ExportArtifactTargets,
        summary: CandidateExportSummary,
        kept_candidate_ids: Vec<String>,
    ) -> Self {
        Self {
            context,
            state,
            targets,
            summary,
            kept_candidate_ids,
        }
    }
}

/// F9 从组合根取得增强导出完整输入的稳定能力端口。
pub trait EnhancedExportInput: Send + Sync {
    /// 一次读取完整增强导出输入（含封账摘要与保留标识）。
    fn load_request(&self) -> Result<EnhancedExportRequest>;
}

/// F9 稳定的增强导出能力端口；S1 只调用一次开始意图（经 A2 路由）。
#[derive(Clone)]
pub struct EnhancedExportPort {
    use_case: Arc<EnhancedExportUseCase>,
    input: Arc<dyn EnhancedExportInput>,
    reader: Arc<dyn CandidateExportReader>,
    active: Arc<AtomicBool>,
    lifecycle: Arc<AtomicU64>,
}

impl EnhancedExportPort {
    /// Construct the production enhanced F9 port with the authoritative 26.1.2 contract.
    pub fn new_enhanced_v26_1_2(
        input: Arc<dyn EnhancedExportInput>,
        reader: Arc<dyn CandidateExportReader>,
        file_system: Arc<dyn ExportFileSystem>,
    ) -> Self {
        Self {
            use_case: Arc::new(EnhancedExportUseCase::new(
                MaterialTable::v26_1_2_school(),
                file_system,
            )),
            input,
            reader,
            active: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 提交一次增强开始意图；输入取得与完整导出均由 F9 端口拥有。
    pub fn start(&self) -> Result<BoundaryExportOperation> {
        if self.active.swap(true, Ordering::SeqCst) {
            return Err(Error::InvalidState("增强导出进行中，不接受新的导出请求"));
        }

        let request = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.input.load_request()
        })) {
            Ok(Ok(request)) => request,
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
        let reader = Arc::clone(&self.reader);
        let active = Arc::clone(&self.active);
        let lifecycle = Arc::clone(&self.lifecycle);
        let generation = lifecycle.load(Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        let spawn_result = std::thread::Builder::new().spawn(move || {
            let result = {
                let _active_guard = ActiveTaskGuard(active);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    use_case.export(&request, reader.as_ref(), &worker_progress)
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
        Ok(BoundaryExportOperation::new(
            progress, receiver, lifecycle, generation,
        ))
    }

    /// Expire an operation result when its presentation context is left.
    pub fn expire_active(&self) {
        self.lifecycle.fetch_add(1, Ordering::SeqCst);
    }
}

/// 把一个保留候选投影 + 块坐标外接范围生成初始校园内容（B18）。
pub(crate) fn generate_candidate_model(
    projection: &KeptCandidateProjection,
    bounds: CandidateBlockBounds,
    engine: &GenerationEngine,
) -> Result<BlockModel> {
    let width = bounds.width_blocks.max(1) as i32;
    let length = bounds.length_blocks.max(1) as i32;
    match projection.category {
        CandidateCategory::Building => {
            // 建筑至少 3x3 格才可渲染（B18 参数约束）；尺寸仍来自投影外接范围。
            let width = bounds.width_blocks.max(3) as i32;
            let length = bounds.length_blocks.max(3) as i32;
            let mut candidate = BuildingCandidate::new(&projection.candidate_id, width, length);
            for (key, value) in &projection.tags {
                match key.to_ascii_lowercase().as_str() {
                    "height" => {
                        if let Ok(height_m) = value.parse::<f64>() {
                            candidate = candidate.with_height_m(height_m);
                        }
                    }
                    "levels" => {
                        if let Ok(levels) = value.parse::<u32>() {
                            candidate = candidate.with_levels(levels);
                        }
                    }
                    "roof" => candidate = candidate.with_roof_shape(value),
                    _ => {}
                }
            }
            engine.generate_building(&candidate).map_err(Into::into)
        }
        CandidateCategory::Road => {
            generate_road(width, length, engine.materials()).map_err(Into::into)
        }
        CandidateCategory::Water => {
            generate_water(width, length, engine.materials()).map_err(Into::into)
        }
        CandidateCategory::Vegetation => {
            generate_vegetation(engine.materials()).map_err(Into::into)
        }
        CandidateCategory::Sports => {
            generate_sports_court(width, length, engine.materials()).map_err(Into::into)
        }
        CandidateCategory::Other => {
            let mut other = OtherCandidate::new(&projection.candidate_id);
            for (key, value) in &projection.tags {
                other.tags.insert(key.clone(), value.clone());
            }
            generate_other(&other, engine.materials()).map_err(Into::into)
        }
        _ => Err(Error::CandidateEligibility(format!(
            "候选 {} 的类别 {} 尚无生成规则",
            projection.candidate_id,
            projection.category.display_name()
        ))),
    }
}

/// 把候选模型按块坐标偏移合并进场地模型（后写覆盖先写）。
pub(crate) fn merge_models(
    target: &mut BlockModel,
    candidate: &BlockModel,
    offset_x: i32,
    offset_z: i32,
) {
    for block in candidate.blocks() {
        target.set_block(
            BlockPosition::new(
                block.position.x + offset_x,
                block.position.y,
                block.position.z + offset_z,
            ),
            &block.block_id,
        );
    }
}

/// 封账摘要 → manifest 候选链事实（保留类别与数量如实记录）。
pub(crate) fn facts_from_summary(summary: &CandidateExportSummary) -> CandidateFacts {
    CandidateFacts {
        candidate_projection_count: summary.candidate_projection_count,
        review_decision_count: summary.review_decision_count,
        retained_candidate_count: summary.keep_total,
        keep_by_category: summary
            .keep_by_category
            .iter()
            .map(|(category, count)| CategoryCount::new(category_identifier(*category), *count))
            .collect(),
    }
}

/// 类别 → manifest 稳定标识（与 B2 数据库词汇一致）。
pub(crate) fn category_identifier(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "Building",
        CandidateCategory::Road => "Road",
        CandidateCategory::Water => "Water",
        CandidateCategory::Vegetation => "Vegetation",
        CandidateCategory::Sports => "Sports",
        CandidateCategory::Other => "Other",
        _ => "Other",
    }
}

/// 候选模型合并后的实际包围盒；预览定位必须与 `merge_models` 使用完全相同的
/// X/Z 偏移，不能继续携带边界中心投影坐标。
fn feature_bounds_after_merge(
    candidate: &BlockModel,
    offset_x: i32,
    offset_z: i32,
    projected: CandidateBlockBounds,
) -> [i32; 6] {
    if let Some(bounds) = candidate.bounding_box() {
        return [
            bounds.min_x + offset_x,
            bounds.min_y,
            bounds.min_z + offset_z,
            bounds.max_x + offset_x,
            bounds.max_y,
            bounds.max_z + offset_z,
        ];
    }
    [
        offset_x,
        0,
        offset_z,
        offset_x + projected.width_blocks.max(1) as i32 - 1,
        0,
        offset_z + projected.length_blocks.max(1) as i32 - 1,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_feature_bounds_match_merged_candidate_coordinates() {
        let mut candidate = BlockModel::new();
        candidate.set_block(BlockPosition::new(2, 1, 3), "minecraft:bricks");
        candidate.set_block(BlockPosition::new(4, 5, 6), "minecraft:bricks");

        assert_eq!(
            feature_bounds_after_merge(
                &candidate,
                10,
                20,
                CandidateBlockBounds {
                    min_x: 0,
                    min_z: 0,
                    width_blocks: 3,
                    length_blocks: 4,
                },
            ),
            [12, 1, 23, 14, 5, 26],
            "定位包围盒必须与 merge_models 后的实际方块坐标一致"
        );
    }
}
