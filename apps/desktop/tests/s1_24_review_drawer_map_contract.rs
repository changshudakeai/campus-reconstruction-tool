//! S1-24 / T38 契约测试：评审步抽屉化（地图为主区 + 左侧抽屉）+ 地图定位/联动。
//!
//! 覆盖：
//! 1. 评审地图 IPC 路由：map_ready / review_object_clicked / error 经评审入口
//!    处理（不触发 OSM 边界获取，不进入边界解析）；
//! 2. 点地图对象 / 点卡片 / "定位到地图" → 双向联动高亮 + 选中详情
//!    （名称/类别/标签与属性/来源/状态；未命名显示"未命名建筑 #id"）；
//! 3. 标注规则状态传播：剔除后卡片保留且详情显示"剔除"，可从卡片改回保留；
//! 4. 评审地图加载失败 → B7 错误弹窗，评审抽屉仍可操作。

use std::sync::Arc;

use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservation,
    RawObservationsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory};
use slint::Model;

/// 种子：3 栋建筑（2 有名称 + 1 未命名）。
fn seed_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    let observations = vec![
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b0",
            serde_json::json!({
                "tags": { "name": "教学楼0", "building": "gymnasium" }
            }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b1",
            serde_json::json!({ "tags": { "building": "yes" } }),
            "overpass",
        ),
        RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            "way/b2",
            serde_json::json!({ "tags": { "name": "图书馆", "amenity": "library" } }),
            "overpass",
        ),
    ];
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
            vec![
                ("building".to_owned(), "yes".to_owned()),
                ("source".to_owned(), observation.data_source_tag.clone()),
            ],
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

fn card_state_key(window: &AppWindow, index: usize) -> String {
    window
        .get_review_cards()
        .row_data(index)
        .expect("评审卡片必须存在")
        .state_key
        .to_string()
}

#[test]
fn review_drawer_map_ipc_highlight_locate_and_annotation_state() {
    let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-24-review-drawer.db");
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
    // 配置密钥：navigate(3) 因此会请求评审地图（is_review_page 生效，IPC 可路由）
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
    assert_eq!(reviewable.len(), 3);
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#.into(),
    );
    window.invoke_workspace_step_clicked(3);
    assert_eq!(window.get_workspace_active_step(), 3);
    window.invoke_workspace_map_status_changed(true);

    // 1. 评审地图就绪 IPC → 评审入口（候选标注推送在无 WebView 测试环境为空操作）
    window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    assert_eq!(
        window.get_review_candidate_count(),
        3,
        "map_ready 不得破坏评审页状态"
    );
    assert_eq!(
        window.get_review_category_labels().row_count(),
        6,
        "六类标签入口必须齐全可见（建筑/道路/水域/植被/体育/其他）"
    );
    assert!(
        !desktop_shell::review_map_text_visible(),
        "评审模式默认必须隐藏易遮挡轮廓的地图文字"
    );
    window.invoke_workspace_map_ipc(r#"{"type":"review_map_text_toggled","visible":true}"#.into());
    assert!(
        desktop_shell::review_map_text_visible(),
        "用户切换开关后地图文字状态必须在当前评审会话内保持"
    );

    // 2a. 点地图对象 → 高亮对应卡片；不再展开占空间的详情卡。
    window.invoke_workspace_map_ipc(
        format!(
            r#"{{"type":"review_object_clicked","candidate_id":"{}"}}"#,
            reviewable[0]
        )
        .into(),
    );
    assert!(
        window
            .get_review_cards()
            .row_data(0)
            .expect("高亮卡片存在")
            .highlighted,
        "地图对象点击后对应卡片必须高亮（双向联动）"
    );

    // 2b. "定位到地图" → 高亮另一候选（未命名标题仍显示在候选卡片）。
    window.invoke_review_locate_clicked(reviewable[1].clone().into());
    let located = window
        .get_review_cards()
        .row_data(1)
        .expect("定位候选卡片存在");
    assert_eq!(located.title.as_str(), "未命名建筑 #way/b1");
    assert!(located.highlighted);

    // 2c. 点卡片 → 高亮（地图↔卡片共用同一份高亮状态）
    window.invoke_review_card_highlight_clicked(reviewable[2].clone().into());
    assert!(
        window
            .get_review_cards()
            .row_data(2)
            .expect("点选卡片存在")
            .highlighted
    );

    // 3. 标注规则：剔除后卡片保留，可直接从卡片改回"保留"。
    window.invoke_review_card_state_clicked(reviewable[0].clone().into(), "remove".into());
    assert_eq!(
        card_state_key(&window, 0),
        "remove",
        "剔除后卡片仍保留（地图隐藏由地图页 JS 执行）"
    );
    window.invoke_review_card_highlight_clicked(reviewable[0].clone().into());
    window.invoke_review_card_state_clicked(reviewable[0].clone().into(), "keep".into());
    assert_eq!(
        card_state_key(&window, 0),
        "keep",
        "剔除候选可从卡片改回保留"
    );

    // 4a. 单候选绘制失败 → B7 通俗弹窗，但不误判整张地图不可用。
    window.invoke_workspace_map_ipc(
        r#"{"type":"error","message":"review_map_draw_failed:overlay_construct"}"#.into(),
    );
    assert!(
        window.get_error_dialog_visible(),
        "候选绘制失败必须经 B7 弹窗，不能只写 console.error"
    );
    assert_eq!(
        window.get_error_dialog_title().as_str(),
        l10n.t("review.map_draw_failed_title")
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("review.map_draw_failed_body")
    );
    assert!(
        window.get_workspace_map_available(),
        "单候选失败只影响部分轮廓，不得把整张地图标为不可用"
    );
    window.invoke_error_dialog_dismissed();

    window.invoke_workspace_map_ipc(
        r#"{"type":"error","message":"review_map_locate_hidden"}"#.into(),
    );
    assert_eq!(
        window.get_error_dialog_title().as_str(),
        l10n.t("review.map_locate_failed_title")
    );
    assert_eq!(
        window.get_error_dialog_body().as_str(),
        l10n.t("review.map_locate_hidden_body")
    );
    assert!(
        window.get_workspace_map_available(),
        "剔除候选不可定位只影响该候选，不得把整张地图标为不可用"
    );
    window.invoke_error_dialog_dismissed();

    // 4b. 评审地图整体失败 → B7 错误弹窗；已有状态保留，新决定暂停
    window.invoke_workspace_map_ipc(r#"{"type":"error","message":"评审地图初始化失败"}"#.into());
    assert!(
        window.get_error_dialog_visible(),
        "评审地图失败必须经 B7 呈现明确错误"
    );
    window.invoke_error_dialog_dismissed();
    assert_eq!(
        window.get_review_candidate_count(),
        3,
        "地图失败不破坏评审抽屉"
    );
    let state_before = card_state_key(&window, 1);
    window.invoke_review_card_state_clicked(reviewable[1].clone().into(), "keep".into());
    assert_eq!(
        card_state_key(&window, 1),
        state_before,
        "地图整体失败后不得写入新的评审决定"
    );
}
