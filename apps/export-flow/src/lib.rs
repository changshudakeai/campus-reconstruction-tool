//! F9 boundary-only application flow.
//!
//! This crate is outside the S1 presentation shell. It owns formal export-input
//! acquisition, immutable request assembly, and submission to the F9 port.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use export_console::{
    BoundaryError, BoundaryExportInput, BoundaryExportPort, BoundaryExportRequest,
};
pub use export_console::{
    BoundaryExportOperation, BoundaryExportResult, Error, ExportFileKind, ExportFileSystem,
    ExportProgressView, Result, StdExportFileSystem,
};
use global_settings::SettingsManager;
use project_management::PlanContextView;
use shared_domain_types::{Boundary, Orientation, PlanId};

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
        Ok(BoundaryExportRequest::new(
            snapshot.campus_name,
            plan_id,
            snapshot.plan_name,
            minecraft_version,
            snapshot.boundary,
            snapshot.boundary_confirmed,
            orientation,
            export_location.join(format!("{plan_stem}.schem")),
            export_location.join(format!("{plan_stem}.foundation_manifest.json")),
        ))
    }
}

/// Complete F9 boundary-only export entry. The implementation is intentionally outside S1.
#[derive(Clone)]
pub struct BoundaryExportFlow {
    input: ExportInputStore,
    port: BoundaryExportPort,
}

impl BoundaryExportFlow {
    pub fn new(file_system: Arc<dyn ExportFileSystem>) -> Self {
        let input = ExportInputStore::default();
        let port =
            BoundaryExportPort::new_boundary_only_v26_1_2(Arc::new(input.clone()), file_system);
        Self { input, port }
    }

    pub fn start(&self) -> Result<BoundaryExportOperation> {
        self.port.start()
    }

    pub fn sync_settings(&self, settings: &SettingsManager) {
        self.input.sync_settings(settings);
    }

    pub fn set_plan(&self, context: &PlanContextView) {
        self.port.expire_active();
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
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};

    use data_persistence::Database;
    use global_settings::SettingsManager;

    use super::*;

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
