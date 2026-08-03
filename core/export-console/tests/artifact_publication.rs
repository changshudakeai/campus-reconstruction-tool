//! M1 双文件发布故障验收：staging、发布与恢复都必须可观测。

use std::io;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use export_console::{
    BoundaryExportInput, BoundaryExportRequest, Error, ExportConsole, ExportFileKind,
    ExportFileSystem, MockSealGate,
};
use manifest_generator::MaterialTable;
use shared_domain_types::{Boundary, PlanId};

#[derive(Clone, Copy)]
enum Failure {
    ManifestStageWrite,
    SchematicStageWrite,
    ManifestPublish,
    ManifestPublishAndRestore,
}

struct FaultFileSystem {
    failure: Failure,
    publish_failed: Mutex<bool>,
}

struct FixedInput(BoundaryExportRequest);

impl BoundaryExportInput for FixedInput {
    fn load_request(&self) -> export_console::Result<BoundaryExportRequest> {
        Ok(self.0.clone())
    }
}

struct BlockingFileSystem {
    started: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingFileSystem {
    fn wait_for_manifest_write(&self) {
        let (lock, signal) = &*self.started;
        let mut started = lock.lock().expect("manifest staging barrier lock");
        while !*started {
            started = signal.wait(started).expect("manifest staging barrier wait");
        }
    }

    fn release_manifest_write(&self) {
        let (lock, signal) = &*self.release;
        *lock.lock().expect("manifest release barrier lock") = true;
        signal.notify_one();
    }
}

impl ExportFileSystem for BlockingFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("m1-manifest"))
        {
            let (lock, signal) = &*self.started;
            *lock.lock().expect("manifest staging barrier lock") = true;
            signal.notify_one();

            let (lock, signal) = &*self.release;
            let mut released = lock.lock().expect("manifest release barrier lock");
            while !*released {
                released = signal
                    .wait(released)
                    .expect("manifest release barrier wait");
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

impl FaultFileSystem {
    fn io_error(operation: &str, path: &Path) -> io::Error {
        io::Error::other(format!("injected {operation} failure: {}", path.display()))
    }
}

impl ExportFileSystem for FaultFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let should_fail = matches!(self.failure, Failure::ManifestStageWrite)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("m1-manifest"));
        if should_fail {
            return Err(Self::io_error("manifest staging write", path));
        }
        let should_fail = matches!(self.failure, Failure::SchematicStageWrite)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("m1-schem"));
        if should_fail {
            return Err(Self::io_error("schematic staging write", path));
        }
        std::fs::write(path, contents)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let publish_failure = matches!(
            self.failure,
            Failure::ManifestPublish | Failure::ManifestPublishAndRestore
        );
        if publish_failure
            && to
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "manifest.json")
        {
            let mut failed = self.publish_failed.lock().expect("故障状态锁");
            let always_fail = matches!(self.failure, Failure::ManifestPublishAndRestore);
            if always_fail || !*failed {
                *failed = true;
                return Err(Self::io_error("manifest publish", to));
            }
        }
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

fn boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.0000, 39.0000],
            [116.0010, 39.0000],
            [116.0010, 39.0010],
            [116.0000, 39.0010],
            [116.0000, 39.0000]
        ]]),
    }
}

fn request(dir: &Path) -> BoundaryExportRequest {
    BoundaryExportRequest::new(
        "测试校区",
        PlanId::generate(),
        "发布故障方案",
        "26.1.2",
        Some(boundary()),
        true,
        None,
        dir.join("schematic.schem"),
        dir.join("manifest.json"),
    )
}

fn console(_dir: &Path, failure: Failure) -> ExportConsole<MockSealGate> {
    ExportConsole::new_with_material_table_and_file_system(
        MockSealGate::new(),
        MaterialTable::v26_1_2_school(),
        Arc::new(FaultFileSystem {
            failure,
            publish_failed: Mutex::new(false),
        }),
    )
}

#[test]
fn manifest_staging_write_failure_is_structured_and_leaves_no_pair() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = console(dir.path(), Failure::ManifestStageWrite);

    let error = console
        .export_confirmed_boundary(request(dir.path()))
        .expect_err("manifest staging 失败不得报告成功");

    assert!(matches!(error, Error::ManifestWrite(_)));
    assert!(!dir.path().join("schematic.schem").exists());
    assert!(!dir.path().join("manifest.json").exists());
}

#[test]
fn schematic_staging_write_failure_is_structured_and_leaves_no_pair() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = console(dir.path(), Failure::SchematicStageWrite);

    let error = console
        .export_confirmed_boundary(request(dir.path()))
        .expect_err("schematic staging 失败不得报告成功");

    assert!(matches!(error, Error::SchematicWrite(_)));
    assert!(!dir.path().join("schematic.schem").exists());
    assert!(!dir.path().join("manifest.json").exists());
}

#[test]
fn first_publish_success_then_second_publish_failure_removes_new_one_sided_pair() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = console(dir.path(), Failure::ManifestPublish);

    let error = console
        .export_confirmed_boundary(request(dir.path()))
        .expect_err("第二个最终文件发布失败不得报告成功");

    assert!(matches!(error, Error::ArtifactWrite(_)));
    assert!(!dir.path().join("schematic.schem").exists());
    assert!(!dir.path().join("manifest.json").exists());
}

#[test]
fn replacing_existing_pair_restores_both_old_files_after_publish_failure() {
    let dir = tempfile::tempdir().unwrap();
    let schematic = dir.path().join("schematic.schem");
    let manifest = dir.path().join("manifest.json");
    std::fs::write(&schematic, b"old schem").unwrap();
    std::fs::write(&manifest, b"old manifest").unwrap();
    let mut console = console(dir.path(), Failure::ManifestPublish);

    let error = console
        .export_confirmed_boundary(request(dir.path()))
        .expect_err("旧双文件必须在发布失败后保持可恢复");

    assert!(matches!(error, Error::ArtifactWrite(_)));
    assert_eq!(std::fs::read(&schematic).unwrap(), b"old schem");
    assert_eq!(std::fs::read(&manifest).unwrap(), b"old manifest");
}

#[test]
fn restoration_failure_is_not_reported_as_an_ordinary_artifact_write() {
    let dir = tempfile::tempdir().unwrap();
    let schematic = dir.path().join("schematic.schem");
    let manifest = dir.path().join("manifest.json");
    std::fs::write(&schematic, b"old schem").unwrap();
    std::fs::write(&manifest, b"old manifest").unwrap();
    let mut console = console(dir.path(), Failure::ManifestPublishAndRestore);

    let error = console
        .export_confirmed_boundary(request(dir.path()))
        .expect_err("恢复失败必须暴露独立诊断");

    assert!(matches!(error, Error::ArtifactRecovery(_)));
    assert!(!error.to_string().is_empty());
}

#[test]
fn async_start_returns_before_export_finishes_and_then_observes_terminal_state() {
    let dir = tempfile::tempdir().unwrap();
    let started = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let file_system = Arc::new(BlockingFileSystem {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    });
    let console = ExportConsole::new_with_material_table_and_file_system(
        MockSealGate::new(),
        MaterialTable::v26_1_2_school(),
        file_system.clone(),
    );
    let port = console.boundary_export_port(Arc::new(FixedInput(request(dir.path()))));
    let mut operation = port.start().expect("Start 搴旂珯鍗虫彁浜ゆ湁鏁忔剰鍥?");

    file_system.wait_for_manifest_write();
    assert!(
        operation.try_complete().is_none(),
        "Start 返回时 staging 被故意阻塞，后台不得已经完成"
    );
    assert!(
        operation.progress_view().visible,
        "UI 应能观察到 F9 的处理中阶段"
    );
    assert!(!dir.path().join("schematic.schem").exists());
    assert!(!dir.path().join("manifest.json").exists());

    file_system.release_manifest_write();
    let deadline = Instant::now() + Duration::from_secs(2);
    let result = loop {
        if let Some(result) = operation.try_complete() {
            break result;
        }
        assert!(Instant::now() < deadline, "后台导出未在测试窗口内完成");
        std::thread::yield_now();
    };
    assert!(result.is_ok());
    assert!(operation.progress_view().is_done);
}
