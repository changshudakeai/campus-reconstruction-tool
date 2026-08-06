//! S1-17 M3 验收：批量确认（含 >=5 项二次确认阈值）、封账写回导出摘要、
//! 重新进入评审台恢复上一轮已封账终态。
use std::sync::Arc;

use data_persistence::{
    CampusCrudApi, CandidateDisplay, CandidateEligibility, CandidateProjection,
    CandidateProjectionsApi, CandidateShape, CandidateValidation, Database, RawObservation,
    RawObservationsApi, ReviewDecisionsApi,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, CandidateCategory};
use slint::Model;

/// 种子：5 栋可评审建筑 + 1 条可评审道路。
fn seed_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    let mut observations = Vec::new();
    for index in 0..5 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼{index}") } }),
            "overpass",
        ));
    }
    observations.push(RawObservation::new(
        plan_id,
        CandidateCategory::Road,
        "way/r0",
        serde_json::json!({ "tags": { "highway": "footway" } }),
        "overpass",
    ));
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
        let display = CandidateDisplay::new(
            observation.source_data["tags"]["name"]
                .as_str()
                .unwrap_or(&observation.entity_id),
            vec![("source".to_owned(), observation.data_source_tag.clone())],
        );
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
                [121.4, 31.2],
                [121.5, 31.2],
                [121.4, 31.3],
                [121.4, 31.2]
            ])),
            CandidateValidation::Retained,
            CandidateEligibility::Reviewable,
        ));
        reviewable.push(candidate_id);
    }
    database
        .write_candidate_projections(&batch.id, &projections)
        .expect("写入候选投影");
    database
        .publish_candidate_batch(&batch.id)
        .expect("发布候选批次");
    reviewable
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
fn review_batch_confirm_seal_and_reopen_restore_terminal_states() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-17-review.db");
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
    let reviewable = {
        let mut database = injector.projects().database();
        seed_candidates(&mut database, &plan_id.to_string())
    };
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    window.invoke_workspace_step_clicked(3);
    assert_eq!(window.get_review_candidate_count(), 6);

    // 全选当前类别（建筑 5 项）→ 批量按钮浮现。
    window.invoke_review_select_all_clicked();
    assert!(window.get_review_bulk_buttons_visible());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 5 }))
    );

    // 批量剔除 5 项（阈值路径）→ 二次确认弹窗。
    window.invoke_review_bulk_state_clicked("remove".into());
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("review.batch_reject_confirm_title")
    );
    assert!(
        window.get_confirm_dialog_body().as_str().contains("5"),
        "确认弹窗正文必须携带待剔除数量"
    );

    // 取消：状态原样不动。
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..5 {
        assert_eq!(
            card_state_key(&window, index),
            "pending",
            "取消二次确认后状态不得改变"
        );
    }

    // 再次批量剔除并确认：5 项全部变为剔除。
    window.invoke_review_bulk_state_clicked("remove".into());
    assert!(window.get_confirm_dialog_visible());
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..5 {
        assert_eq!(card_state_key(&window, index), "remove");
    }

    // 逐项恢复：建筑 0 改回保留；道路切到保留。
    window.invoke_review_card_state_clicked(reviewable[0].clone().into(), "keep".into());
    window.invoke_review_category_clicked(1);
    window.invoke_review_card_state_clicked(reviewable[5].clone().into(), "keep".into());
    assert_eq!(card_state_key(&window, 0), "keep");

    // 封账：成功写出导出摘要（保留 2：建筑 1 + 道路 1；剔除 4；待定 0）。
    window.invoke_review_seal_clicked();
    assert!(window.get_review_sealed());
    assert!(window.get_review_summary_visible());
    let summary = window.get_review_summary_text().to_string();
    assert!(
        summary.contains("保留 2 项")
            && summary.contains("剔除 4 项")
            && summary.contains("待定 0 项"),
        "封账摘要必须如实计数：{summary}"
    );
    assert!(
        summary.contains("建筑 1") && summary.contains("道路 1"),
        "封账摘要必须按类别列出保留明细：{summary}"
    );

    // 数据库终态落账：(待定, 保留, 剔除) = (0, 2, 4)。
    let db = Database::open(&database_path).expect("打开数据库核对终态");
    let (pending, keep, remove) = db
        .count_review_states(&plan_id.to_string())
        .expect("统计评审终态");
    assert_eq!((pending, keep, remove), (0, 2, 4));

    // 重新进入评审台：上一轮封账终态对回（保留/剔除）。
    window.invoke_workspace_step_clicked(2);
    window.invoke_workspace_step_clicked(3);
    assert_eq!(window.get_review_candidate_count(), 6);
    assert_eq!(card_state_key(&window, 0), "keep");
    for index in 1..5 {
        assert_eq!(card_state_key(&window, index), "remove");
    }
    window.invoke_review_category_clicked(1);
    assert_eq!(card_state_key(&window, 0), "keep");
}
