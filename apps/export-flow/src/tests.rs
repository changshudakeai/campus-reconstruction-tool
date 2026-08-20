//! F9 输入冻结、候选生命周期读取与导出路由的集成式单元测试。

use std::io;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

use data_persistence::{
    CandidateDisplay, CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi, ReviewDecision,
    ReviewDecisionsApi, ReviewableValidation,
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
    let mut observations = vec![
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
    if include_isolated {
        observations.push(RawObservation::new(
            &plan_key,
            CandidateCategory::Building,
            "way/iso",
            serde_json::json!({ "tags": { "name": "隔离建筑" } }),
            "overpass",
        ));
    }
    db.write_raw_observations(&observations)
        .expect("写入原始观测");
    let mut drafts = Vec::new();
    for observation in observations
        .iter()
        .filter(|observation| observation.entity_id != "way/iso")
    {
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
        drafts.push(CandidateProjectionDraft::reviewable(
            CandidateSourceIdentity::new(
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
            ),
            observation.entity_type,
            display,
            CandidateShape::polygon(serde_json::json!([
                [116.0001, 39.0001],
                [116.0005, 39.0001],
                [116.0005, 39.0005],
                [116.0001, 39.0001]
            ])),
            ReviewableValidation::Retained,
        ));
    }
    if include_isolated {
        drafts.push(
            CandidateProjectionDraft::isolated(
                CandidateSourceIdentity::new("overpass", "way/iso", "default"),
                CandidateCategory::Building,
                CandidateDisplay::new("隔离建筑", vec![]),
                CandidateShape::point(serde_json::json!([])),
                "missing_source_geometry",
            )
            .expect("隔离事实合法"),
        );
    }
    let revision = db
        .publish_candidate_batch(&plan_key, "export-flow-fixture-boundary", &drafts)
        .expect("原子发布候选批次")
        .batch
        .id;
    let current = db
        .list_current_candidate_projections(&plan_key)
        .expect("读取当前候选投影");
    let reviewable: Vec<String> = observations
        .iter()
        .filter(|observation| observation.entity_id != "way/iso")
        .map(|observation| {
            current
                .iter()
                .find(|projection| projection.source_entity_id == observation.entity_id)
                .expect("来源事实对应稳定候选")
                .candidate_id
                .clone()
        })
        .collect();

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
    db.batch_update_review_decisions_at_revision(&plan_key, &revision, &decisions)
        .expect("封账写回");
    reviewable
}

#[test]
fn production_candidate_store_requires_a_new_keep_after_reappearance() {
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();
    let observation = RawObservation::new(
        &plan_key,
        CandidateCategory::Building,
        "way/reappearing",
        serde_json::json!({ "tags": { "name": "重新出现的教学楼" } }),
        "overpass",
    );
    let draft = CandidateProjectionDraft::reviewable(
        CandidateSourceIdentity::new("overpass", "way/reappearing", "outer"),
        CandidateCategory::Building,
        CandidateDisplay::new("重新出现的教学楼", vec![]),
        CandidateShape::polygon(serde_json::json!([
            [116.0001, 39.0001],
            [116.0005, 39.0001],
            [116.0005, 39.0005],
            [116.0001, 39.0001]
        ])),
        ReviewableValidation::Retained,
    );
    let stable_id = {
        let mut database = db.lock().expect("db");
        database
            .write_raw_observations(&[observation])
            .expect("原始观测写入");
        let revision = database
            .publish_candidate_batch(&plan_key, "boundary-1", std::slice::from_ref(&draft))
            .expect("首次发布")
            .batch
            .id;
        let candidate_id = database
            .list_current_candidate_projections(&plan_key)
            .expect("读取候选")
            .pop()
            .expect("候选存在")
            .candidate_id;
        database
            .batch_update_review_decisions_at_revision(
                &plan_key,
                &revision,
                &[ReviewDecision::new(
                    &plan_key,
                    CandidateCategory::Building,
                    &candidate_id,
                    ReviewState::Keep,
                )],
            )
            .expect("首次保留");
        candidate_id
    };
    let store = crate::candidates::ExportCandidateStore::new(Arc::clone(&db));
    assert_eq!(
        store.kept_candidate_ids(&plan_key).expect("读取保留候选"),
        vec![stable_id.clone()]
    );

    {
        let mut database = db.lock().expect("db");
        database
            .publish_candidate_batch(&plan_key, "boundary-2", &[])
            .expect("候选消失批次");
    }
    assert!(
        store
            .kept_candidate_ids(&plan_key)
            .expect("消失后读取")
            .is_empty(),
        "候选消失后旧 Keep 不能继续导出"
    );

    let reappearance_revision = {
        let mut database = db.lock().expect("db");
        let revision = database
            .publish_candidate_batch(&plan_key, "boundary-3", &[draft])
            .expect("候选重新出现批次")
            .batch
            .id;
        let current = database
            .list_current_candidate_projections(&plan_key)
            .expect("读取重现候选");
        assert_eq!(current[0].candidate_id, stable_id, "稳定候选身份必须复用");
        revision
    };
    assert!(
        store
            .kept_candidate_ids(&plan_key)
            .expect("重现后读取")
            .is_empty(),
        "候选重新出现后仍必须待定，旧 Keep 不得恢复"
    );
    let summary = store.seal_summary(&plan_key).expect("读取封账摘要");
    assert_eq!(summary.keep_total, 0);
    assert_eq!(summary.pending_count, 1);

    db.lock()
        .expect("db")
        .batch_update_review_decisions_at_revision(
            &plan_key,
            &reappearance_revision,
            &[ReviewDecision::new(
                &plan_key,
                CandidateCategory::Building,
                &stable_id,
                ReviewState::Keep,
            )],
        )
        .expect("重新保留");
    assert_eq!(
        store.kept_candidate_ids(&plan_key).expect("重新保留后读取"),
        vec![stable_id],
        "只有用户再次保留后才可进入增强导出"
    );
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

    let cards = flow.kept_candidate_cards().expect("只读候选卡片可读");
    assert_eq!(cards.len(), 2, "只读卡片必须只包含保留候选");
    assert!(cards.iter().all(|card| {
        card.category == CandidateCategory::Building
            && (card.display_title == "教学楼A" || card.display_title == "教学楼B")
    }));
    assert_eq!(
        cards[0].candidate_id, kept[0],
        "卡片标识必须与增强导出的保留候选一致"
    );

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
    let mut settings = SettingsManager::new(Database::open_in_memory().expect("打开测试设置库"));
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
    let mut settings = SettingsManager::new(Database::open_in_memory().expect("打开测试设置库"));
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

fn wait_for_preview(operation: &mut BlockPreviewOperation) -> Result<PreviewRenderPayload> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(result) = operation.try_complete() {
            return result;
        }
        assert!(std::time::Instant::now() < deadline, "预览操作未达到终态");
        std::thread::yield_now();
    }
}

#[test]
fn preview_payload_matches_the_exported_schematic_block_count() {
    let directory = tempfile::tempdir().expect("导出目录");
    let plan_id = PlanId::generate();
    let flow = BoundaryExportFlow::new(Arc::new(StdExportFileSystem));
    flow.sync_settings(&settings_with_export_dir(directory.path()));
    flow.set_plan(&confirmed_plan_context(&plan_id));
    flow.confirm_boundary(
        "Polygon",
        serde_json::json!([[
            [116.0000, 39.0000],
            [116.0006, 39.0000],
            [116.0006, 39.0004],
            [116.0000, 39.0004],
            [116.0000, 39.0000]
        ]]),
    );

    let mut preview_operation = flow.start_preview().expect("预览启动");
    let preview = wait_for_preview(&mut preview_operation).expect("预览成功");
    assert!(preview.block_count > 0);
    let parsed: serde_json::Value = serde_json::from_str(&preview.json).expect("合法 JSON");
    let palette = parsed["palette"].as_array().expect("调色板数组");
    assert!(
        palette.iter().any(|block| block == "minecraft:grass_block"),
        "平整场地必须与导出同源使用草方块"
    );

    let mut export_operation = flow.start().expect("导出启动");
    let export = wait_for_result(&mut export_operation).expect("导出成功");
    let inspection =
        sponge_export::inspect_schematic(&export.schematic_path).expect(".schem 可解析");
    assert_eq!(
        preview.block_count, inspection.non_air_voxels,
        "预览方块数必须与导出 .schem 的非空气方块数一致"
    );
}

#[test]
fn preview_routes_to_enhanced_generation_like_export() {
    let directory = tempfile::tempdir().expect("导出目录");
    let db = Arc::new(Mutex::new(Database::open_in_memory().expect("内存库")));
    let plan_id = PlanId::generate();
    seed_plan_with_sealed_review(&mut db.lock().expect("db"), &plan_id, true);

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

    let mut preview_operation = flow.start_preview().expect("预览路由到增强生成");
    let preview = wait_for_preview(&mut preview_operation).expect("增强预览成功");
    let parsed: serde_json::Value = serde_json::from_str(&preview.json).expect("合法 JSON");
    let palette = parsed["palette"].as_array().expect("调色板数组");
    assert!(
        palette.iter().any(|block| block == "minecraft:bricks"),
        "保留建筑必须生成墙体方块"
    );
    let features = parsed["features"].as_array().expect("要素数组");
    assert_eq!(features.len(), 2, "增强预览必须携带全部保留候选要素");
    assert!(
        features.iter().all(|feature| {
            feature["category"] == "Building"
                && feature["bounds"].as_array().expect("包围盒数组").len() == 6
        }),
        "要素必须携带类别与 6 元包围盒"
    );
    let cards = flow.kept_candidate_cards().expect("只读候选卡片");
    assert_eq!(
        cards
            .iter()
            .map(|card| card.candidate_id.clone())
            .collect::<Vec<_>>(),
        features
            .iter()
            .map(|feature| feature["id"].as_str().expect("要素 id").to_owned())
            .collect::<Vec<_>>(),
        "预览要素标识必须与 A2 只读候选卡片一一对应"
    );

    let mut export_operation = flow.start().expect("导出路由到增强导出");
    let export = wait_for_result(&mut export_operation).expect("增强导出成功");
    let inspection =
        sponge_export::inspect_schematic(&export.schematic_path).expect(".schem 可解析");
    assert_eq!(
        preview.block_count, inspection.non_air_voxels,
        "增强预览与增强导出的方块数必须一致"
    );
}

#[test]
fn preview_failure_never_blocks_export() {
    let directory = tempfile::tempdir().expect("导出目录");
    let plan_id = PlanId::generate();
    let flow = BoundaryExportFlow::new(Arc::new(StdExportFileSystem));
    flow.sync_settings(&settings_with_export_dir(directory.path()));
    flow.set_plan(&confirmed_plan_context(&plan_id));
    // 边界存在但未确认：预览生成失败（NotConfirmed），导出同样在确认后可用。
    flow.set_boundary(
        Some(Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [116.4000, 39.9000],
                [116.4010, 39.9000],
                [116.4010, 39.9010],
                [116.4000, 39.9010],
                [116.4000, 39.9000]
            ]]),
        }),
        false,
    );

    let mut preview_operation = flow.start_preview().expect("预览操作可提交");
    let preview_error = wait_for_preview(&mut preview_operation).expect_err("预览如实失败");
    assert!(matches!(preview_error, Error::Boundary(_)));

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
    let mut export_operation = flow.start().expect("预览失败后仍可导出");
    assert!(
        wait_for_result(&mut export_operation).is_ok(),
        "预览失败绝不阻塞导出流程"
    );
}
