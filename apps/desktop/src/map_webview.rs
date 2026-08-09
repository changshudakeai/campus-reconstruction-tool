//! 屏幕 4 边界地图 WebView 嵌入与生命周期管理
//!
//! 薄壳原则（ADR-0017）：本模块只做嵌入显示与消息桥接，零业务计算——
//! OSM 候选排序在 B3 `gaode_client::BoundarySorter`，校验在 B5
//! `validate_polygon_closure`，HTML 生成在 B3 `gaode_client`。
//!
//! ## 嵌入链路（T21 已验证，禁止推翻）
//! `slint::spawn_local` → `winit_window().await` → `build_as_child`。
//!
//! ## 显隐策略
//! 屏幕 4 显示、其余屏隐藏。wry 0.55 时无 `set_visible` API，采用
//! **drop/recreate** 兜底并诚实声明（验收条目"随屏显隐"以此实现）。
//! 屏幕切换处由注入器调用 [`show`] / [`hide`]。
//!
//! ## resize 跟随（T34：Slint 布局槽位上报真实矩形）
//! slint 1.17 无公开窗口 resize 回调，采用 300ms 轮询兜底：
//! 轮询 `AppWindow::workspace-map-slot-*`（逻辑像素矩形，由 Slint 布局
//! 计算，含左侧抽屉开合让位）与缩放因子，变化时经 `WebView::set_bounds`
//! 重定位；HTML 内 `window resize` 监听再调 `map.resize()` 同步画布。
//! 不再硬编码 (32,184,w-32,340)。
//!
//! ## T34：弹窗遮挡统一机制
//! 地图 WebView 是原生子窗口，会渲染在 Slint 模态遮罩之上；错误/确认/
//! 输入弹窗前统一 [`hide`]，关闭后经 [`restore_after_modal`] 按当前步骤
//! 模式（边界页 vs 朝向页）重建，不得恢复错页。
//!
//! ## T35：销毁延迟到事件循环下一拍（P0 崩溃修复）
//! wry 的 IPC 回调（`window.ipc.postMessage` → `with_ipc_handler`）运行在
//! WebView2 自身 COM 回调栈内。错误/确认/输入弹窗路径都会在回调栈内调用
//! [`hide`]；若同步 drop WebView，WebView2 COM 重入会在 combase.dll 崩溃
//! （WER 0xc0000005 / 0xc0000409）。因此 [`hide`] 是**唯一延迟销毁入口**：
//! 活跃 WebView 立即移入 `retiring`（逻辑隐藏即刻生效），经
//! `slint::invoke_from_event_loop` 排定下一拍回调，IPC 回调返回后才真正
//! drop；全部弹窗遮挡调用点（presenter / presentation / 工作区适配器）
//! 统一走本入口。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::{winit_030::WinitWindowAccessor, Weak};

/// IPC 消息处理签名：输入原始消息文本，输出是否已识别处理。
/// 由功能入口注册；返回值仅用于诊断记录。
pub(crate) type IpcHandler = Rc<dyn Fn(&str)>;

/// 地图加载完成状态处理器：true = WebView 创建成功，false = 创建失败。
/// 由功能入口注册；故障只暂停地图相关操作，不阻塞其他页面（ADR-0037）。
pub(crate) type MapStatusHandler = Rc<dyn Fn(bool)>;

/// 地图页种类：决定弹窗关闭后按哪个页面重建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapPageKind {
    /// 边界编辑页（步骤 ①/③/⑤ 的常驻地图）
    Boundary,
    /// 朝向编辑页（步骤 ②）
    Orientation,
    /// 校区在线搜索页（D-3）
    CampusSearch,
}

/// Slint 布局槽位上报的地图矩形（逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct MapSlotRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct WebViewState {
    /// 活跃 WebView（None = 已隐藏/未创建）
    webview: Option<wry::WebView>,
    /// T35：已排定"下一拍"销毁的 WebView（IPC 回调栈内不得同步 drop）。
    /// [`hide`] 把活跃 WebView 移入此处，事件循环回调返回后统一销毁。
    retiring: Vec<wry::WebView>,
    /// T35：下一拍销毁是否已排定（同一事件循环轮次内多次 hide 只排一次）
    hide_scheduled: bool,
    /// 功能入口注册的 IPC 处理器
    ipc_handler: Option<IpcHandler>,
    /// 功能入口注册的地图加载状态处理器
    status_handler: Option<MapStatusHandler>,
    /// 上次使用的密钥（recreate 时复用）
    last_api_key: String,
    last_security_key: String,
    /// 上次使用的校区锚点（recreate 时复用；模态弹窗隐藏后恢复用）
    last_anchor: Option<(f64, f64)>,
    /// 上次显示的地图页种类（弹窗恢复按此/当前步骤重建）
    last_page_kind: Option<MapPageKind>,
    /// 朝向页配置（含已确认边界半透明参照；弹窗恢复时复用）
    last_orientation_config: Option<gaode_client::BoundaryEditPageConfig>,
    /// 当前 WebView 是否处于"校区搜索页"模式（D-3：区别于边界地图页）
    campus_search_mode: bool,
    /// resize 跟随轮询定时器（hide 时 drop 即停）
    resize_timer: Option<slint::Timer>,
    /// 上次同步的（地图槽位矩形, 缩放因子）——变化才 set_bounds
    last_slot_scale: Option<(MapSlotRect, f32)>,
}

thread_local! {
    /// 全局唯一地图 WebView（单窗口单实例；Slint 单线程 UI 模型）
    static STATE: RefCell<WebViewState> = const {
        RefCell::new(WebViewState {
            webview: None,
            retiring: Vec::new(),
            hide_scheduled: false,
            ipc_handler: None,
            status_handler: None,
            last_api_key: String::new(),
            last_security_key: String::new(),
            last_anchor: None,
            last_page_kind: None,
            last_orientation_config: None,
            campus_search_mode: false,
            resize_timer: None,
            last_slot_scale: None,
        })
    };
}

/// 注册 IPC 处理器（功能入口在 bind 时调用一次）
pub(crate) fn register_ipc_handler(handler: IpcHandler) {
    STATE.with(|s| s.borrow_mut().ipc_handler = Some(handler));
}

/// 注册地图加载完成状态处理器（功能入口在 bind 时调用一次）
pub(crate) fn register_status_handler(handler: MapStatusHandler) {
    STATE.with(|s| s.borrow_mut().status_handler = Some(handler));
}

/// 通知地图加载完成状态（WebView 创建成功或失败后调用一次）
fn notify_status(available: bool) {
    // 与 IPC 回调同纪律：先克隆释放借用，再调用外部 handler（T35）。
    let handler = STATE.with(|s| s.borrow().status_handler.clone());
    if let Some(handler) = handler {
        handler(available);
    }
}

/// 把 WebView2 IPC 原始载荷转交注册的业务 handler。
///
/// T35 根因（RefCell already borrowed → WebView2 回调内不可 unwind 崩溃）：
/// 不得在 `STATE` 借用存活期间调用 handler——`if let Some(handler) =
/// s.borrow()...` 的临时 `Ref` 会活到 if-let 体结束，而错误/确认/输入弹窗
/// 路径会在 handler 内调用 [`hide`]（`borrow_mut`）→ `RefCell` panic
/// （WER 0xc0000409）。这里先克隆出 handler、释放借用，再在回调栈内执行业务。
fn dispatch_ipc(body: &str) {
    let handler = STATE.with(|s| s.borrow().ipc_handler.clone());
    if let Some(handler) = handler {
        handler(body);
    }
}

/// 是否已有活跃 WebView
pub(crate) fn is_visible() -> bool {
    STATE.with(|s| s.borrow().webview.is_some())
}

/// 校区搜索 WebView 是否已就绪（存在且处于校区搜索页模式）
pub(crate) fn campus_search_ready() -> bool {
    STATE.with(|s| {
        let state = s.borrow();
        state.webview.is_some() && state.campus_search_mode
    })
}

/// 当前地图是否为边界编辑页（T34：步骤切换时避免把朝向页留在边界步骤上）。
pub(crate) fn is_boundary_page() -> bool {
    page_kind() == Some(MapPageKind::Boundary)
}

/// 从 Slint 布局槽位读取地图矩形（逻辑像素；T34 不再硬编码坐标）。
///
/// 槽位由 `main.slint` 计算：`workspace-map-slot-x/y/width/height`，
/// 含左侧抽屉开合让位（做法 A）。宽度/高度钳制为非负。
fn map_slot(window: &crate::AppWindow) -> MapSlotRect {
    MapSlotRect {
        x: window.get_workspace_map_slot_x(),
        y: window.get_workspace_map_slot_y(),
        width: window.get_workspace_map_slot_width().max(0.0),
        height: window.get_workspace_map_slot_height().max(0.0),
    }
}

/// 逻辑像素矩形 × scale factor = WebView 物理尺寸。
///
/// T32/T34：调用方保证输入为逻辑像素（`Window::size()` 返回物理像素，
/// 已除 scale 还原）；本函数逻辑 × scale = 物理，避免把物理宽当逻辑宽
/// 二次缩放导致 WebView 超出窗口右缘（T31-D6）。
fn compute_slot_bounds(slot: MapSlotRect, scale: f32) -> wry::Rect {
    let scale = f64::from(scale);
    wry::Rect {
        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
            (f64::from(slot.x) * scale).round() as i32,
            (f64::from(slot.y) * scale).round() as i32,
        )),
        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
            (f64::from(slot.width) * scale).round() as u32,
            (f64::from(slot.height) * scale).round() as u32,
        )),
    }
}

/// T32：slint `Window::size()` 返回物理像素（i-slint-core 1.17
/// `Window::size() -> PhysicalSize`），WebView 布局需要逻辑宽。
/// 物理宽 ÷ scale = 逻辑宽；scale 缺失/异常时按 1.0 兜底。
fn logical_window_width(window: &slint::Window) -> u32 {
    let scale = window.scale_factor().max(0.001);
    let physical = f64::from(window.size().width);
    ((physical / f64::from(scale)).round() as u32).max(1)
}

/// 校区选择页搜索地图区域：x:16 y:110, width:parent.width-32, height:300。
/// 与边界地图页同规则：调用方保证入参为逻辑窗口宽（T32 同一修复）。
fn compute_campus_search_bounds(window_width_logical: u32, scale: f32) -> wry::Rect {
    let scale = f64::from(scale);
    wry::Rect {
        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
            (16.0 * scale) as i32,
            (110.0 * scale) as i32,
        )),
        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
            ((f64::from(window_width_logical) - 32.0).max(300.0) * scale) as u32,
            (300.0 * scale) as u32,
        )),
    }
}

/// 启动 resize 跟随轮询（slint 1.17 无公开 resize 回调 → 定时器兜底）。
///
/// T34：按当前地图页种类取对应矩形——工作区页读 Slint 槽位（含抽屉让位），
/// 校区搜索页用固定搜索条矩形。
fn start_resize_timer(window_weak: Weak<crate::AppWindow>) {
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(300),
        move || {
            let Some(app_window) = window_weak.upgrade() else {
                return;
            };
            let scale = app_window.window().scale_factor();
            let kind = page_kind();
            let next = match kind {
                Some(MapPageKind::CampusSearch) => {
                    let width = logical_window_width(app_window.window());
                    MapSlotRect {
                        x: 16.0,
                        y: 110.0,
                        width: (f64::from(width) - 32.0).max(300.0) as f32,
                        height: 300.0,
                    }
                }
                _ => map_slot(&app_window),
            };
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                if state.last_slot_scale == Some((next, scale)) {
                    return;
                }
                state.last_slot_scale = Some((next, scale));
                if let Some(webview) = state.webview.as_ref() {
                    let _ = webview.set_bounds(compute_slot_bounds(next, scale));
                }
            });
        },
    );
    STATE.with(|s| s.borrow_mut().resize_timer = Some(timer));
}

/// 显示（或重建）边界地图 WebView。
///
/// 幂等：已存在时不重复创建。密钥/锚点被记录供 recreate 复用。
/// 创建失败静默降级（地图不可用 → 人工圈画画布仍在 Slint 侧可用）。
pub(crate) fn show(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
    anchor_lon: f64,
    anchor_lat: f64,
) {
    // 幂等：已显示时不重复创建
    if is_visible() {
        return;
    }
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.last_api_key = api_key.clone();
        state.last_security_key = security_key.clone();
        state.last_anchor = Some((anchor_lon, anchor_lat));
        state.last_page_kind = Some(MapPageKind::Boundary);
        state.campus_search_mode = false;
    });

    let weak_for_timer = window_weak.clone();
    let _ = slint::spawn_local(async move {
        let Some(app_window) = window_weak.upgrade() else {
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            return;
        };

        // HTML 由 B3 生成（密钥注入校验在 B3 内）
        let config = gaode_client::BoundaryEditPageConfig::new(&api_key, &security_key)
            .with_anchor(anchor_lon, anchor_lat);
        let Ok(html) = gaode_client::build_boundary_edit_page_html(&config) else {
            return;
        };

        let scale = app_window.window().scale_factor();
        let bounds = compute_slot_bounds(map_slot(&app_window), scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                log::debug!(
                    "map_webview: WebView2 IPC 回调进入（body={} 字节）",
                    body.len()
                );
                dispatch_ipc(&body);
                log::debug!("map_webview: WebView2 IPC 回调返回");
            })
            .build_as_child(&*winit_win);

        let map_created = result.is_ok();
        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_slot_scale = Some((map_slot(&app_window), scale));
            });
            // resize 跟随（先静态 bounds 验证位置正确，再轮询跟随）
            start_resize_timer(weak_for_timer);
        }
        // 创建成功或失败都如实回报：故障只暂停地图相关操作
        notify_status(map_created);
    });
}

/// 显示（或重建）校区搜索地图 WebView（D-3）。
///
/// 加载 B3 `build_map_page_html`（高德 PlaceSearch 在线搜索页）；结果经
/// `window.ipc` 回传，由响应通道转交 B3 解析。幂等：已存在时不重复创建。
/// 不触发工作区地图状态回调（该回调属于边界地图页语义）。
pub(crate) fn show_campus_search(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
) {
    if is_visible() {
        return;
    }
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.last_api_key = api_key.clone();
        state.last_security_key = security_key.clone();
        state.last_page_kind = Some(MapPageKind::CampusSearch);
        state.campus_search_mode = true;
    });

    let weak_for_timer = window_weak.clone();
    let _ = slint::spawn_local(async move {
        let Some(app_window) = window_weak.upgrade() else {
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            return;
        };

        // HTML 由 B3 生成（密钥注入校验在 B3 内）
        let config = gaode_client::MapPageConfig::new(&api_key, &security_key);
        let Ok(html) = gaode_client::build_map_page_html(&config) else {
            return;
        };

        let scale = app_window.window().scale_factor();
        let width = logical_window_width(app_window.window());
        let bounds = compute_campus_search_bounds(width, scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                log::debug!(
                    "map_webview: WebView2 IPC 回调进入（body={} 字节）",
                    body.len()
                );
                dispatch_ipc(&body);
                log::debug!("map_webview: WebView2 IPC 回调返回");
            })
            .build_as_child(&*winit_win);

        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_slot_scale = Some((
                    MapSlotRect {
                        x: 16.0,
                        y: 110.0,
                        width: (f64::from(width) - 32.0).max(300.0) as f32,
                        height: 300.0,
                    },
                    scale,
                ));
            });
            start_resize_timer(weak_for_timer);
        }
    });
}

/// T25: 显示（或重建）地图 WebView，使用完整配置（朝向模式/已确认边界）。
///
/// 与 [`show`] 不同：本函数允许调用方传入完整 `BoundaryEditPageConfig`，
/// 用于步骤②朝向模式（显示半透明边界参照）。非幂等：朝向模式需要重建
/// WebView 以切换 HTML 初始化参数。
pub(crate) fn show_with_config(
    window_weak: Weak<crate::AppWindow>,
    config: gaode_client::BoundaryEditPageConfig,
) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.last_api_key = config.api_key.clone();
        state.last_security_key = config.security_key.clone();
        state.last_page_kind = Some(MapPageKind::Orientation);
        state.last_orientation_config = Some(config.clone());
    });

    let weak_for_timer = window_weak.clone();
    let _ = slint::spawn_local(async move {
        let Some(app_window) = window_weak.upgrade() else {
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            return;
        };

        // HTML 由 B3 生成（密钥注入校验在 B3 内）
        let Ok(html) = gaode_client::build_boundary_edit_page_html(&config) else {
            return;
        };

        let scale = app_window.window().scale_factor();
        let bounds = compute_slot_bounds(map_slot(&app_window), scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                log::debug!(
                    "map_webview: WebView2 IPC 回调进入（body={} 字节）",
                    body.len()
                );
                dispatch_ipc(&body);
                log::debug!("map_webview: WebView2 IPC 回调返回");
            })
            .build_as_child(&*winit_win);

        let map_created = result.is_ok();
        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_slot_scale = Some((map_slot(&app_window), scale));
            });
            start_resize_timer(weak_for_timer);
        }
        // 创建成功或失败都如实回报：故障只暂停地图相关操作
        notify_status(map_created);
    });
}

/// 隐藏地图 WebView（离开屏幕 4 或模态弹窗前调用）——**唯一延迟销毁入口**。
///
/// 诚实声明：wry 0.55 无 `set_visible` API，这里 drop WebView；返回对应
/// 页面时由 [`show`] / [`show_with_config`] / [`show_campus_search`] 重建。
/// 重建期间地图状态（缩放/编辑内容）不保留——已确认边界已由 Rust 侧保管，
/// 不影响数据正确性。页面种类与配置被保留，供 [`restore_after_modal`] 按
/// 模式重建。
///
/// T35 崩溃根因：错误/确认/输入弹窗的 `hide` 可能发生在 wry IPC 回调栈内
/// （WebView2 COM 回调），同步 drop WebView 导致 COM 重入崩溃。本入口一律
/// 把销毁排到事件循环下一拍：`is_visible()` 立即为 false（逻辑隐藏），
/// 但实际 drop 由 `invoke_from_event_loop` 回调在 IPC 回调返回后执行。
pub(crate) fn hide() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        // 活跃 WebView 立即移入待销毁队列：逻辑隐藏立即生效，drop 延后。
        if let Some(webview) = state.webview.take() {
            state.retiring.push(webview);
        }
        state.resize_timer = None; // drop 即停止轮询
        state.last_slot_scale = None;
        state.campus_search_mode = false;
        if state.hide_scheduled || state.retiring.is_empty() {
            return;
        }
        state.hide_scheduled = true;
        log::debug!(
            "map_webview: hide() 已排定下一拍销毁 {} 个 WebView（IPC 回调返回后 drop）",
            state.retiring.len()
        );
        let dispatched = slint::invoke_from_event_loop(|| {
            let retired = STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.hide_scheduled = false;
                std::mem::take(&mut state.retiring)
            });
            log::debug!(
                "map_webview: 事件循环下一拍，销毁 {} 个待退休 WebView（IPC 回调已返回）",
                retired.len()
            );
            drop(retired);
        });
        if dispatched.is_err() {
            // 事件循环不可用（无界面/单元测试环境）：无法延迟，立即销毁。
            state.hide_scheduled = false;
            state.retiring.clear();
        }
    });
}

/// 当前记录的地图页种类（测试与弹窗恢复决策用）。
fn page_kind() -> Option<MapPageKind> {
    STATE.with(|s| s.borrow().last_page_kind)
}

/// 模态弹窗关闭后是否需要恢复地图（T34 统一机制）。
///
/// - 工作区（screen 4）：步骤 ①②③⑤ 显示地图；步骤 ④ 评审页不显示。
/// - 校区搜索页（screen 1）：取消弹窗后停留在该页时需要恢复搜索地图。
fn should_restore_after_modal(active_screen: i32, active_step: i32) -> bool {
    if active_screen == 4 {
        return active_step != 3;
    }
    active_screen == 1 && page_kind() == Some(MapPageKind::CampusSearch)
}

/// 恢复被模态弹窗隐藏的地图 WebView（仅当应当恢复且锚点/配置已知时）。
///
/// T34：关闭错误/确认/输入弹窗后按**当前步骤模式**重建——
/// 步骤②重建朝向页（复用上次朝向配置，含半透明边界参照），
/// 步骤①③⑤重建边界页；不得恢复错页。
pub(crate) fn restore_after_modal(window: Weak<crate::AppWindow>) {
    let Some(app_window) = window.upgrade() else {
        return;
    };
    let active_screen = app_window.get_active_screen();
    let active_step = app_window.get_workspace_active_step();
    if !should_restore_after_modal(active_screen, active_step) {
        return;
    }
    let (api_key, security_key, anchor) = STATE.with(|s| {
        let state = s.borrow();
        (
            state.last_api_key.clone(),
            state.last_security_key.clone(),
            state.last_anchor,
        )
    });
    if active_screen == 4 {
        if active_step == 1 {
            // 朝向页：必须用上次朝向配置重建，不得回落到边界页
            let config = STATE.with(|s| s.borrow().last_orientation_config.clone());
            if let Some(config) = config {
                show_with_config(window, config);
            }
            return;
        }
        if let Some((anchor_lon, anchor_lat)) = anchor {
            show(window, api_key, security_key, anchor_lon, anchor_lat);
        }
        return;
    }
    if active_screen == 1 && page_kind() == Some(MapPageKind::CampusSearch) {
        show_campus_search(window, api_key, security_key);
    }
}

/// 向 JS 发送命令（Rust→JS 单向通道，如 drawBoundaryGcj / enableManualMode /
/// 抽屉桥接命令 undoManualPointFromDrawer 等）。
pub(crate) fn evaluate_script(script: &str) {
    STATE.with(|s| {
        if let Some(webview) = s.borrow().webview.as_ref() {
            let _ = webview.evaluate_script(script);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initially_hidden() {
        assert!(!is_visible());
    }

    #[test]
    fn hide_is_idempotent_and_keeps_page_kind_for_modal_restore() {
        hide();
        hide();
        assert!(!is_visible());
        // T35：无活跃 WebView 时不得排定下一拍销毁（单元测试无事件循环，
        // 也不允许残留排定标记）。
        STATE.with(|s| {
            let state = s.borrow();
            assert!(!state.hide_scheduled, "无待销毁对象不得排定");
            assert!(state.retiring.is_empty(), "无待销毁对象");
        });
    }

    #[test]
    fn hide_without_webview_keeps_state_clean_in_headless_environment() {
        // T35：单元测试进程没有运行中的 Slint 事件循环，hide() 必须在
        // 无 WebView 时直接返回（不调用 invoke_from_event_loop、不残留
        // 排定标记），否则无界面环境下会误排定一次永不执行的销毁回调。
        hide();
        STATE.with(|s| {
            let state = s.borrow();
            assert!(!state.hide_scheduled);
            assert!(state.retiring.is_empty());
            assert!(!state.campus_search_mode);
            assert_eq!(state.last_slot_scale, None);
        });
        assert!(!is_visible());
    }

    #[test]
    fn ipc_dispatch_releases_borrow_before_invoking_handler() {
        // T35 根因回归（RefCell already borrowed → WebView2 回调内不可 unwind
        // 崩溃，WER 0xc0000409）：错误弹窗路径会在 IPC handler 内调用
        // hide()（对 STATE 做 borrow_mut）。若 dispatch 在 STATE 借用存活期间
        // 调用 handler（旧写法 `if let Some(handler) = s.borrow()...` 的临时
        // Ref 活到 if-let 体结束），这里会直接 panic。
        register_ipc_handler(Rc::new(|_body| {
            hide();
        }));
        dispatch_ipc(r#"{"type":"confirm_boundary","coords":[]}"#);
        // 走到这里说明 handler 调用期间 STATE 无存活借用、无 panic。
        STATE.with(|s| {
            let state = s.borrow();
            assert!(!state.hide_scheduled, "handler 内 hide() 后不得残留排定");
            assert!(state.retiring.is_empty());
        });
        STATE.with(|s| s.borrow_mut().ipc_handler = None);
    }

    #[test]
    fn slot_bounds_follow_layout_slot() {
        // T34：地图矩形来自 Slint 槽位（含抽屉让位），不再硬编码 (32,184)。
        let slot = MapSlotRect {
            x: 20.0,
            y: 128.0,
            width: 800.0 - 20.0 - 16.0,
            height: 666.0 - 128.0 - 16.0,
        };
        let bounds = compute_slot_bounds(slot, 1.0);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!((pos.x, pos.y), (20, 128));
        assert_eq!((sz.width, sz.height), (764, 522));
    }

    #[test]
    fn slot_bounds_scale_to_physical_pixels() {
        // 2 倍缩放：逻辑像素 × 2 = 物理像素
        let slot = MapSlotRect {
            x: 20.0,
            y: 128.0,
            width: 764.0,
            height: 522.0,
        };
        let bounds = compute_slot_bounds(slot, 2.0);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!((pos.x, pos.y), (40, 256));
        assert_eq!((sz.width, sz.height), (1528, 1044));
    }

    #[test]
    fn slot_bounds_stay_inside_window_at_125_percent_scale() {
        // T32/T34：800 逻辑宽 × 1.25 = 1000 物理宽；地图右缘不得越界。
        let slot = MapSlotRect {
            x: 20.0,
            y: 128.0,
            width: 764.0,
            height: 522.0,
        };
        let bounds = compute_slot_bounds(slot, 1.25);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!(pos.x, 25);
        assert_eq!(sz.width, 955);
        assert!(
            pos.x + sz.width as i32 <= 1000,
            "WebView 右缘不得超出窗口物理宽 1000（实际 {}）",
            pos.x + sz.width as i32
        );
    }

    #[test]
    fn drawer_open_shrinks_map_slot() {
        // T34 做法 A：抽屉展开 → 地图右移让位，宽度相应收窄。
        let closed = MapSlotRect {
            x: 20.0,
            y: 128.0,
            width: 764.0,
            height: 522.0,
        };
        let open = MapSlotRect {
            x: 20.0 + 300.0 + 12.0,
            y: 128.0,
            width: 764.0 - 312.0,
            height: 522.0,
        };
        let closed_bounds = compute_slot_bounds(closed, 1.0);
        let open_bounds = compute_slot_bounds(open, 1.0);
        let wry::dpi::Position::Physical(closed_pos) = closed_bounds.position else {
            panic!("物理像素");
        };
        let wry::dpi::Position::Physical(open_pos) = open_bounds.position else {
            panic!("物理像素");
        };
        let wry::dpi::Size::Physical(open_sz) = open_bounds.size else {
            panic!("物理像素");
        };
        assert_eq!(
            open_pos.x - closed_pos.x,
            312,
            "抽屉展开地图右移 312 逻辑像素"
        );
        assert_eq!(open_sz.width, 764 - 312);
    }

    #[test]
    fn campus_search_bounds_follow_search_strip() {
        let bounds = compute_campus_search_bounds(800, 1.0);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!((pos.x, pos.y), (16, 110));
        assert_eq!((sz.width, sz.height), (800 - 32, 300));
    }

    #[test]
    fn modal_restore_guard_keeps_review_page_unchanged() {
        // T34：步骤④评审页不显示地图；弹窗关闭后不得恢复地图。
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.last_page_kind = Some(MapPageKind::Boundary);
        });
        assert!(!should_restore_after_modal(4, 3), "评审页不恢复地图");
        assert!(should_restore_after_modal(4, 0), "边界页恢复地图");
        assert!(should_restore_after_modal(4, 1), "朝向页恢复地图");
        assert!(should_restore_after_modal(4, 2), "采集页恢复地图");
        assert!(should_restore_after_modal(4, 4), "导出页恢复地图");
    }

    #[test]
    fn modal_restore_guard_handles_campus_search() {
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.last_page_kind = Some(MapPageKind::CampusSearch);
        });
        assert!(
            should_restore_after_modal(1, 0),
            "校区搜索页取消弹窗后停留在该页，需恢复搜索地图"
        );
        assert!(
            !should_restore_after_modal(2, 0),
            "确认校区后进入方案列表，不再恢复搜索地图"
        );
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.last_page_kind = Some(MapPageKind::Boundary);
        });
        assert!(
            !should_restore_after_modal(1, 0),
            "非校区搜索页种类不恢复搜索地图"
        );
    }

    #[test]
    fn hide_keeps_kind_and_config_for_modal_restore() {
        // 模拟朝向页：记录配置后 hide，再验证种类与配置保留。
        let config = gaode_client::BoundaryEditPageConfig::new("abc123", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_orientation_mode(true);
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.last_api_key = config.api_key.clone();
            state.last_security_key = config.security_key.clone();
            state.last_anchor = Some((116.4, 39.9));
            state.last_page_kind = Some(MapPageKind::Orientation);
            state.last_orientation_config = Some(config.clone());
            state.webview = None;
        });
        hide();
        assert_eq!(page_kind(), Some(MapPageKind::Orientation));
        STATE.with(|s| {
            let state = s.borrow();
            assert_eq!(state.last_orientation_config.as_ref(), Some(&config));
            assert_eq!(state.last_anchor, Some((116.4, 39.9)));
        });
    }

    #[test]
    fn campus_search_ready_follows_mode_and_visibility() {
        hide();
        assert!(!campus_search_ready(), "无 WebView 时不算就绪");
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.campus_search_mode = true;
            state.webview = None;
        });
        assert!(!campus_search_ready(), "有模式标记但无 WebView 仍不算就绪");
        hide();
        assert!(!campus_search_ready());
    }
}
