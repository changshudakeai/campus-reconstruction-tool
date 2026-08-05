//! M1 acceptance 4.1: the real map page (B3 HTML + JS) must produce complete
//! MultiPolygon geometry through the real wry IPC seam into F9 export.
//!
//! 这不是“注入 IPC 夹具”：测试加载 `build_boundary_edit_page_html` 生成的真实
//! 地图页，在真实 WebView 里执行页面自己的函数与按钮点击（handleMapClick →
//! 确认边界 → 添加区域 → 确认区域 → 提交边界），捕获页面 postMessage 的原始
//! 载荷，再经 parse_ipc_message → 工作区 seam → export-flow 完成导出。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use desktop_shell::{
    assemble_application, AppWindow, OperationPresentationState, ShellDatabases, ShellPresenter,
    ViewModelInjector,
};
use gaode_client::{build_boundary_edit_page_html, parse_ipc_message, BoundaryEditPageConfig};
use global_settings::FirstRunSetup;
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::CampusId;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

thread_local! {
    static MAP_WEBVIEW: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}

const READY_TAG: &str = "__m1_seam_ready__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriverStep {
    WaitReady,
    EnableManual,
    RingOne,
    ConfirmOne,
    AddArea,
    RingTwo,
    ConfirmTwo,
    Submit,
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
            DriverStep::WaitReady => DriverStep::EnableManual,
            DriverStep::EnableManual => DriverStep::RingOne,
            DriverStep::RingOne => DriverStep::ConfirmOne,
            DriverStep::ConfirmOne => DriverStep::AddArea,
            DriverStep::AddArea => DriverStep::RingTwo,
            DriverStep::RingTwo => DriverStep::ConfirmTwo,
            DriverStep::ConfirmTwo => DriverStep::Submit,
            DriverStep::Submit => DriverStep::Done,
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
                        "if (typeof enableManualMode === 'function') {{ window.ipc.postMessage(JSON.stringify({{type:'{READY_TAG}'}})); }}"
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
                DriverStep::EnableManual => {
                    let script = r#"
                        window.map = {
                          on: function(type, fn) { if (type === 'click') { this._clickHandler = fn; } },
                          off: function() {},
                          add: function() {},
                          remove: function() {}
                        };
                        window.AMap = {
                          Polyline: function() { return { setPath: function() {} }; }
                        };
                        enableManualMode();
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                DriverStep::RingOne => {
                    let script = r#"
                        handleMapClick({ lnglat: { lng: 116.4000, lat: 39.9000 } });
                        handleMapClick({ lnglat: { lng: 116.4010, lat: 39.9000 } });
                        handleMapClick({ lnglat: { lng: 116.4010, lat: 39.9010 } });
                        handleMapClick({ lnglat: { lng: 116.4000, lat: 39.9010 } });
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                DriverStep::ConfirmOne => {
                    let _ = webview
                        .evaluate_script("document.getElementById('confirm-manual-btn').click();");
                    self.advance();
                    false
                }
                DriverStep::AddArea => {
                    let _ =
                        webview.evaluate_script("document.getElementById('add-area-btn').click();");
                    self.advance();
                    false
                }
                DriverStep::RingTwo => {
                    let script = r#"
                        handleMapClick({ lnglat: { lng: 116.4050, lat: 39.9050 } });
                        handleMapClick({ lnglat: { lng: 116.4060, lat: 39.9050 } });
                        handleMapClick({ lnglat: { lng: 116.4060, lat: 39.9060 } });
                        handleMapClick({ lnglat: { lng: 116.4050, lat: 39.9060 } });
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                DriverStep::ConfirmTwo => {
                    let _ = webview
                        .evaluate_script("document.getElementById('confirm-area-btn').click();");
                    self.advance();
                    false
                }
                DriverStep::Submit => {
                    let _ = webview.evaluate_script(
                        "document.getElementById('submit-boundary-btn').click();",
                    );
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
                .any(|message| message.contains("\"MultiPolygon\""))
    }
}

fn pump_until_terminal(window: &AppWindow) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if window.get_operation_state() != OperationPresentationState::Processing
                || Instant::now() >= deadline
            {
                slint::quit_event_loop().expect("stop seam export loop");
            }
        },
    );
    slint::run_event_loop_until_quit().expect("run seam export loop");
}

#[test]
fn real_map_page_multipolygon_reaches_f9_with_all_outer_rings() {
    // 1. 真实 B3 地图页 HTML（与生产完全同一构建路径）。
    let config = BoundaryEditPageConfig::new("testapikey1234567890", "testsecuritykey1234567890")
        .with_anchor(116.4005, 39.9025);
    let html = build_boundary_edit_page_html(&config).expect("build real map page");

    // 2. 装配桌面应用（临时库 + 导出目录）。
    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("m1-seam.db");
    let export_dir = directory.path().join("exports");
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
    injector
        .settings_mut()
        .set_default_export_location(export_dir.to_str().expect("temporary path"))
        .expect("set export directory");
    let campus = injector
        .projects_mut()
        .create_campus("seam campus")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "seam plan")
        .expect("create plan");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let _runtime = assemble_application(&window, injector, Arc::clone(&center));
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    assert_eq!(window.get_active_screen(), 4);

    // 3. 真实 wry WebView 加载地图页，捕获页面 postMessage 原始载荷。
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

    // 4. 驱动真实页面完成“多块区域 → 确认 → 提交”，直到收到 MultiPolygon 载荷。
    let driver = Rc::new(RefCell::new(Driver {
        step: DriverStep::WaitReady,
        next_at: Instant::now(),
        captured: Arc::clone(&captured),
    }));
    let driver_weak = Rc::downgrade(&driver);
    let pump_timer = Rc::new(RefCell::new(slint::Timer::default()));
    let timer_for_pump = Rc::clone(&pump_timer);
    let weak_window = window.as_weak();
    let deadline = Instant::now() + Duration::from_secs(20);
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
                if let Some(window) = weak_window.upgrade() {
                    let _ = window;
                }
                slint::quit_event_loop().expect("stop seam driver loop");
                return;
            }
            driver.run_one();
        },
    );
    slint::run_event_loop_until_quit().expect("run seam driver loop");
    assert!(
        Instant::now() < deadline,
        "真实地图页未在期限内产出 MultiPolygon 载荷；驱动状态：{}",
        debug_state.borrow()
    );

    // 5. 从真实页面捕获的原始载荷取最后一个 confirm_boundary（MultiPolygon）。
    let captured = captured.lock().expect("capture lock");
    let multipolygon_payload = captured
        .iter()
        .rev()
        .find(|message| message.contains("\"MultiPolygon\""))
        .cloned()
        .expect("页面必须提交 MultiPolygon 载荷");
    let polygon_payload = captured
        .iter()
        .find(|message| {
            message.contains("\"type\":\"confirm_boundary\"")
                && message.contains("\"coords\"")
                && !message.contains("\"geometry\"")
        })
        .cloned()
        .expect("首区域必须按 Polygon 直交");
    drop(captured);

    // 6. 载荷必须经真实解析器原样进入工作区 seam 与 F9。
    let parsed = parse_ipc_message(&multipolygon_payload).expect("parse real page payload");
    match parsed {
        gaode_client::IpcMessage::ConfirmBoundaryGeometry {
            r#type,
            coordinates,
        } => {
            assert_eq!(r#type, "MultiPolygon");
            let polygons = coordinates.as_array().expect("MultiPolygon 坐标数组");
            assert_eq!(polygons.len(), 2, "两个区域的外环都必须保留");
            for polygon in polygons {
                let ring = polygon
                    .as_array()
                    .and_then(|rings| rings.first())
                    .expect("外环");
                assert_eq!(ring.as_array().expect("环坐标").len(), 4);
            }
        }
        other => panic!("真实页面载荷必须解析为 ConfirmBoundaryGeometry，得到 {other:?}"),
    }

    window.invoke_workspace_map_ipc(multipolygon_payload.clone().into());
    assert!(
        window.get_workspace_boundary_is_determined(),
        "完整 MultiPolygon 必须进入边界确认"
    );
    assert!(
        polygon_payload.contains("116.4") && polygon_payload.contains("39.9"),
        "首区域 Polygon 载荷坐标必须来自真实页面：{polygon_payload}"
    );

    // 7. 直接导出：footprint 覆盖所有外环（单一区域不可能达到两个区域的外接尺寸）。
    window.invoke_workspace_step_clicked(4);
    window.invoke_workspace_export_start_clicked();
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Processing
    );
    pump_until_terminal(&window);
    assert_eq!(
        window.get_operation_state(),
        OperationPresentationState::Succeeded
    );
    assert!(export_dir.join(format!("{plan_id}.schem")).is_file());
    assert!(export_dir
        .join(format!("{plan_id}.foundation_manifest.json"))
        .is_file());
    let subtitle = window.get_workspace_placeholder_subtitle();
    let dimensions = subtitle
        .split_once("尺寸 ")
        .and_then(|(_, value)| value.split_once('）'))
        .map(|(value, _)| value)
        .expect("成功副标题必须暴露 F9 尺寸");
    let values: Vec<usize> = dimensions
        .split('×')
        .map(|value| value.parse::<usize>().expect("尺寸必须是数字"))
        .collect();
    assert!(
        values[0] > 300 && values[2] > 300,
        "footprint 必须覆盖两个外环：{dimensions}"
    );

    // WebView 是 winit 窗口的子窗口：先显式释放，避免窗口析构后 COM 崩溃。
    MAP_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
}
