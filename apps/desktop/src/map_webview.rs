//! 地图会话的 Windows WebView2 适配器（ADR-0017/0045）。
//!
//! 本模块只负责原生子窗口、延迟销毁、串行创建与代际过滤；显示意图、
//! 方案身份、弹窗遮挡、视野和结构化命令均由 `map_session` 持有。
//! 嵌入链路保持为 `slint::spawn_local → winit_window → build_as_child`。
//!
//! WebView2 IPC 回调栈内同步 drop 会触发 COM 重入崩溃，因此 `hide` 先把
//! 活跃实例移到 `retiring`，下一事件循环拍再释放；新实例只在退休队列清空
//! 后创建。每次请求递增 generation，创建结果、状态与 IPC 都必须同时匹配
//! 当前 generation 和页面种类，退休页面的迟到结果一律丢弃。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;
use slint::{winit_030::WinitWindowAccessor, Weak};

/// 地图加载超时（T36）：WebView 创建/SDK 就绪的硬性上限。
pub(crate) const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// T36：Rust 侧超时经错误 IPC 上报时使用的标记；工作区入口把它本地化为
/// 明确的超时文案（ADR-0005 禁止硬编码可见文本）。
pub(crate) const MAP_LOAD_TIMEOUT_MARKER: &str = "__t36_map_load_timeout__";

/// IPC 消息处理签名：输入原始消息文本，输出是否已识别处理。
/// 由功能入口注册；返回值仅用于诊断记录。
pub(crate) type IpcHandler = Rc<dyn Fn(MapPageKind, &str)>;

/// 地图加载完成状态处理器：true = WebView 创建成功，false = 创建失败。
/// 由功能入口注册；故障只暂停地图相关操作，不阻塞其他页面（ADR-0037）。
pub(crate) type MapStatusHandler = Rc<dyn Fn(MapPageKind, bool)>;

/// 地图页面种类：决定弹窗关闭后按哪个页面重建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapPageKind {
    /// 边界编辑页（步骤 ①②③ 的常驻地图）
    Boundary,
    /// 朝向编辑页（步骤 ②）
    Orientation,
    /// 评审地图页（步骤 ④：候选三态标注 + 定位/联动高亮，T38）
    Review,
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

/// T36：一次待执行的 show 请求（retiring 未清空或在途创建时暂存，后到覆盖先到）。
#[derive(Clone)]
enum PendingShow {
    Boundary {
        window: Weak<crate::AppWindow>,
        api_key: String,
        security_key: String,
        anchor_lon: f64,
        anchor_lat: f64,
        initial_viewport: Option<gaode_client::MapViewport>,
    },
    CampusSearch {
        window: Weak<crate::AppWindow>,
        api_key: String,
        security_key: String,
    },
    Orientation {
        window: Weak<crate::AppWindow>,
        config: gaode_client::BoundaryEditPageConfig,
    },
    Review {
        window: Weak<crate::AppWindow>,
        api_key: String,
        security_key: String,
        anchor_lon: f64,
        anchor_lat: f64,
        map_text_label: String,
        map_text_visible: bool,
        initial_viewport: Option<gaode_client::MapViewport>,
    },
}

impl PendingShow {
    fn page_kind(&self) -> MapPageKind {
        match self {
            PendingShow::Boundary { .. } => MapPageKind::Boundary,
            PendingShow::CampusSearch { .. } => MapPageKind::CampusSearch,
            PendingShow::Orientation { .. } => MapPageKind::Orientation,
            PendingShow::Review { .. } => MapPageKind::Review,
        }
    }

    /// 工作区页（边界/朝向）需要回传 map_available 状态；校区搜索页不回传。
    fn reports_status(&self) -> bool {
        !matches!(self, PendingShow::CampusSearch { .. })
    }

    /// T39：评审地图页不挂 Rust 侧 10s 加载超时——候选不再内嵌 HTML，
    /// 地图创建只承担 SDK/瓦片就绪；千级候选在 `map_ready` 后分批推送并
    /// 由 JS 定时上屏（发生在创建超时之外），继续保留 10s 兜底会把慢速
    /// 注入/慢网络的评审地图误杀。页面自身 5s SDK 超时与 onerror 仍上报。
    fn has_load_timeout(&self) -> bool {
        !matches!(
            self,
            PendingShow::Review { .. } | PendingShow::CampusSearch { .. }
        )
    }
}

struct WebViewState {
    /// 活跃 WebView（None = 已隐藏/未创建）
    webview: Option<wry::WebView>,
    /// T35：已排定"下一拍"销毁的 WebView（IPC 回调栈内不得同步 drop）。
    /// [`hide`] 把活跃 WebView 移入此处，事件循环回调返回后统一销毁。
    retiring: Vec<wry::WebView>,
    /// T35：下一拍销毁是否已排定（同一事件循环轮次内多次 hide 只排一次）
    hide_scheduled: bool,
    /// T36：show 请求代际；在途创建完成时只有"当前代"可生效。
    generation: u64,
    /// T36：是否已有在途的异步 WebView 创建（防止重复 spawn）。
    creation_in_flight: bool,
    /// T36：等待执行的 show 请求（retiring 未清空或在途创建时暂存）。
    pending_show: Option<PendingShow>,
    /// T36：工作区页创建加载超时（一次性；成功后或 hide 时取消）。
    load_timer: Option<slint::Timer>,
    /// 功能入口注册的 IPC 处理器
    ipc_handler: Option<IpcHandler>,
    /// 功能入口注册的地图加载状态处理器
    status_handler: Option<MapStatusHandler>,
    /// 上次显示的地图页面种类（弹窗恢复按此步骤重建）
    last_page_kind: Option<MapPageKind>,
    /// 当前 WebView 是否处于"校区搜索页"模式（D-3：区别于边界地图页）
    campus_search_mode: bool,
    /// 评审会话内“显示地图文字”开关状态（默认 false：隐藏地标/POI 文字）。
    review_map_text_visible: bool,
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
            generation: 0,
            creation_in_flight: false,
            pending_show: None,
            load_timer: None,
            ipc_handler: None,
            status_handler: None,
            last_page_kind: None,
            campus_search_mode: false,
            review_map_text_visible: false,
            resize_timer: None,
            last_slot_scale: None,
        })
    };
}

// T39：评审地图 Rust→JS 回推命令计数（契约测试注入观察；生产为廉价
// 线程局部计数，不做业务分支）。一次高亮/三态操作只应产生 1 条回推，
// 不再是 clear + 21 批全量。
thread_local! {
    static REVIEW_PUSH_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 评审地图回推命令累计条数（诊断/验收用）。
#[doc(hidden)]
pub(crate) fn review_push_count() -> usize {
    REVIEW_PUSH_COUNT.with(|s| s.get())
}

/// 清零评审地图回推计数（验收测试起点）。
#[doc(hidden)]
pub(crate) fn reset_review_push_count() {
    REVIEW_PUSH_COUNT.with(|s| s.set(0));
    REVIEW_PUSHED_SCRIPTS.with(|s| s.borrow_mut().clear());
}

/// 无 WebView 测试环境下模拟"评审地图可见"，使回推路径可被计数观察；
/// 生产不设置（恒为实际可见性）。
#[doc(hidden)]
pub(crate) fn set_review_push_probe_visible(visible: bool) {
    REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.set(visible));
}

thread_local! {
    static REVIEW_PUSH_PROBE_VISIBLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static REVIEW_PUSHED_SCRIPTS: std::cell::RefCell<Vec<String>> = const {
        std::cell::RefCell::new(Vec::new())
    };
    /// 地图可见性契约探针（本工单）：无真实 WebView 的测试环境用它模拟
    /// “地图已显示”，[`hide`] 同步清除，使“离开工作区必须隐藏地图”可断言。
    static MAP_VISIBLE_PROBE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 契约测试探针：无真实 WebView 的环境里模拟“地图已显示”。
///
/// [`hide`] 会同步清除该探针；真实 WebView 存在时以真实可见性为准。
#[doc(hidden)]
pub(crate) fn set_map_visible_probe(visible: bool) {
    MAP_VISIBLE_PROBE.with(|s| s.set(visible));
}

/// 契约测试观测：地图 WebView 是否可见（探针 + 真实状态）。
#[doc(hidden)]
pub(crate) fn map_visible() -> bool {
    is_visible() || MAP_VISIBLE_PROBE.with(|s| s.get())
}

/// 测试用 JS 地图 spy：返回本次探针开启后记录到的 Rust→JS 回推脚本。
#[doc(hidden)]
pub(crate) fn review_pushed_scripts() -> Vec<String> {
    REVIEW_PUSHED_SCRIPTS.with(|s| s.borrow().clone())
}

/// 记录一条评审地图回推命令（在任何 evaluate_script 前调用，无 WebView
/// 环境同样计数，供 T39 验收断言"一次高亮只产生 1 条回推"）。探针开启时
/// 同时记录脚本内容，供 JS 地图 spy 断言 center/zoom/overlay/highlight/文字开关。
pub(crate) fn note_review_push(script: &str) {
    REVIEW_PUSH_COUNT.with(|s| s.set(s.get() + 1));
    if REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.get()) {
        REVIEW_PUSHED_SCRIPTS.with(|s| s.borrow_mut().push(script.to_owned()));
    }
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
///
/// 与 IPC 回调同纪律：先克隆释放借用，再调用外部 handler（T35）。
fn notify_status(generation: u64, page_kind: MapPageKind, available: bool) {
    log::debug!(
        "map_webview: notify_status(page={page_kind:?}, generation={generation}, available={available})"
    );
    let handler = STATE.with(|s| {
        let state = s.borrow();
        (state.generation == generation && state.last_page_kind == Some(page_kind))
            .then(|| state.status_handler.clone())
            .flatten()
    });
    if let Some(handler) = handler {
        handler(page_kind, available);
    }
}

/// 把 WebView2 IPC 原始载荷转交注册的业务 handler。
///
/// T35 根因（RefCell already borrowed → WebView2 回调内不可 unwind 崩溃，
/// WER 0xc0000409）：不得在 `STATE` 借用存活期间调用 handler。这里先克隆出
/// handler、释放借用，再在回调栈内执行业务（handler 内可能调用 [`hide`]）。
fn dispatch_ipc(generation: u64, page_kind: MapPageKind, body: &str) {
    let handler = STATE.with(|s| {
        let state = s.borrow();
        (state.generation == generation && state.last_page_kind == Some(page_kind))
            .then(|| state.ipc_handler.clone())
            .flatten()
    });
    if let Some(handler) = handler {
        handler(page_kind, body);
    } else {
        log::debug!("map_webview: 丢弃退休页面事件（page={page_kind:?}, generation={generation}）");
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

/// 记录评审会话内“显示地图文字”开关状态（由评审地图 IPC 上报）。
///
/// 状态只保存在本进程的 WebView 会话状态中：评审页被隐藏/弹窗恢复重建时，
/// [`show_review`] 会按该状态重新生成地图页，从而在当前评审会话内保持。
pub(crate) fn set_review_map_text_visible(visible: bool) {
    STATE.with(|s| {
        s.borrow_mut().review_map_text_visible = visible;
    });
}

/// 读取评审会话内“显示地图文字”开关状态（验收测试观察用）。
#[doc(hidden)]
pub(crate) fn review_map_text_visible() -> bool {
    STATE.with(|s| s.borrow().review_map_text_visible)
}

/// 从 Slint 布局槽位读取地图矩形（逻辑像素；T34 不再硬编码坐标）。
///
/// 槽位由 `main.slint` 计算：`workspace-map-slot-x/y/width/height`，
/// 含左侧抽屉开合让位（做法 A）。宽高钳制为非负。
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
/// T34：按当前地图页面种类取对应矩形——工作区页读 Slint 槽位（含抽屉让位），
/// 校区搜索页用固定搜索条矩形。
fn start_resize_timer(window_weak: Weak<crate::AppWindow>) {
    let timer = slint::Timer::default();
    // T38：WebView 创建后前 1.5s 跳过 set_bounds——让过页面加载/就绪窗口，
    // 避免评审地图创建初期与页面自绘/首批 IPC 的 COM 通道竞争（防御性让位；
    // 边界页因 OSM 获取耗时天然错开，评审页需要显式让位）。
    let settle_deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(300),
        move || {
            let Some(app_window) = window_weak.upgrade() else {
                return;
            };
            if std::time::Instant::now() < settle_deadline {
                return;
            }
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

/// 为待执行请求构建 HTML（同步；T36：创建前失败如实上报，不静默）。
fn build_html_for(request: &PendingShow) -> Result<String, gaode_client::Error> {
    match request {
        PendingShow::Boundary {
            api_key,
            security_key,
            anchor_lon,
            anchor_lat,
            initial_viewport,
            ..
        } => {
            let mut config = gaode_client::BoundaryEditPageConfig::new(api_key, security_key)
                .with_anchor(*anchor_lon, *anchor_lat);
            if let Some(viewport) = initial_viewport {
                config = config.with_initial_viewport(*viewport);
            }
            gaode_client::build_boundary_edit_page_html(&config)
        }
        PendingShow::CampusSearch {
            api_key,
            security_key,
            ..
        } => {
            let config = gaode_client::MapPageConfig::new(api_key, security_key);
            gaode_client::build_map_page_html(&config)
        }
        PendingShow::Orientation { config, .. } => {
            gaode_client::build_boundary_edit_page_html(config)
        }
        PendingShow::Review {
            api_key,
            security_key,
            anchor_lon,
            anchor_lat,
            map_text_label,
            map_text_visible,
            initial_viewport,
            ..
        } => {
            let mut config = gaode_client::ReviewMapPageConfig::new(api_key, security_key)
                .with_anchor(*anchor_lon, *anchor_lat)
                .with_map_text_toggle(map_text_label, *map_text_visible);
            if let Some(viewport) = initial_viewport {
                config = config.with_initial_viewport(*viewport);
            }
            gaode_client::build_review_map_page_html(&config)
        }
    }
}

/// T35：把待销毁 WebView 排定到事件循环下一拍（唯一延迟销毁出口）。
///
/// 调用方必须已持有 `STATE` 借用（borrow_mut）并把 WebView 移入 `retiring`。
/// 销毁回调执行后调用 [`pump_creation`]，使等待中的 show 请求（T36 串行化）
/// 只在上一份真正 drop 后放行。
fn schedule_drop(state: &mut WebViewState) {
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
        // T36：销毁完成后才允许新建（串行化 hide→show）。
        pump_creation();
    });
    if dispatched.is_err() {
        // 事件循环不可用（无界面/单元测试环境）：无法延迟，立即销毁。
        log::debug!("map_webview: 事件循环不可用，立即销毁待退休 WebView");
        state.hide_scheduled = false;
        state.retiring.clear();
    }
}

/// T36：在当前状态允许时发起 pending 的 WebView 创建。
///
/// 放行条件：无活跃 WebView、无在途创建、`retiring` 已清空。HTML 构建
/// 同步完成——失败立即上报 `map_available=false`（工作区页），不进入异步
/// 创建；成功则带代际令牌异步 `build_as_child`。
fn pump_creation() {
    let action = STATE.with(|s| {
        let mut state = s.borrow_mut();
        if state.webview.is_some() || state.creation_in_flight || !state.retiring.is_empty() {
            return None;
        }
        let request = state.pending_show.take()?;
        let reports_status = request.reports_status();
        let page_kind = request.page_kind();
        match build_html_for(&request) {
            Err(error) => {
                if reports_status {
                    log::warn!(
                        "map_webview: HTML 构建失败（page={page_kind:?}），如实上报地图不可用: {error}"
                    );
                }
                Some(PumpAction::HtmlFailed {
                    generation: state.generation,
                    page_kind,
                    reports_status,
                })
            }
            Ok(html) => {
                state.creation_in_flight = true;
                let generation = state.generation;
                let window = request_window(&request);
                if reports_status && request.has_load_timeout() {
                    state.load_timer = Some(start_load_timer(generation));
                }
                Some(PumpAction::Spawn {
                    html,
                    generation,
                    window,
                    page_kind,
                    reports_status,
                })
            }
        }
    });
    match action {
        Some(PumpAction::HtmlFailed {
            generation,
            page_kind,
            reports_status,
        }) => {
            if reports_status {
                notify_status(generation, page_kind, false);
            }
        }
        Some(PumpAction::Spawn {
            html,
            generation,
            window,
            page_kind,
            reports_status,
        }) => {
            spawn_creation(html, generation, window, page_kind, reports_status);
        }
        None => {}
    }
}

enum PumpAction {
    HtmlFailed {
        generation: u64,
        page_kind: MapPageKind,
        reports_status: bool,
    },
    Spawn {
        html: String,
        generation: u64,
        window: Weak<crate::AppWindow>,
        page_kind: MapPageKind,
        reports_status: bool,
    },
}

fn request_window(request: &PendingShow) -> Weak<crate::AppWindow> {
    match request {
        PendingShow::Boundary { window, .. }
        | PendingShow::CampusSearch { window, .. }
        | PendingShow::Orientation { window, .. }
        | PendingShow::Review { window, .. } => window.clone(),
    }
}

/// T36：工作区页（边界/朝向）创建附带 10s 加载超时（一次性）。
/// T39：评审页/校区搜索页不挂此超时（见 [`PendingShow::has_load_timeout`]）。
fn start_load_timer(generation: u64) -> slint::Timer {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::SingleShot, LOAD_TIMEOUT, move || {
        handle_load_timeout(generation);
    });
    timer
}

/// T36：加载超时——使在途创建过期、取消 pending，经错误 IPC 弹明确错误。
///
/// 页面自身的 onerror / 5s SDK 超时已会回传 `{"type":"error",...}` IPC，
/// 本超时是 Rust 侧兜底（WebView 创建/SDK 就绪 10 秒未完成）。
fn handle_load_timeout(generation: u64) {
    let timed_out = STATE.with(|s| {
        let mut state = s.borrow_mut();
        if generation != state.generation || state.webview.is_some() {
            return None;
        }
        log::warn!("map_webview: 地图加载超时（{LOAD_TIMEOUT:?}），上报失败并禁止自动重建");
        state.generation += 1;
        state.creation_in_flight = false;
        state.pending_show = None;
        state.load_timer = None;
        Some((state.generation, state.last_page_kind))
    });
    if let Some((current_generation, Some(page_kind))) = timed_out {
        // 状态（map_available=false / 处理中清除）由工作区入口在错误 IPC
        // 分支统一处理并弹明确错误对话框；这里只发 IPC，避免重复通知。
        dispatch_ipc(
            current_generation,
            page_kind,
            &format!(r#"{{"type":"error","message":"{MAP_LOAD_TIMEOUT_MARKER}"}}"#),
        );
    }
    pump_creation();
}

/// 异步创建 WebView 并以其完成结果收口（代际校验）。
fn spawn_creation(
    html: String,
    generation: u64,
    window_weak: Weak<crate::AppWindow>,
    page_kind: MapPageKind,
    reports_status: bool,
) {
    log::debug!(
        "map_webview: 开始异步创建 WebView（page={page_kind:?}, generation={generation}, 上报状态={reports_status}）"
    );
    let _ = slint::spawn_local(async move {
        let Some(app_window) = window_weak.upgrade() else {
            log::debug!("map_webview: 窗口已失效，放弃创建（generation={generation}）");
            finish_creation(
                generation,
                Err(wry::Error::NotMainThread),
                window_weak,
                page_kind,
            );
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            log::debug!("map_webview: 拿不到 winit 窗口，放弃创建（generation={generation}）");
            finish_creation(
                generation,
                Err(wry::Error::NotMainThread),
                window_weak,
                page_kind,
            );
            return;
        };

        let scale = app_window.window().scale_factor();
        let bounds = match page_kind {
            MapPageKind::CampusSearch => {
                let width = logical_window_width(app_window.window());
                compute_campus_search_bounds(width, scale)
            }
            _ => compute_slot_bounds(map_slot(&app_window), scale),
        };

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            // WebView2 默认背景透明：HTML 首帧绘制前会透出桌面。设为不透明白，
            // 与 boundary/review/campus 各页 html/body 背景（默认白）及 Slint
            // Theme.surface（#ffffff）一致，避免改出突兀色块或白闪。
            .with_background_color((255, 255, 255, 255))
            .with_bounds(bounds)
            .with_focused(false)
            .with_ipc_handler(move |request: wry::http::Request<String>| {
                let body = request.body().to_string();
                log::debug!(
                    "map_webview: WebView2 IPC 回调进入（body={} 字节）",
                    body.len()
                );
                dispatch_ipc(generation, page_kind, &body);
                log::debug!("map_webview: WebView2 IPC 回调返回");
            })
            .build_as_child(&*winit_win);

        #[cfg(windows)]
        if result.is_ok() {
            crate::map_webview_focus::install(&*winit_win);
        }

        finish_creation(generation, result, window_weak, page_kind);
    });
}

/// T40：WebView 创建成功后的页面激活脚本（按页种类；朝向页在创建成功
/// 回调里立即激活两点选择，不依赖 map_ready 是否到达）。
fn creation_activation_script(page_kind: MapPageKind) -> Option<&'static str> {
    match page_kind {
        MapPageKind::Orientation => Some("activateOrientationWhenReady();"),
        MapPageKind::Boundary | MapPageKind::CampusSearch | MapPageKind::Review => None,
    }
}

/// 一次创建尝试的收口：当前代生效（设置活跃/上报），过期代转入延迟销毁。
fn finish_creation(
    generation: u64,
    result: std::result::Result<wry::WebView, wry::Error>,
    window_weak: Weak<crate::AppWindow>,
    page_kind: MapPageKind,
) {
    enum Outcome {
        Active,
        Failed,
        Stale,
    }
    let outcome = STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.creation_in_flight = false;
        state.load_timer = None;
        if generation != state.generation {
            log::debug!(
                "map_webview: 过期创建完成（gen={generation}，当前={}），转入延迟销毁",
                state.generation
            );
            if let Ok(webview) = result {
                state.retiring.push(webview);
                schedule_drop(&mut state);
            }
            return Outcome::Stale;
        }
        match result {
            Ok(webview) => {
                log::debug!("map_webview: WebView 创建成功（gen={generation}）");
                state.webview = Some(webview);
                Outcome::Active
            }
            Err(error) => {
                log::warn!("map_webview: WebView 创建失败（gen={generation}）: {error}");
                Outcome::Failed
            }
        }
    });
    match outcome {
        Outcome::Active => {
            start_resize_timer(window_weak);
            notify_status(generation, page_kind, true);
            // T40：朝向页创建成功即经可靠通道激活（不依赖 map_ready）；页面
            // 脚本在 SDK/地图未就绪时静默等待自动激活兜底。
            if let Some(script) = creation_activation_script(page_kind) {
                log::debug!("map_webview: 朝向页创建成功，执行激活脚本（创建成功通道）: {script}");
                evaluate_script(script);
            }
        }
        Outcome::Failed => notify_status(generation, page_kind, false),
        Outcome::Stale => {}
    }
    pump_creation();
}

/// 记录待执行请求并尝试放行（T36 统一 show 入口）。
fn request_show(request: PendingShow) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        // 显示意图的幂等与方案身份由 map_session 判定。适配器不能只凭页面
        // 种类短路，否则同类页面的新方案、锚点或配置会沿用旧现场。
        match &request {
            PendingShow::Boundary { .. } => {
                state.last_page_kind = Some(MapPageKind::Boundary);
                state.campus_search_mode = false;
            }
            PendingShow::CampusSearch { .. } => {
                state.last_page_kind = Some(MapPageKind::CampusSearch);
                state.campus_search_mode = true;
            }
            PendingShow::Orientation { .. } => {
                state.last_page_kind = Some(MapPageKind::Orientation);
                state.campus_search_mode = false;
            }
            PendingShow::Review {
                map_text_visible, ..
            } => {
                state.last_page_kind = Some(MapPageKind::Review);
                state.campus_search_mode = false;
                state.review_map_text_visible = *map_text_visible;
            }
        }
        // 异页已显示：先隐藏（延迟销毁），再排队重建。
        if state.webview.is_some() {
            if let Some(webview) = state.webview.take() {
                state.retiring.push(webview);
            }
            state.resize_timer = None;
            state.last_slot_scale = None;
            state.campus_search_mode = false;
            schedule_drop(&mut state);
        }
        state.generation += 1;
        state.pending_show = Some(request);
    });
    pump_creation();
}

pub(crate) fn show_boundary_with_viewport(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
    anchor_lon: f64,
    anchor_lat: f64,
    initial_viewport: Option<gaode_client::MapViewport>,
) {
    request_show(PendingShow::Boundary {
        window: window_weak,
        api_key,
        security_key,
        anchor_lon,
        anchor_lat,
        initial_viewport,
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
    request_show(PendingShow::CampusSearch {
        window: window_weak,
        api_key,
        security_key,
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
    request_show(PendingShow::Orientation {
        window: window_weak,
        config,
    });
}

pub(crate) fn show_review_with_viewport(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
    anchor_lon: f64,
    anchor_lat: f64,
    map_text_label: String,
    initial_viewport: Option<gaode_client::MapViewport>,
) {
    let map_text_visible = STATE.with(|s| s.borrow().review_map_text_visible);
    request_show(PendingShow::Review {
        window: window_weak,
        api_key,
        security_key,
        anchor_lon,
        anchor_lat,
        map_text_label,
        map_text_visible,
        initial_viewport,
    });
}

/// T38：进程退出前同步释放全部 WebView（供窗口关闭回调调用）。
///
/// 窗口关闭（run() 返回）后进程退出时，主线程 TLS 析构会 drop 仍持有的
/// `InnerWebView`，其 `ICoreWebView2Controller::Close()` 在 COM 拆除阶段
/// 触发 combase.dll 0xc0000005（CoUnmarshalInterface 通道对端已死）。这里在
/// 事件循环仍存活、COM 仍健康时先同步关闭控制器（含待退休 WebView），
/// 使退出时 TLS 析构为空操作。不得在事件循环停止后再调用本函数。
pub(crate) fn shutdown() {
    let (webview, retiring) = STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.resize_timer = None;
        state.load_timer = None;
        state.creation_in_flight = false;
        state.pending_show = None;
        let webview = state.webview.take();
        let retiring = std::mem::take(&mut state.retiring);
        (webview, retiring)
    });
    log::debug!(
        "map_webview: shutdown() 同步释放 {} 个活跃 + {} 个待退休 WebView",
        usize::from(webview.is_some()),
        retiring.len()
    );
    drop(webview);
    for retired in retiring {
        drop(retired);
    }
}

/// 隐藏地图 WebView——**唯一延迟销毁入口**。
///
/// wry 0.55 无 `set_visible` API，因此这里 drop WebView；返回时由上层地图
/// 会话重新下发页面配置、视野和未提交草稿，让用户继续同一处地图现场。
///
/// T35 崩溃根因：错误/确认/输入弹窗的 `hide` 可能发生在 wry IPC 回调栈内
/// （WebView2 COM 回调），同步 drop WebView 导致 COM 重入崩溃。本入口一律
/// 把销毁排到事件循环下一拍：`is_visible()` 立即为 false（逻辑隐藏），
/// 但实际 drop 由 `invoke_from_event_loop` 回调在 IPC 回调返回后执行。
/// T36：同时使在途创建过期（完成后转入延迟销毁）并取消等待中的 show。
pub(crate) fn hide() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.generation += 1;
        state.creation_in_flight = false;
        state.pending_show = None;
        if let Some(webview) = state.webview.take() {
            state.retiring.push(webview);
        }
        state.resize_timer = None; // drop 即停止轮询
        state.last_slot_scale = None;
        state.campus_search_mode = false;
        state.load_timer = None;
        schedule_drop(&mut state);
    });
    MAP_VISIBLE_PROBE.with(|s| s.set(false));
}

/// T36：地图加载失败标记——弹窗关闭不得自动重建（避免反复失败弹窗）。
/// 用户显式切换步骤（新的 [`show`] 请求）时清除。
pub(crate) fn mark_map_failed() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if state.creation_in_flight {
            state.generation += 1;
            state.creation_in_flight = false;
            state.pending_show = None;
            state.load_timer = None;
        }
    });
    pump_creation();
}

/// 当前记录的地图页面种类（测试与弹窗恢复决策用）。
fn page_kind() -> Option<MapPageKind> {
    STATE.with(|s| s.borrow().last_page_kind)
}

/// 向 JS 发送命令（Rust→JS 单向通道，如 drawBoundaryGcj / enableManualMode /
/// 抽屉桥接命令 undoManualPointFromDrawer / 评审标注 clearReviewOverlays /
/// addReviewCandidate 等）。
pub(crate) fn evaluate_script(script: &str) {
    STATE.with(|s| {
        let state = s.borrow();
        let Some(webview) = state.webview.as_ref() else {
            return;
        };
        log::debug!("map_webview: evaluate_script 执行（len={}）", script.len());
        let _ = webview.evaluate_script(script);
    });
}

#[cfg(test)]
#[path = "map_webview_tests.rs"]
mod tests;
