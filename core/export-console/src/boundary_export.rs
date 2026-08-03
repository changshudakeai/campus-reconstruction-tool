//! F9 边界直出完整用例（ADR-0041）。
//!
//! 该用例拥有一次导出所需的完整业务决定：校验边界确认状态、核对
//! Minecraft 26.1.2 兼容配置、决定默认正北或用户朝向、调用 B18 生成
//! 最小场地、调用 B17 写 manifest，再调用 B4 编码 Sponge 文件。S1 只
//! 通过 [`BoundaryExportPort`] 提交一次开始意图，不拆解这条链。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use chrono::Utc;
use foundation_mode::boundary_footprint_with_orientation;
use generation_engine::GenerationEngine;
use manifest_generator::{
    CandidateFacts, FoundationManifest, ManifestGenerator, ManifestOrientation,
    ManifestOrientationSource, MaterialTable, MinecraftVersion, PlanInfo,
};
use shared_domain_types::{Boundary, Orientation, PlanId};
use sponge_export::{SchematicProfile, MINECRAFT_26_1_2_PROFILE};

use crate::data::ExportStage;
use crate::error::{
    ArtifactKind, ArtifactRecoveryError, ArtifactWriteError, BoundaryError, Error, Result,
    VersionError,
};
use crate::pipeline;
use crate::progress::ProgressTracker;
use crate::views::ExportProgressView;

static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 文件系统窄端口；F9 通过它写 staging、发布和恢复，测试可注入故障。
pub trait ExportFileSystem: Send + Sync {
    /// 创建受控输出目录。
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    /// 写入一个完整 staging 文件。
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// 在同一输出目录内移动文件。
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// 删除一个文件或 staging/backup 文件。
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// 查询路径类别；不存在时返回 None。
    fn kind(&self, path: &Path) -> io::Result<Option<ExportFileKind>>;
}

/// 文件系统路径类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFileKind {
    /// 普通文件。
    File,
    /// 目录。
    Directory,
}

/// 生产使用的标准文件系统实现。
#[derive(Debug, Default)]
pub struct StdExportFileSystem;

impl ExportFileSystem for StdExportFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        fs::write(path, contents)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn kind(&self, path: &Path) -> io::Result<Option<ExportFileKind>> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(ExportFileKind::File)),
            Ok(metadata) if metadata.is_dir() => Ok(Some(ExportFileKind::Directory)),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// F9 从组合根取得完整输入的稳定能力端口。
///
/// 实现可以读取方案正式状态、全局版本和默认输出位置，但这些读取发生在
/// F9 端口内部；S1 不读取中间状态，也不拼装 [`BoundaryExportRequest`]。
pub trait BoundaryExportInput: Send + Sync {
    /// 一次读取完整边界直出输入。
    fn load_request(&self) -> Result<BoundaryExportRequest>;
}

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
        reason = "the F9 request keeps all export inputs explicit at the stable capability port"
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

#[derive(Debug, Clone, Copy)]
struct ExportVersionContract {
    version: &'static str,
    material_version: MinecraftVersion,
    schematic_profile: SchematicProfile,
}

fn version_contract(
    request: &BoundaryExportRequest,
    material_table: &MaterialTable,
) -> Result<ExportVersionContract> {
    let requested = request.plan.minecraft_version.as_str();
    let contract = match requested {
        "26.1.2" => ExportVersionContract {
            version: "26.1.2",
            material_version: MinecraftVersion::V26_1_2,
            schematic_profile: MINECRAFT_26_1_2_PROFILE,
        },
        _ => {
            return Err(Error::Version(VersionError::Unsupported {
                requested: requested.to_owned(),
            }));
        }
    };
    if material_table.minecraft_version != contract.material_version {
        return Err(Error::Version(VersionError::MaterialTableMismatch {
            requested: requested.to_owned(),
            material_table: material_table.minecraft_version.to_string(),
        }));
    }
    if contract.schematic_profile.minecraft_version != contract.version {
        return Err(Error::Version(VersionError::SchematicProfileMismatch {
            requested: requested.to_owned(),
            data_version: contract.schematic_profile.data_version,
        }));
    }
    Ok(contract)
}

/// F9 内部完整导出用例；不向 S1 暴露 B18/B17/B4 的分段接口。
pub(crate) struct BoundaryExportUseCase {
    material_table: MaterialTable,
    file_system: Arc<dyn ExportFileSystem>,
}

impl BoundaryExportUseCase {
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
        let contract = version_contract(request, &self.material_table)?;
        self.material_table
            .validate_configured_blocks()
            .map_err(|error| {
                Error::Version(VersionError::InvalidMaterialTable {
                    version: self.material_table.minecraft_version.to_string(),
                    detail: error.to_string(),
                })
            })?;
        validate_targets(request, self.file_system.as_ref())?;

        // 默认值判定只在完整 F9 用例发生，S1 仅传入 Option<Orientation>。
        let (orientation, degree, source) = match request.orientation {
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

        // B5：验证每个 Polygon/MultiPolygon 外环，并按实际朝向计算完整覆盖范围。
        let footprint = boundary_footprint_with_orientation(boundary, orientation)
            .map_err(|error| Error::Boundary(BoundaryError::Invalid(error.to_string())))?;

        // B18：空候选时生成边界覆盖范围内的一层最小平整场地。
        progress.set_stage(ExportStage::Generating);
        progress.report_percent(5);
        let engine = GenerationEngine::new(self.material_table.clone());
        let model = engine.generate_flat_ground(footprint.width_blocks, footprint.length_blocks)?;
        progress.report_percent(35);

        // B17：不伪造候选投影、评审决定或保留候选，manifest 如实记录全为零。
        let actual_plan = PlanInfo::new(
            request.plan.campus_name.clone(),
            request.plan.plan_id,
            request.plan.plan_name.clone(),
            contract.version,
        );
        let manifest = ManifestGenerator::new()
            .generate_manifest_with_facts(
                &actual_plan,
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
                .ok_or_else(|| invalid_target("manifest 没有父目录"))?;
            let manifest_filename = staged_manifest
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid_target("manifest 临时文件名无效"))?;
            ManifestGenerator::new()
                .write_to_file_with(
                    &manifest,
                    manifest_parent,
                    manifest_filename,
                    |path, bytes| self.file_system.write(path, bytes),
                )
                .map_err(|error| Error::ManifestWrite(error.to_string()))?;
            progress.report_percent(65);

            // B4：复用 Sponge 编码器，经 F9 的受控文件端口写 staging。
            pipeline::export_schematic_staged_with_file_system(
                &model,
                &staged_schematic,
                &actual_plan.plan_name,
                contract.schematic_profile,
                self.file_system.as_ref(),
                progress,
            )?;
            progress.report_percent(90);

            publish_pair(
                self.file_system.as_ref(),
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
            if let Err(recovery) = cleanup_staging(
                self.file_system.as_ref(),
                [&staged_schematic, &staged_manifest],
            ) {
                return Err(Error::ArtifactRecovery(ArtifactRecoveryError::Failed {
                    primary: result
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "导出失败".to_owned()),
                    recovery,
                    paths: vec![staged_schematic, staged_manifest],
                }));
            }
        }
        result
    }
}

/// 后台边界导出操作；UI 只读取真实 F9 进度并轮询终态。
pub struct BoundaryExportOperation {
    progress: ProgressTracker,
    result: mpsc::Receiver<Result<BoundaryExportResult>>,
}

impl BoundaryExportOperation {
    /// 当前真实阶段/百分比对应的呈现数据。
    pub fn progress_view(&self) -> ExportProgressView {
        ExportProgressView::from_tracker(&self.progress)
    }

    /// 非阻塞取得后台终态；没有终态时返回 None。
    pub fn try_complete(&mut self) -> Option<Result<BoundaryExportResult>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::BackgroundTask)),
        }
    }
}

/// F9 稳定的完整导出能力端口；S1 只调用 [`Self::start`] 一次。
#[derive(Clone)]
pub struct BoundaryExportPort {
    use_case: Arc<BoundaryExportUseCase>,
    input: Arc<dyn BoundaryExportInput>,
    active: Arc<AtomicBool>,
}

impl BoundaryExportPort {
    pub(crate) fn new(
        use_case: Arc<BoundaryExportUseCase>,
        input: Arc<dyn BoundaryExportInput>,
    ) -> Self {
        Self {
            use_case,
            input,
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 提交一次最小开始意图；输入取得和完整导出均由 F9 端口拥有。
    pub fn start(&self) -> Result<BoundaryExportOperation> {
        if self.active.swap(true, Ordering::SeqCst) {
            return Err(Error::InvalidState("导出进行中，不接受新的导出请求"));
        }
        let progress = ProgressTracker::new();
        let worker_progress = progress.clone();
        let use_case = Arc::clone(&self.use_case);
        let input = Arc::clone(&self.input);
        let active = Arc::clone(&self.active);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = match input.load_request() {
                Ok(request) => use_case.export(&request, &worker_progress),
                Err(error) => {
                    worker_progress.fail();
                    Err(error)
                }
            };
            active.store(false, Ordering::SeqCst);
            let _send_result = sender.send(result);
        });
        Ok(BoundaryExportOperation {
            progress,
            result: receiver,
        })
    }
}

fn validate_targets(
    request: &BoundaryExportRequest,
    file_system: &dyn ExportFileSystem,
) -> Result<()> {
    if request.schematic_path.as_os_str().is_empty() || request.manifest_path.as_os_str().is_empty()
    {
        return Err(invalid_target("导出目标路径为空"));
    }
    if request.schematic_path == request.manifest_path {
        return Err(invalid_target(
            "Sponge 文件与 manifest 不能使用同一目标路径",
        ));
    }
    let schematic_parent = request
        .schematic_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid_target(".schem 没有父目录"))?;
    let manifest_parent = request
        .manifest_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid_target("manifest 没有父目录"))?;
    if schematic_parent != manifest_parent {
        return Err(invalid_target(
            ".schem 与 manifest 必须属于同一受控输出目录",
        ));
    }
    match file_system.kind(schematic_parent) {
        Ok(Some(ExportFileKind::Directory)) | Ok(None) => {}
        Ok(Some(ExportFileKind::File)) => return Err(invalid_target("导出目标父路径不是目录")),
        Err(error) => {
            return Err(invalid_target(format!(
                "无法检查导出目录 {}：{error}",
                schematic_parent.display()
            )));
        }
    }
    file_system
        .create_dir_all(schematic_parent)
        .map_err(|error| {
            invalid_target(format!(
                "无法创建导出目录 {}：{error}",
                schematic_parent.display()
            ))
        })
}

fn invalid_target(detail: impl Into<String>) -> Error {
    Error::ArtifactWrite(ArtifactWriteError::InvalidTarget {
        detail: detail.into(),
    })
}

fn staged_path(final_path: &Path, kind: &str) -> Result<PathBuf> {
    let parent = final_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid_target("导出目标没有父目录"))?;
    let filename = final_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_target("导出目标文件名无效"))?;
    let sequence = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{filename}.m1-{kind}-{}-{sequence}",
        std::process::id()
    )))
}

fn publish_pair(
    file_system: &dyn ExportFileSystem,
    staged_schematic: &Path,
    schematic: &Path,
    staged_manifest: &Path,
    manifest: &Path,
) -> Result<()> {
    let schematic_backup = backup_existing(file_system, schematic, ArtifactKind::Schematic)?;
    let manifest_backup = match backup_existing(file_system, manifest, ArtifactKind::Manifest) {
        Ok(backup) => backup,
        Err(primary) => {
            if let Err(recovery) = restore_backups(
                file_system,
                schematic,
                schematic_backup.as_ref(),
                manifest,
                None,
            ) {
                return Err(recovery_error(primary, recovery, [schematic, manifest]));
            }
            return Err(primary);
        }
    };

    let mut published_schematic = false;
    let mut published_manifest = false;
    let publish_result = (|| {
        file_system
            .rename(staged_schematic, schematic)
            .map_err(|error| publish_error(ArtifactKind::Schematic, schematic, error))?;
        published_schematic = true;
        file_system
            .rename(staged_manifest, manifest)
            .map_err(|error| publish_error(ArtifactKind::Manifest, manifest, error))?;
        published_manifest = true;
        Ok::<(), Error>(())
    })();

    if let Err(primary) = publish_result {
        let rollback = rollback_pair(
            file_system,
            schematic,
            published_schematic,
            schematic_backup.as_ref(),
            manifest,
            published_manifest,
            manifest_backup.as_ref(),
        );
        if let Err(recovery) = rollback {
            return Err(recovery_error(primary, recovery, [schematic, manifest]));
        }
        return Err(primary);
    }

    cleanup_backup(file_system, schematic_backup)?;
    cleanup_backup(file_system, manifest_backup)?;
    Ok(())
}

fn backup_existing(
    file_system: &dyn ExportFileSystem,
    final_path: &Path,
    artifact: ArtifactKind,
) -> Result<Option<PathBuf>> {
    match file_system.kind(final_path).map_err(|error| {
        invalid_target(format!(
            "无法检查既有 {artifact} 文件 {}：{error}",
            final_path.display()
        ))
    })? {
        None => Ok(None),
        Some(ExportFileKind::Directory) => Err(invalid_target(format!(
            "既有 {artifact} 目标不是文件：{}",
            final_path.display()
        ))),
        Some(ExportFileKind::File) => {
            let backup = staged_path(final_path, &format!("backup-{artifact}"))?;
            file_system
                .rename(final_path, &backup)
                .map_err(|error| publish_error(artifact, final_path, error))?;
            Ok(Some(backup))
        }
    }
}

fn publish_error(artifact: ArtifactKind, path: &Path, error: io::Error) -> Error {
    Error::ArtifactWrite(ArtifactWriteError::Publish {
        artifact,
        path: path.to_owned(),
        detail: error.to_string(),
    })
}

fn rollback_pair(
    file_system: &dyn ExportFileSystem,
    schematic: &Path,
    published_schematic: bool,
    schematic_backup: Option<&PathBuf>,
    manifest: &Path,
    published_manifest: bool,
    manifest_backup: Option<&PathBuf>,
) -> std::result::Result<(), String> {
    let mut diagnostics = Vec::new();
    if published_manifest {
        if let Err(error) = file_system.remove_file(manifest) {
            diagnostics.push(format!(
                "删除新 manifest {} 失败：{error}",
                manifest.display()
            ));
        }
    }
    if published_schematic {
        if let Err(error) = file_system.remove_file(schematic) {
            diagnostics.push(format!(
                "删除新 .schem {} 失败：{error}",
                schematic.display()
            ));
        }
    }
    if let Some(backup) = schematic_backup {
        if let Err(error) = file_system.rename(backup, schematic) {
            diagnostics.push(format!("恢复 .schem {} 失败：{error}", schematic.display()));
        }
    }
    if let Some(backup) = manifest_backup {
        if let Err(error) = file_system.rename(backup, manifest) {
            diagnostics.push(format!(
                "恢复 manifest {} 失败：{error}",
                manifest.display()
            ));
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics.join("; "))
    }
}

fn restore_backups(
    file_system: &dyn ExportFileSystem,
    schematic: &Path,
    schematic_backup: Option<&PathBuf>,
    manifest: &Path,
    manifest_backup: Option<&PathBuf>,
) -> std::result::Result<(), String> {
    let mut diagnostics = Vec::new();
    if let Some(backup) = schematic_backup {
        if let Err(error) = file_system.rename(backup, schematic) {
            diagnostics.push(format!("恢复 .schem 失败：{error}"));
        }
    }
    if let Some(backup) = manifest_backup {
        if let Err(error) = file_system.rename(backup, manifest) {
            diagnostics.push(format!("恢复 manifest 失败：{error}"));
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics.join("; "))
    }
}

fn cleanup_backup(file_system: &dyn ExportFileSystem, backup: Option<PathBuf>) -> Result<()> {
    let Some(backup) = backup else { return Ok(()) };
    match file_system.kind(&backup).map_err(|error| {
        Error::ArtifactWrite(ArtifactWriteError::Cleanup {
            path: backup.clone(),
            detail: error.to_string(),
        })
    })? {
        None => Ok(()),
        Some(ExportFileKind::File) => file_system.remove_file(&backup).map_err(|error| {
            Error::ArtifactWrite(ArtifactWriteError::Cleanup {
                path: backup,
                detail: error.to_string(),
            })
        }),
        Some(ExportFileKind::Directory) => Err(Error::ArtifactWrite(ArtifactWriteError::Cleanup {
            path: backup,
            detail: "备份路径意外是目录".to_owned(),
        })),
    }
}

fn cleanup_staging<const N: usize>(
    file_system: &dyn ExportFileSystem,
    paths: [&Path; N],
) -> std::result::Result<(), String> {
    let mut diagnostics = Vec::new();
    for path in paths {
        match file_system.kind(path) {
            Ok(Some(ExportFileKind::File)) => {
                if let Err(error) = file_system.remove_file(path) {
                    diagnostics.push(format!("清理 staging {} 失败：{error}", path.display()));
                }
            }
            Ok(Some(ExportFileKind::Directory)) => {
                diagnostics.push(format!("清理 staging {} 失败：路径是目录", path.display()))
            }
            Ok(None) => {}
            Err(error) => {
                diagnostics.push(format!("检查 staging {} 失败：{error}", path.display()))
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics.join("; "))
    }
}

fn recovery_error<const N: usize>(primary: Error, recovery: String, paths: [&Path; N]) -> Error {
    Error::ArtifactRecovery(ArtifactRecoveryError::Failed {
        primary: primary.to_string(),
        recovery,
        paths: paths.into_iter().map(Path::to_owned).collect(),
    })
}
