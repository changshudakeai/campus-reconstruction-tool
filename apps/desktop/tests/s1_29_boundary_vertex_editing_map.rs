//! 边界顶点编辑 JS 层契约（真实 WebView2 + 桩 AMap）：
//! - 点击边界标记点 → 选中（vertex_selected），高亮 + 相邻两条边中点 "+" 按钮；
//! - 未选中时无 "+" 叠加；点击空白取消选中（vertex_deselected）；
//! - 点击 "+" 在对应边上插入新点，新点自动选中，顶点序列与 boundary_update
//!   载荷正确；
//! - 抽屉"删除选中点"（deleteSelectedVertexFromDrawer）删除选中顶点；
//!   剩余点数 <= 3 时拒绝并回传 delete_vertex_rejected，路径不被破坏；
//! - PolygonEditor dragnode → boundary_update（拖拽实时同步）；
//! - submitBoundaryFromDrawer 用编辑后的路径提交 confirm_boundary。
//!
//! 与 s1_26 同一真实 WebView 驱动手法：加载生产同一构建路径的
//! `build_boundary_edit_page_html`，注入 AMap/地图桩后调用页面自身的
//! `drawBoundaryGcj` 与点击/桥接入口，捕获页面 postMessage 原始载荷再经
//! `parse_ipc_message` 断言。本契约不写入高德密钥，应用侧不创建 WebView。

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

thread_local! {
    static MAP_WEBVIEW: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}

const READY_TAG: &str = "__vertex_editing_ready__";

const SQUARE: [[f64; 2]; 4] = [
    [116.40, 39.90],
    [116.41, 39.90],
    [116.41, 39.91],
    [116.40, 39.91],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriverStep {
    WaitReady,
    StubAndDraw,
    ManualThenEdit, // 回归：先人工圈画点击，再进入编辑态，旧模式不得残留
    ManualThenEditAssert,
    ClickVertex1,
    ClickPlus,
    DeleteSelected,
    DeleteNoSelection,
    ClickVertex2,
    DeleteToThree,
    ClickVertex1Again,
    DeleteRejected,
    ClickEmpty,
    DragNode,
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
            DriverStep::WaitReady => DriverStep::StubAndDraw,
            DriverStep::StubAndDraw => DriverStep::ManualThenEdit,
            DriverStep::ManualThenEdit => DriverStep::ManualThenEditAssert,
            DriverStep::ManualThenEditAssert => DriverStep::ClickVertex1,
            DriverStep::ClickVertex1 => DriverStep::ClickPlus,
            DriverStep::ClickPlus => DriverStep::DeleteSelected,
            DriverStep::DeleteSelected => DriverStep::DeleteNoSelection,
            DriverStep::DeleteNoSelection => DriverStep::ClickVertex2,
            DriverStep::ClickVertex2 => DriverStep::DeleteToThree,
            DriverStep::DeleteToThree => DriverStep::ClickVertex1Again,
            DriverStep::ClickVertex1Again => DriverStep::DeleteRejected,
            DriverStep::DeleteRejected => DriverStep::ClickEmpty,
            DriverStep::ClickEmpty => DriverStep::DragNode,
            DriverStep::DragNode => DriverStep::Submit,
            DriverStep::Submit | DriverStep::Done => DriverStep::Done,
        };
        self.next_at = Instant::now() + Duration::from_millis(80);
    }

    fn snapshot_active_overlays(&self, webview: &wry::WebView) {
        // 经 wry 注入的 window.ipc 回传快照（evaluate_script 无返回值通道）
        let _ = webview.evaluate_script(
            "(function(){ var mid=0, hl=0; window.__stub.midMarkers.forEach(function(m){ if(!m.removed){ mid++; } }); window.__stub.highlightMarkers.forEach(function(m){ if(!m.removed){ hl++; } }); window.ipc.postMessage(JSON.stringify({type:'__overlay_snapshot__', mid: mid, highlight: hl})); })()",
        );
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
                    let _ = webview.evaluate_script(&format!(
                        "if (typeof drawBoundaryGcj === 'function') {{ window.ipc.postMessage(JSON.stringify({{type:'{READY_TAG}'}})); }}"
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
                DriverStep::StubAndDraw => {
                    let square_json = serde_json::to_string(&SQUARE).expect("square json");
                    let script = format!(
                        r#"
                        window.__stub = {{
                          polygon: null,
                          editor: null,
                          midMarkers: [],
                          highlightMarkers: [],
                          clickHandler: null
                        }};
                        window.AMap = {{
                          LngLat: function(lng, lat) {{ return {{ lng: lng, lat: lat }}; }},
                          Pixel: function(x, y) {{ return {{ x: x, y: y }}; }},
                          Map: function() {{
                            return {{
                              on: function(type, fn) {{
                                if (type === 'click') {{
                                  // 同回调同上下文只保留一份（AMap 语义）
                                  if (window.__stub.clickHandler !== fn) {{
                                    window.__stub.clickHandler = fn;
                                  }}
                                }}
                              }},
                              off: function(type, fn) {{
                                // AMap v2.0 语义：按函数对象移除
                                if (type === 'click' && window.__stub.clickHandler === fn) {{
                                  window.__stub.clickHandler = null;
                                }}
                              }},
                              add: function() {{}},
                              remove: function(obj) {{ if (obj && typeof obj === 'object') {{ obj.removed = true; }} }},
                              resize: function() {{}},
                              lngLatToContainer: function(lnglat) {{
                                return {{ x: (lnglat.lng - 116.40) * 100000 + 50, y: (39.91 - lnglat.lat) * 100000 + 50 }};
                              }}
                            }};
                          }},
                          Polygon: function(opts) {{
                            function toLngLat(c) {{
                              return Array.isArray(c)
                                ? new AMap.LngLat(c[0], c[1])
                                : new AMap.LngLat(c.lng, c.lat);
                            }}
                            window.__stub.polygon = {{
                              opts: opts,
                              path: (opts.path || []).map(toLngLat),
                              getPath: function() {{ return this.path; }},
                              setPath: function(p) {{ this.path = p.map(toLngLat); }}
                            }};
                            return window.__stub.polygon;
                          }},
                          PolygonEditor: function(map, polygon, opts) {{
                            window.__stub.editor = {{
                              map: map,
                              polygon: polygon,
                              opts: opts,
                              handlers: {{}},
                              opened: false,
                              on: function(type, fn) {{ this.handlers[type] = fn; }},
                              open: function() {{ this.opened = true; }},
                              close: function() {{ this.opened = false; }}
                            }};
                            return window.__stub.editor;
                          }},
                          CircleMarker: function(opts) {{
                            var marker = {{ opts: opts, removed: false }};
                            window.__stub.highlightMarkers.push(marker);
                            return marker;
                          }},
                          TextMarker: function(opts) {{
                            var marker = {{
                              opts: opts,
                              removed: false,
                              handlers: {{}},
                              on: function(type, fn) {{ this.handlers[type] = fn; }}
                            }};
                            window.__stub.midMarkers.push(marker);
                            return marker;
                          }},
                          Polyline: function(opts) {{
                            return {{
                              opts: opts,
                              removed: false,
                              setPath: function(p) {{ this.path = p; }}
                            }};
                          }},
                          Marker: function() {{ return {{ addTo: function() {{}} }}; }}
                        }};
                        window.map = new AMap.Map('map-container', {{}});
                        window.drawBoundaryGcj({square_json}, '测试校区');
                        "#,
                        square_json = square_json
                    );
                    let _ = webview.evaluate_script(&script);
                    self.advance();
                    false
                }
                DriverStep::ManualThenEdit => {
                    // 复现严重 bug 路径：抓取失败 → 人工圈画模式点击加点；
                    // 随后重新获取成功 → drawBoundaryGcj 进入编辑态。
                    // 旧实现裸 map.off('click') 不生效，手动点击监听残留，
                    // 编辑态点击会同时发 manual_point。这里断言不再出现。
                    let square_json = serde_json::to_string(&SQUARE).expect("square json");
                    let script = format!(
                        r#"
                        window.enableManualMode();
                        window.__stub.clickHandler({{ lnglat: {{ lng: 116.405, lat: 39.905 }} }});
                        window.drawBoundaryGcj({square_json}, '测试校区');
                        window.__stub.clickHandler({{ lnglat: {{ lng: 116.4100, lat: 39.9001 }} }});
                        window.ipc.postMessage(JSON.stringify({{ type: '__diag__', ch: typeof window.__stub.clickHandler, chName: window.__stub.clickHandler ? window.__stub.clickHandler.name : null, polygon: !!window.__stub.polygon, editor: !!window.__stub.editor, manualPoints: (typeof window.manualPoints !== 'undefined' ? window.manualPoints.length : 'undef'), enableManualModeType: typeof window.enableManualMode }}));
                        "#,
                        square_json = square_json
                    );
                    let _ = webview.evaluate_script(&script);
                    self.advance();
                    false
                }
                DriverStep::ManualThenEditAssert => {
                    // IPC 消息异步投递，延后一拍再断言；诊断消息未到则下一拍重试。
                    let captured = self.captured.lock().expect("capture lock");
                    let diag_seen = captured
                        .iter()
                        .any(|message| message.contains("\"type\":\"__diag__\""));
                    if !diag_seen {
                        drop(captured);
                        return false;
                    }
                    let manual_after_edit = captured
                        .iter()
                        .filter(|message| message.contains("manual_point"))
                        .count();
                    let vertex_selected = captured
                        .iter()
                        .filter(|message| message.contains("vertex_selected"))
                        .count();
                    drop(captured);
                    assert_eq!(
                        manual_after_edit, 1,
                        "人工圈画模式只允许产生 1 个 manual_point；进入编辑态后点击不得再触发 manual_point（严重 bug 回归）"
                    );
                    assert!(
                        vertex_selected >= 1,
                        "编辑态点击顶点必须产生 vertex_selected"
                    );
                    self.advance();
                    false
                }
                DriverStep::ClickVertex1 => {
                    // 点击靠近第 1 个顶点（像素差 10 < 16px 阈值）→ 选中 index=1
                    let _ = webview.evaluate_script(
                        "window.__stub.clickHandler({ lnglat: { lng: 116.4100, lat: 39.9001 } });",
                    );
                    self.snapshot_active_overlays(webview);
                    self.advance();
                    false
                }
                DriverStep::ClickPlus => {
                    // 点击中点 "+"（边 1→2 的中点）→ 插入新顶点并自动选中
                    let _ = webview.evaluate_script(
                        "var target = null; for (var i = 0; i < window.__stub.midMarkers.length; i++) { var m = window.__stub.midMarkers[i]; if (!m.removed) { var p = m.opts.position; if (Math.abs(p.lng - 116.41) < 1e-9 && Math.abs(p.lat - 39.905) < 1e-9) { target = m; } } } if (target) { target.handlers.click(); }",
                    );
                    self.advance();
                    false
                }
                DriverStep::DeleteSelected => {
                    let _ = webview.evaluate_script("window.deleteSelectedVertexFromDrawer();");
                    self.advance();
                    false
                }
                DriverStep::DeleteNoSelection => {
                    // 未选中时删除是 no-op（无任何 IPC 载荷）
                    let _ = webview.evaluate_script("window.deleteSelectedVertexFromDrawer();");
                    self.advance();
                    false
                }
                DriverStep::ClickVertex2 => {
                    // 重新选中第 2 个顶点（删除到 3 点）
                    let _ = webview.evaluate_script(
                        "window.__stub.clickHandler({ lnglat: { lng: 116.41, lat: 39.9099 } });",
                    );
                    self.advance();
                    false
                }
                DriverStep::DeleteToThree => {
                    let _ = webview.evaluate_script("window.deleteSelectedVertexFromDrawer();");
                    self.advance();
                    false
                }
                DriverStep::ClickVertex1Again => {
                    // 只剩 3 点时选中第 1 个顶点，再删除必须被拒绝
                    let _ = webview.evaluate_script(
                        "window.__stub.clickHandler({ lnglat: { lng: 116.4099, lat: 39.9001 } });",
                    );
                    self.advance();
                    false
                }
                DriverStep::DeleteRejected => {
                    let _ = webview.evaluate_script("window.deleteSelectedVertexFromDrawer();");
                    self.advance();
                    false
                }
                DriverStep::ClickEmpty => {
                    // 点击空白取消选中
                    let _ = webview.evaluate_script(
                        "window.__stub.clickHandler({ lnglat: { lng: 116.402, lat: 39.904 } });",
                    );
                    self.snapshot_active_overlays(webview);
                    self.advance();
                    false
                }
                DriverStep::DragNode => {
                    // 触发 PolygonEditor dragnode → boundary_update
                    let _ = webview.evaluate_script(
                        "if (window.__stub.editor.handlers.dragnode) { window.__stub.editor.handlers.dragnode({}); }",
                    );
                    self.advance();
                    false
                }
                DriverStep::Submit => {
                    let _ = webview.evaluate_script("window.submitBoundaryFromDrawer();");
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
                .any(|message| message.contains("\"type\":\"confirm_boundary\""))
    }
}

#[test]
fn s1_29_real_boundary_page_vertex_editing_payloads() {
    let config = BoundaryEditPageConfig::new("testapikey1234567890", "testsecuritykey1234567890")
        .with_anchor(116.3980, 39.9160);
    let html = build_boundary_edit_page_html(&config).expect("build real boundary page");

    let window = AppWindow::new().expect("create AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("s1-29.db");
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

    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();

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
                slint::quit_event_loop().expect("stop vertex editing driver loop");
                return;
            }
            driver.run_one();
        },
    );
    slint::run_event_loop_until_quit().expect("run vertex editing driver loop");
    assert!(
        Instant::now() < deadline,
        "真实边界页未在期限内产出 confirm_boundary 载荷；驱动状态：{}",
        debug_state.borrow()
    );

    // 选中后相邻两条边中点出现 "+"（2 个），未选中/点击空白后清零。
    let captured_guard = captured.lock().expect("capture lock");
    let mut overlay_snapshots: Vec<serde_json::Value> = Vec::new();
    for message in captured_guard.iter() {
        if message.contains("__overlay_snapshot__") {
            overlay_snapshots.push(serde_json::from_str(message).expect("overlay snapshot json"));
        }
    }
    assert_eq!(
        overlay_snapshots.len(),
        2,
        "必须有选中后与点击空白后两张快照"
    );
    assert_eq!(
        overlay_snapshots[0]["mid"], 2,
        "选中后相邻两条边各有一个 '+' 按钮"
    );
    assert_eq!(overlay_snapshots[0]["highlight"], 1, "选中点必须有高亮");
    assert_eq!(
        overlay_snapshots[1]["mid"], 0,
        "点击空白后 '+' 按钮必须全部移除"
    );
    assert_eq!(
        overlay_snapshots[1]["highlight"], 0,
        "点击空白后高亮必须移除"
    );
    let mut actual: Vec<gaode_client::IpcMessage> = Vec::new();
    for message in captured_guard.iter() {
        if let Ok(parsed) = parse_ipc_message(message) {
            if !matches!(parsed, gaode_client::IpcMessage::MapReady) {
                actual.push(parsed);
            }
        }
    }
    let expected: Vec<gaode_client::IpcMessage> = vec![
        // 回归（严重 bug）：先人工圈画点击 → 1 个 manual_point；
        // 重新获取成功进入编辑态后点击顶点 → 只选中、不再发 manual_point
        gaode_client::IpcMessage::ManualPoint {
            lon: 116.405,
            lat: 39.905,
            total: 1,
        },
        // 回归脚本末尾的编辑态顶点点击
        gaode_client::IpcMessage::VertexSelected { index: 1, count: 4 },
        // 主流程 ClickVertex1：再次选中第 1 个顶点
        gaode_client::IpcMessage::VertexSelected { index: 1, count: 4 },
        // 点击 "+"：插入新点并自动选中，随后上报 5 点路径
        gaode_client::IpcMessage::VertexSelected { index: 2, count: 5 },
        gaode_client::IpcMessage::BoundaryUpdate {
            coords: inserted_path(),
        },
        // 删除选中点：先取消选中，再上报 4 点路径
        gaode_client::IpcMessage::VertexDeselected,
        gaode_client::IpcMessage::BoundaryUpdate {
            coords: square_path(),
        },
        // 未选中时删除 no-op；重新选中第 2 个顶点
        gaode_client::IpcMessage::VertexSelected { index: 2, count: 4 },
        // 删除到 3 点
        gaode_client::IpcMessage::VertexDeselected,
        gaode_client::IpcMessage::BoundaryUpdate {
            coords: triangle_path(),
        },
        // 只剩 3 点再选中并删除 → 拒绝，路径不变
        gaode_client::IpcMessage::VertexSelected { index: 1, count: 3 },
        gaode_client::IpcMessage::DeleteVertexRejected {
            reason: "too_few_points".to_owned(),
        },
        // 点击空白取消选中
        gaode_client::IpcMessage::VertexDeselected,
        // 拖拽 → 3 点路径
        gaode_client::IpcMessage::BoundaryUpdate {
            coords: triangle_path(),
        },
        // 确认 → 编辑后的 3 点路径
        gaode_client::IpcMessage::ConfirmBoundary {
            coords: triangle_path(),
        },
    ];
    assert_eq!(actual.len(), expected.len(), "载荷序列不符：{actual:?}");
    for (index, (got, want)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "第 {index} 条载荷不符");
    }
    drop(captured_guard);

    // 经工作区 seam 回填：vertex_selected 使抽屉"删除选中点"可用。
    window.invoke_workspace_map_ipc(r#"{"type":"vertex_selected","index":1,"count":4}"#.into());
    assert!(window.get_workspace_boundary_delete_selected_enabled());
    window.invoke_workspace_map_ipc(r#"{"type":"vertex_deselected"}"#.into());
    assert!(!window.get_workspace_boundary_delete_selected_enabled());
    window.invoke_workspace_map_ipc(
        r#"{"type":"delete_vertex_rejected","reason":"too_few_points"}"#.into(),
    );
    assert!(window.get_error_dialog_visible());

    // 经公开入口隐藏应用侧地图 WebView（进入评审步骤即 hide），避免窗口
    // 析构后 WebView2 子窗口 COM 崩溃。
    window.invoke_workspace_step_clicked(3);
    MAP_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
}

fn square_path() -> Vec<[f64; 2]> {
    SQUARE.to_vec()
}

fn inserted_path() -> Vec<[f64; 2]> {
    vec![
        [116.40, 39.90],
        [116.41, 39.90],
        [116.41, 39.905],
        [116.41, 39.91],
        [116.40, 39.91],
    ]
}

fn triangle_path() -> Vec<[f64; 2]> {
    vec![[116.40, 39.90], [116.41, 39.90], [116.40, 39.91]]
}
