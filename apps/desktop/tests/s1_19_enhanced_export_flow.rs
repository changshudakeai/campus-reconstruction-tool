//! M4 S1 验收：已封账保留候选 → 导出页呈现增强提示 → 一次开始意图
//! 经 A2 路由到增强导出；失败经 B7 呈现且不产生伪成功产物。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
    ReviewDecision, ReviewDecisionsApi,
};
use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use manifest_generator::ExportKind;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, CandidateCategory, PlanId, ReviewState};
use slint::ComponentHandle;

/// 真实发布候选投影 + 封账写回：2 保留建筑 + 1 待定 + 1 剔除 + 1 隔离。
fn seed_sealed_review(database: &mut Database, plan_id: &str) {
    let observations = vec![
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b0",
            serde_json::json!({ "tags": { "name": "教学楼A" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b1",
            serde_json::json!({ "tags": { "name": "教学楼B" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Road,
            "way/r0",
            serde_json::json!({ "tags": { "name": "待定道路" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Water,
            "way/w0",
            serde_json::json!({ "tags": { "name": "剔除水域" } }),
            "overpass",
        ),
    ];
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let batch = database
        .prepare_candidate_batch(plan_id)
        .expect("准备候选批次");
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
            plan_id,
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
    projections.push(
        CandidateProjection::new(
            "overpass:way/iso:outer",
            plan_id,
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
    database
        .write_candidate_projections(&batch.id, &projections)
        .expect("写入候选投影");
    database
        .publish_candidate_batch(&batch.id)
        .expect("发布候选批次");
    database
        .batch_update_review_decisions(&[
            ReviewDecision::new(
                plan_id,
                CandidateCategory::Building,
                &reviewable[0],
                ReviewState::Keep,
            ),
            ReviewDecision::new(
                plan_id,
                CandidateCategory::Building,
                &reviewable[1],
                ReviewState::Keep,
            ),
            ReviewDecision::new(
                plan_id,
                CandidateCategory::Road,
                &reviewable[2],
                ReviewState::Pending,
            ),
            ReviewDecision::new(
                plan_id,
                CandidateCategory::Water,
                &reviewable[3],
                ReviewState::Remove,
            ),
        ])
        .expect("封账写回");
}

struct TestApp {
    _directory: tempfile::TempDir,
    window: AppWindow,
    _center: Arc<NotificationCenter>,
    _runtime: ApplicationRuntime,
    plan_id: PlanId,
    export_dir: PathBuf,
    database_path: PathBuf,
}

impl TestApp {
    fn new() -> Self {
        let window = AppWindow::new().expect("创建 AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("临时目录");
        let database_path = directory.path().join("m4-enhanced.db");
        let export_dir = directory.path().join("exports");
        let databases = ShellDatabases::open(&database_path).expect("打开正式连接组");
        let mut injector = ViewModelInjector::new(databases).expect("创建注入器");
        injector
            .settings_mut()
            .complete_first_run(&FirstRunSetup {
                language: "zh-CN".into(),
                minecraft_version: "26.1.2".into(),
                acknowledged: true,
            })
            .expect("完成首次设置");
        injector
            .settings_mut()
            .set_default_export_location(export_dir.to_str().expect("临时路径有效"))
            .expect("设置导出目录");
        let campus = injector
            .projects_mut()
            .create_campus("M4 校区")
            .expect("创建校区");
        let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "M4 增强导出")
            .expect("创建方案");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("记录最近校区");
        {
            let mut database = injector.projects().database();
            seed_sealed_review(&mut database, &plan_id.to_string());
        }
        let runtime = assemble_application(&window, injector, Arc::clone(&center));

        window.invoke_plan_list_card_clicked(plan_id.to_string().into());
        assert_eq!(window.get_active_screen(), 4);

        Self {
            _directory: directory,
            window,
            _center: center,
            _runtime: runtime,
            plan_id,
            export_dir,
            database_path,
        }
    }

    fn schematic_path(&self) -> PathBuf {
        self.export_dir.join(format!("{}.schem", self.plan_id))
    }

    fn manifest_path(&self) -> PathBuf {
        self.export_dir
            .join(format!("{}.foundation_manifest.json", self.plan_id))
    }

    fn confirm_boundary_and_open_export_step(&self) {
        let raw_confirm = r#"{"type":"confirm_boundary","coords":[[116.0,39.0],[116.001,39.0],[116.001,39.001],[116.0,39.001]]}"#;
        self.window.invoke_workspace_map_ipc(raw_confirm.into());
        assert!(self.window.get_workspace_boundary_is_determined());
        self.window.invoke_workspace_step_clicked(4);
        assert_eq!(self.window.get_workspace_active_step(), 4);
    }
}

fn pump_until_terminal(window: &AppWindow, deadline: Duration) {
    let deadline_at = Instant::now() + deadline;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if window.get_operation_state() != OperationPresentationState::Processing
                || Instant::now() >= deadline_at
            {
                slint::quit_event_loop().expect("停止导出事件循环");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("运行导出事件循环");
}

#[test]
fn sealed_keep_candidates_show_enhanced_hint_and_export_enhanced_content() {
    let app = TestApp::new();
    app.confirm_boundary_and_open_export_step();

    let subtitle = app.window.get_workspace_placeholder_subtitle().to_string();
    assert!(
        subtitle.contains("增强导出") && subtitle.contains("保留候选 2 项"),
        "导出页必须呈现增强提示：{subtitle}"
    );
    assert!(subtitle.contains("待定 1 项") && subtitle.contains("剔除 1 项"));

    app.window.invoke_workspace_export_start_clicked();
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Processing,
        "Start 必须先返回处理中"
    );
    pump_until_terminal(&app.window, Duration::from_secs(5));
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Succeeded
    );

    let inspection =
        sponge_export::inspect_schematic(&app.schematic_path()).expect(".schem 可解析");
    assert!(
        inspection.dimensions[1] > 1,
        "增强导出必须包含保留建筑的高度内容"
    );
    assert!(
        inspection.non_air_voxels > inspection.dimensions[0] * inspection.dimensions[2],
        "增强导出方块计数必须大于纯基础场地"
    );
    let manifest = manifest_generator::FoundationManifest::from_json(
        &std::fs::read_to_string(app.manifest_path()).expect("manifest 可读"),
    )
    .expect("manifest 有效");
    assert_eq!(manifest.export_kind, ExportKind::Enhanced);
    assert_eq!(manifest.candidate_facts.retained_candidate_count, 2);
    assert_eq!(manifest.candidate_facts.review_decision_count, 4);
    let buildings = manifest
        .candidate_facts
        .keep_by_category
        .iter()
        .find(|entry| entry.category == "Building")
        .expect("建筑类别计数");
    assert_eq!(buildings.count, 2);
    assert!(
        manifest
            .candidate_facts
            .keep_by_category
            .iter()
            .all(|entry| entry.category == "Building"),
        "待定道路与剔除水域不得进入保留计数"
    );

    // 导出路径不制造空封账记录：评审终态仍是种子写回的那 4 条。
    let database = Database::open(&app.database_path).expect("重新打开数据库");
    let decisions = database
        .list_review_decisions(&app.plan_id.to_string())
        .expect("读取评审决定");
    assert_eq!(decisions.len(), 4);
}
