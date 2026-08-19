//! S1-37 / T51 契约测试：评审台置信度分档、筛选刷新、排序与页面级全选。
//!
//! 真实 30 个可评审建筑进入评审工作台（高 10 / 中 10 / 低 10），验证：
//! 1. 四个置信度芯片（全部/高/中/低）与计数；
//! 2. 点击芯片后卡片列表正确刷新（含同长度筛选之间切换）且分页保持可用；
//! 3. 候选列表与评审地图按 高→中→低 排序（地图优先接收高置信候选）；
//! 4. 一键应用只把 10 个高置信候选改为保留并先弹确认；
//! 5. 固定批量行的"全选"只作用于当前页，不跨页。

use std::sync::Arc;

use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateNameSource,
    CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity,
    Database, RawObservation, RawObservationsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory};
use slint::Model;

/// 种子：30 栋建筑 = 高 10（有名称 + 完整面环 + 标签）/
/// 中 10（未命名完整面环）/ 低 10（有名称 + 点形状可疑）。
fn seed_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    let mut observations = Vec::new();
    for index in 0..10 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/h{index}"),
            serde_json::json!({ "tags": { "name": format!("高置信楼{index}"), "building": "yes" } }),
            "overpass",
        ));
    }
    for index in 0..10 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/m{index}"),
            serde_json::json!({ "tags": { "building": "yes" } }),
            "overpass",
        ));
    }
    for index in 0..10 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/l{index}"),
            serde_json::json!({ "tags": { "name": format!("低置信楼{index}"), "building": "yes" } }),
            "overpass",
        ));
    }
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let mut drafts = Vec::new();
    let mut reviewable_sources = Vec::new();
    for (index, observation) in observations.iter().enumerate() {
        let display = CandidateDisplay::new(
            observation.source_data["tags"]["name"]
                .as_str()
                .unwrap_or(&observation.entity_id),
            vec![("building".to_owned(), "yes".to_owned())],
        );
        let shape = if (20..30).contains(&index) {
            CandidateShape::point(serde_json::json!([121.4 + index as f64 * 0.1, 31.2]))
        } else {
            CandidateShape::polygon(serde_json::json!([
                [121.4 + index as f64 * 0.2, 31.2],
                [121.41 + index as f64 * 0.2, 31.2],
                [121.41 + index as f64 * 0.2, 31.21],
                [121.4 + index as f64 * 0.2, 31.21],
                [121.4 + index as f64 * 0.2, 31.2]
            ]))
        };
        let name_source = if (10..20).contains(&index) {
            CandidateNameSource::Unnamed
        } else {
            CandidateNameSource::Osm
        };
        drafts.push(
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new(
                    &observation.data_source_tag,
                    &observation.entity_id,
                    "default",
                ),
                observation.entity_type,
                display,
                shape,
                ReviewableValidation::Retained,
            )
            .with_name_source(name_source),
        );
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

#[test]
fn confidence_filters_refresh_sort_apply_and_page_scoped_select_all() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-37-confidence.db");
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
    let mut settings =
        SettingsManager::new(data_persistence::Database::open(&database_path).expect("重开设置库"));
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("保存 API Key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("保存安全密钥");
    let reviewable = {
        let mut database = injector.projects().database();
        seed_candidates(&mut database, &plan_id.to_string())
    };
    assert_eq!(reviewable.len(), 30);
    open_plan_and_review(&window, &center, injector, &plan_id.to_string());
    assert_eq!(window.get_workspace_active_step(), 3);
    assert_eq!(window.get_review_candidate_count(), 30);

    // 1. 四个置信度芯片与计数（全部 30 / 高 10 / 中 10 / 低 10）。
    let labels: Vec<String> = window
        .get_review_confidence_filter_labels()
        .iter()
        .map(|label| label.to_string())
        .collect();
    assert_eq!(labels.len(), 4);
    assert!(
        labels[0].contains("全部") && labels[0].contains("30"),
        "{labels:?}"
    );
    assert!(
        labels[1].contains("高") && labels[1].contains("10"),
        "{labels:?}"
    );
    assert!(
        labels[2].contains("中") && labels[2].contains("10"),
        "{labels:?}"
    );
    assert!(
        labels[3].contains("低") && labels[3].contains("10"),
        "{labels:?}"
    );

    // 2. 排序：全部筛选下第 1 页 = 10 高 + 10 中；第 2 页 = 10 低。
    assert_eq!(window.get_review_page_total(), 2);
    let page_one_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(page_one_ids.len(), 20);
    assert_eq!(
        &page_one_ids[0..10],
        &reviewable[0..10],
        "第 1 页前 10 张必须是高置信候选"
    );
    assert_eq!(
        &page_one_ids[10..20],
        &reviewable[10..20],
        "第 1 页后 10 张必须是中置信候选"
    );

    // 3. 点击芯片刷新列表：高 → 中 → 低（同长度筛选之间切换也正确刷新）。
    window.invoke_review_confidence_filter_clicked(1);
    assert_eq!(window.get_review_page_index(), 0, "筛选后必须回到第一页");
    assert_eq!(window.get_review_cards().row_count(), 10);
    let high_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(high_ids, reviewable[0..10].to_vec());

    window.invoke_review_confidence_filter_clicked(2);
    assert_eq!(window.get_review_cards().row_count(), 10);
    let medium_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(medium_ids, reviewable[10..20].to_vec());

    window.invoke_review_confidence_filter_clicked(3);
    assert_eq!(window.get_review_cards().row_count(), 10);
    let low_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(low_ids, reviewable[20..30].to_vec());

    // 筛选后分页保持可用且页码行常显（只剩一页时 1/1）。
    assert_eq!(window.get_review_page_total(), 1);
    assert!(window.get_review_page_label().contains("1/1"));

    // 4. 一键应用：只把 10 个高置信候选改为保留，先弹确认且不剔除。
    window.invoke_review_confidence_filter_clicked(0);
    assert!(window.get_review_apply_suggestions_enabled());
    window.invoke_review_apply_suggestions_clicked();
    assert!(window.get_confirm_dialog_visible());
    assert_eq!(
        window.get_confirm_dialog_title().as_str(),
        l10n.t("review.apply_suggest_confirm_title")
    );
    let body = window.get_confirm_dialog_body().to_string();
    assert!(
        body.contains("10") && body.contains("不会剔除任何候选"),
        "{body}"
    );
    window.invoke_confirm_dialog_confirmed();
    assert!(!window.get_confirm_dialog_visible());
    for index in 0..10 {
        assert_eq!(
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .state_key
                .as_str(),
            "keep",
            "高置信候选必须全部保留"
        );
    }
    for index in 10..20 {
        assert_eq!(
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .state_key
                .as_str(),
            "pending",
            "中/低置信候选不得被一键应用改变"
        );
    }

    // 5. 全选＝当前页：第 1 页 20 张、第 2 页 10 张，互不跨页。
    window.invoke_review_select_all_toggled();
    assert!(window.get_review_all_page_selected());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 20 }))
    );
    window.invoke_review_page_next_clicked();
    assert_eq!(window.get_review_page_index(), 1);
    window.invoke_review_select_all_toggled();
    assert!(window.get_review_all_page_selected());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 30 }))
    );
    window.invoke_review_page_prev_clicked();
    assert!(window.get_review_all_page_selected(), "第 1 页勾选仍保持");
    window.invoke_review_select_all_toggled();
    assert!(!window.get_review_all_page_selected());
    assert_eq!(
        window.get_review_selected_count_label().as_str(),
        l10n.t_with_args("review.selected_count", serde_json::json!({ "count": 10 })),
        "取消第 1 页全选不得影响第 2 页的勾选"
    );

    // 6. 评审地图按 高→中→低 接收可见集合（map_ready 后切分类触发全量推送）。
    desktop_shell::set_review_push_probe_visible(true);
    desktop_shell::reset_review_push_count();
    window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    window.invoke_review_category_clicked(1);
    window.invoke_review_category_clicked(0);
    {
        let scripts = desktop_shell::review_pushed_scripts();
        let building_push = scripts.last().expect("最后一次全量推送为建筑当前页");
        assert!(
            building_push.starts_with("window.setReviewCandidates("),
            "{building_push}"
        );
        let json_start = building_push
            .find('[')
            .expect("setReviewCandidates 参数为数组");
        let json_end = building_push.rfind(']').expect("数组结尾存在");
        let array: Vec<serde_json::Value> =
            serde_json::from_str(&building_push[json_start..=json_end])
                .expect("建筑页可见集合 JSON 必须是数组");
        let pushed_ids: Vec<String> = array
            .iter()
            .map(|object| object["candidate_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            pushed_ids,
            reviewable[0..20].to_vec(),
            "地图必须按 高→中→低 顺序优先接收高置信候选"
        );
    }
    desktop_shell::set_review_push_probe_visible(false);
}
