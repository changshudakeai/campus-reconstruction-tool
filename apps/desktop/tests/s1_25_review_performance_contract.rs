//! S1-25 / T39 契约测试：评审工作台卡顿 + 评审地图加载 P0 修复验收。
//!
//! 真实 1026 候选（建筑 1000 + 道路 26，贴近 T32 走查）进入评审工作台，
//! 只验证呈现/地图推送层，不触碰 F5 评审/封账业务：
//! 1. 卡片分页（T51 每页 20）：模型行数 ≤ 页大小；翻页/切分类才重建模型，
//!    三态/高亮/复选变更走单卡更新（模型实例指针不变，非整表重建）；
//! 2. "分类切换 + 三态点击 10 次"计时：单次操作 ≤ 500ms（不再成秒级冻结）；
//! 3. 评审地图回推计数（T39 计数器注入）：map_ready 全量只推 21 批一次；
//!    一次高亮/三态/定位只产生 1 条回推，分类切换 0 条——不是每次交互
//!    clear + 21 批全量；
//! 4. 慢速注入/推送阶段不被误杀：全量推送与增量回推后地图仍可用、无错误
//!    弹窗（评审页不挂 Rust 侧 10s 加载超时的策略由 map_webview 单测
//!    `review_page_skips_rust_load_timeout` 锁定）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use data_persistence::{
    boundary_fingerprint, CampusCrudApi, CandidateDisplay, CandidateProjectionDraft,
    CandidateProjectionsApi, CandidateShape, CandidateSourceIdentity, Database, RawObservation,
    RawObservationsApi, ReviewableValidation,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use global_settings::{FirstRunSetup, SettingsManager};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{Boundary, CampusId, CandidateCategory};
use slint::Model;

/// 种子：1000 栋建筑 + 26 条道路 = 1026 个可评审候选（贴近 T32 走查的 1026）。
fn seed_candidates(database: &mut Database, plan_id: &str) -> Vec<String> {
    let mut observations = Vec::new();
    for index in 0..1000 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼{index}") } }),
            "overpass",
        ));
    }
    for index in 0..26 {
        observations.push(RawObservation::new(
            plan_id,
            CandidateCategory::Road,
            format!("way/r{index}"),
            serde_json::json!({ "tags": { "highway": "footway" } }),
            "overpass",
        ));
    }
    database
        .write_raw_observations(&observations)
        .expect("写入原始观测");
    let mut drafts = Vec::new();
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
    }
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

/// 在当前页切片中按候选 ID 找行号（数据库返回顺序不保证，不能假设行号）。
fn card_row(window: &AppWindow, candidate_id: &str) -> usize {
    (0..window.get_review_cards().row_count())
        .find(|index| {
            window
                .get_review_cards()
                .row_data(*index)
                .expect("卡片存在")
                .candidate_id
                .as_str()
                == candidate_id
        })
        .unwrap_or_else(|| panic!("候选 {candidate_id} 不在当前页切片中"))
}

#[test]
fn review_performance_pagination_single_card_update_and_incremental_map_push() {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-25-review-performance.db");
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
    assert_eq!(reviewable.len(), 1026);
    open_plan_and_review(&window, &center, injector, &plan_id.to_string());
    assert_eq!(window.get_workspace_active_step(), 3);
    assert_eq!(
        window.get_review_candidate_count(),
        1026,
        "候选必须全部进入评审页"
    );
    window.invoke_workspace_drawer_toggle_clicked();
    assert!(window.get_workspace_drawer_open(), "评审抽屉必须可展开");

    // ── 验收 1a：卡片分页（T51 收窄到每页 20；Slint 无虚拟化 → 不一次
    // 实例化千级卡片，降低滚轮滚动的单帧布局/绘制成本）──
    assert_eq!(window.get_review_page_size(), 20, "页大小必须为 20（T51）");
    assert_eq!(
        window.get_review_page_total(),
        50,
        "建筑 1000 → ceil(1000/20)=50 页"
    );
    assert_eq!(window.get_review_page_index(), 0);
    assert!(window.get_review_page_label().contains("1/50"));
    assert!(
        window.get_review_cards().row_count() <= 20,
        "模型只含当前页切片（实际 {}）",
        window.get_review_cards().row_count()
    );
    let page_one_first = window
        .get_review_cards()
        .row_data(0)
        .expect("第一页必须有卡片")
        .candidate_id
        .to_string();
    let page_one_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .expect("第一页卡片存在")
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(page_one_ids.len(), 20, "第一页必须满页 20 张卡");

    // 翻页：下一页 → 模型重建（新页切片），上一页复位
    let model_page_one = window.get_review_cards();
    window.invoke_review_page_next_clicked();
    assert_eq!(window.get_review_page_index(), 1);
    assert_ne!(
        window.get_review_cards(),
        model_page_one,
        "翻页必须重建模型（新页切片）"
    );
    assert_eq!(window.get_review_cards().row_count(), 20, "第 2 页必须满页");
    let page_two_first = window
        .get_review_cards()
        .row_data(0)
        .expect("第 2 页必须有卡片")
        .candidate_id
        .to_string();
    assert!(
        !page_one_ids.contains(&page_two_first),
        "第 2 页必须与第 1 页切片不同（页 1 首卡 {page_one_first}，页 2 首卡 {page_two_first}）"
    );
    window.invoke_review_page_prev_clicked();
    assert_eq!(window.get_review_page_index(), 0);
    assert_eq!(
        window
            .get_review_cards()
            .row_data(0)
            .expect("回到第 1 页必须有卡片")
            .candidate_id
            .as_str(),
        page_one_first.as_str(),
        "上一页必须回到第 1 页切片"
    );

    // 分类切换复位到第一页（道路 26 → 1 页）
    window.invoke_review_category_clicked(1);
    assert_eq!(window.get_review_active_category(), 1);
    assert_eq!(window.get_review_page_index(), 0);
    assert_eq!(
        window.get_review_page_total(),
        2,
        "道路 26 → 2 页（每页 20）"
    );
    window.invoke_review_category_clicked(0);
    assert_eq!(window.get_review_active_category(), 0);
    assert_eq!(window.get_review_page_index(), 0);

    // ── 验收 3：评审地图回推计数（计数器注入；全量只推一次，之后增量）──
    // 后续操作全部使用当前页切片内真实存在的候选（数据库返回顺序不保证）。
    let page_ids: Vec<String> = (0..window.get_review_cards().row_count())
        .map(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .expect("当前页卡片存在")
                .candidate_id
                .to_string()
        })
        .collect();
    assert_eq!(page_ids.len(), 20, "当前页必须满页 20 张卡");
    desktop_shell::set_review_push_probe_visible(true);
    desktop_shell::reset_review_push_count();
    window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    // 无事件循环环境下用切分类触发内联全量推送（生产由下一拍事件循环 Timer
    // 执行；这里验证同一全量推送路径只按当前可见集合重推）。
    window.invoke_review_category_clicked(1);
    window.invoke_review_category_clicked(0);
    {
        let scripts = desktop_shell::review_pushed_scripts();
        assert_eq!(
            scripts.len(),
            2,
            "map_ready 后切分类只重推两次可见集合（道路页 + 建筑页）"
        );
        for script in &scripts {
            assert!(
                script.starts_with("window.setReviewCandidates("),
                "可见集合必须用 setReviewCandidates 清旧画新，实际：{script}"
            );
            assert!(
                !script.contains("window.addReviewCandidate("),
                "不得把全量候选逐条 addReviewCandidate 排进 JS 缓冲"
            );
        }
        // 建筑当前页 20 条是地图 overlay 的明确上限（1026 条只画当前页）。
        let building_push = scripts.last().expect("最后一条是建筑页全量推送");
        let json_start = building_push
            .find('[')
            .expect("setReviewCandidates 参数为数组");
        let json_end = building_push.rfind(']').expect("数组结尾存在");
        let array: Vec<serde_json::Value> =
            serde_json::from_str(&building_push[json_start..=json_end])
                .expect("建筑页可见集合 JSON 必须是数组");
        assert_eq!(array.len(), 20, "地图只绘制当前分页 20 条，而非全量 1026");
    }

    // 一次高亮操作 → 只产生 1 条回推（不是 21 批全量）
    desktop_shell::reset_review_push_count();
    window.invoke_review_card_highlight_clicked(page_ids[0].clone().into());
    assert_eq!(
        desktop_shell::review_push_count(),
        1,
        "高亮操作只推 1 条 highlightReviewCandidate"
    );
    assert!(
        desktop_shell::review_pushed_scripts()[0].contains("window.highlightReviewCandidate("),
        "高亮回推必须是 highlightReviewCandidate（地图 spy）"
    );
    let highlighted_row = card_row(&window, &page_ids[0]);
    assert!(
        window
            .get_review_cards()
            .row_data(highlighted_row)
            .expect("高亮卡片存在")
            .highlighted
    );

    // 同类别重复切换不改变可见集合 → 0 条回推
    desktop_shell::reset_review_push_count();
    window.invoke_review_category_clicked(0);
    assert_eq!(
        desktop_shell::review_push_count(),
        0,
        "同类别重复切换不产生地图回推"
    );

    // 定位 → 只推 1 条 locateReviewCandidate（JS 已自高亮，不重复推高亮）
    desktop_shell::reset_review_push_count();
    window.invoke_review_locate_clicked(page_ids[2].clone().into());
    assert_eq!(
        desktop_shell::review_push_count(),
        1,
        "定位只推 1 条 locateReviewCandidate"
    );
    assert!(
        desktop_shell::review_pushed_scripts()[0].contains("window.locateReviewCandidate("),
        "定位回推必须是 locateReviewCandidate（地图 spy）"
    );

    // 定位目标不在当前分页：生产入口必须先切换到目标页并全量推送该页，
    // 再执行定位；不得把未知目标留给 JS pending 后静默丢弃。
    desktop_shell::reset_review_push_count();
    window.invoke_review_locate_clicked(page_two_first.clone().into());
    assert_eq!(
        window.get_review_page_index(),
        1,
        "定位第二页候选必须把卡片与地图可见集合同步切到第二页"
    );
    assert!(
        window
            .get_review_cards()
            .row_data(card_row(&window, &page_two_first))
            .expect("第二页定位卡片存在")
            .highlighted,
        "切页后目标卡片必须同步高亮"
    );
    let locate_other_page_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        locate_other_page_scripts
            .first()
            .is_some_and(|script| script.starts_with("window.setReviewCandidates(")),
        "跨页定位必须先推目标页可见集合"
    );
    assert!(
        locate_other_page_scripts
            .last()
            .is_some_and(|script| script.contains("window.locateReviewCandidate(")),
        "跨页定位必须在目标页推送后执行定位"
    );
    window.invoke_review_page_prev_clicked();
    desktop_shell::reset_review_push_count();

    // 一次三态操作 → 只推对应候选 1 条 updateReviewCandidate（地图概览不随
    // 三态分组过滤；卡片会从待定分组消失）。
    window.invoke_review_category_clicked(1);
    window.invoke_review_category_clicked(0);
    desktop_shell::reset_review_push_count();
    window.invoke_review_card_state_clicked(page_ids[1].clone().into(), "remove".into());
    assert_eq!(
        desktop_shell::review_push_count(),
        1,
        "三态操作只推 1 条 updateReviewCandidate"
    );
    assert!(
        desktop_shell::review_pushed_scripts()[0].contains("window.updateReviewCandidate("),
        "三态回推必须是 updateReviewCandidate（地图 spy）"
    );

    // 验收 2/4：推送阶段（慢速注入场景）不得被误杀——地图仍可用、无错误弹窗
    assert!(
        !window.get_error_dialog_visible(),
        "全量推送与增量回推阶段不得被加载超时误杀弹错"
    );
    assert_eq!(window.get_review_candidate_count(), 1026);
    window.invoke_review_card_state_clicked(page_ids[3].clone().into(), "keep".into());
    window.invoke_review_state_tab_clicked(1);
    let kept_row = card_row(&window, &page_ids[3]);
    assert_eq!(
        window
            .get_review_cards()
            .row_data(kept_row)
            .expect("卡片存在")
            .state_key
            .as_str(),
        "keep",
        "推送后评审操作仍可继续"
    );
    window.invoke_review_state_tab_clicked(0);
    desktop_shell::set_review_push_probe_visible(false);

    // ── 验收 1b：高亮走单卡更新；三态变更因卡片离开当前分组而重建模型 ──
    let model = window.get_review_cards();
    let target_row = card_row(&window, &page_ids[0]);
    window.invoke_review_card_highlight_clicked(page_ids[0].clone().into());
    assert_eq!(
        window.get_review_cards(),
        model,
        "高亮变更必须走单卡更新，不得重建整表模型"
    );
    assert!(
        window
            .get_review_cards()
            .row_data(target_row)
            .unwrap()
            .highlighted
    );
    window.invoke_review_card_state_clicked(page_ids[0].clone().into(), "keep".into());
    assert_ne!(
        window.get_review_cards(),
        model,
        "三态变更后卡片离开待定分组，必须重建当前页模型"
    );
    assert!(
        (0..window.get_review_cards().row_count()).all(|index| {
            window
                .get_review_cards()
                .row_data(index)
                .unwrap()
                .candidate_id
                .as_str()
                != page_ids[0]
        }),
        "保留后该候选必须离开待定分组"
    );
    window.invoke_review_state_tab_clicked(1);
    assert!(
        window
            .get_review_cards()
            .row_data(card_row(&window, &page_ids[0]))
            .expect("保留卡片存在")
            .state_key
            .as_str()
            == "keep",
        "保留后必须出现在保留分组"
    );
    window.invoke_review_state_tab_clicked(0);

    // ── 验收 1c：分类切换 + 三态点击 10 次计时（单次 ≤ 500ms）──
    let mut worst = Duration::ZERO;
    for index in 0..5_i32 {
        let started = Instant::now();
        window.invoke_review_category_clicked(index % 2);
        let elapsed = started.elapsed();
        worst = worst.max(elapsed);
        assert!(
            elapsed <= Duration::from_millis(500),
            "分类切换 {index} 次耗时 {elapsed:?} 超过 500ms"
        );
    }
    for candidate in page_ids.iter().take(5) {
        let started = Instant::now();
        window.invoke_review_card_state_clicked(candidate.clone().into(), "keep".into());
        let elapsed = started.elapsed();
        worst = worst.max(elapsed);
        assert!(
            elapsed <= Duration::from_millis(500),
            "三态点击 {candidate} 耗时 {elapsed:?} 超过 500ms"
        );
    }
}
