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
mod tests;
