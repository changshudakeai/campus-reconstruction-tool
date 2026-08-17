//! S1-22 / T33 契约测试：大量评审候选下抽屉内列表可滚动、操作栏始终可达。
//!
//! 构造 1026 个可评审候选（建筑 1000 + 道路 26，贴近 T32 走查 1026）真实进入
//! 评审工作台（T38 抽屉布局：地图为主区 + 左侧评审抽屉），仅做呈现层断言：
//! 1. 滚轮向下滚动有效（PointerScrolled 后 viewport-y < 0）；
//! 2. 键盘滚动有效（点击列表获得焦点后 DownArrow 继续向下滚动）；
//! 3. 分类切换后滚动状态合理（真实标签点击复位到顶）；
//! 4. “封账完成评审”按钮固定于抽屉底部，大量候选下真实可点（点击后封账落账）。
//!
//! 窗口以默认 800×600 显示并短跑事件循环完成真实布局；断言全程不触碰
//! 评审/封账业务逻辑本身（F5 入口不变），仅验证 UI 布局呈现与可点性。

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
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, CandidateCategory};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::Model;
use slint::{ComponentHandle, LogicalPosition};

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
fn review_large_candidate_list_scrolls_and_keeps_seal_action_reachable() {
    let window = AppWindow::new().expect("创建 AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    let directory = tempfile::tempdir().expect("临时目录");
    let database_path = directory.path().join("s1-22-scroll-contract.db");
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
    assert_eq!(reviewable.len(), 1026);
    open_plan_and_review(&window, &center, injector, &plan_id.to_string());

    assert_eq!(window.get_workspace_active_step(), 3);
    assert_eq!(
        window.get_review_candidate_count(),
        1026,
        "候选必须全部进入评审页"
    );
    // T38：评审步为抽屉布局——展开左侧抽屉（地图为主区 + 左抽屉）
    window.invoke_workspace_drawer_toggle_clicked();
    assert!(window.get_workspace_drawer_open(), "评审抽屉必须可展开");
    // 未显示窗口时项目树未实例化/布局未计算：显示窗口并短跑一次事件循环，
    // 让渲染首帧完成真实布局（对齐 s1_08/s1_15 的做法）。
    window.show().expect("显示窗口");
    slint::Timer::single_shot(std::time::Duration::from_millis(150), || {
        slint::quit_event_loop().expect("退出布局事件循环");
    });
    slint::run_event_loop().expect("运行布局事件循环");
    // 证据模式（仅 T33_EVIDENCE_HOLD_MS 显式设置时生效，默认不阻塞）：
    // 保持窗口可见，供验收截图工具在大量候选页面状态下抓取。
    if let Ok(hold) = std::env::var("T33_EVIDENCE_HOLD_MS") {
        if let Ok(hold_ms) = hold.parse::<u64>() {
            slint::Timer::single_shot(std::time::Duration::from_millis(hold_ms), || {
                slint::quit_event_loop().expect("退出证据保持事件循环");
            });
            slint::run_event_loop().expect("运行证据保持事件循环");
        }
    }
    // 布局已就绪后隐藏窗口：后续输入事件仍正常处理，但避免逐事件重绘 1026 行场景。
    window.hide().expect("隐藏窗口");

    // 抽屉几何（逻辑像素）：x=20、宽=drawer-width、y=map-slot-y、高=map-slot-height
    let drawer_x = 20.0_f32;
    let drawer_y = window.get_workspace_map_slot_y();
    let drawer_h = window.get_workspace_map_slot_height();
    let drawer_w = window.get_workspace_drawer_width();
    assert!(drawer_h > 200.0, "抽屉必须有足够高度容纳操作栏");

    // 契约 1：滚轮向下滚动有效（真实 PointerScrolled 事件，落在抽屉列表区；
    // 建议筛选区在列表上方，因此取列表中部偏下的位置）。
    // 建议筛选区位于列表上方，列表区位置随内容下移：沿抽屉中部到底部扫描
    // 真实列表区域（一次派发，单次事件循环推进动画到终值），确认滚轮向下
    // 滚动有效（真实 PointerScrolled 事件）。
    for fraction in [
        0.1_f32, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85,
        0.9, 0.95,
    ] {
        let list_position =
            LogicalPosition::new(drawer_x + drawer_w / 2.0, drawer_y + drawer_h * fraction);
        window
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: list_position,
                delta_x: 0.0,
                delta_y: -240.0,
            });
    }
    // 短跑事件循环推进滚轮滚动动画（180ms）到终值。
    slint::Timer::single_shot(std::time::Duration::from_millis(250), || {
        slint::quit_event_loop().expect("退出滚轮动画事件循环");
    });
    slint::run_event_loop().expect("运行滚轮动画事件循环");
    let wheel_y = window.get_review_list_viewport_y();
    assert!(
        wheel_y < 0.0,
        "滚轮向下滚动后 viewport-y 必须为负（当前 {wheel_y}）"
    );

    // 契约 2：键盘滚动有效（点击列表卡片获得焦点后 DownArrow 继续向下滚动）。
    // 先网格扫描确认真实点击能命中卡片（点卡片 → 高亮 → FocusScope 聚焦）。
    let mut clicked_card = false;
    for x in [60.0_f32, 110.0, 170.0, 230.0, 290.0] {
        for fraction in [0.55_f32, 0.6, 0.65, 0.7, 0.75, 0.8] {
            let position = LogicalPosition::new(drawer_x + x, drawer_y + drawer_h * fraction);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
            let highlighted: Vec<bool> = (0..window.get_review_cards().row_count())
                .map(|index| {
                    window
                        .get_review_cards()
                        .row_data(index)
                        .unwrap()
                        .highlighted
                })
                .collect();
            if highlighted.iter().any(|h| *h) {
                clicked_card = true;
                break;
            }
        }
        if clicked_card {
            break;
        }
    }
    assert!(
        clicked_card,
        "真实点击必须命中抽屉内候选卡片并触发高亮（卡片→地图联动入口）"
    );
    let before_key = window.get_review_list_viewport_y();
    window.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::DownArrow.into(),
    });
    let after_key = window.get_review_list_viewport_y();
    assert!(
        after_key < before_key,
        "DownArrow 必须继续向下滚动：before={before_key} after={after_key}"
    );

    // 契约 3a：真实点击“建筑”标签（第一行第一个）→ 滚动位置复位到顶。
    // 抽屉标题下第一行标签中心约在 y=+54px、x=左缘+56px。
    let first_tab = LogicalPosition::new(drawer_x + 56.0, drawer_y + 54.0);
    window.window().dispatch_event(WindowEvent::PointerPressed {
        position: first_tab,
        button: PointerEventButton::Left,
    });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased {
            position: first_tab,
            button: PointerEventButton::Left,
        });
    assert_eq!(
        window.get_review_list_viewport_y(),
        0.0,
        "分类标签点击后滚动位置必须复位到顶部"
    );

    // 契约 3b：回调切换分类后行为一致（列表仍可滚动）。
    window.invoke_review_category_clicked(1);
    assert_eq!(window.get_review_active_category(), 1);
    window.invoke_review_category_clicked(0);
    assert_eq!(window.get_review_active_category(), 0);

    // 契约 4：大量候选下“封账完成评审”固定于抽屉底部，真实可见可点。
    // 抽屉底部为操作行（暂停/继续/封账），先点预估中心，未命中则小范围扫描，
    // 点击成功后 F5 封账落账（1026 条决定一次性写回），呈现 sealed + 导出摘要。
    let mut sealed = window.get_review_sealed();
    // 封账行位于抽屉底部右侧；详情面板高度随高亮候选变化会小幅移动其位置，
    // 因此扫描底部右侧密集网格（避开左侧暂停/继续按钮，x 限制在 0.72+）。
    'scan: for x_ratio in [0.72_f32, 0.77, 0.82, 0.87, 0.92, 0.97] {
        for y in (0..=34).map(|step| drawer_y + drawer_h - 140.0 + step as f32 * 4.0) {
            let position = LogicalPosition::new(drawer_x + drawer_w * x_ratio, y);
            window.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
            if window.get_review_sealed() {
                sealed = true;
                break 'scan;
            }
        }
    }
    assert!(
        sealed,
        "大量候选下操作栏（封账）必须真实可见可点：点击后应完成封账"
    );
    assert!(
        window.get_review_summary_visible(),
        "封账成功后必须呈现导出摘要"
    );
}
