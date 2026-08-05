//! 集成测试：封账语义 + 导出失败回滚（ADR-0022 验收标准）
//!
//! 用真实的 F5 review-workbench + B2 data-persistence 验证：
//! 1. seal() → 评审终态批量写回且评审不可再改（封账语义）；
//! 2. 导出失败 → 封账失效 + 评审状态可恢复（回滚语义）；
//! 3. 导出成功 → 跳转导出完成页且 .schem 合法（贯穿弹缝 6）。

use std::sync::{Arc, Mutex};

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
};
use export_console::{ExportConsole, ExportRequest, NavigationTarget, SealGate};
use generation_engine::{BlockModel, BlockPosition};
use review_workbench::{CandidateKey, CommandOutcome, ReviewWorkbench, StateChange};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

const CANDIDATE_ID: &str = "overpass:way/1:outer";

/// 写入一条原始观测并发布对应 Reviewable 候选投影，返回 (db, plan_id)。
fn seed_database() -> (Database, PlanId) {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let observation = RawObservation::new(
        plan_id.to_string(),
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({ "tags": { "name": "教学楼" } }),
        "overpass",
    );
    db.write_raw_observations(std::slice::from_ref(&observation))
        .expect("观测写入成功");
    let batch = db
        .prepare_candidate_batch(&plan_id.to_string())
        .expect("候选批次准备成功");
    let projection = CandidateProjection::new(
        CANDIDATE_ID,
        plan_id.to_string(),
        observation.id,
        "overpass",
        "way/1",
        "outer",
        CandidateCategory::Building,
        CandidateDisplay::new("教学楼", vec![("name".to_owned(), "教学楼".to_owned())]),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    );
    db.write_candidate_projections(&batch.id, &[projection])
        .expect("候选投影写入成功");
    db.publish_candidate_batch(&batch.id)
        .expect("候选批次发布成功");
    (db, plan_id)
}

/// 组装缝 5 导出请求（保留 1 项建筑、待定 0 项）
fn export_request(plan_id: &PlanId) -> ExportRequest {
    ExportRequest::new(
        plan_id.to_string(),
        vec![(CandidateCategory::Building, 1)],
        1,
        0,
        0,
        vec![format!("Building/{CANDIDATE_ID}")],
    )
}

/// 单元验收点：seal_export → 评审终态被标记为封账（Done）且不可再改
#[test]
fn seal_marks_review_done_and_immutable() {
    let (mut db, plan_id) = seed_database();

    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert!(!workbench.is_sealed());

    // 把候选改为保留后封账
    let key = CandidateKey::new(CANDIDATE_ID);
    workbench
        .submit(StateChange::single(key.clone(), ReviewState::Keep))
        .unwrap();
    let summary = workbench.seal(&mut db).unwrap();
    assert_eq!(summary.keep_total, 1);
    assert!(workbench.is_sealed());

    // 封账后尝试修改 → AlreadySealed（评审决定不可再改）
    let result = workbench.submit(StateChange::single(key, ReviewState::Remove));
    assert!(matches!(
        result,
        Err(review_workbench::Error::AlreadySealed)
    ));
}

/// 真实门控：seal 走 F5 封账写回；release 丢弃已封账实例
/// （下次进台从 B2 重新读入，评审恢复可改——回滚语义的壳侧实现）
struct WorkbenchSealGate {
    db: Arc<Mutex<Database>>,
    sealed_workbench: Arc<Mutex<Option<ReviewWorkbench>>>,
}

impl SealGate for WorkbenchSealGate {
    fn seal(&self, plan_id: &PlanId) -> Result<(), String> {
        let mut db = self.db.lock().expect("测试库锁不可中毒");
        let mut workbench = ReviewWorkbench::load(&db, plan_id).map_err(|e| e.to_string())?;
        workbench.seal(&mut db).map_err(|e| e.to_string())?;
        *self.sealed_workbench.lock().unwrap() = Some(workbench);
        Ok(())
    }

    fn release(&self, _plan_id: &PlanId) -> Result<(), String> {
        *self.sealed_workbench.lock().unwrap() = None;
        Ok(())
    }
}

/// 集成验收点：导出失败 → 封账失效 + 评审状态可恢复（回滚语义验证）
#[test]
fn export_failure_rolls_back_seal_and_review_is_editable_again() {
    let (db, plan_id) = seed_database();
    let db = Arc::new(Mutex::new(db));
    let sealed_workbench = Arc::new(Mutex::new(None));

    let gate = WorkbenchSealGate {
        db: Arc::clone(&db),
        sealed_workbench: Arc::clone(&sealed_workbench),
    };
    let mut console = ExportConsole::new(gate);

    // 缝 5：递交请求 → 确认 → 封账生效（评审入口禁用）
    console.load_request(export_request(&plan_id)).unwrap();
    console.confirm_export().unwrap();
    assert!(console.is_exporting());
    assert!(sealed_workbench
        .lock()
        .unwrap()
        .as_ref()
        .expect("封账后必有已封账实例")
        .is_sealed());

    // 缝 6：落盘目标是一个已存在的目录 → 导出失败
    let dir = tempfile::tempdir().unwrap();
    let bad_path = dir.path().join("campus.schem");
    std::fs::create_dir(&bad_path).unwrap();
    let err = console
        .execute_export(&BlockModel::new(), &bad_path)
        .unwrap_err();
    assert!(matches!(err, export_console::Error::SchematicWrite(_)));

    // 回滚断言 1：封账失效（已封账实例被丢弃，评审入口重新可用）
    assert!(sealed_workbench.lock().unwrap().is_none());
    assert_eq!(
        console.failure_target(),
        Some(NavigationTarget::ContinueReview)
    );

    // 回滚断言 2：重新进台的评审台未封账、评审状态可再改
    let db_guard = db.lock().unwrap();
    let mut workbench = ReviewWorkbench::load(&db_guard, &plan_id).unwrap();
    assert!(!workbench.is_sealed());
    let key = CandidateKey::new(CANDIDATE_ID);
    let outcome = workbench
        .submit(StateChange::single(key, ReviewState::Pending))
        .expect("回滚后评审状态必须可改");
    assert!(matches!(outcome, CommandOutcome::Applied { .. }));
}

/// 集成验收点：导出成功 → 封账保持 + 跳转导出完成页 + .schem 合法
#[test]
fn export_success_keeps_seal_and_yields_completed_target() {
    let (db, plan_id) = seed_database();
    let db = Arc::new(Mutex::new(db));
    let sealed_workbench = Arc::new(Mutex::new(None));

    let gate = WorkbenchSealGate {
        db: Arc::clone(&db),
        sealed_workbench: Arc::clone(&sealed_workbench),
    };
    let mut console = ExportConsole::new(gate);

    console.load_request(export_request(&plan_id)).unwrap();
    console.confirm_export().unwrap();

    // 缝 6：一栋按规则起的楼落进 .schem（贯穿弹剧本第四步的最小样本）
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("campus.schem");
    let mut model = BlockModel::new();
    model.set_block(BlockPosition::new(0, 0, 0), "minecraft:stone_bricks");
    model.set_block(BlockPosition::new(0, 1, 0), "minecraft:bricks");

    let target = console.execute_export(&model, &output).unwrap();
    let NavigationTarget::ExportCompleted(summary) = target else {
        panic!("导出成功必须跳转到导出完成页");
    };
    assert_eq!(summary.plan_id, plan_id.to_string());
    assert_eq!(summary.export_count, 1);

    // 封账保持（导出成功不回滚）
    assert!(sealed_workbench.lock().unwrap().is_some());

    // .schem 合法且方块如数落盘
    let inspection = sponge_inspect(&output);
    assert_eq!(inspection.sponge_version, 3);
    assert_eq!(inspection.non_air_voxels, 2);

    // 进度条走完（非阻塞进度条终态）
    assert_eq!(console.progress().percent(), 100);
    assert!(console.progress_view().is_done);
}

/// 经由 B4 公开 API 验证 .schem（避免在断言里散落 anyhow unwrap 细节）
fn sponge_inspect(path: &std::path::Path) -> sponge_export::SchematicInspection {
    sponge_export::inspect_schematic(path).expect(".schem 可解析")
}
