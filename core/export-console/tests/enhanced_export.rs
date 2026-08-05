//! M4 增强导出验收：保留候选生成初始校园内容，封账语义与资格边界。
//!
//! F9 只消费应用流程传入的保留候选标识 + 规范化投影；原始观测、待定、
//! 剔除、隔离对象绝不进入导出；任何失败都不产生伪成功产物。

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use export_console::{
    BoundaryExportOperation, CandidateExportReader, CandidateExportSummary, EnhancedExportInput,
    EnhancedExportPort, EnhancedExportRequest, Error, ExportFileKind, ExportFileSystem,
    KeptCandidateProjection, StdExportFileSystem,
};
use manifest_generator::{ExportKind, FoundationManifest};
use shared_domain_types::{Boundary, CandidateCategory, PlanId};

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

fn building_projection(
    id: &str,
    lon: f64,
    lat: f64,
    size: f64,
    height: Option<f64>,
) -> KeptCandidateProjection {
    let mut tags = vec![("source".to_owned(), "fixture".to_owned())];
    if let Some(height) = height {
        tags.push(("height".to_owned(), height.to_string()));
    }
    KeptCandidateProjection {
        candidate_id: id.to_owned(),
        category: CandidateCategory::Building,
        display_title: format!("教学楼 {id}"),
        tags,
        shape_kind: "polygon".to_owned(),
        coordinates: serde_json::json!([
            [lon, lat],
            [lon + size, lat],
            [lon + size, lat + size],
            [lon, lat + size],
            [lon, lat]
        ]),
        reviewable: true,
    }
}

fn road_projection(id: &str, lon: f64, lat: f64) -> KeptCandidateProjection {
    KeptCandidateProjection {
        candidate_id: id.to_owned(),
        category: CandidateCategory::Road,
        display_title: format!("道路 {id}"),
        tags: vec![("highway".to_owned(), "residential".to_owned())],
        shape_kind: "line_string".to_owned(),
        coordinates: serde_json::json!([[lon, lat], [lon + 0.0004, lat]]),
        reviewable: true,
    }
}

fn water_projection(id: &str, lon: f64, lat: f64) -> KeptCandidateProjection {
    KeptCandidateProjection {
        candidate_id: id.to_owned(),
        category: CandidateCategory::Water,
        display_title: format!("水域 {id}"),
        tags: vec![("natural".to_owned(), "water".to_owned())],
        shape_kind: "polygon".to_owned(),
        coordinates: serde_json::json!([
            [lon, lat],
            [lon + 0.0002, lat],
            [lon + 0.0002, lat + 0.0002],
            [lon, lat + 0.0002],
            [lon, lat]
        ]),
        reviewable: true,
    }
}

fn isolated_projection(id: &str) -> KeptCandidateProjection {
    let mut projection = building_projection(id, 116.0001, 39.0001, 0.0001, None);
    projection.reviewable = false;
    projection
}

fn make_summary(
    keep_by_category: Vec<(CandidateCategory, usize)>,
    keep_total: usize,
    pending: usize,
    remove: usize,
) -> CandidateExportSummary {
    CandidateExportSummary {
        candidate_projection_count: 7,
        review_decision_count: keep_total + pending + remove,
        keep_total,
        keep_by_category,
        pending_count: pending,
        remove_count: remove,
    }
}

struct FakeInput {
    request: EnhancedExportRequest,
}

impl EnhancedExportInput for FakeInput {
    fn load_request(&self) -> export_console::Result<EnhancedExportRequest> {
        Ok(self.request.clone())
    }
}

#[derive(Clone)]
struct FakeReader {
    projections: Arc<HashMap<String, KeptCandidateProjection>>,
}

impl FakeReader {
    fn new(projections: Vec<KeptCandidateProjection>) -> Self {
        let mut map = HashMap::new();
        for projection in projections {
            map.insert(projection.candidate_id.clone(), projection);
        }
        Self {
            projections: Arc::new(map),
        }
    }
}

impl CandidateExportReader for FakeReader {
    fn kept_projection(
        &self,
        _plan_id: &str,
        candidate_id: &str,
    ) -> export_console::Result<Option<KeptCandidateProjection>> {
        Ok(self.projections.get(candidate_id).cloned())
    }
}

fn request(
    dir: &Path,
    summary: CandidateExportSummary,
    kept_ids: Vec<String>,
) -> EnhancedExportRequest {
    EnhancedExportRequest::new(
        "测试校区",
        PlanId::generate(),
        "增强导出方案",
        "26.1.2",
        Some(boundary()),
        true,
        None,
        dir.join("campus.schem"),
        dir.join("foundation_manifest.json"),
        summary,
        kept_ids,
    )
}

fn read_manifest(path: &Path) -> FoundationManifest {
    let json = std::fs::read_to_string(path).expect("manifest 必须实际写入");
    FoundationManifest::from_json(&json).expect("manifest 必须是有效 JSON")
}

fn wait_for_result(
    operation: &mut BoundaryExportOperation,
) -> export_console::Result<export_console::BoundaryExportResult> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = operation.try_complete() {
            return result;
        }
        assert!(Instant::now() < deadline, "增强导出未在期限内达到终态");
        std::thread::yield_now();
    }
}

#[test]
fn enhanced_export_generates_retained_candidates_with_truthful_manifest() {
    let dir = tempfile::tempdir().expect("临时目录");
    let building_a = building_projection("keep/b1", 116.0001, 39.0001, 0.0004, Some(12.0));
    let building_b = building_projection("keep/b2", 116.0006, 39.0002, 0.0003, None);
    let road = road_projection("keep/r1", 116.0002, 39.0006);
    let water = water_projection("keep/w1", 116.0006, 39.0006);
    let summary = make_summary(
        vec![
            (CandidateCategory::Building, 2),
            (CandidateCategory::Road, 1),
            (CandidateCategory::Water, 1),
        ],
        4,
        2,
        1,
    );
    let kept_ids = vec![
        "keep/b1".to_owned(),
        "keep/b2".to_owned(),
        "keep/r1".to_owned(),
        "keep/w1".to_owned(),
    ];
    let port = EnhancedExportPort::new_enhanced_v26_1_2(
        Arc::new(FakeInput {
            request: request(dir.path(), summary.clone(), kept_ids.clone()),
        }),
        Arc::new(FakeReader::new(vec![building_a, building_b, road, water])),
        Arc::new(StdExportFileSystem),
    );

    let mut operation = port.start().expect("Start 应提交后台增强导出");
    let result = wait_for_result(&mut operation).expect("增强导出成功");
    assert!(result.schematic_path.is_file());
    assert!(result.manifest_path.is_file());

    let inspection = sponge_export::verify_worldedit_import_contract(&result.schematic_path)
        .expect("增强导出必须是可导入的 Sponge .schem");
    assert_eq!(inspection.data_version, 3955);
    assert!(
        inspection.dimensions[1] > 1,
        "保留建筑必须让场地获得高度内容：{:?}",
        inspection.dimensions
    );
    assert!(
        inspection.non_air_voxels > inspection.dimensions[0] * inspection.dimensions[2],
        "保留候选内容必须让方块计数大于纯基础场地"
    );
    assert!(
        inspection
            .palette
            .keys()
            .any(|block| block.contains("glass") || block.contains("brick")),
        "建筑内容（墙体/窗）必须出现在 .schem 中"
    );

    let manifest = read_manifest(&result.manifest_path);
    assert_eq!(manifest.export_kind, ExportKind::Enhanced);
    assert_eq!(manifest.candidate_facts.candidate_projection_count, 7);
    assert_eq!(manifest.candidate_facts.review_decision_count, 7);
    assert_eq!(manifest.candidate_facts.retained_candidate_count, 4);
    let mut counts: HashMap<&str, usize> = manifest
        .candidate_facts
        .keep_by_category
        .iter()
        .map(|entry| (entry.category.as_str(), entry.count))
        .collect();
    assert_eq!(counts.remove("Building"), Some(2));
    assert_eq!(counts.remove("Road"), Some(1));
    assert_eq!(counts.remove("Water"), Some(1));
    assert!(
        counts.is_empty(),
        "待定/剔除/隔离类别不得进入 manifest 计数"
    );
    let included: Vec<&str> = manifest
        .categories
        .iter()
        .filter(|category| category.included)
        .map(|category| category.name.as_str())
        .collect();
    assert_eq!(included, vec!["建筑", "道路", "水域"]);
}

#[test]
fn kept_candidate_whose_current_projection_is_not_reviewable_fails_without_artifacts() {
    let dir = tempfile::tempdir().expect("临时目录");
    let summary = make_summary(vec![(CandidateCategory::Building, 1)], 1, 0, 0);
    let kept_ids = vec!["keep/b1".to_owned()];
    let port = EnhancedExportPort::new_enhanced_v26_1_2(
        Arc::new(FakeInput {
            request: request(dir.path(), summary, kept_ids),
        }),
        Arc::new(FakeReader::new(vec![isolated_projection("keep/b1")])),
        Arc::new(StdExportFileSystem),
    );

    let mut operation = port.start().expect("Start 应提交后台增强导出");
    let result = wait_for_result(&mut operation);
    assert!(matches!(result, Err(Error::CandidateEligibility(_))));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
    assert!(
        std::fs::read_dir(dir.path())
            .expect("输出目录可读")
            .next()
            .is_none(),
        "失败不得留下 staging 或备份残留"
    );
}

#[test]
fn missing_projection_and_summary_mismatch_are_explicit_failures() {
    let dir = tempfile::tempdir().expect("临时目录");
    let summary = make_summary(vec![(CandidateCategory::Building, 1)], 1, 0, 0);
    let port = EnhancedExportPort::new_enhanced_v26_1_2(
        Arc::new(FakeInput {
            request: request(dir.path(), summary, vec!["keep/missing".to_owned()]),
        }),
        Arc::new(FakeReader::new(vec![])),
        Arc::new(StdExportFileSystem),
    );
    let mut operation = port.start().expect("Start 应提交后台增强导出");
    assert!(matches!(
        wait_for_result(&mut operation),
        Err(Error::CandidateEligibility(_))
    ));

    let mismatched = make_summary(vec![(CandidateCategory::Building, 1)], 2, 0, 0);
    let port = EnhancedExportPort::new_enhanced_v26_1_2(
        Arc::new(FakeInput {
            request: request(dir.path(), mismatched, vec!["keep/b1".to_owned()]),
        }),
        Arc::new(FakeReader::new(vec![building_projection(
            "keep/b1", 116.0001, 39.0001, 0.0002, None,
        )])),
        Arc::new(StdExportFileSystem),
    );
    let mut operation = port.start().expect("Start 应提交后台增强导出");
    assert!(matches!(
        wait_for_result(&mut operation),
        Err(Error::CandidateFactsMismatch(_))
    ));
    assert!(!dir.path().join("campus.schem").exists());
    assert!(!dir.path().join("foundation_manifest.json").exists());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureMode {
    None,
    ManifestStaging,
    SchematicStaging,
    ManifestPublish,
    ManifestPublishAndRestore,
}

#[derive(Clone)]
struct FailingFileSystem {
    mode: Arc<Mutex<FailureMode>>,
    standard: Arc<StdExportFileSystem>,
}

impl FailingFileSystem {
    fn new() -> (Self, Arc<Mutex<FailureMode>>) {
        let mode = Arc::new(Mutex::new(FailureMode::None));
        (
            Self {
                mode: Arc::clone(&mode),
                standard: Arc::new(StdExportFileSystem),
            },
            mode,
        )
    }
}

impl ExportFileSystem for FailingFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.standard.create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mode = *self.mode.lock().expect("failure mode lock");
        let is_manifest = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".m1-manifest-"));
        if mode == FailureMode::ManifestStaging && is_manifest {
            return Err(io::Error::other("注入 manifest staging 写失败"));
        }
        if mode == FailureMode::SchematicStaging && !is_manifest {
            return Err(io::Error::other("注入 .schem staging 写失败"));
        }
        self.standard.write(path, contents)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mode = *self.mode.lock().expect("failure mode lock");
        let publishing_manifest = to
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("foundation_manifest"));
        if (mode == FailureMode::ManifestPublish || mode == FailureMode::ManifestPublishAndRestore)
            && publishing_manifest
        {
            return Err(io::Error::other("注入 manifest 发布失败"));
        }
        let restoring = from
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("backup-manifest"));
        if mode == FailureMode::ManifestPublishAndRestore && restoring {
            return Err(io::Error::other("注入 manifest 恢复失败"));
        }
        self.standard.rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.standard.remove_file(path)
    }

    fn kind(&self, path: &Path) -> io::Result<Option<ExportFileKind>> {
        self.standard.kind(path)
    }
}

#[test]
fn enhanced_export_failures_leave_no_fake_success_artifacts() {
    for mode in [
        FailureMode::ManifestStaging,
        FailureMode::SchematicStaging,
        FailureMode::ManifestPublish,
        FailureMode::ManifestPublishAndRestore,
    ] {
        let dir = tempfile::tempdir().expect("临时目录");
        let (file_system, mode_handle) = FailingFileSystem::new();
        *mode_handle.lock().expect("mode lock") = mode;
        let summary = make_summary(vec![(CandidateCategory::Building, 1)], 1, 0, 0);
        let kept_ids = vec!["keep/b1".to_owned()];
        let port = EnhancedExportPort::new_enhanced_v26_1_2(
            Arc::new(FakeInput {
                request: request(dir.path(), summary, kept_ids),
            }),
            Arc::new(FakeReader::new(vec![building_projection(
                "keep/b1", 116.0001, 39.0001, 0.0002, None,
            )])),
            Arc::new(file_system),
        );

        let mut operation = port.start().expect("Start 应提交后台增强导出");
        let result = wait_for_result(&mut operation);
        assert!(result.is_err(), "模式 {mode:?} 必须结构化失败");
        assert!(
            !dir.path().join("campus.schem").exists(),
            "模式 {mode:?} 不得留下伪成功 .schem"
        );
        assert!(
            !dir.path().join("foundation_manifest.json").exists(),
            "模式 {mode:?} 不得留下伪成功 manifest"
        );
        let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
            .expect("输出目录可读")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        assert!(
            leftovers.is_empty(),
            "模式 {mode:?} 不得留下 staging/备份残留：{leftovers:?}"
        );
    }
}
