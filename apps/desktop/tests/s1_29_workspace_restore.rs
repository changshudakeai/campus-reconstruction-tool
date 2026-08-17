//! workspace-restore 工单验收（A 部分）：
//! - 重启（含意外退出，即不做任何退出保存钩子）后恢复到上次打开方案的
//!   方案/步骤/已确认边界/自定义朝向（A.1/A.2/A.3）；
//! - 未封账评审三态检查点恢复，封账终态保持权威（A.4）；
//! - 数据损坏时明确提示并回退到重新圈画，不伪造"已确认"（A.7）；
//! - 采集数据不受恢复影响（A.5，回归 s1_07 套件覆盖）。

use data_acquisition::overpass::{BoundarySourceKind, CampusBoundaryResult};
use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservationsApi,
    ReviewDraftApi, ReviewableValidation, WorkspaceStateApi,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory, PlanId};
use slint::{ComponentHandle, Model};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, BoundaryFetchSource,
    OperationPresentationState, ShellDatabases, ShellPresenter, ViewModelInjector,
};

fn fake_boundary_source() -> BoundaryFetchSource {
    Arc::new(|_, _, _, _progress| CampusBoundaryResult::AutoSelected {
        name: "恢复测试校区".to_owned(),
        gcj02: vec![
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
            [121.40, 31.21],
        ],
        source: BoundarySourceKind::OverpassAmenity,
        candidate_count: 1,
    })
}

/// 与 s1_17 相同的候选夹具：5 建筑 + 1 道路，全部 Reviewable。
/// 返回按写入顺序的稳定候选 ID 列表。
fn seed_review_candidates(database_path: &std::path::Path, plan_id: &PlanId) -> Vec<String> {
    let mut database = Database::open(database_path).expect("打开数据库写候选");
    let mut observations = Vec::new();
    for index in 0..5 {
        observations.push(data_persistence::RawObservation::new(
            plan_id.to_string(),
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼{index}") } }),
            "overpass",
        ));
    }
    observations.push(data_persistence::RawObservation::new(
        plan_id.to_string(),
        CandidateCategory::Road,
        "way/r0",
        serde_json::json!({ "tags": { "highway": "footway" } }),
        "overpass",
    ));
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let plan_key = plan_id.to_string();
    let mut drafts = Vec::new();
    let mut reviewable_sources = Vec::new();
    for observation in &observations {
        let display = CandidateDisplay::new(
            observation.source_data["tags"]["name"]
                .as_str()
                .unwrap_or(&observation.entity_id),
            vec![("source".to_owned(), observation.data_source_tag.clone())],
        );
        drafts.push(CandidateProjectionDraft::reviewable(
            CandidateSourceIdentity::new(
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
            ),
            observation.entity_type,
            display,
            CandidateShape::polygon(serde_json::json!([
                [121.4, 31.2],
                [121.5, 31.2],
                [121.4, 31.3],
                [121.4, 31.2]
            ])),
            ReviewableValidation::Retained,
        ));
        reviewable_sources.push(observation.entity_id.clone());
    }
    database
        .publish_candidate_batch(&plan_key, &workspace_boundary_fingerprint(), &drafts)
        .expect("原子发布候选批次");
    let ids_by_source = database
        .list_reviewable_candidate_projections(&plan_key)
        .expect("读取合法评审候选")
        .into_iter()
        .map(|projection| (projection.source_entity_id, projection.candidate_id))
        .collect::<std::collections::HashMap<_, _>>();
    reviewable_sources
        .into_iter()
        .map(|source| ids_by_source[&source].clone())
        .collect()
}

fn workspace_boundary_fingerprint() -> String {
    boundary_fingerprint(&Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [121.40, 31.20],
            [121.41, 31.20],
            [121.41, 31.21],
            [121.40, 31.21]
        ]]),
    })
}

struct RestartHarness {
    directory: tempfile::TempDir,
    database_path: PathBuf,
    export_dir: PathBuf,
    plan_id: PlanId,
    reviewable: Vec<String>,
}

fn build_harness() -> RestartHarness {
    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("workspace-restore.db");
    let export_dir = directory.path().join("exports");
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(&database_path).expect("连接数据库"))
            .expect("创建注入器");
    injector
        .settings_mut()
        .complete_first_run(&FirstRunSetup {
            language: "zh-CN".into(),
            minecraft_version: "26.1.2".into(),
            acknowledged: true,
        })
        .expect("完成首开设置");
    injector
        .settings_mut()
        .set_gaode_api_key("testapikey1234567890")
        .expect("写入测试密钥");
    injector
        .settings_mut()
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("写入测试安全密钥");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("导出目录"))
        .expect("设置导出目录");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("恢复测试校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "恢复测试方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记住校区");
    drop(injector);
    let reviewable = seed_review_candidates(&database_path, &plan_id);
    RestartHarness {
        directory,
        database_path,
        export_dir,
        plan_id,
        reviewable,
    }
}

/// 启动一个完整应用实例（同一数据库；等同"重启"）。
fn launch(
    harness: &RestartHarness,
    export_dir: &Path,
) -> (AppWindow, ApplicationRuntime, Arc<NotificationCenter>) {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let mut injector = ViewModelInjector::new_with_boundary_source(
        ShellDatabases::open(&harness.database_path).expect("重开数据库"),
        fake_boundary_source(),
    )
    .expect("创建注入器");
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("导出目录"))
        .expect("设置导出目录");
    let runtime = assemble_application(&window, injector, Arc::clone(&center));
    (window, runtime, center)
}

fn pump_until_terminal(window: &AppWindow) -> OperationPresentationState {
    let deadline = Instant::now() + Duration::from_secs(30);
    let weak = window.as_weak();
    let terminal = Arc::new(std::sync::Mutex::new(None));
    let flag = Arc::clone(&terminal);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(20),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let state = window.get_operation_state();
            let terminal_state = matches!(
                state,
                OperationPresentationState::Succeeded | OperationPresentationState::Failed
            );
            if terminal_state || Instant::now() >= deadline {
                *flag.lock().expect("terminal lock") = Some(state);
                slint::quit_event_loop().expect("停止导出轮询");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行导出轮询");
    let value = terminal
        .lock()
        .expect("terminal lock")
        .expect("导出必须到达终态");
    value
}

fn card_state_key(window: &AppWindow, index: usize) -> String {
    window
        .get_review_cards()
        .row_data(index)
        .expect("评审卡片必须存在")
        .state_key
        .to_string()
}

#[test]
fn workspace_restore_acceptance_matrix() {
    // 场景一：确认边界 → 朝向 → 未封账三态 → 重启恢复并可导出
    let harness = build_harness();
    let export_dir_first = harness.directory.path().join("exports-first");
    let plan_key = harness.plan_id.to_string();

    // ── 第一段会话：确认边界 → 设朝向 → 评审三态 → 停在导出步 ──
    {
        let (window, _runtime, _center) = launch(&harness, &export_dir_first);
        window.invoke_plan_list_card_clicked(harness.plan_id.to_string().into());
        window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.41,31.20],[121.41,31.21],[121.40,31.21]]}"#
                .into(),
        );
        assert!(
            window.get_workspace_boundary_is_determined(),
            "第一段会话确认边界必须立即可见"
        );

        window.invoke_workspace_step_clicked(1);
        window.invoke_workspace_map_status_changed(true);
        window.set_workspace_orientation_input_text("90".into());
        window.invoke_workspace_orientation_submit_clicked();
        assert!(
            window.get_workspace_orientation_is_determined(),
            "第一段会话朝向必须已保存"
        );

        window.invoke_workspace_step_clicked(3);
        window.invoke_workspace_map_status_changed(true);
        assert_eq!(window.get_review_candidate_count(), 6);
        window
            .invoke_review_card_state_clicked(harness.reviewable[0].clone().into(), "keep".into());
        window.invoke_review_card_state_clicked(
            harness.reviewable[1].clone().into(),
            "remove".into(),
        );
        assert_eq!(card_state_key(&window, 0), "keep");
        assert_eq!(card_state_key(&window, 1), "remove");
        assert_eq!(card_state_key(&window, 2), "pending");

        window.invoke_workspace_step_clicked(4);
        assert_eq!(window.get_workspace_active_step(), 4);
        // 异常退出：不调用任何退出保存钩子，直接 drop（检查点已随状态变更落库）
    }

    // ── 第二段会话：重启后自动恢复（A.1/A.2/A.3）──
    let (window, _runtime, _center) = launch(&harness, &harness.export_dir.clone());
    // 启动即自动打开上次方案
    assert_eq!(
        window.get_workspace_plan_name().as_str(),
        "恢复测试方案",
        "启动后必须恢复上次打开方案（A.1）"
    );
    assert_eq!(
        window.get_workspace_active_step(),
        4,
        "工作区步骤必须恢复（A.1）"
    );
    assert!(
        window.get_workspace_boundary_is_determined(),
        "边界状态必须为“已确认”，可直接导出（A.2）"
    );
    assert_eq!(window.get_workspace_boundary_point_count(), 4);
    assert!(
        window.get_workspace_orientation_is_determined(),
        "自定义朝向必须恢复（A.3）"
    );
    assert_eq!(window.get_workspace_orientation_angle() as i32, 90);

    // 恢复后直接导出（A.2）：无需重新抓取
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        pump_until_terminal(&window),
        OperationPresentationState::Succeeded,
        "重启后恢复的已确认边界必须可直接导出"
    );
    assert!(
        harness
            .export_dir
            .join(format!("{plan_key}.schem"))
            .is_file(),
        "恢复后导出必须产出 .schem"
    );

    // 未封账三态恢复（A.4）
    window.invoke_workspace_step_clicked(3);
    assert_eq!(window.get_review_candidate_count(), 6);
    assert_eq!(
        card_state_key(&window, 0),
        "keep",
        "未封账的保留决定必须恢复"
    );
    assert_eq!(
        card_state_key(&window, 1),
        "remove",
        "未封账的剔除决定必须恢复"
    );
    assert_eq!(
        card_state_key(&window, 2),
        "pending",
        "未封账的待定状态必须保持"
    );
    // 场景二：数据损坏 → 明确提示 + 回退，不伪造"已确认"
    let harness = build_harness();
    let export_dir_first = harness.directory.path().join("exports-corrupt-first");

    // 第一段：确认边界（写检查点）
    {
        let (window, _runtime, _center) = launch(&harness, &export_dir_first);
        window.invoke_plan_list_card_clicked(harness.plan_id.to_string().into());
        window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.41,31.20],[121.41,31.21],[121.40,31.21]]}"#
                .into(),
        );
        assert!(window.get_workspace_boundary_is_determined());
    }

    // 模拟数据损坏：边界标记为"已确认"但坐标缺失（非法环）——恢复层必须
    // 明确拒绝而非伪造"已确认"（A.7）。
    {
        let mut db = Database::open(&harness.database_path).expect("打开数据库");
        db.save_plan_workspace_state(&data_persistence::PlanWorkspaceState::new(
            harness.plan_id.to_string(),
            "损坏",
            Vec::new(),
            true,
            None,
            0,
        ))
        .expect("写坏数据准备");
    }

    let export_dir_after = harness.directory.path().join("exports-corrupt-after");
    let (window, _runtime, center) = launch(&harness, &export_dir_after);
    assert!(
        !window.get_workspace_boundary_is_determined(),
        "数据损坏时不得静默伪造“已确认”（A.7）"
    );

    // 明确提示（A.7）：B7 一本账应出现"工作现场恢复失败"
    let board = center.board_records();
    assert!(
        board
            .iter()
            .any(|record| record.notification().title.contains("工作现场恢复失败")),
        "恢复失败必须给出明确提示；实际公告：{:?}",
        board
            .iter()
            .map(|record| record.notification().title.as_str())
            .collect::<Vec<_>>()
    );

    // 无边界时导出明确拒绝（不产生产物）
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        pump_until_terminal(&window),
        OperationPresentationState::Failed,
        "损坏数据回退后，无有效边界导出必须失败"
    );
    // 场景三：封账终态权威，残留草稿不得覆盖
    let harness = build_harness();
    let export_dir_first = harness.directory.path().join("exports-seal-first");

    // 第一段：评审 → 保留 1 项 → 封账（终态写 review_decisions）
    {
        let (window, _runtime, _center) = launch(&harness, &export_dir_first);
        window.invoke_plan_list_card_clicked(harness.plan_id.to_string().into());
        window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[121.40,31.20],[121.41,31.20],[121.41,31.21],[121.40,31.21]]}"#
                .into(),
        );
        window.invoke_workspace_step_clicked(3);
        window.invoke_workspace_map_status_changed(true);
        window
            .invoke_review_card_state_clicked(harness.reviewable[0].clone().into(), "keep".into());
        window.invoke_review_seal_clicked();
        assert!(window.get_review_sealed(), "第一段必须封账成功");
    }

    // 模拟封账写入后、草稿清理前的崩溃窗口：手工塞入一条与终态冲突的草稿
    {
        let mut db = Database::open(&harness.database_path).expect("打开数据库");
        let draft = data_persistence::ReviewDraft {
            plan_id: harness.plan_id.to_string(),
            active_category: CandidateCategory::Building,
            entries: vec![data_persistence::ReviewDraftEntry {
                candidate_id: harness.reviewable[0].clone(),
                review_state: shared_domain_types::ReviewState::Remove,
                selected: false,
            }],
        };
        db.save_review_draft(&draft).expect("注入冲突草稿");
    }

    // 第二段：终态必须保持权威，草稿不得覆盖（A.4 封账语义不变）
    let export_dir_after = harness.directory.path().join("exports-seal-after");
    let (window, _runtime, _center) = launch(&harness, &export_dir_after);
    window.invoke_workspace_step_clicked(3);
    assert_eq!(
        card_state_key(&window, 0),
        "keep",
        "封账终态必须恢复且草稿不得覆盖（A.4）"
    );

    // 残留草稿已被清理（终态以 review_decisions 为唯一权威）
    let db = Database::open(&harness.database_path).expect("打开数据库核对");
    assert!(
        db.load_review_draft(&harness.plan_id.to_string())
            .expect("读取草稿")
            .is_none(),
        "封账后残留草稿必须被清理"
    );
    // WebView2 多窗口退出在单进程测试内的 teardown 崩溃（T38 已知 COM 时序）：
    // 最后一个窗口销毁前统一走 run_dev 同款安全释放路径。
    desktop_shell::shutdown();
}
