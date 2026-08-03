//! F9 边界直出完整用例（ADR-0041）。
//!
//! 该用例拥有一次导出所需的完整业务决定：校验边界确认状态、决定默认正北
//! 或用户朝向、调用 B18 生成最小场地、调用 B17 写 manifest，再调用 B4
//! 写 Sponge 文件。S1 只提交一次 [`BoundaryExportRequest`]，不拆解这条链。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use foundation_mode::boundary_footprint;
use generation_engine::GenerationEngine;
use manifest_generator::{
    CandidateFacts, FoundationManifest, ManifestGenerator, ManifestOrientation,
    ManifestOrientationSource, MaterialTable, PlanInfo,
};
use shared_domain_types::{Boundary, Orientation, PlanId};

use crate::data::ExportStage;
use crate::error::{BoundaryError, Error, Result};
use crate::pipeline;
use crate::progress::ProgressTracker;

static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// F9 完整导出入口的一次请求。
#[derive(Debug, Clone)]
pub struct BoundaryExportRequest {
    /// 方案与全局版本信息（B17 直接复用的方案信息）。
    pub(crate) plan: PlanInfo,
    /// 方案边界；None 表示没有取得边界。
    pub boundary: Option<Boundary>,
    /// 边界是否已经通过用户确认。
    pub boundary_confirmed: bool,
    /// 用户自定义朝向；None 由本用例决定为地图正北。
    pub orientation: Option<Orientation>,
    /// 最终 `.schem` 目标路径。
    pub schematic_path: PathBuf,
    /// 最终 manifest 目标路径。
    pub manifest_path: PathBuf,
}

impl BoundaryExportRequest {
    /// 创建一次边界直出请求。
    #[allow(
        clippy::too_many_arguments,
        reason = "the F9 request keeps all export inputs explicit at the presentation seam"
    )]
    pub fn new(
        campus_name: impl Into<String>,
        plan_id: PlanId,
        plan_name: impl Into<String>,
        minecraft_version: impl Into<String>,
        boundary: Option<Boundary>,
        boundary_confirmed: bool,
        orientation: Option<Orientation>,
        schematic_path: impl Into<PathBuf>,
        manifest_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            plan: PlanInfo::new(campus_name, plan_id, plan_name, minecraft_version),
            boundary,
            boundary_confirmed,
            orientation,
            schematic_path: schematic_path.into(),
            manifest_path: manifest_path.into(),
        }
    }

    /// 供已经持有 B17 `PlanInfo` 的 F9 测试/内部调用使用。
    pub fn from_plan_info(
        plan: PlanInfo,
        boundary: Option<Boundary>,
        boundary_confirmed: bool,
        orientation: Option<Orientation>,
        schematic_path: impl Into<PathBuf>,
        manifest_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            plan,
            boundary,
            boundary_confirmed,
            orientation,
            schematic_path: schematic_path.into(),
            manifest_path: manifest_path.into(),
        }
    }
}

/// 边界直出成功结果；路径与 manifest 均来自实际发布成功的文件。
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryExportResult {
    /// 实际发布的 `.schem` 路径。
    pub schematic_path: PathBuf,
    /// 实际发布的 manifest 路径。
    pub manifest_path: PathBuf,
    /// 与 manifest 同一份事实对象，便于呈现层显示成功结果。
    pub manifest: FoundationManifest,
}

/// F9 内部完整导出用例；不向 S1 暴露 B18/B17/B4 的分段接口。
pub(crate) struct BoundaryExportUseCase {
    material_table: MaterialTable,
}

impl BoundaryExportUseCase {
    pub(crate) fn new(material_table: MaterialTable) -> Self {
        Self { material_table }
    }

    pub(crate) fn export(
        &self,
        request: &BoundaryExportRequest,
        progress: &ProgressTracker,
    ) -> Result<BoundaryExportResult> {
        let result = self.export_inner(request, progress);
        if result.is_err() {
            progress.fail();
        }
        result
    }

    fn export_inner(
        &self,
        request: &BoundaryExportRequest,
        progress: &ProgressTracker,
    ) -> Result<BoundaryExportResult> {
        let boundary = request
            .boundary
            .as_ref()
            .ok_or(Error::Boundary(BoundaryError::Missing))?;
        if !request.boundary_confirmed {
            return Err(Error::Boundary(BoundaryError::NotConfirmed));
        }
        if request.schematic_path == request.manifest_path {
            return Err(Error::ArtifactWrite(
                "Sponge 文件与 manifest 不能使用同一目标路径".to_owned(),
            ));
        }
        validate_target(&request.schematic_path)?;
        validate_target(&request.manifest_path)?;

        let footprint = boundary_footprint(boundary)
            .map_err(|error| Error::Boundary(BoundaryError::Invalid(error.to_string())))?;

        // 默认值判定只在完整 F9 用例发生，S1 仅传入 Option<Orientation>。
        let (degree, source) = request
            .orientation
            .map_or((0.0, ManifestOrientationSource::MapNorth), |orientation| {
                (orientation.degree(), ManifestOrientationSource::Custom)
            });

        // B18：空候选时生成边界覆盖范围内的一层最小平整场地。
        progress.set_stage(ExportStage::Generating);
        progress.report_percent(5);
        let engine = GenerationEngine::new(self.material_table.clone());
        let model = engine.generate_flat_ground(footprint.width_blocks, footprint.length_blocks)?;
        progress.report_percent(35);

        // B17：不伪造候选投影、评审决定或保留候选，manifest 如实记录全为零。
        let manifest = ManifestGenerator::new()
            .generate_manifest_with_facts(
                &request.plan,
                &[],
                uuid::Uuid::new_v4().to_string(),
                Utc::now().to_rfc3339(),
                Some(ManifestOrientation { degree, source }),
                CandidateFacts::default(),
            )
            .map_err(|error| Error::ManifestWrite(error.to_string()))?;
        progress.report_percent(55);

        let staged_schematic = staged_path(&request.schematic_path, "schem")?;
        let staged_manifest = staged_path(&request.manifest_path, "manifest")?;
        let result = (|| {
            // B17 先写入 staging；只有 B4 与最终双文件发布都成功才返回成功。
            let manifest_parent = request
                .manifest_path
                .parent()
                .ok_or_else(|| Error::ArtifactWrite("manifest 没有父目录".to_owned()))?;
            let manifest_filename = staged_manifest
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::ArtifactWrite("manifest 临时文件名无效".to_owned()))?;
            ManifestGenerator::new()
                .write_to_file(&manifest, manifest_parent, manifest_filename)
                .map_err(|error| Error::ManifestWrite(error.to_string()))?;
            progress.report_percent(65);

            // B4：沿用现有 F9 → B18 → B4 适配器，不复制 Sponge 编码逻辑。
            pipeline::export_schematic_staged(
                &model,
                &staged_schematic,
                &request.plan.plan_name,
                progress,
            )?;
            progress.report_percent(90);

            publish_pair(
                &staged_schematic,
                &request.schematic_path,
                &staged_manifest,
                &request.manifest_path,
            )?;
            progress.finish();
            Ok(BoundaryExportResult {
                schematic_path: request.schematic_path.clone(),
                manifest_path: request.manifest_path.clone(),
                manifest: manifest.clone(),
            })
        })();

        if result.is_err() {
            let _ = fs::remove_file(&staged_schematic);
            let _ = fs::remove_file(&staged_manifest);
        }
        result
    }
}

fn validate_target(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::ArtifactWrite("导出目标路径为空".to_owned()));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| Error::ArtifactWrite(format!("导出目标没有父目录：{}", path.display())))?;
    if parent.exists() && !parent.is_dir() {
        return Err(Error::ArtifactWrite(format!(
            "导出目标父路径不是目录：{}",
            parent.display()
        )));
    }
    if path.exists() && !path.is_file() {
        return Err(Error::ArtifactWrite(format!(
            "导出目标不是文件：{}",
            path.display()
        )));
    }
    fs::create_dir_all(parent).map_err(|error| {
        Error::ArtifactWrite(format!("无法创建导出目录 {}：{error}", parent.display()))
    })?;
    Ok(())
}

fn staged_path(final_path: &Path, kind: &str) -> Result<PathBuf> {
    let parent = final_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| Error::ArtifactWrite("导出目标没有父目录".to_owned()))?;
    let filename = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::ArtifactWrite("导出目标文件名无效".to_owned()))?;
    let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{filename}.m1-{kind}-{}-{sequence}",
        std::process::id()
    )))
}

/// 发布两个已由各自基础模块写好的文件；失败时恢复既有目标。
fn publish_pair(
    staged_schematic: &Path,
    schematic: &Path,
    staged_manifest: &Path,
    manifest: &Path,
) -> Result<()> {
    let schematic_backup = publish_one(staged_schematic, schematic, "schem")?;
    match publish_one(staged_manifest, manifest, "manifest") {
        Ok(manifest_backup) => {
            remove_backup(schematic_backup);
            remove_backup(manifest_backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(schematic);
            restore_backup(schematic, schematic_backup);
            Err(error)
        }
    }
}

fn publish_one(stage: &Path, final_path: &Path, kind: &str) -> Result<Option<PathBuf>> {
    let backup = if final_path.exists() {
        let backup = staged_path(final_path, &format!("backup-{kind}"))?;
        fs::rename(final_path, &backup).map_err(|error| {
            Error::ArtifactWrite(format!(
                "无法备份既有导出 {}：{error}",
                final_path.display()
            ))
        })?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = fs::rename(stage, final_path) {
        restore_backup(final_path, backup.clone());
        return Err(Error::ArtifactWrite(format!(
            "无法发布导出文件 {}：{error}",
            final_path.display()
        )));
    }
    Ok(backup)
}

fn restore_backup(final_path: &Path, backup: Option<PathBuf>) {
    if let Some(backup) = backup {
        let _ = fs::rename(backup, final_path);
    }
}

fn remove_backup(backup: Option<PathBuf>) {
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
}
