//! 评审地图 JS spy：由生产适配器生成真实 Rust→JS 回推脚本，再把这些脚本
//! 送入真实评审页与真实 WebView2。只在 AMap 系统边界注入最小 spy，断言
//! overlay、定位、高对比高亮及地图文字开关真实触发对应的地图 API。
//!
//! 本测试只借用 Slint 窗口作为 WebView 父窗口，不设置高德密钥，因此应用侧
//! 不会创建自己的 WebView；与 s1_26 的真实 WebView 驱动手法一致。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use data_persistence::{
    CampusCrudApi, CandidateDisplay, CandidateEligibility, CandidateProjection,
    CandidateProjectionsApi, CandidateShape, CandidateValidation, Database, RawObservation,
    RawObservationsApi,
};
use desktop_shell::{
    assemble_application, AppWindow, ShellDatabases, ShellPresenter, ViewModelInjector,
};
use gaode_client::{build_review_map_page_html, ReviewMapPageConfig};
use global_settings::{FirstRunSetup, SettingsManager};
use notification_center::{NotificationCenter, PresenterRegistry};
use shared_domain_types::{CampusId, CandidateCategory};
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

thread_local! {
    static REVIEW_SPY_WEBVIEW: RefCell<Option<wry::WebView>> = const { RefCell::new(None) };
}

const SPY_READY_TAG: &str = "__review_spy_ready__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpyStep {
    WaitReady,
    StubBoot,
    QueueLocate,
    Pump,
    SetPending,
    RemoveAndLocate,
    PumpRemoved,
    ToggleText,
    BreakOverlay,
    PumpFailure,
    PostSpy,
    Done,
}

struct SpyDriver {
    step: SpyStep,
    next_at: Instant,
    captured: Arc<Mutex<Vec<String>>>,
    production_scripts: Vec<String>,
    pending_scripts: Vec<String>,
    removed_scripts: Vec<String>,
}

impl SpyDriver {
    fn advance(&mut self) {
        self.step = match self.step {
            SpyStep::WaitReady => SpyStep::StubBoot,
            SpyStep::StubBoot => SpyStep::QueueLocate,
            SpyStep::QueueLocate => SpyStep::Pump,
            SpyStep::Pump => SpyStep::SetPending,
            SpyStep::SetPending => SpyStep::RemoveAndLocate,
            SpyStep::RemoveAndLocate => SpyStep::PumpRemoved,
            SpyStep::PumpRemoved => SpyStep::ToggleText,
            SpyStep::ToggleText => SpyStep::BreakOverlay,
            SpyStep::BreakOverlay => SpyStep::PumpFailure,
            SpyStep::PumpFailure => SpyStep::PostSpy,
            SpyStep::PostSpy | SpyStep::Done => SpyStep::Done,
        };
        self.next_at = Instant::now() + Duration::from_millis(90);
    }

    fn run_one(&mut self) -> bool {
        if Instant::now() < self.next_at {
            return false;
        }
        let done = REVIEW_SPY_WEBVIEW.with(|slot| {
            let slot_borrow = slot.borrow();
            let Some(webview) = slot_borrow.as_ref() else {
                return false;
            };
            match self.step {
                SpyStep::WaitReady => {
                    let _ = webview.evaluate_script(
                        "if (typeof window.setReviewCandidates === 'function') { window.ipc.postMessage(JSON.stringify({type:'__review_spy_ready__'})); }",
                    );
                    let ready = self
                        .captured
                        .lock()
                        .expect("spy capture lock")
                        .iter()
                        .any(|message| message.contains(SPY_READY_TAG));
                    if ready {
                        self.advance();
                    }
                    false
                }
                SpyStep::StubBoot => {
                    let script = r#"
                        window.__reviewSpy = [];
                        window.__pump = null;
                        window.setInterval = function(fn) { window.__pump = fn; return 1; };
                        window.clearInterval = function() {};
                        window.map = {
                          add: function(o) { window.__reviewSpy.push(['add', o.kind]); return o; },
                          remove: function(o) { window.__reviewSpy.push(['remove', o.kind]); },
                          resize: function() {},
                          setZoomAndCenter: function(zoom, center) { window.__reviewSpy.push(['center', zoom, center]); },
                          setFitView: function() { window.__reviewSpy.push(['fit']); },
                          setFeatures: function(f) { window.__reviewSpy.push(['features', f]); }
                        };
                        window.AMap = {
                          CircleMarker: function() { return { kind: 'CircleMarker', on: function(){}, setOptions: function(x){ window.__reviewSpy.push(['setOptions','CircleMarker',x]); } }; },
                          Polyline: function(x) { window.__reviewSpy.push(['construct','Polyline',x]); return { kind: 'Polyline', on: function(){}, setOptions: function(x){ window.__reviewSpy.push(['setOptions','Polyline',x]); } }; },
                          Polygon: function(x) { window.__reviewSpy.push(['construct','Polygon',x]); return { kind: 'Polygon', on: function(){}, setOptions: function(x){ window.__reviewSpy.push(['setOptions','Polygon',x]); } }; },
                          Map: function(el, opts) { window.__reviewSpy.push(['map', opts.zoom, opts.center]); return window.map; }
                        };
                        window.boot();
                    "#;
                    let _ = webview.evaluate_script(script);
                    self.advance();
                    false
                }
                SpyStep::QueueLocate => {
                    for script in &self.production_scripts {
                        let _ = webview.evaluate_script(script);
                    }
                    self.advance();
                    false
                }
                SpyStep::Pump => {
                    let _ = webview.evaluate_script("window.__pump();");
                    self.advance();
                    false
                }
                SpyStep::SetPending => {
                    for script in &self.pending_scripts {
                        let _ = webview.evaluate_script(script);
                    }
                    self.advance();
                    false
                }
                SpyStep::RemoveAndLocate => {
                    let _ = webview.evaluate_script("window.__reviewSpy.push(['phase','remove']);");
                    for script in &self.removed_scripts {
                        let _ = webview.evaluate_script(script);
                    }
                    self.advance();
                    false
                }
                SpyStep::PumpRemoved => {
                    let _ = webview.evaluate_script("window.__pump();");
                    self.advance();
                    false
                }
                SpyStep::ToggleText => {
                    let _ = webview.evaluate_script(
                        "window.setReviewMapText(true); window.setReviewMapText(false);",
                    );
                    self.advance();
                    false
                }
                SpyStep::BreakOverlay => {
                    let _ = webview.evaluate_script(
                        "window.AMap.Polygon = function() { throw new Error('candidate=private; coordinates=private'); };",
                    );
                    if let Some(script) = self
                        .production_scripts
                        .iter()
                        .find(|script| script.starts_with("window.setReviewCandidates("))
                    {
                        let _ = webview.evaluate_script(script);
                    }
                    self.advance();
                    false
                }
                SpyStep::PumpFailure => {
                    let _ = webview.evaluate_script("window.__pump();");
                    self.advance();
                    false
                }
                SpyStep::PostSpy => {
                    let _ = webview.evaluate_script(
                        "window.ipc.postMessage(JSON.stringify({type:'__review_spy__', records: window.__reviewSpy}));",
                    );
                    self.advance();
                    false
                }
                SpyStep::Done => false,
            }
        });
        if done {
            self.step = SpyStep::Done;
            true
        } else {
            false
        }
    }

    fn waiting_for_spy(&self, captured: &[String]) -> bool {
        captured
            .iter()
            .any(|message| message.contains("\"type\":\"__review_spy__\""))
    }
}

const CANDIDATE_ID: &str = "overpass:way/review-spy:outer";

fn seed_review_candidate(database: &mut Database, plan_id: &str) {
    let observation = RawObservation::new(
        plan_id,
        CandidateCategory::Building,
        "way/review-spy",
        serde_json::json!({"tags": {"name": "评审接缝夹具", "building": "yes"}}),
        "overpass",
    );
    database
        .write_raw_observations(std::slice::from_ref(&observation))
        .expect("写入原始观测");
    let batch = database
        .prepare_candidate_batch(plan_id)
        .expect("准备候选批次");
    let projection = CandidateProjection::new(
        CANDIDATE_ID,
        plan_id,
        &observation.id,
        &observation.data_source_tag,
        &observation.entity_id,
        "default",
        observation.entity_type,
        CandidateDisplay::new(
            "评审接缝夹具",
            vec![("building".to_owned(), "yes".to_owned())],
        ),
        CandidateShape::polygon(serde_json::json!([
            [121.0, 31.0],
            [121.1, 31.0],
            [121.0, 31.1],
            [121.0, 31.0]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    );
    database
        .write_candidate_projections(&batch.id, &[projection])
        .expect("写入候选投影");
    database
        .publish_candidate_batch(&batch.id)
        .expect("发布候选批次");
}

fn capture_production_review_scripts(
    window: &AppWindow,
    center: &Arc<NotificationCenter>,
    database_path: &std::path::Path,
) -> desktop_shell::ApplicationRuntime {
    let mut injector =
        ViewModelInjector::new(ShellDatabases::open(database_path).expect("open databases"))
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
        .create_campus("评审接缝夹具校区")
        .expect("create campus");
    let campus_id = CampusId::parse(&campus.id).expect("parse campus id");
    let plan_id = injector
        .projects_mut()
        .create_plan(&campus_id, "评审接缝夹具方案")
        .expect("create plan");
    injector
        .settings_mut()
        .remember_campus(&campus_id)
        .expect("remember campus");
    let mut settings = SettingsManager::new(
        data_persistence::Database::open(database_path).expect("reopen settings database"),
    );
    settings
        .set_gaode_api_key("testapikey1234567890")
        .expect("save test API key");
    settings
        .set_gaode_security_key("testsecuritykey1234567890")
        .expect("save test security key");
    {
        let mut database = injector.projects().database();
        seed_review_candidate(&mut database, &plan_id.to_string());
    }

    let runtime = assemble_application(window, injector, Arc::clone(center));
    window.invoke_plan_list_card_clicked(plan_id.to_string().into());
    window.invoke_workspace_tutorial_dismiss_clicked();
    window.invoke_workspace_map_status_changed(true);
    window.invoke_workspace_map_ipc(
        r#"{"type":"confirm_boundary","coords":[[116.40,39.90],[116.41,39.90],[116.41,39.91],[116.40,39.91]]}"#.into(),
    );
    window.invoke_workspace_step_clicked(3);
    window.invoke_workspace_map_status_changed(true);
    runtime
}

#[test]
fn review_map_js_spy_asserts_center_zoom_overlay_highlight_and_text_toggle() {
    let html = build_review_map_page_html(
        &ReviewMapPageConfig::new("testapikey1234567890", "testsecuritykey1234567890")
            .with_anchor(121.0, 31.0)
            .with_map_text_toggle("显示地图文字", false),
    )
    .expect("build real review map page");

    let capture_window = AppWindow::new().expect("create production capture AppWindow");
    let center = NotificationCenter::init(PresenterRegistry::new());
    center
        .registry()
        .set_presenter(ShellPresenter::install(&capture_window));
    let directory = tempfile::tempdir().expect("temporary directory");
    let database_path = directory.path().join("review-map-js-spy.db");
    let runtime = capture_production_review_scripts(&capture_window, &center, &database_path);

    desktop_shell::set_review_push_probe_visible(true);
    desktop_shell::reset_review_push_count();
    capture_window.invoke_workspace_map_ipc(r#"{"type":"map_ready"}"#.into());
    capture_window.invoke_review_category_clicked(0);
    let mut production_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        production_scripts
            .iter()
            .any(|script| script.starts_with("window.setReviewCandidates(")),
        "必须捕获生产适配器生成的完整 setReviewCandidates 脚本"
    );

    desktop_shell::reset_review_push_count();
    capture_window.invoke_review_card_state_clicked(CANDIDATE_ID.into(), "keep".into());
    let update_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        update_scripts
            .iter()
            .any(|script| script.starts_with("window.updateReviewCandidate(")),
        "必须捕获生产适配器生成的 updateReviewCandidate 脚本"
    );
    production_scripts.extend(update_scripts);

    desktop_shell::reset_review_push_count();
    capture_window.invoke_review_locate_clicked(CANDIDATE_ID.into());
    let locate_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        locate_scripts
            .iter()
            .any(|script| script.starts_with("window.locateReviewCandidate(")),
        "必须捕获生产适配器生成的 locateReviewCandidate 脚本"
    );
    production_scripts.extend(locate_scripts);

    desktop_shell::reset_review_push_count();
    capture_window.invoke_review_card_state_clicked(CANDIDATE_ID.into(), "pending".into());
    let pending_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        pending_scripts
            .iter()
            .any(|script| script.starts_with("window.updateReviewCandidate(")),
        "必须捕获生产适配器生成的待定增量脚本"
    );

    desktop_shell::reset_review_push_count();
    capture_window.invoke_review_card_state_clicked(CANDIDATE_ID.into(), "remove".into());
    let mut removed_scripts = desktop_shell::review_pushed_scripts();
    assert!(
        removed_scripts
            .iter()
            .any(|script| script.starts_with("window.updateReviewCandidate(")),
        "必须捕获生产适配器生成的剔除增量脚本"
    );
    desktop_shell::reset_review_push_count();
    capture_window.invoke_review_locate_clicked(CANDIDATE_ID.into());
    removed_scripts.extend(desktop_shell::review_pushed_scripts());
    desktop_shell::set_review_push_probe_visible(false);
    drop(runtime);
    drop(capture_window);

    // WebView2 驱动使用独立父窗口，避免正式地图 PendingShow 与 spy WebView
    // 在同一个 native child-window 生命周期中互相干扰。
    let window = AppWindow::new().expect("create WebView spy AppWindow");

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
                    .expect("spy capture lock")
                    .push(request.body().to_string());
            })
            .build_as_child(&*winit_win);
        if let Ok(webview) = result {
            REVIEW_SPY_WEBVIEW.with(|slot| *slot.borrow_mut() = Some(webview));
        }
    });

    let driver = Rc::new(RefCell::new(SpyDriver {
        step: SpyStep::WaitReady,
        next_at: Instant::now(),
        captured: Arc::clone(&captured),
        production_scripts,
        pending_scripts,
        removed_scripts,
    }));
    let driver_weak = Rc::downgrade(&driver);
    let pump_timer = Rc::new(RefCell::new(slint::Timer::default()));
    let timer_for_pump = Rc::clone(&pump_timer);
    let deadline = Instant::now() + Duration::from_secs(30);
    let debug_state = Rc::new(RefCell::new(String::new()));
    let debug_weak = Rc::downgrade(&debug_state);
    pump_timer.borrow().start(
        slint::TimerMode::Repeated,
        Duration::from_millis(30),
        move || {
            let Some(driver) = driver_weak.upgrade() else {
                return;
            };
            let mut driver = driver.borrow_mut();
            let spy_seen = {
                let captured = driver.captured.lock().expect("spy capture lock");
                driver.waiting_for_spy(&captured)
            };
            if spy_seen || Instant::now() >= deadline {
                timer_for_pump.borrow().stop();
                if let Some(debug) = debug_weak.upgrade() {
                    *debug.borrow_mut() = format!(
                        "step={:?} captured={:?}",
                        driver.step,
                        driver
                            .captured
                            .lock()
                            .expect("spy capture lock")
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                    );
                }
                slint::quit_event_loop().expect("stop review spy loop");
                return;
            }
            driver.run_one();
        },
    );
    slint::run_event_loop_until_quit().expect("run review spy loop");
    assert!(
        Instant::now() < deadline,
        "真实评审地图 JS spy 未在期限内完成；驱动状态：{}",
        debug_state.borrow()
    );

    let captured_guard = captured.lock().expect("spy capture lock");
    let spy_payload = captured_guard
        .iter()
        .find(|message| message.contains("\"type\":\"__review_spy__\""))
        .cloned()
        .expect("JS spy 必须回传记录");
    drop(captured_guard);
    let payload: serde_json::Value =
        serde_json::from_str(&spy_payload).expect("JS spy 载荷必须是 JSON");
    let records = payload
        .get("records")
        .and_then(serde_json::Value::as_array)
        .expect("JS spy 必须回传 records 数组");

    let has_center = records.iter().any(|record| {
        record.get(0).and_then(serde_json::Value::as_str) == Some("center")
            && record.get(1).and_then(serde_json::Value::as_f64) == Some(18.0)
    });
    assert!(has_center, "定位必须设置合理缩放：records={records:?}");
    let center_position = records
        .iter()
        .position(|record| record.get(0).and_then(serde_json::Value::as_str) == Some("center"))
        .expect("定位必须产生 center 记录");
    let remove_phase_position = records
        .iter()
        .position(|record| {
            record.get(0).and_then(serde_json::Value::as_str) == Some("phase")
                && record.get(1).and_then(serde_json::Value::as_str) == Some("remove")
        })
        .expect("剔除阶段必须有边界记录");
    assert!(
        !records[center_position..remove_phase_position]
            .iter()
            .any(|record| record.get(0).and_then(serde_json::Value::as_str) == Some("fit")),
        "定位在绘制队列完成的同一轮不得再 setFitView 覆盖目标中心：records={records:?}"
    );

    assert!(
        records.iter().any(|record| {
            record.get(0).and_then(serde_json::Value::as_str) == Some("setOptions")
                && record
                    .get(2)
                    .and_then(serde_json::Value::as_object)
                    .and_then(|options| options.get("strokeStyle"))
                    .and_then(serde_json::Value::as_str)
                    == Some("dashed")
                && record
                    .get(2)
                    .and_then(serde_json::Value::as_object)
                    .and_then(|options| options.get("strokeDasharray"))
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|dash| dash.len() == 2)
        }),
        "待定轮廓必须调用高德 v2 支持的 dashed + strokeDasharray：records={records:?}"
    );

    let has_overlay = records.iter().any(|record| {
        record.get(0).and_then(serde_json::Value::as_str) == Some("add")
            && record.get(1).and_then(serde_json::Value::as_str) == Some("Polygon")
    });
    assert!(
        has_overlay,
        "目标必须补绘为地图 overlay：records={records:?}"
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| {
                record.get(0).and_then(serde_json::Value::as_str) == Some("add")
                    && record.get(1).and_then(serde_json::Value::as_str) == Some("Polygon")
            })
            .count(),
        1,
        "全量候选仍在队列时收到增量状态不得重复创建 overlay：records={records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.get(0).and_then(serde_json::Value::as_str) == Some("remove")
                && record.get(1).and_then(serde_json::Value::as_str) == Some("Polygon")
        }),
        "剔除候选必须从地图移除：records={records:?}"
    );

    let has_highlight = records.iter().any(|record| {
        record.get(0).and_then(serde_json::Value::as_str) == Some("setOptions")
            && record
                .get(2)
                .and_then(serde_json::Value::as_object)
                .and_then(|options| options.get("strokeColor"))
                .and_then(serde_json::Value::as_str)
                == Some("#e74c3c")
    });
    assert!(
        has_highlight,
        "定位/选中必须使用高对比红色轮廓：records={records:?}"
    );

    let has_hidden_text = records.iter().any(|record| {
        record.get(0).and_then(serde_json::Value::as_str) == Some("features")
            && record
                .get(1)
                .and_then(serde_json::Value::as_array)
                .map(|features| {
                    features
                        .iter()
                        .all(|feature| feature.as_str() != Some("point"))
                })
                == Some(true)
    });
    let has_visible_text = records.iter().any(|record| {
        record.get(0).and_then(serde_json::Value::as_str) == Some("features")
            && record
                .get(1)
                .and_then(serde_json::Value::as_array)
                .map(|features| {
                    features
                        .iter()
                        .any(|feature| feature.as_str() == Some("point"))
                })
                == Some(true)
    });
    assert!(
        has_hidden_text,
        "评审模式默认必须隐藏 point 标签：records={records:?}"
    );
    assert!(
        has_visible_text,
        "恢复地图文字必须重新启用 point 标签：records={records:?}"
    );

    let captured_guard = captured.lock().expect("spy capture lock");
    let drawing_failure = captured_guard
        .iter()
        .find(|message| message.contains("review_map_draw_failed:overlay_construct"))
        .cloned();
    assert!(
        drawing_failure.is_some(),
        "单候选 overlay 构造失败必须经 JSON IPC 回传安全阶段码；captured={:?}",
        captured_guard.as_slice()
    );
    let drawing_failure = drawing_failure.expect("checked above");
    assert!(
        !drawing_failure.contains(CANDIDATE_ID)
            && !drawing_failure.contains("coordinates")
            && !drawing_failure.contains("private"),
        "绘制失败 IPC 不得泄露候选 ID、坐标或原始异常详情：{drawing_failure}"
    );
    assert!(
        captured_guard
            .iter()
            .any(|message| message.contains("review_map_locate_hidden")),
        "定位剔除候选必须给出安全、明确反馈，不得补绘或静默 return：captured={:?}",
        captured_guard.as_slice()
    );

    REVIEW_SPY_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
}
