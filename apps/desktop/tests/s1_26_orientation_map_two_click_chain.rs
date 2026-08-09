//! T36 正式回归（a）：真实朝向页 + 桩 AMap——点击两次必须产出
//! `orientation_points`（锁死 JS 链路：页面点击 → `window.ipc.postMessage`
//! → 真实解析 → 工作区 seam → F5 角度计算 → 抽屉出现角度与参考线）。
//!
//! 与 s1_15 同一真实 WebView 驱动手法：加载 `build_boundary_edit_page_html`
//! 生成的真实朝向页（orientation_mode=true），注入 AMap/地图桩后调用页面
//! 自己的 `initOrientationMode` 与地图 click 处理器，捕获页面 postMessage
//! 原始载荷，再经 `parse_ipc_message` + 工作区 seam 断言角度/参考线回填。
//!
//! 说明：本契约不写入高德密钥，`navigate(1)` 走空键分支（不创建应用侧真实
//! AMap WebView）——测试自建单个真实 WebView 驱动页面，避免同一进程内
//! 两个 WebView2 子窗口并存导致 EmbeddedBrowserWebView 崩溃（zz_diag
//! 曾在此环境 0xc000041d 复现）；生产环境的 hide→show 串行化由
//! map_webview 单测与真机走查覆盖。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use data_persistence::CampusCrudApi;
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use gaode_client::{build_boundary_edit_page_html, parse_ipc_message, BoundaryEditPageConfig};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;
use slint::Model;

thread_local! {
    static MAP_WEBVIEW: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}

const READY_TAG: &str = "__t36_orientation_ready__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriverStep {
    WaitReady,
    StubAndInit,
    ClickOne,
    ClickTwo,
    Done,
}

struct Driver {
    step: DriverStep,
    next_at: Instant,
    captured: Arc<Mutex<Vec<String>>>,
}

impl Driver {
    fn advance(&mut self) {
        self.step = match self.step {
            DriverStep::WaitReady => DriverStep::StubAndInit,
            DriverStep::StubAndInit => DriverStep::ClickOne,
            DriverStep::ClickOne => DriverStep::ClickTwo,
            DriverStep::ClickTwo => DriverStep::Done,
            DriverStep::Done => DriverStep::Done,
        };
        self.next_at = Instant::now() + Duration::from_millis(120);
    }

    fn run_one(&mut self) -> bool {
        if Instant::now() < self.next_at {
            return false;
        }
        let done = MAP_WEBVIEW.with(|slot| {
            let slot_borrow = slot.borrow();
            let Some(webview) = slot_borrow.as_ref() else {
                return false;
            };
            match self.step {
                DriverStep::WaitReady => {
                    // 页面脚本就绪后回传一个 ready 标记；未加载时静默重试。
                    let _ = webview.evaluate_script(&format!(
                        "if (typeof initOrientationMode === 'function') {{ window.ipc.postMessage(JSON.stringify({{type:'{READY_TAG}'}})); }}"
                    ));
                    let ready = self
                        .captured
                        .lock()
                        .expect("capture lock")
                        .iter()
                        .any(|message| message.contains(READY_TAG));
                    if ready {
                        self.advance();
                    }
                    false
                }
                DriverStep::StubAndInit => {
                    // 桩 AMap/地图：页面自己的点击回调经 map.on('click', ...) 注册。
                    let script = r#"
                        window.map = {
                          on: function(type, fn) { if (type === 'click') { this._clickHandler = fn; } },
                          off: function() {},
                          add: function() {},
                          remove: function() {},
                          resize: function() {}
                        };
                        window.AMap = {
                          LngLat: function(lng, lat) { return { lng: lng, lat: lat }; },
                          Polyline: function() { return { setPath: function() {} }; },
                          Polygon: function() { return {}; },
                          Map: function() { return {}; },
                          Marker: function() { return { addTo: function() {} }; },
                          Pixel: function() { return {}; }
                        };
                        window.initOrientationMode();
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                DriverStep::ClickOne => {
                    let script = r#"
                        window.map._clickHandler({ lnglat: { lng: 116.3975, lat: 39.9160 } });
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                DriverStep::ClickTwo => {
                    let script = r#"
                        window.map._clickHandler({ lnglat: { lng: 116.3985, lat: 39.9160 } });
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    true
                }
                DriverStep::Done => true,
            }
        });
        if done {
            self.step = DriverStep::Done;
            true
        } else {
            false
        }
    }

    fn waiting_for_payload(&self, captured: &[String]) -> bool {
        self.step == DriverStep::Done
            && captured
                .iter()
                .any(|message| message.contains("\"type\":\"orientation_points\""))
    }
}

#[test]
fn s1_26_real_orientation_page_two_clicks_produce_orientation_points() {
    // 1. 真实朝向页 HTML（与生产完全同一构建路径）。
    let config = BoundaryEditPageConfig::new("testapikey1234567890", "testsecuritykey1234567890")
        .with_anchor(116.3980, 39.9160)
        .with_orientation_mode(true);
    let html = build_boundary_edit_page_html(&config).expect("build real orientation page");

    // 2. 装配桌面应用（临时库 + 高德密钥）。
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("s1-26.db");
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

    // 3. 打开方案、确认边界、进入步骤②。
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#
            .into(),
    );
    assert!(window.get_workspace_boundary_is_determined());
    window.invoke_workspace_step_clicked(1);
    assert_eq!(window.get_workspace_active_step(), 1);

    // 4. 真实 wry WebView 加载朝向页，捕获页面 postMessage 原始载荷。
    let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_web = Arc::clone(&captured);
    let weak_window = window.as_weak();
    let _ = slint::spawn_local(async move {
        let Some(app_window) = weak_window.upgrade() else {
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            return;
        };
        let scale = app_window.window().scale_factor();
        let width = app_window.window().size().width;
        let bounds = wry::Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 0)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
                (width as f32 * scale) as u32,
                400,
            )),
        };
        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(move |request: wry::http::Request<String>| {
                captured_web
                    .lock()
                    .expect("capture lock")
                    .push(request.body().to_string());
            })
            .build_as_child(&*winit_win);
        if let Ok(webview) = result {
            MAP_WEBVIEW.with(|slot| *slot.borrow_mut() = Some(webview));
        }
    });

    // 5. 驱动真实页面完成"两次点击 → orientation_points"，直到收到载荷。
    let driver = Rc::new(RefCell::new(Driver {
        step: DriverStep::WaitReady,
        next_at: Instant::now(),
        captured: Arc::clone(&captured),
    }));
    let driver_weak = Rc::downgrade(&driver);
    let pump_timer = Rc::new(RefCell::new(slint::Timer::default()));
    let timer_for_pump = Rc::clone(&pump_timer);
    let deadline = Instant::now() + Duration::from_secs(60);
    let debug_state = Rc::new(RefCell::new(String::new()));
    let debug_weak = Rc::downgrade(&debug_state);
    pump_timer.borrow().start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            let Some(driver) = driver_weak.upgrade() else {
                return;
            };
            let mut driver = driver.borrow_mut();
            let payload_seen = {
                let captured = driver.captured.lock().expect("capture lock");
                driver.waiting_for_payload(&captured)
            };
            if payload_seen || Instant::now() >= deadline {
                timer_for_pump.borrow().stop();
                if let Some(debug) = debug_weak.upgrade() {
                    *debug.borrow_mut() = format!(
                        "step={:?} captured={:?}",
                        driver.step,
                        driver
                            .captured
                            .lock()
                            .expect("capture lock")
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                    );
                }
                slint::quit_event_loop().expect("stop t36 driver loop");
                return;
            }
            driver.run_one();
        },
    );
    slint::run_event_loop_until_quit().expect("run t36 driver loop");
    assert!(
        Instant::now() < deadline,
        "真实朝向页未在期限内产出 orientation_points 载荷；驱动状态：{}",
        debug_state.borrow()
    );

    // 6. 载荷必须来自页面两次点击（两个真实经纬度点）。
    let captured_guard = captured.lock().expect("capture lock");
    let orientation_payload = captured_guard
        .iter()
        .find(|message| message.contains("\"type\":\"orientation_points\""))
        .cloned()
        .expect("页面必须提交 orientation_points 载荷");
    drop(captured_guard);
    let parsed = parse_ipc_message(&orientation_payload).expect("parse real page payload");
    match parsed {
        gaode_client::IpcMessage::OrientationPoints { points } => {
            assert_eq!(points.len(), 2, "两次点击必须产出两个点");
            assert!(
                points[0][0] > 116.0 && points[0][1] > 39.0,
                "点坐标必须来自真实页面点击：{points:?}"
            );
        }
        other => panic!("真实页面载荷必须解析为 OrientationPoints：{other:?}"),
    }

    // 7. 经工作区 seam 回填：抽屉出现两点、参考线与角度（F5 计算）。
    window.invoke_workspace_map_ipc(orientation_payload.into());
    assert_eq!(
        window.get_workspace_orientation_points().row_count(),
        2,
        "两点必须回填到抽屉"
    );
    assert!(
        !window.get_workspace_orientation_path_commands().is_empty(),
        "参考线路径必须回填"
    );
    assert!(
        !window.get_workspace_orientation_arrow_commands().is_empty(),
        "方向箭头必须回填"
    );
    assert!(
        (window.get_workspace_orientation_angle() - 90.0).abs() < 1.0,
        "正东两点角度应为 90°：{}",
        window.get_workspace_orientation_angle()
    );
    assert!(window.get_workspace_orientation_status().contains("已计算"));

    // "确认两点朝向"可用：首次设定直接保存。
    window.invoke_workspace_orientation_confirm_two_points_clicked();
    assert!(window.get_workspace_orientation_is_determined());
    assert_eq!(window.get_workspace_completed_steps(), 2);

    // 经公开入口隐藏应用侧地图 WebView（进入评审步骤即 hide），避免窗口
    // 析构后 WebView2 子窗口 COM 崩溃。
    window.invoke_workspace_step_clicked(3);

    // WebView 是 winit 窗口的子窗口：先显式释放，避免窗口析构后 COM 崩溃。
    MAP_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
}
