//! S1-17 M3 验收：批量确认（含 >=5 项二次确认阈值）、封账写回导出摘要、
//! 重新进入评审台恢复上一轮已封账终态。
use std::sync::Arc;

use data_persistence::CandidateNameSource;
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

/// 建议夹具：混合信号的可评审候选（几何互不重叠，避免干扰建议规则）。
fn seed_suggestion_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    fn ring(offset: f64) -> serde_json::Value {
        serde_json::json!([
            [121.4 + offset, 31.2],
            [121.5 + offset, 31.2],
            [121.5 + offset, 31.3],
            [121.4 + offset, 31.3],
            [121.4 + offset, 31.2]
        ])
    }

    let observations = vec![
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b0",
            serde_json::json!({ "tags": { "name": "教学楼甲", "building": "school" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b1",
            serde_json::json!({ "tags": { "name": "教学楼乙", "building": "school" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b2",
            serde_json::json!({ "tags": { "building": "yes" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b3",
            serde_json::json!({ "tags": { "name": "教学楼甲", "building": "school" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b4",
            serde_json::json!({ "tags": { "name": "实验楼", "building": "lab" } }),
            "overpass",
        ),
    ];
    database
        .write_raw_observations(&observations)
        .expect("写入建议夹具原始观测");
    let batch = database
        .prepare_candidate_batch(plan_id)
        .expect("准备建议夹具候选批次");

    let mut projections = Vec::new();
    let mut reviewable = Vec::new();
    for (index, observation) in observations.iter().enumerate() {
        let candidate_id = format!("overpass:{}:outer", observation.entity_id);
        let (title, tags, validation, name_source) = match index {
            // b0 干净 → 建议保留
            0 => (
                "教学楼甲".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                CandidateValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b1 干净 → 建议保留
            1 => (
                "教学楼乙".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                CandidateValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b2 未命名 → 建议人工确认（未命名）
            2 => (
                observation.entity_id.clone(),
                vec![("building".to_owned(), "yes".to_owned())],
                CandidateValidation::Retained,
                CandidateNameSource::Unnamed,
            ),
            // b3 与 b0 同来源实体 + 同几何 → 重复投影 → 建议剔除
            3 => (
                "教学楼甲".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                CandidateValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b4 几何自动修复 → 建议人工确认（形状经修复）
            _ => (
                "实验楼".to_owned(),
                vec![("building".to_owned(), "lab".to_owned())],
                CandidateValidation::Repaired,
                CandidateNameSource::Osm,
            ),
        };
        let source_entity_id = if index == 3 {
            "way/b0"
        } else {
            &observation.entity_id
        };
        // b3 与 b0 同来源实体 + 同几何（重复投影）；其余互不重叠。
        let geometry_offset = if index == 3 { 0.0 } else { index as f64 * 0.2 };
        projections.push(
            CandidateProjection::new(
                &candidate_id,
                plan_id,
                &observation.id,
                &observation.data_source_tag,
                source_entity_id,
                "default",
                observation.entity_type,
                CandidateDisplay::new(title, tags),
                CandidateShape::polygon(ring(geometry_offset)),
                validation,
                CandidateEligibility::Reviewable,
            )
            .with_name_source(name_source),
        );
        reviewable.push(candidate_id);
    }
    database
        .write_candidate_projections(&batch.id, &projections)
        .expect("写入建议夹具候选投影");
    database
        .publish_candidate_batch(&batch.id)
        .expect("发布建议夹具候选批次");
    reviewable
}

/// 进入指定方案的评审步：方案列表点卡 → 边界确认 → 评审步。
fn enter_review(window: &AppWindow, plan_id: &str) {
    window.invoke_plan_list_card_clicked(plan_id.into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    for (x, y) in [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)] {
        window.invoke_workspace_boundary_canvas_clicked(x, y);
    }
    window.invoke_workspace_boundary_confirm_clicked();
    window.invoke_workspace_step_clicked(3);
}

/// 从工作区返回方案列表（若存在离开确认弹窗则确认；评审步无未保存边界/
/// 运行中操作时直接放行，S1-13 语义）。
fn leave_to_plan_list(window: &AppWindow) {
    window.invoke_workspace_back_to_plan_list_clicked();
    if window.get_confirm_dialog_visible() {
        window.invoke_confirm_dialog_confirmed();
    }
}

/// 轻量建议辅助的 AppWindow 契约：筛选组合、一键应用确认/取消、撤销恢复、
/// 仅生成建议不改三态、封账后不可撤销。
fn run_suggestion_contract(
    window: &AppWindow,
    l10n: &Localization,
    database_path: &std::path::Path,
    plan_id: &str,
    reviewable: &[String],
) {
    enter_review(window, plan_id);
    assert_eq!(window.get_review_candidate_count(), 5);

    // 建议筛选区可见：五个筛选器 + 计数（建议保留 2 / 建议剔除 1 / 未命名 1 /
    // 建议人工确认 2 / 需要关注 2）。
    let labels: Vec<String> = window
        .get_review_suggestion_filter_labels()
        .iter()
        .map(|label| label.to_string())
        .collect();
    assert_eq!(labels.len(), 5);
    assert_eq!(
        window.get_review_suggestion_filters_label().as_str(),
        l10n.t("review.suggestion_filters_label")
    );
    assert!(
        labels[0].contains("需要关注") && labels[0].contains("2"),
        "筛选标签 0 不符：{labels:?}"
    );
    assert!(
        labels[1].contains("未命名") && labels[1].contains("1"),
        "筛选标签 1 不符：{labels:?}"
    );
    assert!(
        labels[2].contains("建议保留") && labels[2].contains("2"),
        "筛选标签 2 不符：{labels:?}"
    );
    assert!(
        labels[3].contains("建议人工确认") && labels[3].contains("2"),
        "筛选标签 3 不符：{labels:?}"
    );
    assert!(
        labels[4].contains("建议剔除") && labels[4].contains("1"),
        "筛选标签 4 不符：{labels:?}"
    );

    // 仅生成建议：五张卡片都未裁决（待定），生成建议不改变 ReviewState。
    for index in 0..5 {
        assert_eq!(card_state_key(window, index), "pending");
    }

    // 切换"建议保留"筛选：列表收缩为 2 张建议保留卡，应用按钮可用。
    window.invoke_review_suggestion_filter_clicked(2);
    assert_eq!(window.get_review_candidate_count(), 5);
    assert_eq!(window.get_review_cards().row_count(), 2);
    assert!(window.get_review_apply_suggestions_enabled());
    let cards = window.get_review_cards();
    let suggestions: Vec<String> = (0..cards.row_count())
        .map(|index| cards.row_data(index).unwrap().suggestion_reason.to_string())
        .collect();
    assert!(
        suggestions.iter().all(|reason| !reason.is_empty()),
        "建议保留筛选下的每张卡都必须带可读理由：{suggestions:?}"
    );

    // 一键应用 → 确认弹窗：对象数量 + 主要理由分布。
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("review.apply_suggest_confirm_title")
    );
    let body = window.get_confirm_dialog_body().to_string();
    assert!(
        body.contains("2") && body.contains("保留 2 项"),
        "确认框必须显示对象数量与保留/剔除拆分：{body}"
    );
    assert!(
        body.contains(&l10n.t("review.apply_suggest_reason_label")) && body.contains("无需处理"),
        "确认框必须显示主要理由分布：{body}"
    );

    // 取消：状态不变、无撤销可用。
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..2 {
        assert_eq!(card_state_key(window, index), "pending");
    }
    assert!(!window.get_review_undo_available());

    // 再次应用并确认：b0/b1 变为保留。
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    assert_eq!(card_state_key(window, 0), "keep");
    assert_eq!(card_state_key(window, 1), "keep");
    assert!(window.get_review_undo_available());

    // 撤销上一批：恢复到应用前（全部待定）。
    window.invoke_review_undo_suggestions_clicked();
    assert!(!window.get_review_undo_available());
    for index in 0..2 {
        assert_eq!(card_state_key(window, index), "pending");
    }

    // 建议剔除批：切换"建议剔除"筛选 → 1 张卡（b3 重复投影）。
    window.invoke_review_suggestion_filter_clicked(2); // 关闭"建议保留"
    window.invoke_review_suggestion_filter_clicked(4); // 开启"建议剔除"
    assert_eq!(window.get_review_cards().row_count(), 1);
    assert_eq!(
        window
            .get_review_cards()
            .row_data(0)
            .unwrap()
            .candidate_id
            .to_string(),
        reviewable[3]
    );
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    let body = window.get_confirm_dialog_body().to_string();
    assert!(
        body.contains("剔除 1 项") && body.contains("重复投影"),
        "剔除批确认框必须显示数量与理由：{body}"
    );
    window.invoke_confirm_dialog_confirmed();
    assert_eq!(card_state_key(window, 0), "remove");

    // 撤销：b3 回到待定。
    window.invoke_review_undo_suggestions_clicked();
    assert_eq!(card_state_key(window, 0), "pending");

    // 封账后不可撤销：先应用一批（保留 + 剔除混合批），再封账，
    // 撤销按钮不可用且请求被拒绝。
    window.invoke_review_suggestion_filter_clicked(4); // 关闭"建议剔除"
    window.invoke_review_suggestion_filter_clicked(2); // 开启"建议保留"
    window.invoke_review_suggestion_filter_clicked(4); // 同时开启"建议剔除"
    window.invoke_review_apply_suggestions_clicked();
    window.invoke_confirm_dialog_confirmed();
    assert!(window.get_review_undo_available());
    window.invoke_review_seal_clicked();
    assert!(window.get_review_sealed());
    assert!(!window.get_review_undo_available(), "封账后不可撤销");
    assert!(
        !window.get_review_apply_suggestions_enabled(),
        "封账后不可再应用建议"
    );

    // 数据库终态落账：b0/b1 保留、b3 剔除、b2/b4 待定（应用 + 封账的最终决定）。
    let db = Database::open(database_path).expect("打开数据库核对终态");
    let (pending, keep, remove) = db.count_review_states(plan_id).expect("统计评审终态");
    assert_eq!((pending, keep, remove), (2, 2, 1));
}

/// 既有 M3 批量确认/封账/重进恢复终态的 AppWindow 契约（保持原语义）。
fn run_batch_contract(
    window: &AppWindow,
    l10n: &Localization,
    database_path: &std::path::Path,
    plan_id: &str,
    reviewable: &[String],
) {
    enter_review(window, plan_id);
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
            card_state_key(window, index),
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
        assert_eq!(card_state_key(window, index), "remove");
    }

    // 逐项恢复：建筑 0 改回保留；道路切到保留。
    window.invoke_review_card_state_clicked(reviewable[0].clone().into(), "keep".into());
    window.invoke_review_category_clicked(1);
    window.invoke_review_card_state_clicked(reviewable[5].clone().into(), "keep".into());
    assert_eq!(card_state_key(window, 0), "keep");

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
    let db = Database::open(database_path).expect("打开数据库核对终态");
    let (pending, keep, remove) = db.count_review_states(plan_id).expect("统计评审终态");
    assert_eq!((pending, keep, remove), (0, 2, 4));

    // 重新进入评审台：上一轮封账终态对回（保留/剔除）。
    window.invoke_workspace_step_clicked(2);
    window.invoke_workspace_step_clicked(3);
    assert_eq!(window.get_review_candidate_count(), 6);
    assert_eq!(card_state_key(window, 0), "keep");
    for index in 1..5 {
        assert_eq!(card_state_key(window, index), "remove");
    }
    window.invoke_review_category_clicked(1);
    assert_eq!(card_state_key(window, 0), "keep");
}

/// S1-17 单个 AppWindow 测试（Slint/winit 单事件循环约束：一个测试二进制
/// 只允许一个真实窗口）：先走轻量建议辅助契约（方案 A），再走既有批量确认/
/// 封账契约（方案 B）。
#[test]
fn s1_17_review_workbench_batch_and_suggestion_contracts() {
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
    let suggestion_plan = injector
        .projects_mut()
        .create_plan(&campus_id, "建议验收方案")
        .expect("创建建议方案");
    let batch_plan = injector
        .projects_mut()
        .create_plan(&campus_id, "批量验收方案")
        .expect("创建批量方案");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("记录最近校区");
    let suggestion_reviewable = {
        let mut database = injector.projects().database();
        seed_suggestion_candidates(&mut database, &suggestion_plan.to_string())
    };
    let batch_reviewable = {
        let mut database = injector.projects().database();
        seed_candidates(&mut database, &batch_plan.to_string())
    };
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    // 阶段一：轻量建议辅助契约（方案 A；结束时已封账）。
    run_suggestion_contract(
        &window,
        &l10n,
        &database_path,
        &suggestion_plan.to_string(),
        &suggestion_reviewable,
    );

    // 阶段二：既有批量确认/封账契约（方案 B；含重进恢复终态）。
    leave_to_plan_list(&window);
    run_batch_contract(
        &window,
        &l10n,
        &database_path,
        &batch_plan.to_string(),
        &batch_reviewable,
    );
}
