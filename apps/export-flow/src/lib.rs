//! F9 boundary-only application flow.
//!
//! This crate is outside the S1 presentation shell. It owns formal export-input
//! acquisition, immutable request assembly, and submission to the F9 port.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use data_persistence::Database;
use export_console::{
    BoundaryError, BoundaryExportInput, BoundaryExportPort, BoundaryExportRequest,
    EnhancedExportInput, EnhancedExportPort, EnhancedExportRequest, ExportArtifactTargets,
    ExportPlanContext, ExportPlanState,
};
pub use export_console::{
    BoundaryExportOperation, BoundaryExportResult, Error, ExportFileKind, ExportFileSystem,
    ExportProgressView, Result, StdExportFileSystem,
};
use global_settings::SettingsManager;
use project_management::PlanContextView;
use shared_domain_types::{Boundary, CandidateCategory, Orientation, PlanId};

mod candidates;

use candidates::ExportCandidateStore;

/// 边界直出与增强导出共用的完整输入快照（Start 前冻结）。
struct FrozenExportInput {
    plan_key: String,
    context: ExportPlanContext,
    state: ExportPlanState,
    targets: ExportArtifactTargets,
}

/// Boundary data needed by the map presentation; the formal F9 request stays private to this flow.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryView {
    pub r#type: String,
    pub coordinates: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
struct ExportInputSnapshot {
    plan_id: Option<String>,
    campus_name: String,
    plan_name: String,
    boundary: Option<Boundary>,
    boundary_confirmed: bool,
    settings_error: Option<String>,
    orientation: Option<f32>,
    minecraft_version: Option<String>,
    export_location: Option<PathBuf>,
    plans: HashMap<String, PlanExportSnapshot>,
}

#[derive(Debug, Clone, Default)]
struct PlanExportSnapshot {
    campus_name: String,
    plan_name: String,
    boundary: Option<Boundary>,
    boundary_confirmed: bool,
    orientation: Option<f32>,
}

#[derive(Clone, Default)]
struct ExportInputStore {
    snapshot: Arc<Mutex<ExportInputSnapshot>>,
    candidate_store: Option<Arc<ExportCandidateStore>>,
}

impl ExportInputStore {
    fn sync_settings(&self, settings: &SettingsManager) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        match (settings.settings(), settings.default_export_location()) {
            (Ok(settings), Ok(location)) => {
                snapshot.settings_error = None;
                snapshot.minecraft_version = Some(settings.minecraft_version);
                snapshot.export_location = Some(PathBuf::from(location));
            }
            (Err(error), _) | (_, Err(error)) => {
                snapshot.settings_error = Some(error.to_string());
                snapshot.minecraft_version = None;
                snapshot.export_location = None;
            }
        }
    }

    fn set_plan(&self, context: &PlanContextView) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        if snapshot.plan_id.as_deref() != Some(context.plan_id.as_str()) {
            snapshot.save_current_plan();
        }
        snapshot.plan_id = Some(context.plan_id.clone());
        if let Some(previous) = snapshot.plans.get(&context.plan_id).cloned() {
            snapshot.campus_name = previous.campus_name;
            snapshot.plan_name = previous.plan_name;
            snapshot.boundary = previous.boundary;
            snapshot.boundary_confirmed = previous.boundary_confirmed;
            snapshot.orientation = previous.orientation;
        } else {
            snapshot.campus_name = context.campus_name.clone();
            snapshot.plan_name = context.plan_name.clone();
            snapshot.boundary = None;
            snapshot.boundary_confirmed = false;
            snapshot.orientation = None;
        }
    }

    fn set_boundary(&self, boundary: Option<Boundary>, confirmed: bool) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        snapshot.boundary = boundary;
        snapshot.boundary_confirmed = confirmed;
        snapshot.save_current_plan();
    }

    fn set_orientation(&self, orientation: Option<f32>) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        snapshot.orientation = orientation;
        snapshot.save_current_plan();
    }

    fn plan_boundary_confirmed(&self, plan_id: &str) -> bool {
        let snapshot = self.snapshot.lock().expect("export input snapshot lock");
        if snapshot.plan_id.as_deref() == Some(plan_id) {
            return snapshot.boundary_confirmed;
        }
        snapshot
            .plans
            .get(plan_id)
            .is_some_and(|plan| plan.boundary_confirmed)
    }
}

impl ExportInputSnapshot {
    fn save_current_plan(&mut self) {
        let Some(plan_id) = self.plan_id.clone() else {
            return;
        };
        self.plans.insert(
            plan_id,
            PlanExportSnapshot {
                campus_name: self.campus_name.clone(),
                plan_name: self.plan_name.clone(),
                boundary: self.boundary.clone(),
                boundary_confirmed: self.boundary_confirmed,
                orientation: self.orientation,
            },
        );
    }
}

impl BoundaryExportInput for ExportInputStore {
    fn load_request(&self) -> Result<BoundaryExportRequest> {
        let frozen = self.freeze_input()?;
        Ok(BoundaryExportRequest::new(
            frozen.context,
            frozen.state,
            frozen.targets,
        ))
    }
}

impl EnhancedExportInput for ExportInputStore {
    fn load_request(&self) -> Result<EnhancedExportRequest> {
        let store = self
            .candidate_store
            .as_ref()
            .ok_or(Error::InvalidState("增强导出未配置候选存储（B2 连接）"))?;
        let frozen = self.freeze_input()?;
        let summary = store.seal_summary(&frozen.plan_key)?;
        let kept_candidate_ids = store.kept_candidate_ids(&frozen.plan_key)?;
        Ok(EnhancedExportRequest::new(
            frozen.context,
            frozen.state,
            frozen.targets,
            summary,
            kept_candidate_ids,
        ))
    }
}

impl ExportInputStore {
    fn freeze_input(&self) -> Result<FrozenExportInput> {
        let snapshot = self
            .snapshot
            .lock()
            .expect("export input snapshot lock")
            .clone();
        if let Some(error) = snapshot.settings_error {
            return Err(Error::SettingsRead(error));
        }
        let Some(plan_id_text) = snapshot.plan_id else {
            return Err(Error::Boundary(BoundaryError::Missing));
        };
        let plan_id =
            PlanId::parse(&plan_id_text).map_err(|error| Error::BadPlanId(error.to_string()))?;
        let orientation = match snapshot.orientation {
            Some(degree) => Some(Orientation::new(degree).ok_or_else(|| {
                Error::Boundary(BoundaryError::Invalid(
                    "orientation is outside the supported range".to_owned(),
                ))
            })?),
            None => None,
        };
        let Some(minecraft_version) = snapshot.minecraft_version else {
            return Err(Error::SettingsRead(
                "Minecraft version setting is unavailable".to_owned(),
            ));
        };
        let Some(export_location) = snapshot.export_location else {
            return Err(Error::SettingsRead(
                "export location setting is unavailable".to_owned(),
            ));
        };
        let plan_stem = plan_id.to_string();
        Ok(FrozenExportInput {
            plan_key: plan_stem.clone(),
            context: ExportPlanContext::new(
                snapshot.campus_name,
                plan_id,
                snapshot.plan_name,
                minecraft_version,
            ),
            state: ExportPlanState::new(
                snapshot.boundary,
                snapshot.boundary_confirmed,
                orientation,
            ),
            targets: ExportArtifactTargets::new(
                export_location.join(format!("{plan_stem}.schem")),
                export_location.join(format!("{plan_stem}.foundation_manifest.json")),
            ),
        })
    }

    fn active_plan_id(&self) -> Option<String> {
        self.snapshot
            .lock()
            .expect("export input snapshot lock")
            .plan_id
            .clone()
    }
}

/// 增强导出的呈现提示（S1 只显示，不做业务判断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedExportHint {
    /// 本次实际保留候选数量。
    pub keep_total: usize,
    /// 保留候选按类别计数（仅列出保留数 > 0 的类别）。
    pub keep_by_category: Vec<(CandidateCategory, usize)>,
    /// 待定项数（如实报数，不导出）。
    pub pending_count: usize,
    /// 剔除项数（如实报数，不导出）。
    pub remove_count: usize,
}

/// Complete F9 boundary-only export entry. The implementation is intentionally outside S1.
#[derive(Clone)]
pub struct BoundaryExportFlow {
    input: ExportInputStore,
    port: BoundaryExportPort,
    enhanced_port: Option<EnhancedExportPort>,
    candidate_store: Option<Arc<ExportCandidateStore>>,
}

impl BoundaryExportFlow {
    pub fn new(file_system: Arc<dyn ExportFileSystem>) -> Self {
        let input = ExportInputStore::default();
        let port =
            BoundaryExportPort::new_boundary_only_v26_1_2(Arc::new(input.clone()), file_system);
        Self {
            input,
            port,
            enhanced_port: None,
            candidate_store: None,
        }
    }

    /// 生产构造：与壳内共享 B2 连接组一起启用增强导出入口。
    pub fn new_with_candidate_store(
        file_system: Arc<dyn ExportFileSystem>,
        db: Arc<Mutex<Database>>,
    ) -> Self {
        let candidate_store = Arc::new(ExportCandidateStore::new(db));
        let input = ExportInputStore {
            candidate_store: Some(Arc::clone(&candidate_store)),
            ..ExportInputStore::default()
        };
        let boundary_port = BoundaryExportPort::new_boundary_only_v26_1_2(
            Arc::new(input.clone()),
            file_system.clone(),
        );
        let enhanced_port = EnhancedExportPort::new_enhanced_v26_1_2(
            Arc::new(input.clone()),
            Arc::clone(&candidate_store) as Arc<dyn export_console::CandidateExportReader>,
            file_system,
        );
        Self {
            input,
            port: boundary_port,
            enhanced_port: Some(enhanced_port),
            candidate_store: Some(candidate_store),
        }
    }

    /// S1 只调用一次开始意图；A2 依据 B2 封账事实在内部路由：
    /// 存在已封账保留候选 → 增强导出；否则保持 M1 边界直出。
    pub fn start(&self) -> Result<BoundaryExportOperation> {
        if let Some(enhanced_port) = &self.enhanced_port {
            let enhanced_available = match &self.candidate_store {
                Some(store) => {
                    let Some(plan_id) = self.input.active_plan_id() else {
                        return self.port.start();
                    };
                    store
                        .seal_summary(&plan_id)
                        .map(|summary| summary.keep_total > 0)?
                }
                None => false,
            };
            if enhanced_available {
                return enhanced_port.start();
            }
        }
        self.port.start()
    }

    /// 增强导出的独立完整操作入口（直接可测；生产仍经 [`Self::start`] 路由）。
    pub fn start_enhanced(&self) -> Result<BoundaryExportOperation> {
        let enhanced_port = self
            .enhanced_port
            .as_ref()
            .ok_or(Error::InvalidState("增强导出未配置候选存储（B2 连接）"))?;
        enhanced_port.start()
    }

    pub fn sync_settings(&self, settings: &SettingsManager) {
        self.input.sync_settings(settings);
    }

    pub fn set_plan(&self, context: &PlanContextView) {
        self.port.expire_active();
        if let Some(enhanced_port) = &self.enhanced_port {
            enhanced_port.expire_active();
        }
        self.input.set_plan(context);
    }

    /// Submit the user's confirmed map geometry; F9 owns conversion to its formal Boundary.
    pub fn confirm_boundary(
        &self,
        boundary_type: impl Into<String>,
        coordinates: serde_json::Value,
    ) {
        self.input.set_boundary(
            Some(Boundary {
                r#type: boundary_type.into(),
                coordinates,
            }),
            true,
        );
    }

    pub fn reset_boundary(&self) {
        self.input.set_boundary(None, false);
    }

    pub fn set_boundary(&self, boundary: Option<Boundary>, confirmed: bool) {
        self.input.set_boundary(boundary, confirmed);
    }

    pub fn set_orientation(&self, orientation: Option<f32>) {
        self.input.set_orientation(orientation);
    }

    pub fn boundary_confirmed(&self) -> bool {
        self.input
            .snapshot
            .lock()
            .expect("export input snapshot lock")
            .boundary_confirmed
    }

    pub fn plan_boundary_confirmed(&self, plan_id: &str) -> bool {
        self.input.plan_boundary_confirmed(plan_id)
    }

    pub fn boundary_view(&self) -> Option<BoundaryView> {
        self.input
            .snapshot
            .lock()
            .expect("export input snapshot lock")
            .boundary
            .as_ref()
            .map(|boundary| BoundaryView {
                r#type: boundary.r#type.clone(),
                coordinates: boundary.coordinates.clone(),
            })
    }

    /// Expire the current operation when its presentation context is left.
    pub fn leave(&self) {
        self.port.expire_active();
        if let Some(enhanced_port) = &self.enhanced_port {
            enhanced_port.expire_active();
        }
    }

    /// 导出页呈现提示：存在已封账保留候选时返回增强内容（S1 只显示）。
    pub fn enhanced_hint(&self) -> Result<Option<EnhancedExportHint>> {
        let Some(store) = &self.candidate_store else {
            return Ok(None);
        };
        let Some(plan_id) = self.input.active_plan_id() else {
            return Ok(None);
        };
        let summary = store.seal_summary(&plan_id)?;
        if summary.keep_total == 0 {
            return Ok(None);
        }
        Ok(Some(EnhancedExportHint {
            keep_total: summary.keep_total,
            keep_by_category: summary.keep_by_category,
            pending_count: summary.pending_count,
            remove_count: summary.remove_count,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};

    use data_persistence::{
        CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
        CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
        ReviewDecision, ReviewDecisionsApi,
    };
    use global_settings::SettingsManager;
    use manifest_generator::ExportKind;
    use shared_domain_types::{CandidateCategory, ReviewState};

    use super::*;

    /// 种子：2 栋可评审建筑 + 1 待定 + 1 剔除 + 1 隔离投影，并封账写回。
    fn seed_plan_with_sealed_review(
        db: &mut Database,
        plan_id: &PlanId,
        include_isolated: bool,
    ) -> Vec<String> {
        let plan_key = plan_id.to_string();
        let observations = vec![
            RawObservation::new(
                &plan_key,
                CandidateCategory::Building,
                "way/b0",
                serde_json::json!({ "tags": { "name": "教学楼A" } }),
                "overpass",
            ),
            RawObservation::new(
                &plan_key,
                CandidateCategory::Building,
                "way/b1",
                serde_json::json!({ "tags": { "name": "教学楼B" } }),
                "overpass",
            ),
            RawObservation::new(
                &plan_key,
                CandidateCategory::Road,
                "way/r0",
                serde_json::json!({ "tags": { "name": "待定道路" } }),
                "overpass",
            ),
            RawObservation::new(
                &plan_key,
                CandidateCategory::Water,
                "way/w0",
                serde_json::json!({ "tags": { "name": "剔除水域" } }),
                "overpass",
            ),
        ];
        db.write_raw_observations(&observations)
            .expect("写入原始观测");
        let batch = db.prepare_candidate_batch(&plan_key).expect("准备候选批次");
        let mut projections = Vec::new();
        let mut reviewable = Vec::new();
        for observation in &observations {
            let candidate_id = format!("overpass:{}:outer", observation.entity_id);
            let display = if observation.entity_type == CandidateCategory::Building {
                CandidateDisplay::new(
                    observation.source_data["tags"]["name"]
                        .as_str()
                        .unwrap_or(&observation.entity_id),
                    vec![("height".to_owned(), "12".to_owned())],
                )
            } else {
                CandidateDisplay::new(
                    observation.source_data["tags"]["name"]
                        .as_str()
                        .unwrap_or(&observation.entity_id),
                    vec![],
                )
            };
            projections.push(CandidateProjection::new(
                &candidate_id,
                &plan_key,
                &observation.id,
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
                observation.entity_type,
                display,
                CandidateShape::polygon(serde_json::json!([
                    [116.0001, 39.0001],
                    [116.0005, 39.0001],
                    [116.0005, 39.0005],
                    [116.0001, 39.0001]
                ])),
                CandidateValidation::Retained,
                CandidateEligibility::Reviewable,
            ));
            reviewable.push(candidate_id);
        }
        if include_isolated {
            projections.push(
                CandidateProjection::new(
                    "overpass:way/iso:outer",
                    &plan_key,
                    "raw-iso",
                    "overpass",
                    "way/iso",
                    "default",
                    CandidateCategory::Building,
                    CandidateDisplay::new("隔离建筑", vec![]),
                    CandidateShape::point(serde_json::json!([])),
                    CandidateValidation::Rejected,
                    CandidateEligibility::Isolated,
                )
                .isolated_reason("missing_source_geometry"),
            );
        }
        db.write_candidate_projections(&batch.id, &projections)
            .expect("写入候选投影");
        db.publish_candidate_batch(&batch.id).expect("发布候选批次");

        let decisions = vec![
            ReviewDecision::new(
                &plan_key,
                CandidateCategory::Building,
                &reviewable[0],
                ReviewState::Keep,
            ),
            ReviewDecision::new(
                &plan_key,
                CandidateCategory::Building,
                &reviewable[1],
                ReviewState::Keep,
            ),
            ReviewDecision::new(
                &plan_key,
                CandidateCategory::Road,
                &reviewable[2],
                ReviewState::Pending,
            ),
            ReviewDecision::new(
                &plan_key,
                CandidateCategory::Water,
                &reviewable[3],
                ReviewState::Remove,
            ),
        ];
        db.batch_update_review_decisions(&decisions)
            .expect("封账写回");
        reviewable
    }

    fn settings_with_export_dir(dir: &Path) -> SettingsManager {
        let mut settings = SettingsManager::new(Database::open_in_memory().expect("测试设置库"));
        settings
            .set_default_export_location(dir.to_str().expect("临时路径"))
            .expect("设置导出目录");
        settings
    }

    fn confirmed_plan_context(plan_id: &PlanId) -> PlanContextView {
        PlanContextView {
            plan_id: plan_id.to_string(),
            plan_name: "增强导出方案".to_owned(),
            campus_id: "campus-m4".to_owned(),
            campus_name: "M4 校区".to_owned(),
            anchor_lng: 116.4,
            anchor_lat: 39.9,
        }
    }

    #[test]
    fn start_routes_to_enhanced_export_when_sealed_keep_candidates_exist() {
        let directory = tempfile::tempdir().expect("导出目录");
        let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
        let plan_id = PlanId::generate();
        let kept = seed_plan_with_sealed_review(&mut db.lock().expect("db"), &plan_id, true);
        assert_eq!(kept.len(), 4);

        let flow = BoundaryExportFlow::new_with_candidate_store(
            Arc::new(StdExportFileSystem),
            Arc::clone(&db),
        );
        flow.sync_settings(&settings_with_export_dir(directory.path()));
        flow.set_plan(&confirmed_plan_context(&plan_id));
        flow.confirm_boundary(
            "Polygon",
            serde_json::json!([[
                [116.0000, 39.0000],
                [116.0010, 39.0000],
                [116.0010, 39.0010],
                [116.0000, 39.0010],
                [116.0000, 39.0000]
            ]]),
        );

        let hint = flow
            .enhanced_hint()
            .expect("呈现提示可读")
            .expect("保留候选存在");
        assert_eq!(hint.keep_total, 2);
        assert_eq!(hint.pending_count, 1);
        assert_eq!(hint.remove_count, 1);

        let mut operation = flow.start().expect("Start 路由到增强导出");
        let result = wait_for_result(&mut operation).expect("增强导出成功");
        assert!(result.schematic_path.is_file());
        let inspection =
            sponge_export::inspect_schematic(&result.schematic_path).expect(".schem 可解析");
        assert!(inspection.dimensions[1] > 1, "保留建筑必须产生高度内容");
        let manifest = manifest_generator::FoundationManifest::from_json(
            &std::fs::read_to_string(&result.manifest_path).expect("manifest 可读"),
        )
        .expect("manifest 有效");
        assert_eq!(manifest.export_kind, ExportKind::Enhanced);
        assert_eq!(manifest.candidate_facts.retained_candidate_count, 2);
        assert_eq!(manifest.candidate_facts.review_decision_count, 4);
        assert_eq!(manifest.candidate_facts.candidate_projection_count, 4);
        let buildings = manifest
            .candidate_facts
            .keep_by_category
            .iter()
            .find(|entry| entry.category == "Building")
            .expect("建筑类别计数");
        assert_eq!(buildings.count, 2);

        // 封账只来自评审写回，导出路径不得新增或伪造封账记录。
        let decisions = db
            .lock()
            .expect("db")
            .list_review_decisions(&plan_id.to_string())
            .expect("读评审决定");
        assert_eq!(decisions.len(), 4);
    }

    #[test]
    fn start_enhanced_is_an_independent_complete_entry() {
        let directory = tempfile::tempdir().expect("导出目录");
        let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
        let plan_id = PlanId::generate();
        seed_plan_with_sealed_review(&mut db.lock().expect("db"), &plan_id, false);

        let flow = BoundaryExportFlow::new_with_candidate_store(
            Arc::new(StdExportFileSystem),
            Arc::clone(&db),
        );
        flow.sync_settings(&settings_with_export_dir(directory.path()));
        flow.set_plan(&confirmed_plan_context(&plan_id));
        flow.confirm_boundary(
            "Polygon",
            serde_json::json!([[
                [116.0000, 39.0000],
                [116.0010, 39.0000],
                [116.0010, 39.0010],
                [116.0000, 39.0010],
                [116.0000, 39.0000]
            ]]),
        );

        let mut operation = flow.start_enhanced().expect("独立增强入口");
        let result = wait_for_result(&mut operation).expect("增强导出成功");
        assert!(result.manifest_path.is_file());
        let manifest = manifest_generator::FoundationManifest::from_json(
            &std::fs::read_to_string(&result.manifest_path).expect("manifest 可读"),
        )
        .expect("manifest 有效");
        assert_eq!(manifest.export_kind, ExportKind::Enhanced);
        assert_eq!(manifest.candidate_facts.retained_candidate_count, 2);
    }

    #[test]
    fn base_export_without_seal_keeps_boundary_only_and_creates_no_empty_seal() {
        let directory = tempfile::tempdir().expect("导出目录");
        let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
        let plan_id = PlanId::generate();

        let flow = BoundaryExportFlow::new_with_candidate_store(
            Arc::new(StdExportFileSystem),
            Arc::clone(&db),
        );
        flow.sync_settings(&settings_with_export_dir(directory.path()));
        flow.set_plan(&confirmed_plan_context(&plan_id));
        flow.confirm_boundary(
            "Polygon",
            serde_json::json!([[
                [116.0000, 39.0000],
                [116.0010, 39.0000],
                [116.0010, 39.0010],
                [116.0000, 39.0010],
                [116.0000, 39.0000]
            ]]),
        );
        assert!(flow.enhanced_hint().expect("提示可读").is_none());

        let mut operation = flow.start().expect("无保留候选时走边界直出");
        let result = wait_for_result(&mut operation).expect("基础导出成功");
        let inspection =
            sponge_export::inspect_schematic(&result.schematic_path).expect(".schem 可解析");
        assert_eq!(inspection.dimensions[1], 1, "基础导出只有一层平整场地");
        let manifest = manifest_generator::FoundationManifest::from_json(
            &std::fs::read_to_string(&result.manifest_path).expect("manifest 可读"),
        )
        .expect("manifest 有效");
        assert_eq!(manifest.export_kind, ExportKind::Base);
        assert_eq!(
            manifest.candidate_facts,
            manifest_generator::CandidateFacts::default()
        );

        let decisions = db
            .lock()
            .expect("db")
            .list_review_decisions(&plan_id.to_string())
            .expect("读评审决定");
        assert!(decisions.is_empty(), "基础导出不得制造空封账记录");
    }

    #[derive(Clone)]
    struct BlockingManifestFileSystem {
        manifest_started: Arc<(Mutex<bool>, Condvar)>,
        release_manifest: Arc<(Mutex<bool>, Condvar)>,
    }

    impl BlockingManifestFileSystem {
        fn wait_for_manifest(&self) {
            let (lock, signal) = &*self.manifest_started;
            let mut started = lock.lock().expect("manifest start lock");
            while !*started {
                started = signal.wait(started).expect("manifest start wait");
            }
        }

        fn release_manifest(&self) {
            let (lock, signal) = &*self.release_manifest;
            *lock.lock().expect("manifest release lock") = true;
            signal.notify_one();
        }
    }

    impl ExportFileSystem for BlockingManifestFileSystem {
        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            std::fs::create_dir_all(path)
        }

        fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".m1-manifest-"))
            {
                let (lock, signal) = &*self.manifest_started;
                *lock.lock().expect("manifest start lock") = true;
                signal.notify_one();

                let (lock, signal) = &*self.release_manifest;
                let mut released = lock.lock().expect("manifest release lock");
                while !*released {
                    released = signal.wait(released).expect("manifest release wait");
                }
            }
            std::fs::write(path, contents)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            std::fs::rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            std::fs::remove_file(path)
        }

        fn kind(&self, path: &Path) -> io::Result<Option<ExportFileKind>> {
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() => Ok(Some(ExportFileKind::File)),
                Ok(metadata) if metadata.is_dir() => Ok(Some(ExportFileKind::Directory)),
                Ok(_) => Ok(None),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
        }
    }

    #[test]
    fn start_freezes_boundary_before_reset_after_start() {
        let initial_dir = tempfile::tempdir().expect("initial export directory");
        let plan_id = PlanId::generate();
        let plan = PlanContextView {
            plan_id: plan_id.to_string(),
            plan_name: "冻结请求测试方案".to_owned(),
            campus_id: "freeze-campus".to_owned(),
            campus_name: "冻结请求测试校区".to_owned(),
            anchor_lng: 116.4,
            anchor_lat: 39.9,
        };

        let file_system = Arc::new(BlockingManifestFileSystem {
            manifest_started: Arc::new((Mutex::new(false), Condvar::new())),
            release_manifest: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let flow = BoundaryExportFlow::new(file_system.clone());
        let mut settings =
            SettingsManager::new(Database::open_in_memory().expect("打开测试设置库"));
        settings
            .set_default_export_location(initial_dir.path().to_str().expect("temporary path"))
            .expect("设置导出目录");
        flow.sync_settings(&settings);
        flow.set_plan(&plan);
        flow.confirm_boundary(
            "Polygon",
            serde_json::json!([[
                [116.4000, 39.9000],
                [116.4010, 39.9000],
                [116.4010, 39.9010],
                [116.4000, 39.9010],
                [116.4000, 39.9000]
            ]]),
        );
        let mut operation = flow.start().expect("Start 应立即提交后台操作");

        file_system.wait_for_manifest();
        flow.reset_boundary();
        file_system.release_manifest();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let result = loop {
            if let Some(result) = operation.try_complete() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline, "冻结请求未完成");
            std::thread::yield_now();
        };
        let result = result.expect("Start 前冻结的边界仍应完成导出");
        assert!(result.schematic_path.is_file());
        assert!(result.manifest_path.is_file());
    }

    #[test]
    fn missing_settings_are_reported_through_the_public_flow() {
        let flow = BoundaryExportFlow::new(Arc::new(StdExportFileSystem));
        flow.set_plan(&PlanContextView {
            plan_id: PlanId::generate().to_string(),
            plan_name: "璁剧疆璇锋眰娴嬭瘯鏂规".to_owned(),
            campus_id: "settings-campus".to_owned(),
            campus_name: "璁剧疆璇锋眰娴嬭瘯鏍″尯".to_owned(),
            anchor_lng: 116.4,
            anchor_lat: 39.9,
        });

        let error = match flow.start() {
            Ok(_) => panic!("F9 must preserve an unavailable settings error"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::SettingsRead(detail) if detail.contains("unavailable")));
    }

    #[test]
    fn switching_plans_restores_latest_boundary_and_expires_old_result() {
        let directory = tempfile::tempdir().expect("export directory");
        let file_system = Arc::new(BlockingManifestFileSystem {
            manifest_started: Arc::new((Mutex::new(false), Condvar::new())),
            release_manifest: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let flow = BoundaryExportFlow::new(file_system.clone());
        let mut settings =
            SettingsManager::new(Database::open_in_memory().expect("打开测试设置库"));
        settings
            .set_default_export_location(directory.path().to_str().expect("temporary path"))
            .expect("设置导出目录");
        flow.sync_settings(&settings);

        let plan_a = PlanContextView {
            plan_id: PlanId::generate().to_string(),
            plan_name: "方案 A".to_owned(),
            campus_id: "campus-a".to_owned(),
            campus_name: "校区".to_owned(),
            anchor_lng: 116.4,
            anchor_lat: 39.9,
        };
        let plan_b = PlanContextView {
            plan_id: PlanId::generate().to_string(),
            plan_name: "方案 B".to_owned(),
            ..plan_a.clone()
        };
        let boundary_a = Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [116.4000, 39.9000],
                [116.4010, 39.9000],
                [116.4010, 39.9010],
                [116.4000, 39.9010],
                [116.4000, 39.9000]
            ]]),
        };

        flow.set_plan(&plan_a);
        flow.set_boundary(Some(boundary_a), true);
        let mut old_operation = flow.start().expect("start plan A export");
        file_system.wait_for_manifest();

        flow.set_plan(&plan_b);
        file_system.release_manifest();
        let old_result = wait_for_result(&mut old_operation);
        assert!(
            matches!(old_result, Err(Error::InvalidState(_))),
            "离开方案后旧结果必须过期，不能交付到方案 B"
        );

        flow.set_plan(&plan_a);
        let mut restored_operation = flow.start().expect("reopen plan A export");
        let restored_result = wait_for_result(&mut restored_operation);
        assert!(
            restored_result.is_ok(),
            "方案 A 的最新确认边界必须可直接导出"
        );
        assert!(directory
            .path()
            .join(format!("{}.schem", plan_a.plan_id))
            .is_file());
    }

    fn wait_for_result(operation: &mut BoundaryExportOperation) -> Result<BoundaryExportResult> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(result) = operation.try_complete() {
                return result;
            }
            assert!(std::time::Instant::now() < deadline, "导出操作未达到终态");
            std::thread::yield_now();
        }
    }
}
