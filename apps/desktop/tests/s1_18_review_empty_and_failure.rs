//! S1-18 M3 验收：无候选空态不阻塞导出、不伪造评审完成；
//! 封账写回失败经 B7 呈现结构化失败，评审状态保持可修改，无伪成功产物。
use std::path::PathBuf;
use std::sync::Arc;

use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservation,
    RawObservationsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory};
use slint::Model;

fn seed_two_buildings(database: &mut Database, plan_id: &str) -> Vec<String> {
    let observations: Vec<_> = (0..2)
        .map(|index| {
            RawObservation::new(
                plan_id,
                CandidateCategory::Building,
                format!("way/b{index}"),
                serde_json::json!({ "tags": { "name": format!("教学楼{index}") } }),
                "overpass",
            )
        })
        .collect();
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let drafts: Vec<_> = observations
        .iter()
        .map(|observation| {
            CandidateProjectionDraft::reviewable(
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
                    Vec::new(),
                ),
                CandidateShape::polygon(serde_json::json!([
                    [121.4, 31.2],
                    [121.5, 31.2],
                    [121.4, 31.3],
                    [121.4, 31.2]
                ])),
                ReviewableValidation::Retained,
            )
        })
        .collect();
    database
        .publish_candidate_batch(plan_id, &review_boundary_fingerprint(), &drafts)
        .expect("原子发布候选批次");
    database
        .list_reviewable_candidate_projections(plan_id)
        .expect("读取合法评审候选")
        .into_iter()
        .map(|projection| projection.candidate_id)
        .collect()
}

fn review_boundary_fingerprint() -> String {
    boundary_fingerprint(&Boundary {
        r#type: "Polygon".to_owned(),
        coordinates: serde_json::json!([[
            [116.40, 39.90],
            [116.41, 39.90],
            [116.41, 39.91],
            [116.40, 39.91]
        ]]),
    })
}

fn open_plan_and_review(
    window: &AppWindow,
    center: &Arc<NotificationCenter>,
    injector: ViewModelInjector,
    plan_id: &str,
) {
    let _runtime = assemble_application(window, injector, Arc::clone(center));
    window.invoke_plan_list_card_clicked(plan_id.into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#.into(),
    );
    window.invoke_workspace_step_clicked(3);
    window.invoke_workspace_map_status_changed(true);
}

fn build_injector(directory: &tempfile::TempDir, name: &str) -> (ViewModelInjector, String) {
    let database_path = directory.path().join(name);
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
        .expect("完成首次设置");
    let campus = injector
        .projects_mut()
        .database()
        .create_campus("验收校区")
        .expect("创建校区");
    let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "验收方案")
        .expect("创建方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    (injector, plan_id.to_string())
}

fn journal_path(database_path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}-journal", database_path.display()))
}

#[test]
fn review_empty_state_does_not_block_export_and_seal_failure_is_b7_visible() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");

    // ── 场景一：A1 未解锁 / 候选为空 → 评审页明确空态，不阻塞导出、不伪造完成。──
    {
        let window = AppWindow::new().expect("创建 AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));
        let directory = tempfile::tempdir().expect("临时目录");
        let database_path = directory.path().join("s1-18-empty.db");
        let (injector, plan_id) = build_injector(&directory, "s1-18-empty.db");
        open_plan_and_review(&window, &center, injector, &plan_id);

        assert_eq!(window.get_workspace_active_step(), 3);
        assert_eq!(window.get_review_candidate_count(), 0);
        assert_eq!(
            window.get_review_empty_text().as_str(),
            l10n.t("review.empty"),
            "无候选必须显示明确空态文案"
        );
        assert!(!window.get_review_sealed(), "空态不得伪造评审完成");
        assert!(!window.get_review_summary_visible(), "空态不得显示封账摘要");
        assert!(!window.get_error_dialog_visible(), "空态不得报错阻塞");

        // 导出入口不被阻塞：直接进入导出步骤且不呈现失败。
        window.invoke_workspace_step_clicked(4);
        assert_eq!(window.get_workspace_active_step(), 4);
        assert_ne!(
            window.get_operation_state(),
            OperationPresentationState::Failed,
            "无候选评审不得阻塞或污染导出"
        );
        let _ = &database_path;
    }

    // ── 场景二：封账写回失败 → B7 结构化失败、状态可继续修改、无伪成功产物。──
    {
        let window = AppWindow::new().expect("创建 AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));
        let directory = tempfile::tempdir().expect("临时目录");
        let database_path = directory.path().join("s1-18-seal-failure.db");
        let (injector, plan_id) = build_injector(&directory, "s1-18-seal-failure.db");
        let candidate_ids = {
            let mut database = injector.projects().database();
            seed_two_buildings(&mut database, &plan_id)
        };
        open_plan_and_review(&window, &center, injector, &plan_id);
        assert_eq!(window.get_review_candidate_count(), 2);
        window.invoke_review_card_state_clicked(candidate_ids[0].clone().into(), "keep".into());

        // 用“-journal 位置被目录占用”模拟写回失败（Windows 下 SQLite 无法打开 journal）。
        let journal = journal_path(&database_path);
        std::fs::create_dir(&journal).expect("创建阻塞 journal 目录");
        window.invoke_review_seal_clicked();

        assert!(
            window.get_error_dialog_visible(),
            "封账写回失败必须经 B7 错误弹窗呈现"
        );
        assert_eq!(
            window.get_error_dialog_title().as_str(),
            l10n.t("review.seal_failed_title")
        );
        assert_eq!(
            window.get_error_dialog_body().as_str(),
            l10n.t("review.seal_failed_body")
        );
        assert_eq!(
            window.get_operation_state(),
            OperationPresentationState::Failed
        );
        assert!(!window.get_review_sealed(), "写回失败不得置为已封账");
        assert!(
            !window.get_review_summary_visible(),
            "写回失败不得显示导出摘要（无伪成功产物）"
        );
        window.invoke_error_dialog_dismissed();

        // 恢复可写后，评审状态仍可继续修改并最终封账成功。
        std::fs::remove_dir(&journal).expect("移除阻塞 journal 目录");
        window.invoke_review_card_state_clicked(candidate_ids[1].clone().into(), "keep".into());
        window.invoke_review_state_tab_clicked(1);
        assert_eq!(
            window.get_review_cards().row_count(),
            2,
            "保留分组应为 2 项"
        );
        for index in 0..2 {
            assert_eq!(
                window
                    .get_review_cards()
                    .row_data(index)
                    .expect("评审卡片必须存在")
                    .state_key
                    .as_str(),
                "keep",
                "封账失败后评审状态必须保持可修改"
            );
        }
        window.invoke_review_seal_clicked();
        assert!(window.get_review_sealed());
        assert!(window.get_review_summary_visible());
        assert!(
            window
                .get_review_summary_text()
                .as_str()
                .contains("保留 2 项"),
            "恢复后封账成功必须写出真实摘要"
        );
    }
}
