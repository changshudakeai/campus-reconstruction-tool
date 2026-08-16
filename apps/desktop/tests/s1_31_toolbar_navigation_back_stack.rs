//! 本工单契约测试：通知中心跳转修复 + 顶部工具栏重构 + 全局返回历史栈。
//!
//! 覆盖验收条目：
//! - A.1/A.2：工作区/方案列表点“通知中心”均可跳转，且地图 WebView 隐藏
//!   （经 `map_visible` 探针断言，避免原生子窗口盖住页面）。
//! - B.3-B.5：每页左上=返回+上下文标题；右上=通知/设置常驻（保留未读角标）；
//!   回收站/切换校区收进“…”溢出菜单（菜单项带文字标签）。
//! - B.7：800×666 / 1000×666 两档按钮矩形可见、互不重叠、不越界
//!   （沿用 T34 矩形断言方法）。
//! - C.8-C.13：历史栈逐层返回（方案列表→工作区→通知→回收站→逐层返回）；
//!   从通知进回收站，返回→通知中心；返回工作区恢复方案与步骤；
//!   校区选择作为入口页（从零进入无返回）；无上一页不显示返回按钮。
//!
//! Slint 每进程只能初始化一次平台，整个文件一个 `#[test]` 串行跑完。

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, map_visible, set_map_visible_probe, AppWindow, ShellDatabases,
    ShellPresenter, ViewModelInjector,
};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::ComponentHandle;
use std::sync::Arc;

const RIGHT_PAD: f32 = 16.0;
const BUTTON_W: f32 = 40.0;
const BUTTON_H: f32 = 32.0;
const SPACING: f32 = 8.0;
const BACK_W: f32 = 72.0;

struct TestApp {
    _directory: tempfile::TempDir,
    window: AppWindow,
    _runtime: desktop_shell::ApplicationRuntime,
    plan_id: String,
}

impl TestApp {
    /// 老用户：完成首设、建校区与方案，启动直接着陆方案列表。
    fn returning_user() -> Self {
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("s1-31.db");
        let mut injector =
            ViewModelInjector::new(ShellDatabases::open(&database_path).expect("open databases"))
                .expect("construct injector");
        injector
            .settings_mut()
            .complete_first_run(&FirstRunSetup {
                language: "zh-CN".into(),
                minecraft_version: "26.1.2".into(),
                acknowledged: true,
            })
            .expect("complete first run");
        let campus = injector
            .projects_mut()
            .database()
            .create_campus("验收校区")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "验收方案")
            .expect("create plan");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        Self {
            _directory: directory,
            window,
            _runtime,
            plan_id: plan_id.to_string(),
        }
    }

    /// 全新库：尚未完成首次设置，启动落在首启向导。
    fn first_run() -> Self {
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("s1-31-first-run.db");
        let injector =
            ViewModelInjector::new(ShellDatabases::open(&database_path).expect("open databases"))
                .expect("construct injector");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        Self {
            _directory: directory,
            window,
            _runtime,
            plan_id: String::new(),
        }
    }

    /// 预置“上次打开方案”：启动即工作现场恢复，直接从零进入工作区。
    fn restored_workspace() -> Self {
        let window = AppWindow::new().expect("create AppWindow");
        let center = NotificationCenter::init(PresenterRegistry::new());
        center
            .registry()
            .set_presenter(ShellPresenter::install(&window));

        let directory = tempfile::tempdir().expect("temporary directory");
        let database_path = directory.path().join("s1-31-restore.db");
        let mut injector =
            ViewModelInjector::new(ShellDatabases::open(&database_path).expect("open databases"))
                .expect("construct injector");
        injector
            .settings_mut()
            .complete_first_run(&FirstRunSetup {
                language: "zh-CN".into(),
                minecraft_version: "26.1.2".into(),
                acknowledged: true,
            })
            .expect("complete first run");
        let campus = injector
            .projects_mut()
            .database()
            .create_campus("验收校区")
            .expect("create campus");
        let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "验收方案")
            .expect("create plan");
        injector
            .settings_mut()
            .remember_campus(&campus_id)
            .expect("remember campus");
        let plan_id_text = plan_id.to_string();
        injector
            .save_last_active_plan(Some(&plan_id_text))
            .expect("记录上次打开方案");
        let _runtime = assemble_application(&window, injector, Arc::clone(&center));

        Self {
            _directory: directory,
            window,
            _runtime,
            plan_id: plan_id_text,
        }
    }

    fn open_plan(&self) {
        self.window
            .invoke_plan_list_card_clicked(self.plan_id.clone().into());
        assert_eq!(self.window.get_active_screen(), 4, "方案卡片单击进入工作区");
    }

    fn confirm_boundary(&self) {
        self.window.invoke_workspace_map_ipc(
            r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
                .into(),
        );
        assert!(self.window.get_workspace_boundary_is_determined());
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    fn in_bounds(self, width: f32, height: f32) -> bool {
        self.x >= 0.0
            && self.y >= 0.0
            && self.x + self.w <= width + 0.5
            && self.y + self.h <= height + 0.5
    }
}

fn logical_size(window: &AppWindow) -> (f32, f32) {
    let scale = window.window().scale_factor().max(0.001);
    let size = window.window().size();
    (size.width as f32 / scale, size.height as f32 / scale)
}

fn assert_close(actual: f32, expected: f32, message: &str) {
    assert!(
        (actual - expected).abs() < 0.5,
        "{message}：期望 {expected}，实际 {actual}"
    );
}

fn toolbar_rects(window: &AppWindow) -> [Rect; 4] {
    [
        Rect {
            x: window.get_toolbar_back_rect_x(),
            y: window.get_toolbar_back_rect_y(),
            w: window.get_toolbar_back_rect_width(),
            h: window.get_toolbar_back_rect_height(),
        },
        Rect {
            x: window.get_toolbar_notice_rect_x(),
            y: window.get_toolbar_notice_rect_y(),
            w: window.get_toolbar_notice_rect_width(),
            h: window.get_toolbar_notice_rect_height(),
        },
        Rect {
            x: window.get_toolbar_settings_rect_x(),
            y: window.get_toolbar_settings_rect_y(),
            w: window.get_toolbar_settings_rect_width(),
            h: window.get_toolbar_settings_rect_height(),
        },
        Rect {
            x: window.get_toolbar_overflow_rect_x(),
            y: window.get_toolbar_overflow_rect_y(),
            w: window.get_toolbar_overflow_rect_width(),
            h: window.get_toolbar_overflow_rect_height(),
        },
    ]
}

/// 断言工具栏矩形：可见元素不越界、两两不重叠；返回按钮按预期显隐；
/// 右簇（通知/设置/溢出）恒为右缘对齐（回收站/切换校区不再占用主工具栏）。
fn assert_toolbar_geometry(window: &AppWindow, width: f32, height: f32, back_expected: bool) {
    let [back, notice, settings, overflow] = toolbar_rects(window);

    if back_expected {
        assert!(back.w > 0.0, "历史栈非空时返回按钮必须可见");
    } else {
        assert_eq!(back.w, 0.0, "无上一页时返回按钮必须隐藏（宽度为 0）");
    }

    for (name, rect) in [
        ("back", back),
        ("notice", notice),
        ("settings", settings),
        ("overflow", overflow),
    ] {
        assert!(
            rect.in_bounds(width, height),
            "{name} 按钮矩形越界：{rect:?}（窗口 {width}×{height}）"
        );
    }

    let visible: Vec<(&str, Rect)> = [
        ("back", back),
        ("notice", notice),
        ("settings", settings),
        ("overflow", overflow),
    ]
    .into_iter()
    .filter(|(_, rect)| rect.w > 0.0 && rect.h > 0.0)
    .collect();
    for (i, (left_name, left)) in visible.iter().enumerate() {
        for (right_name, right) in visible.iter().skip(i + 1) {
            assert!(
                !left.overlaps(*right),
                "{left_name} 与 {right_name} 按钮矩形重叠：{left:?} vs {right:?}"
            );
        }
    }

    // 右簇恒为右缘对齐：通知 / 设置 / 溢出三按钮依次排布，间距 8px。
    assert_close(
        notice.x,
        width - RIGHT_PAD - 3.0 * BUTTON_W - 2.0 * SPACING,
        "通知按钮 x",
    );
    assert_close(
        settings.x,
        width - RIGHT_PAD - 2.0 * BUTTON_W - SPACING,
        "设置按钮 x",
    );
    assert_close(overflow.x, width - RIGHT_PAD - BUTTON_W, "溢出按钮 x");
    assert_close(notice.y, 12.0, "按钮 y");
    assert_close(notice.h, BUTTON_H, "按钮高");
    if back_expected {
        assert_close(back.x, 16.0, "返回按钮 x");
        assert_close(back.w, BACK_W, "返回按钮宽");
    }
}

#[test]
fn s1_31_toolbar_navigation_and_back_stack_contract() {
    // ── C.9/C.13：首启向导无返回；校区选择从零进入无返回 ────────────
    let fresh = TestApp::first_run();
    assert_eq!(fresh.window.get_active_screen(), 0, "全新库落在首启向导");
    assert!(
        !fresh.window.get_toolbar_back_visible(),
        "首启向导不显示返回按钮"
    );
    assert!(
        !fresh.window.get_wizard_continue_enabled(),
        "知情告知未勾选且必填高德 Key 缺失时'继续'必须禁用"
    );
    fresh.window.set_wizard_acknowledged(true);
    assert!(
        !fresh.window.get_wizard_continue_enabled(),
        "只勾选知情告知、仍缺必填高德 Key 时'继续'必须禁用"
    );
    fresh.window.invoke_wizard_continue_clicked();
    assert_eq!(
        fresh.window.get_active_screen(),
        0,
        "缺少必填高德 Key 时首启不得放行"
    );
    fresh
        .window
        .set_wizard_gaode_api_key("0123456789abcdef01234567".into());
    fresh
        .window
        .set_wizard_gaode_security_key("fedcba9876543210fedcba98".into());
    assert!(
        fresh.window.get_wizard_continue_enabled(),
        "填齐必填高德 Key 后'继续'必须可用"
    );
    fresh.window.invoke_wizard_continue_clicked();
    assert_eq!(
        fresh.window.get_active_screen(),
        1,
        "首启完成后进入校区选择"
    );
    assert!(
        !fresh.window.get_toolbar_back_visible(),
        "校区选择从零进入时无返回按钮"
    );
    assert_eq!(
        fresh.window.get_toolbar_title().as_str(),
        "校区选择",
        "校区选择页工具栏标题"
    );
    // 校区选择（尚未选定校区）工具栏矩阵：设置/通知可见，回收站/切换校区隐藏
    assert!(
        fresh.window.get_settings_toolbar_button_visible(),
        "校区选择页设置入口必须可见（用户需要在此改 Key）"
    );
    assert!(
        fresh.window.get_notice_toolbar_button_visible(),
        "校区选择页通知入口可显示"
    );
    assert!(
        !fresh.window.get_trash_toolbar_button_visible(),
        "尚未选定校区时回收站入口必须隐藏"
    );
    assert!(
        !fresh.window.get_switch_campus_toolbar_button_visible(),
        "尚未选定校区时切换校区入口必须隐藏"
    );
    // 校区选择（入口页）→ 通知中心 → 返回 = 校区选择
    fresh.window.invoke_notice_toolbar_button_clicked();
    assert_eq!(fresh.window.get_active_screen(), 5);
    fresh.window.invoke_toolbar_back_clicked();
    assert_eq!(fresh.window.get_active_screen(), 1, "校区选择返回上一页");

    // ── 主场景：老用户着陆方案列表 ──────────────────────────────────
    let app = TestApp::returning_user();
    let window = &app.window;
    assert_eq!(window.get_active_screen(), 2, "老用户着陆方案列表");
    assert!(!window.get_toolbar_back_visible(), "方案列表从零进入无返回");
    assert_eq!(
        window.get_toolbar_title().as_str(),
        "方案列表",
        "方案列表页工具栏标题"
    );
    assert!(window.get_notice_toolbar_button_visible(), "通知图标常驻");
    assert!(window.get_settings_toolbar_button_visible(), "设置图标常驻");

    // ── B.7：800×666 与 1000×666 两档矩形断言（方案列表）──────────
    window
        .window()
        .set_size(slint::LogicalSize::new(800.0, 666.0));
    let (w800, h666) = logical_size(window);
    assert_close(w800, 800.0, "800 逻辑宽生效");
    assert_toolbar_geometry(window, w800, h666, false);

    window
        .window()
        .set_size(slint::LogicalSize::new(1000.0, 666.0));
    let (w1000, h1000) = logical_size(window);
    assert_close(w1000, 1000.0, "1000 逻辑宽生效");
    assert_toolbar_geometry(window, w1000, h1000, false);

    // ── B.5：溢出菜单含回收站/切换校区（带文字标签）────────────────
    window.set_toolbar_overflow_open(true);
    assert!(window.get_toolbar_overflow_open(), "溢出菜单可打开");
    assert_eq!(
        window.get_trash_toolbar_label().as_str(),
        "回收站",
        "溢出菜单回收站项文字标签"
    );
    assert_eq!(
        window.get_switch_campus_toolbar_label().as_str(),
        "切换校区",
        "溢出菜单切换校区项文字标签"
    );
    // 回收站经溢出菜单进入：返回 = 方案列表
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 6, "回收站");
    assert_eq!(window.get_toolbar_title().as_str(), "回收站");
    assert!(window.get_toolbar_back_visible());
    assert_toolbar_geometry(window, w1000, h1000, true);
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2, "回收站返回方案列表");

    // 切换校区经溢出菜单进入：校区选择返回 = 方案列表
    window.set_toolbar_overflow_open(true);
    window.invoke_switch_campus_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 1, "校区选择（入口页）");
    assert_eq!(window.get_toolbar_title().as_str(), "校区选择");
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2, "校区选择返回方案列表");

    // ── A.2：方案列表点通知中心可正常跳转（且地图保持隐藏）──────────
    set_map_visible_probe(true);
    window.invoke_notice_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 5, "方案列表进入通知中心");
    assert_eq!(window.get_toolbar_title().as_str(), "通知中心");
    assert!(
        !map_visible(),
        "进入通知中心必须隐藏地图 WebView（方案列表路径）"
    );
    assert_toolbar_geometry(window, w1000, h1000, true);
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2);

    // ── A.1：工作区点通知中心 → 屏 5 且地图隐藏（bug 根因回归）──────
    app.open_plan();
    assert_eq!(
        window.get_toolbar_title().as_str(),
        "验收校区 / 验收方案",
        "工作区标题=校区/方案名"
    );
    assert!(window.get_toolbar_back_visible(), "工作区有上一页");
    assert_toolbar_geometry(window, w1000, h1000, true);

    set_map_visible_probe(true);
    assert!(map_visible(), "探针模拟工作区地图已显示");
    window.invoke_notice_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 5, "工作区点通知中心可跳转");
    assert!(
        !map_visible(),
        "工作区进入通知中心必须隐藏地图 WebView（bug 根因修复）"
    );

    // ── C.10/C.11：逐层返回 + 返回工作区恢复方案与步骤 ──────────────
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 4, "通知中心返回工作区");
    app.confirm_boundary();
    window.invoke_workspace_step_clicked(2);
    assert_eq!(window.get_workspace_active_step(), 2, "进入采集步骤");

    window.invoke_notice_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 5);
    // 从通知进回收站：回收站返回 → 通知中心
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 6, "通知→回收站");
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 5, "回收站返回→通知中心");
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 4, "通知中心返回→工作区");
    assert_eq!(
        window.get_workspace_active_step(),
        2,
        "返回工作区恢复步骤（复用会话状态）"
    );
    assert_eq!(
        window.get_workspace_plan_name().as_str(),
        "验收方案",
        "返回工作区恢复方案"
    );
    assert!(
        window.get_workspace_boundary_is_determined(),
        "返回工作区保留已确认边界"
    );
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2, "工作区返回→方案列表");
    assert!(
        !window.get_toolbar_back_visible(),
        "回到方案列表后无上一页，返回按钮隐藏"
    );

    // ── C.10 链条：方案列表 → 工作区 → 通知 → 回收站 → 逐层返回 ────
    app.open_plan();
    window.invoke_notice_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 5);
    window.invoke_trash_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 6);
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 5, "链：回收站→通知中心");
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 4, "链：通知中心→工作区");
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2, "链：工作区→方案列表");
    assert!(!window.get_toolbar_back_visible());
    // 栈空时返回按钮无动作
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 2, "无上一页时返回无动作");

    // ── C.9：设置页覆盖 + 返回 ──────────────────────────────────────
    app.open_plan();
    window.invoke_settings_toolbar_button_clicked();
    assert_eq!(window.get_active_screen(), 3, "设置页");
    assert_eq!(window.get_toolbar_title().as_str(), "设置");
    assert_toolbar_geometry(window, w1000, h1000, true);
    window.invoke_toolbar_back_clicked();
    assert_eq!(window.get_active_screen(), 4, "设置返回→工作区");

    // ── B.4：通知未读角标保留（进入页面后仍上报）────────────────────
    let center = NotificationCenter::global().expect("全局通知中心");
    center.publish(notification_center::Notification::warn(
        "应用",
        "设置已保存",
        "语言切换为中文",
    ));
    assert!(window.get_notice_unread_count() > 0, "未读角标仍保留");

    // ── C.12：工作区从零进入（工作现场恢复）也必须可返回方案列表 ────
    // （放在最后：新建窗口的 NotificationCenter::init 会替换全局实例，
    //   不干扰上面基于全局中心的通知断言。）
    let restored = TestApp::restored_workspace();
    assert_eq!(
        restored.window.get_active_screen(),
        4,
        "启动恢复上次打开方案（从零进入工作区）"
    );
    assert!(
        restored.window.get_toolbar_back_visible(),
        "工作区从零进入仍显示返回按钮（回落方案列表）"
    );
    restored.window.invoke_toolbar_back_clicked();
    assert_eq!(
        restored.window.get_active_screen(),
        2,
        "工作区返回当前校区方案列表"
    );
    assert!(
        !restored.window.get_toolbar_back_visible(),
        "方案列表无上一页时返回按钮隐藏"
    );
}
