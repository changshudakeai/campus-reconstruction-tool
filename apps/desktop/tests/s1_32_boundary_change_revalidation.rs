//! 边界变化后的本地候选资格重验证（D 工单，壳层验收）。
//!
//! - 已有采集结果的方案上手动改边界并确认 → 网络请求数为 0（计数数据源断言），
//!   不触发重新采集；
//! - 跑到边界外的候选：B2 资格 Isolated、不再出现在评审台（Reviewable 列表），
//!   其旧评审决定作废并标注（保留记录 + 作废历史，不物理删除）；
//! - 新进入边界的候选：资格 Reviewable、评审待定，出现在评审台；
//! - 边界未变化时不触发重验证（无新作废/写库）；
//! - 无效边界（自相交）→ 明确报错，不破坏已确认边界与评审状态；
//! - 评审台读取的就是 B2 当前批次（单一真相源）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use data_acquisition::overpass::{BoundarySourceKind, CampusBoundaryResult};
use data_acquisition::{DataSource, RawEntity};
use data_persistence::{
    boundary_fingerprint, BoundaryRevalidationApi, CampusCrudApi, CandidateDisplay,
    CandidateEligibility, CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi, ReviewDecision,
    ReviewDecisionsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::StdExportFileSystem;
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory, ReviewState};

fn canned_boundary() -> CampusBoundaryResult {
    CampusBoundaryResult::AutoSelected {
        name: "重验证测试校区".to_owned(),
        gcj02: vec![
            [116.4000, 39.9000],
            [116.4010, 39.9000],
            [116.4010, 39.9010],
            [116.4000, 39.9010],
        ],
        source: BoundarySourceKind::OverpassAmenity,
        candidate_count: 1,
    }
}

/// 计数数据源：D 工单验收断言重验证期间网络请求数为 0。
struct CountingCollectionSource {
    calls: Arc<AtomicUsize>,
}

impl DataSource for CountingCollectionSource {
    fn source_tag(&self) -> &str {
        "counting-fake"
    }

    fn fetch_raw_entities(&self, _boundary: &Boundary) -> data_acquisition::Result<Vec<RawEntity>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

fn original_boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.4000, 39.9000],
            [116.4010, 39.9000],
            [116.4010, 39.9010],
            [116.4000, 39.9010],
            [116.4000, 39.9000]
        ]]),
    }
}

/// 新边界：覆盖 B、排除 A。
fn shifted_boundary() -> Boundary {
    Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.4008, 39.9008],
            [116.4018, 39.9008],
            [116.4018, 39.9018],
            [116.4008, 39.9018],
            [116.4008, 39.9008]
        ]]),
    }
}

fn shifted_boundary_ipc() -> String {
    r#"{"type":"confirm_boundary","coords":[[116.4008,39.9008],[116.4018,39.9008],[116.4018,39.9018],[116.4008,39.9018],[116.4008,39.9008]]}"#
        .to_owned()
}

fn bowtie_ipc() -> String {
    r#"{"type":"confirm_boundary","coords":[[116.4000,39.9000],[116.4010,39.9010],[116.4010,39.9000],[116.4000,39.9010],[116.4000,39.9000]]}"#
        .to_owned()
}

struct RevalidationWorkspace {
    window: AppWindow,
    _runtime: desktop_shell::ApplicationRuntime,
    _directory: tempfile::TempDir,
    plan_id: String,
    db: Arc<std::sync::Mutex<Database>>,
    network_calls: Arc<AtomicUsize>,
}

fn build_workspace() -> RevalidationWorkspace {
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("s1-32-revalidation.db");
    let export_dir = directory.path().join("exports");
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let network_calls = Arc::new(AtomicUsize::new(0));
    let mut injector = ViewModelInjector::new_with_boundary_and_collection_source(
        ShellDatabases::open(&database_path).expect("open databases"),
        Arc::new(StdExportFileSystem),
        Arc::new(|_, _, _, _| canned_boundary()),
        Arc::new(CountingCollectionSource {
            calls: Arc::clone(&network_calls),
        }),
    )
    .expect("construct injector with counting source");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("complete first run");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("temporary export path"))
        .expect("set export directory");
    let db = injector.projects_mut().shared_database();
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("重验证测试校区")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "重验证测试方案")
        .expect("create plan")
        .to_string();
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));
    RevalidationWorkspace {
        window,
        _runtime,
        _directory: directory,
        plan_id,
        db,
        network_calls,
    }
}

/// 种子：已有采集结果（A 在边界内 Reviewable、B 在边界外 Isolated）、
/// 采集边界指纹、A 的已封账"保留"决定。
fn seed_collection_results(
    db: &Arc<std::sync::Mutex<Database>>,
    plan_id: &str,
) -> (String, String) {
    let mut db = db.lock().expect("database lock");
    let observation_a = RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({
            "name": "教学楼A",
            "tags": {"building": "school"},
            "payload": {"id": "way/1"},
            "source_geometry": {"kind": "point", "coordinates": [116.4003, 39.9003]},
            "geometry_part_id": "point"
        }),
        "overpass",
    );
    let observation_b = RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/2",
        serde_json::json!({
            "name": "教学楼B",
            "tags": {"building": "school"},
            "payload": {"id": "way/2"},
            "source_geometry": {"kind": "point", "coordinates": [116.4012, 39.9012]},
            "geometry_part_id": "point"
        }),
        "overpass",
    );
    db.write_raw_observations(&[observation_a.clone(), observation_b.clone()])
        .expect("write raw observations");
    let drafts = [
        CandidateProjectionDraft::reviewable(
            CandidateSourceIdentity::new("overpass", "way/1", "point"),
            CandidateCategory::Building,
            CandidateDisplay::new("教学楼A", vec![]),
            CandidateShape::point(serde_json::json!([116.4003, 39.9003])),
            ReviewableValidation::Retained,
        ),
        CandidateProjectionDraft::isolated(
            CandidateSourceIdentity::new("overpass", "way/2", "point"),
            CandidateCategory::Building,
            CandidateDisplay::new("教学楼B", vec![]),
            CandidateShape::point(serde_json::json!([116.4012, 39.9012])),
            "outside_confirmed_plan_boundary",
        )
        .expect("隔离事实必须合法"),
    ];
    let published = db
        .publish_candidate_batch(
            plan_id,
            &boundary_fingerprint(&original_boundary()),
            &drafts,
        )
        .expect("atomically publish candidate batch");
    let ids_by_source = db
        .list_current_candidate_projections(plan_id)
        .expect("read current candidate projections")
        .into_iter()
        .map(|projection| (projection.source_entity_id, projection.candidate_id))
        .collect::<std::collections::HashMap<_, _>>();
    let candidate_a = ids_by_source["way/1"].clone();
    let candidate_b = ids_by_source["way/2"].clone();
    db.batch_update_review_decisions_at_revision(
        plan_id,
        &published.batch.id,
        &[ReviewDecision::new(
            plan_id,
            CandidateCategory::Building,
            &candidate_a,
            ReviewState::Keep,
        )],
    )
    .expect("write keep decision");
    (candidate_a, candidate_b)
}

#[test]
fn s1_32_boundary_change_revalidates_locally_without_network() {
    let workspace = build_workspace();
    let (candidate_a, candidate_b) = seed_collection_results(&workspace.db, &workspace.plan_id);
    let plan_id = workspace.plan_id.clone();

    workspace
        .window
        .invoke_plan_list_card_clicked(plan_id.clone().into());
    workspace.window.invoke_workspace_tutorial_dismiss_clicked();
    workspace.window.invoke_workspace_map_status_changed(true);

    // 改边界并确认（不触发 map_ready 自动抓取，直接走确认链路）。
    workspace
        .window
        .invoke_workspace_map_ipc(shifted_boundary_ipc().into());

    assert_eq!(
        workspace.network_calls.load(Ordering::SeqCst),
        0,
        "边界变化后的本地重验证不得产生任何网络请求"
    );
    let db = workspace.db.lock().expect("database lock");
    let all = db
        .list_current_candidate_projections(&plan_id)
        .expect("全量投影");
    let a = all
        .iter()
        .find(|projection| projection.candidate_id == candidate_a)
        .expect("A 投影保留");
    assert_eq!(a.eligibility(), CandidateEligibility::Isolated);
    assert_eq!(
        a.isolation_reason(),
        Some("outside_confirmed_plan_boundary")
    );
    let b = all
        .iter()
        .find(|projection| projection.candidate_id == candidate_b)
        .expect("B 投影");
    assert_eq!(b.eligibility(), CandidateEligibility::Reviewable);

    // 评审台（只读 B2）：A 不再出现，B 进入。
    let reviewable = db
        .list_reviewable_candidate_projections(&plan_id)
        .expect("Reviewable API");
    assert_eq!(reviewable.len(), 1);
    assert_eq!(reviewable[0].candidate_id, candidate_b);

    // A 的旧"保留"决定作废并标注：记录保留 + 作废历史，不物理删除；
    // 常规三态/列表读取不再把作废决定当有效决定。
    let voided = db.list_voided_review_decisions(&plan_id).expect("作废标注");
    assert_eq!(voided.len(), 1);
    assert_eq!(voided[0].candidate_id, candidate_a);
    assert_eq!(voided[0].review_state, ReviewState::Keep);
    assert_eq!(
        voided[0].voided_reason.as_deref(),
        Some("candidate_became_isolated_after_boundary_change")
    );
    let history = db
        .list_review_decision_invalidations(&plan_id)
        .expect("作废历史");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].previous_state, ReviewState::Keep);
    let (pending, keep, remove) = db.count_review_states(&plan_id).expect("三态计数");
    assert_eq!((pending, keep, remove), (1, 0, 0));
    assert_eq!(
        db.load_plan_collection_boundary(&plan_id)
            .expect("边界指纹")
            .as_deref(),
        Some(boundary_fingerprint(&shifted_boundary()).as_str()),
        "重验证后记录最新边界指纹"
    );
    drop(db);

    // 边界未变化：再次确认同一边界 → 不触发重验证（无新作废/写库）。
    workspace
        .window
        .invoke_workspace_map_ipc(shifted_boundary_ipc().into());
    let db = workspace.db.lock().expect("database lock");
    assert_eq!(
        db.list_review_decision_invalidations(&plan_id)
            .expect("作废历史")
            .len(),
        1,
        "边界未变化不得产生新的作废记录"
    );
    assert_eq!(
        db.list_voided_review_decisions(&plan_id)
            .expect("作废标注")
            .len(),
        1
    );
    drop(db);
    assert_eq!(workspace.network_calls.load(Ordering::SeqCst), 0);

    // 无效边界（自相交蝴蝶结）：明确报错，不破坏已确认边界与评审状态。
    workspace
        .window
        .invoke_workspace_map_ipc(bowtie_ipc().into());
    assert!(
        workspace.window.get_error_dialog_visible(),
        "自相交边界必须明确报错"
    );
    let db = workspace.db.lock().expect("database lock");
    assert_eq!(
        db.list_reviewable_candidate_projections(&plan_id)
            .expect("Reviewable API")
            .len(),
        1,
        "无效边界不得改变候选资格"
    );
    assert_eq!(
        db.list_voided_review_decisions(&plan_id)
            .expect("作废标注")
            .len(),
        1,
        "无效边界不得产生新的作废"
    );
    assert_eq!(
        db.load_plan_collection_boundary(&plan_id)
            .expect("边界指纹")
            .as_deref(),
        Some(boundary_fingerprint(&shifted_boundary()).as_str()),
        "无效边界不得改写边界指纹"
    );
    drop(db);

    // ── 评审台进入时按当前已确认边界鉴别（本工单补充）──────────────
    // 模拟旧版本遗留场景：指纹记录与当前边界不一致（旧数据无指纹、或换边界
    // 后未重新确认）。重新进入评审台应触发一次本地重鉴别并纠正指纹，
    // 评审台与 B2 保持一致，全程不联网。
    {
        let mut db = workspace.db.lock().expect("database lock");
        db.save_plan_collection_boundary(&plan_id, "legacy-stale-fingerprint")
            .expect("模拟旧数据指纹");
        drop(db);
    }
    workspace.window.invoke_error_dialog_dismissed();
    // 切到导出步再回评审步，走真实 ReviewRequest::Open。
    workspace.window.invoke_workspace_step_clicked(4);
    workspace.window.invoke_workspace_step_clicked(3);
    {
        let db = workspace.db.lock().expect("database lock");
        assert_eq!(
            db.load_plan_collection_boundary(&plan_id)
                .expect("边界指纹")
                .as_deref(),
            Some(boundary_fingerprint(&shifted_boundary()).as_str()),
            "评审台进入必须按当前已确认边界纠正指纹"
        );
        let reviewable = db
            .list_reviewable_candidate_projections(&plan_id)
            .expect("Reviewable API");
        assert_eq!(reviewable.len(), 1, "评审台只显示当前边界内候选");
        assert_eq!(reviewable[0].candidate_id, candidate_b);
        drop(db);
    }
    assert_eq!(
        workspace.network_calls.load(Ordering::SeqCst),
        0,
        "全程网络请求数必须为 0（含评审台进入重鉴别）"
    );
}
