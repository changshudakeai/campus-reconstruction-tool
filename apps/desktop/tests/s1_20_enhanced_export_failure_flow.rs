//! M4 S1 失败验收：增强导出写入失败 → 结构化失败经 B7 呈现，
//! 不产生伪成功产物（复用 ExportFileSystem 故障注入）。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservation,
    RawObservationsApi, ReviewDecision, ReviewDecisionsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ApplicationRuntime, OperationPresentationState,
    ShellDatabases, ShellPresenter, ViewModelInjector,
};
use export_flow::{ExportFileKind, ExportFileSystem, StdExportFileSystem};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory, PlanId, ReviewState};
use slint::ComponentHandle;

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
    ];
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let mut drafts = Vec::new();
    let mut reviewable_sources = Vec::new();
    for observation in &observations {
        drafts.push(CandidateProjectionDraft::reviewable(
            CandidateSourceIdentity::new(
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
            ),
            observation.entity_type,
            CandidateDisplay::new(
                observation.source_data["tags"]["name"]
                    .as_str()
                    .unwrap_or(&observation.entity_id),
                vec![("height".to_owned(), "12".to_owned())],
            ),
            CandidateShape::polygon(serde_json::json!([
                [116.0001, 39.0001],
                [116.0005, 39.0001],
                [116.0005, 39.0005],
                [116.0001, 39.0001]
            ])),
            ReviewableValidation::Retained,
        ));
        reviewable_sources.push(observation.entity_id.clone());
    }
    let published = database
        .publish_candidate_batch(plan_id, &export_boundary_fingerprint(), &drafts)
        .expect("原子发布候选批次");
    let ids_by_source = database
        .list_reviewable_candidate_projections(plan_id)
        .expect("读取合法评审候选")
        .into_iter()
        .map(|projection| (projection.source_entity_id, projection.candidate_id))
        .collect::<std::collections::HashMap<_, _>>();
    let reviewable = reviewable_sources
        .into_iter()
        .map(|source| ids_by_source[&source].clone())
        .collect::<Vec<_>>();
    database
        .batch_update_review_decisions_at_revision(
            plan_id,
            &published.batch.id,
            &[
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
            ],
        )
        .expect("封账写回");
}

fn export_boundary_fingerprint() -> String {
    boundary_fingerprint(&Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.0, 39.0],
            [116.001, 39.0],
            [116.001, 39.001],
            [116.0, 39.001]
        ]]),
    })
}

struct TestApp {
    _directory: tempfile::TempDir,
    window: AppWindow,
    _center: Arc<NotificationCenter>,
    _runtime: ApplicationRuntime,
    _l10n: Localization,
    plan_id: PlanId,
    export_dir: PathBuf,
}

impl TestApp {
    fn new(file_system: Arc<dyn ExportFileSystem>) -> Self {
        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
        desktop_shell::set_webview_creation_probe(true);
        let window = AppWindow::new().expect("创建 AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("临时目录");
        let database_path = directory.path().join("m4-enhanced-failure.db");
        let export_dir = directory.path().join("exports");
        let databases = ShellDatabases::open(&database_path).expect("打开正式连接组");
        let mut injector = ViewModelInjector::new_with_export_file_system(databases, file_system)
            .expect("创建注入器");
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
            .database()
            .create_campus("M4 校区")
            .expect("创建校区");
        let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "M4 增强导出失败")
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
            _l10n: l10n,
            plan_id,
            export_dir,
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

#[derive(Clone)]
struct FailingFileSystem {
    standard: Arc<StdExportFileSystem>,
    fail_manifest_staging: Arc<AtomicBool>,
}

impl ExportFileSystem for FailingFileSystem {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.standard.create_dir_all(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if self.fail_manifest_staging.load(Ordering::SeqCst)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(".m1-manifest-"))
        {
            return Err(io::Error::other("注入 manifest staging 写失败"));
        }
        self.standard.write(path, contents)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
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
fn enhanced_export_failure_is_presented_via_b7_without_fake_artifacts() {
    let file_system = FailingFileSystem {
        standard: Arc::new(StdExportFileSystem),
        fail_manifest_staging: Arc::new(AtomicBool::new(true)),
    };
    let app = TestApp::new(Arc::new(file_system));
    app.confirm_boundary_and_open_export_step();

    app.window.invoke_workspace_export_start_clicked();
    pump_until_terminal(&app.window, Duration::from_secs(5));
    assert_eq!(
        app.window.get_operation_state(),
        OperationPresentationState::Failed,
        "增强导出失败不得呈现为成功"
    );
    assert!(
        app.window.get_error_dialog_visible(),
        "结构化失败必须经 B7 错误弹窗呈现"
    );
    assert!(!app.schematic_path().exists(), "不得留下伪成功 .schem");
    assert!(!app.manifest_path().exists(), "不得留下伪成功 manifest");
    app.window.invoke_error_dialog_dismissed();
}
