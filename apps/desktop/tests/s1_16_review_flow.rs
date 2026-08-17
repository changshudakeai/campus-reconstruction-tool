//! S1-16 M3 验收：评审页按六类分组展示 Reviewable 候选，逐项三态判定。
//!
//! Isolated 与无投影原始观测绝不进入评审页；F5 只读取
//! `list_reviewable_candidate_projections`（ADR-0040 资格边界）。
use std::sync::Arc;

use data_persistence::{
    CampusCrudApi, CandidateDisplay, CandidateEligibility, CandidateProjection,
    CandidateProjectionsApi, CandidateShape, CandidateValidation, Database, RawObservation,
    RawObservationsApi,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, CandidateCategory};
use slint::Model;

/// 种子：3 栋有名称可评审建筑 + 1 栋未命名可评审建筑 + 1 条可评审道路 +
/// 1 处可评审水域；
/// 另加 1 条无投影原始观测与 1 个 Isolated 投影（均不得进入评审页）。
fn seed_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    let mut observations = Vec::new();
    for index in 0..3 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼{index}") } }),
            "overpass",
        ));
    }
    // 未命名建筑：无 name 标签 → 标题回退为实体 ID，评审抽屉显示"未命名建筑 #id"
    observations.push(RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/b3",
        serde_json::json!({ "tags": { "building": "yes" } }),
        "overpass",
    ));
    observations.push(RawObservation::new(
        plan_id,
        CandidateCategory::Road,
        "way/r0",
        serde_json::json!({ "tags": { "highway": "footway" } }),
        "overpass",
    ));
    observations.push(RawObservation::new(
        plan_id,
        CandidateCategory::Water,
        "way/w0",
        serde_json::json!({ "tags": { "name": "泳池" } }),
        "overpass",
    ));
    // 无投影原始观测：只作为来源证据，永远不构成评审候选。
    observations.push(RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/raw-only",
        serde_json::json!({ "tags": { "name": "无投影观测" } }),
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
        if observation.entity_id == "way/raw-only" {
            continue;
        }
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

    // Isolated 投影：资格边界（ADR-0040）——绝不进入 F5 评审页。
    let isolated_observation = RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/isolated",
        serde_json::json!({ "tags": { "name": "隔离观测" } }),
        "overpass",
    );
    database
        .write_raw_observations(std::slice::from_ref(&isolated_observation))
        .expect("写入隔离观测");
    projections.push(CandidateProjection::new(
        "overpass:way/isolated:outer",
        plan_id,
        &isolated_observation.id,
        "overpass",
        "way/isolated",
        "default",
        CandidateCategory::Building,
        CandidateDisplay::new("隔离观测", Vec::new()),
        CandidateShape::point(serde_json::json!([121.4, 31.2])),
        CandidateValidation::Rejected,
        CandidateEligibility::Isolated,
    ));
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

fn card_title(window: &AppWindow, index: usize) -> String {
    window
        .get_review_cards()
        .row_data(index)
        .expect("评审卡片必须存在")
        .title
        .to_string()
}

#[test]
fn review_page_groups_reviewable_candidates_by_six_categories_and_judges_tri_state() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-16-review.db");
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
    assert_eq!(reviewable.len(), 6);
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#.into(),
    );
    window.invoke_workspace_step_clicked(3);
    window.invoke_workspace_map_status_changed(true);

    // 只读入 Reviewable：3 建筑 + 1 道路 + 1 水域 = 5；隔离/无投影绝不进入。
    assert_eq!(window.get_workspace_active_step(), 3);
    assert_eq!(
        window.get_review_candidate_count(),
        6,
        "Isolated 与无投影原始观测不得进入评审页"
    );
    assert_eq!(window.get_review_category_labels().row_count(), 6);
    let counts: Vec<i32> = (0..6)
        .map(|index| window.get_review_category_counts().row_data(index).unwrap())
        .collect();
    assert_eq!(
        counts,
        vec![4, 1, 1, 0, 0, 0],
        "六类标签页计数必须来自 Reviewable"
    );
    assert_eq!(window.get_review_active_category(), 0);

    // 当前激活类别（建筑）显示 4 张卡，初始全部“待定”。
    assert_eq!(window.get_review_cards().row_count(), 4);
    assert_eq!(card_title(&window, 0), "教学楼0");
    assert_eq!(card_title(&window, 2), "教学楼2");
    for index in 0..4 {
        assert_eq!(
            card_state_key(&window, index),
            "pending",
            "候选初始三态必须为待定"
        );
    }
    // 未命名候选：卡片标题为"未命名建筑 #id"（id = 回退实体 ID），named=false
    assert_eq!(card_title(&window, 3), "未命名建筑 #way/b3");
    assert!(
        !window
            .get_review_cards()
            .row_data(3)
            .expect("未命名卡片必须存在")
            .named,
        "未命名候选必须标记 named=false"
    );
    assert_eq!(
        window.get_review_locate_label().as_str(),
        l10n.t("review.locate"),
        "定位到地图按钮文案必须经 B6 注入"
    );

    // 逐项判定：建筑 0 改为保留，卡片立即呈现“保留”。
    window.invoke_review_card_state_clicked(reviewable[0].clone().into(), "keep".into());
    assert_eq!(window.get_review_cards().row_count(), 4);
    assert_eq!(
        window
            .get_review_cards()
            .row_data(0)
            .expect("评审卡片必须存在")
            .state_label
            .as_str(),
        l10n.t("review.keep")
    );
    assert_eq!(card_state_key(&window, 0), "keep");

    // 切换道路标签页：只显示该类别候选，并可将状态改为剔除。
    window.invoke_review_category_clicked(1);
    assert_eq!(window.get_review_active_category(), 1);
    assert_eq!(window.get_review_cards().row_count(), 1);
    window.invoke_review_card_state_clicked(reviewable[4].clone().into(), "remove".into());
    assert_eq!(card_state_key(&window, 0), "remove");
    assert_eq!(
        window
            .get_review_cards()
            .row_data(0)
            .expect("评审卡片必须存在")
            .state_label
            .as_str(),
        l10n.t("review.reject")
    );

    // 切回建筑：三态判定结果保留在内存会话中。
    window.invoke_review_category_clicked(0);
    assert_eq!(window.get_review_cards().row_count(), 4);
    assert_eq!(card_state_key(&window, 0), "keep");
    assert_eq!(card_state_key(&window, 1), "pending");

    // 水域标签页也有可评审候选。
    window.invoke_review_category_clicked(2);
    assert_eq!(window.get_review_cards().row_count(), 1);
    assert_eq!(card_state_key(&window, 0), "pending");

    // T38：点卡片 → 高亮 + 选中详情（名称/类别/标签与属性/来源/状态）
    window.invoke_review_category_clicked(0);
    window.invoke_review_card_highlight_clicked(reviewable[0].clone().into());
    assert!(window.get_review_detail_visible(), "高亮候选必须显示详情");
    assert_eq!(window.get_review_detail_title().as_str(), "教学楼0");
    assert!(
        window.get_review_detail_source_label().contains("overpass"),
        "详情必须给出候选来源"
    );
    assert!(
        window
            .get_review_cards()
            .row_data(0)
            .expect("高亮卡片必须存在")
            .highlighted,
        "高亮候选的卡片必须带 highlighted 标记（地图↔卡片联动）"
    );

    // T38：卡片"定位到地图"→ 高亮同一候选（地图中心跳转由地图页 JS 负责）
    window.invoke_review_locate_clicked(reviewable[3].clone().into());
    assert_eq!(
        window.get_review_detail_title().as_str(),
        "未命名建筑 #way/b3",
        "定位后详情必须切换并显示未命名标题"
    );
    assert!(
        window
            .get_review_cards()
            .row_data(3)
            .expect("定位候选卡片必须存在")
            .highlighted
    );
}
