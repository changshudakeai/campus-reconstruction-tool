//! F9 boundary-only application flow.
//!
//! This crate is outside the S1 presentation shell. It owns formal export-input
//! acquisition, immutable request assembly, and submission to the F9 port.

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
        snapshot.plan_id = Some(context.plan_id.clone());
        snapshot.campus_name = context.campus_name.clone();
        snapshot.plan_name = context.plan_name.clone();
        snapshot.boundary = None;
        snapshot.boundary_confirmed = false;
        snapshot.orientation = None;
    }

    fn set_boundary(&self, boundary: Option<Boundary>, confirmed: bool) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        snapshot.boundary = boundary;
        snapshot.boundary_confirmed = confirmed;
    }

    fn set_orientation(&self, orientation: Option<f32>) {
        let mut snapshot = self.snapshot.lock().expect("export input snapshot lock");
        snapshot.orientation = orientation;
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
        self.input.set_plan(context);
    }

    pub fn set_boundary(&self, boundary: Option<Boundary>, confirmed: bool) {
        self.input.set_boundary(boundary, confirmed);
    }

    pub fn set_orientation(&self, orientation: Option<f32>) {
        self.input.set_orientation(orientation);
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};

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
        let input = ExportInputStore::default();
        let plan_id = PlanId::generate();
        let initial_boundary = Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [116.4000, 39.9000],
                [116.4010, 39.9000],
                [116.4010, 39.9010],
                [116.4000, 39.9010],
                [116.4000, 39.9000]
            ]]),
        };
        {
            let mut snapshot = input.snapshot.lock().expect("export input snapshot lock");
            snapshot.plan_id = Some(plan_id.to_string());
            snapshot.campus_name = "冻结测试校区".to_owned();
            snapshot.plan_name = "冻结测试方案".to_owned();
            snapshot.boundary = Some(initial_boundary);
            snapshot.boundary_confirmed = true;
            snapshot.minecraft_version = Some("26.1.2".to_owned());
            snapshot.export_location = Some(initial_dir.path().to_owned());
        }

        let file_system = Arc::new(BlockingManifestFileSystem {
            manifest_started: Arc::new((Mutex::new(false), Condvar::new())),
            release_manifest: Arc::new((Mutex::new(false), Condvar::new())),
        });
        let port = BoundaryExportPort::new_boundary_only_v26_1_2(
            Arc::new(input.clone()),
            file_system.clone(),
        );
        let mut operation = port.start().expect("Start 应立即提交后台操作");

        file_system.wait_for_manifest();
        input.set_boundary(None, false);
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
    fn settings_read_error_is_not_replaced_by_export_defaults() {
        let input = ExportInputStore::default();
        input
            .snapshot
            .lock()
            .expect("export input snapshot lock")
            .settings_error = Some("database read failed".to_owned());

        let error = input
            .load_request()
            .expect_err("F9 must preserve a settings read failure");
        assert!(matches!(error, Error::SettingsRead(detail) if detail == "database read failed"));
    }
}
