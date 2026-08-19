//! S1-17 M3 验收：批量确认（含 >=5 项二次确认阈值）、封账写回导出摘要、
//! 重新进入评审台恢复上一轮已封账终态。
use std::sync::Arc;

use data_persistence::CandidateNameSource;
use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservation,
    RawObservationsApi, ReviewDecisionsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory};
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
        .publish_candidate_batch(plan_id, &review_boundary_fingerprint(), &drafts)
        .expect("原子发布候选批次");
    let ids_by_source = database
        .list_reviewable_candidate_projections(plan_id)
        .expect("读取合法评审候选")
        .into_iter()
        .map(|projection| (projection.source_entity_id, projection.candidate_id))
        .collect::<std::collections::HashMap<_, _>>();
    reviewable_sources
        .into_iter()
        .map(|source| ids_by_source[&source].clone())
        .collect()
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
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b5",
            serde_json::json!({ "tags": { "name": "岗亭", "building": "yes" } }),
            "overpass",
        ),
    ];
    database
        .write_raw_observations(&observations)
        .expect("写入建议夹具原始观测");
    let mut drafts = Vec::new();
    let mut reviewable_sources = Vec::new();
    for (index, observation) in observations.iter().enumerate() {
        let (title, tags, validation, name_source) = match index {
            // b0 干净 → 建议保留
            0 => (
                "教学楼甲".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                ReviewableValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b1 干净 → 建议保留
            1 => (
                "教学楼乙".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                ReviewableValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b2 未命名 → 建议人工确认（未命名）
            2 => (
                observation.entity_id.clone(),
                vec![("building".to_owned(), "yes".to_owned())],
                ReviewableValidation::Retained,
                CandidateNameSource::Unnamed,
            ),
            // b3 与 b0 同来源实体 + 同几何 → 重复投影 → 建议剔除
            3 => (
                "教学楼甲".to_owned(),
                vec![("building".to_owned(), "school".to_owned())],
                ReviewableValidation::Retained,
                CandidateNameSource::Osm,
            ),
            // b4 几何自动修复 → 建议人工确认（形状经修复，低）
            4 => (
                "实验楼".to_owned(),
                vec![("building".to_owned(), "lab".to_owned())],
                ReviewableValidation::Repaired,
                CandidateNameSource::Osm,
            ),
            // b5 建筑点形状可疑 → 建议人工确认（中）
            _ => (
                "岗亭".to_owned(),
                vec![("building".to_owned(), "yes".to_owned())],
                ReviewableValidation::Retained,
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
        let geometry_part_id = if index == 3 { "duplicate" } else { "default" };
        let shape = if index == 5 {
            CandidateShape::point(serde_json::json!([121.4, 31.9]))
        } else {
            CandidateShape::polygon(ring(geometry_offset))
        };
        drafts.push(
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new(
                    &observation.data_source_tag,
                    source_entity_id,
                    geometry_part_id,
                ),
                observation.entity_type,
                CandidateDisplay::new(title, tags),
                shape,
                validation,
            )
            .with_name_source(name_source),
        );
        reviewable_sources.push((source_entity_id.to_owned(), geometry_part_id.to_owned()));
    }
    database
        .publish_candidate_batch(plan_id, &review_boundary_fingerprint(), &drafts)
        .expect("原子发布建议夹具候选批次");
    let ids_by_source = database
        .list_reviewable_candidate_projections(plan_id)
        .expect("读取合法建议候选")
        .into_iter()
        .map(|projection| {
            (
                (projection.source_entity_id, projection.geometry_part_id),
                projection.candidate_id,
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    reviewable_sources
        .into_iter()
        .map(|source| ids_by_source[&source].clone())
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

/// 进入指定方案的评审步：方案列表点卡 → 边界确认 → 评审步。
fn enter_review(window: &AppWindow, plan_id: &str) {
    window.invoke_plan_list_card_clicked(plan_id.into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#.into(),
    );
    window.invoke_workspace_step_clicked(3);
    window.invoke_workspace_map_status_changed(true);
}

/// 从工作区返回方案列表（若存在离开确认弹窗则确认；评审步无未保存边界/
/// 运行中操作时直接放行，S1-13 语义）。
fn leave_to_plan_list(window: &AppWindow) {
    window.invoke_workspace_back_to_plan_list_clicked();
    if window.get_confirm_dialog_visible() {
        window.invoke_confirm_dialog_confirmed();
    }
}

/// T51 置信度分档 + 一键应用建议的 AppWindow 契约：四个芯片与计数、
/// 筛选后卡片刷新、高→中→低排序、一键只保留高置信并确认（不剔除）、
/// 撤销恢复、封账后不可撤销。
fn run_suggestion_contract(
    window: &AppWindow,
    l10n: &Localization,
    database_path: &std::path::Path,
    plan_id: &str,
    reviewable: &[String],
) {
    enter_review(window, plan_id);
    assert_eq!(window.get_review_candidate_count(), 6);

    // 四个置信度芯片 + 计数（建筑分类内：全部 6 / 高 2 / 中 2 / 低 2）。
    let labels: Vec<String> = window
        .get_review_confidence_filter_labels()
        .iter()
        .map(|label| label.to_string())
        .collect();
    assert_eq!(labels.len(), 4, "芯片必须固定为 全部/高/中/低 四档");
    assert_eq!(
        window.get_review_confidence_filters_label().as_str(),
        l10n.t("review.confidence_filters_label")
    );
    assert!(
        labels[0].contains("全部") && labels[0].contains("6"),
        "芯片 0 不符：{labels:?}"
    );
    assert!(
        labels[1].contains("高") && labels[1].contains("2"),
        "芯片 1 不符：{labels:?}"
    );
    assert!(
        labels[2].contains("中") && labels[2].contains("2"),
        "芯片 2 不符：{labels:?}"
    );
    assert!(
        labels[3].contains("低") && labels[3].contains("2"),
        "芯片 3 不符：{labels:?}"
    );

    // 默认"全部"：建筑 6 张卡初始全部待定，且按 高→中→低 排序
    // （高=重复对前序 b0/b1，中=b2 未命名/b4 修复，低=b3 后序/b5 点形状）。
    assert_eq!(window.get_review_cards().row_count(), 6);
    for index in 0..6 {
        assert_eq!(card_state_key(window, index), "pending");
    }
    let expected_order = vec![
        reviewable[0].clone(),
        reviewable[1].clone(),
        reviewable[2].clone(),
        reviewable[4].clone(),
        reviewable[3].clone(),
        reviewable[5].clone(),
    ];
    let order: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(order, expected_order, "卡片必须按 高→中→低 排序");

    // 切换"高"：卡片收缩为 2 张高置信卡，应用按钮可用，芯片激活标记正确。
    window.invoke_review_confidence_filter_clicked(1);
    assert_eq!(window.get_review_candidate_count(), 6);
    assert_eq!(window.get_review_cards().row_count(), 2);
    assert_eq!(
        window
            .get_review_confidence_filter_active()
            .row_data(1)
            .unwrap(),
        1,
        "高置信芯片必须激活"
    );
    assert!(window.get_review_apply_suggestions_enabled());
    let suggestions: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .suggestion_reason
                .to_string()
        })
        .collect();
    assert!(
        suggestions.iter().all(|reason| !reason.is_empty()),
        "高置信筛选下的每张卡都必须带可读理由：{suggestions:?}"
    );

    // 切换"低"：卡片刷新为 2 张，分页仍可用（1/1 也常显）。
    window.invoke_review_confidence_filter_clicked(3);
    assert_eq!(window.get_review_cards().row_count(), 2);
    assert_eq!(window.get_review_page_total(), 1);
    assert!(window.get_review_page_label().contains("1/1"));

    // 切回"全部"后一键应用（范围=全部尚未保留的高置信候选，跨芯片）。
    window.invoke_review_confidence_filter_clicked(0);
    assert_eq!(window.get_review_cards().row_count(), 6);
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("review.apply_suggest_confirm_title")
    );
    let body = window.get_confirm_dialog_body().to_string();
    assert!(
        body.contains("2") && body.contains("不会剔除任何候选"),
        "确认框必须显示变更数量并明示不剔除：{body}"
    );
    assert!(
        body.contains(&l10n.t("review.apply_suggest_reason_label")) && body.contains("无需处理"),
        "确认框必须显示主要理由分布：{body}"
    );

    // 取消：状态不变、无撤销可用。
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..6 {
        assert_eq!(card_state_key(window, index), "pending");
    }
    assert!(!window.get_review_undo_available());

    // 再次应用并确认：只有 2 个高置信候选变为保留，低/中候选保持待定。
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    assert!(window.get_review_undo_available());
    let kept: Vec<String> = (0..window.get_review_cards().row_count())
        .filter(|index| {
            window
                .get_review_cards()
                .row_data(*index)
                .unwrap()
                .state_key
                .as_str()
                == "keep"
        })
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(kept.len(), 2, "一键应用只保留高置信：{kept:?}");
    assert!(kept.contains(&reviewable[0]) && kept.contains(&reviewable[1]));
    assert!(
        !window.get_review_apply_suggestions_enabled(),
        "全部高置信候选已保留后应用按钮应禁用"
    );

    // 撤销上一批：恢复到应用前（全部待定）。
    window.invoke_review_undo_suggestions_clicked();
    assert!(!window.get_review_undo_available());
    for index in 0..6 {
        assert_eq!(card_state_key(window, index), "pending");
    }

    // 重新应用并封账：封账后不可撤销、不可再应用。
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

    // 数据库终态落账：2 个高置信保留、4 个中/低候选待定、0 个剔除。
    let db = Database::open(database_path).expect("打开数据库核对终态");
    let (pending, keep, remove) = db.count_review_states(plan_id).expect("统计评审终态");
    assert_eq!((pending, keep, remove), (4, 2, 0));
}

/// T51 固定批量行契约：全选＝当前页、批量剔除无门槛但始终确认、
/// 批量改保留/待定直接执行，封账与重进恢复终态语义保持不变。
fn run_batch_contract(
    window: &AppWindow,
    l10n: &Localization,
    database_path: &std::path::Path,
    plan_id: &str,
    reviewable: &[String],
) {
    enter_review(window, plan_id);
    assert_eq!(window.get_review_candidate_count(), 6);

    // 固定批量行初始状态：未全选、批量按钮禁用。
    assert!(!window.get_review_all_page_selected());
    assert!(!window.get_review_batch_buttons_enabled());

    // 全选＝当前页：建筑当前页 5 项全部勾选。
    window.invoke_review_select_all_toggled();
    assert!(window.get_review_all_page_selected());
    assert!(window.get_review_batch_buttons_enabled());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 5 }))
    );

    // 再点一次全选：只取消当前页勾选。
    window.invoke_review_select_all_toggled();
    assert!(!window.get_review_all_page_selected());
    assert!(!window.get_review_batch_buttons_enabled());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 0 }))
    );

    // 批量剔除 1 项也必须弹确认（移除 ≥5 门槛）。
    window.invoke_review_card_selection_toggled(reviewable[0].clone().into());
    assert!(window.get_review_batch_buttons_enabled());
    window.invoke_review_bulk_state_clicked("remove".into());
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("review.batch_reject_confirm_title")
    );
    assert!(
        window.get_confirm_dialog_body().as_str().contains("1"),
        "确认弹窗正文必须携带待剔除数量"
    );
    window.invoke_confirm_dialog_cancelled();
    assert!(!window.get_confirm_dialog_visible());
    assert_eq!(card_state_key(window, 0), "pending", "取消后状态不得改变");

    // 全选当前页 → 批量剔除 5 项 → 确认执行。
    window.invoke_review_select_all_toggled();
    assert!(window.get_review_all_page_selected());
    window.invoke_review_bulk_state_clicked("remove".into());
    assert!(window.get_confirm_dialog_visible());
    assert!(
        window.get_confirm_dialog_body().as_str().contains("5"),
        "确认弹窗正文必须携带待剔除数量"
    );
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..5 {
        assert_eq!(card_state_key(window, index), "remove");
    }

    // 批量改保留直接执行（无需确认）：当前页仍为勾选态。
    window.invoke_review_bulk_state_clicked("keep".into());
    assert!(!window.get_confirm_dialog_visible(), "批量改保留不得弹确认");
    for index in 0..5 {
        assert_eq!(card_state_key(window, index), "keep");
    }

    // 批量改待定直接执行。
    window.invoke_review_bulk_state_clicked("pending".into());
    assert!(!window.get_confirm_dialog_visible(), "批量改待定不得弹确认");
    for index in 0..5 {
        assert_eq!(card_state_key(window, index), "pending");
    }

    // 再次批量剔除并确认：5 项全部剔除；随后逐项恢复两个候选。
    window.invoke_review_bulk_state_clicked("remove".into());
    window.invoke_confirm_dialog_confirmed();
    for index in 0..5 {
        assert_eq!(card_state_key(window, index), "remove");
    }
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
