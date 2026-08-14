//! 屏幕 4 边界地图 WebView 嵌入与生命周期管理。
//!
//! 薄壳原则：（ADR-0017）本模块只做嵌入显示与消息桥接，零业务计算——
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
//! ## T35：销毁延迟到事件循环下一拍（P0 崩溃修复，本分支自带）
//! wry 的 IPC 回调（`window.ipc.postMessage` → `with_ipc_handler`）运行在
//! WebView2 自身 COM 回调栈内。错误/确认/输入弹窗路径都会在回调栈内调用
//! [`hide`]；若同步 drop WebView，WebView2 COM 重入会在 combase.dll 崩溃
//! （WER 0xc0000005 / 0xc0000409）。因此 [`hide`] 是**唯一延迟销毁入口**：
//! 活跃 WebView 立即移入 `retiring`（逻辑隐藏即刻生效），经
//! `slint::invoke_from_event_loop` 排定下一拍回调，IPC 回调返回后才真正
//! drop；全部弹窗遮挡调用点（presenter / presentation / 工作区适配器）
//! 统一走本入口。同时 IPC/状态回调先克隆 handler 释放 `STATE` 借用再执行
//! （`RefCell already borrowed` 根因，WER 0xc0000409）。
//!
//! ## T36：hide→show 步骤切换串行化（P1）
//! T35 延迟销毁后，"旧 WebView 尚未真正 drop 就新建下一份"会产生时序窗口：
//! 两个 WebView2 子窗口并存、Z 序/焦点错乱，朝向页点击无反应。本模块把
//! 创建也纳入同一生命周期：
//! - 每次 show 请求自增 `generation`；在途创建的完成结果只允许"当前代"
//!   生效，过期结果一律转入 `retiring` 延迟销毁，绝不触碰当前状态；
//! - 新建 WebView 前必须满足：无活跃 WebView、无在途创建、`retiring`
//!   队列已清空（销毁回调执行完后 `pump_creation` 才放行）——步骤切换
//!   hide→show 由此串行化；
//! - HTML 构建改为同步：密钥/锚点非法等创建前失败**如实**上报
//!   `map_available=false`，不再保持上一次的 true；
//! - 工作区页（边界/朝向）创建附带 10s 加载超时（[`LOAD_TIMEOUT`]）：超时后使在途
//!   创建过期、经错误 IPC 弹明确错误对话框，用户可退回"方位角手动输入"。
//!   T39：评审地图页不挂该超时（候选不再内嵌 HTML；千级候选在 map_ready
//!   后分批上屏，属于创建超时之外的绘制阶段，不应被 10s 兜底误杀）。
//!
//! ## T40：朝向页创建成功即激活两点选择（不依赖 map_ready）
//! 朝向步 WebView 创建成功后立即 `evaluate_script("activateOrientationWhenReady();")`
//! （页面脚本在 SDK/地图未就绪时静默等待，由 onMapReadyForMode 自动激活兜底）；
//! 朝向页同时在 onMapReadyForMode 之外发送 map_ready，启用 T37 的
//! `map_ready_for_active_step` 兜底——两个独立激活通道保证页面就绪后两点点击挂接。
// ignore-tidy-filelength: T38 评审地图页生命周期（种类路由/show_review/弹窗恢复）并入本文件
// 后短暂超限；失效里程碑：v2.1.0（2026-12-31），届时按页种类拆出 WebView 生命周期助手后消除

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;
use slint::{winit_030::WinitWindowAccessor, Weak};

/// 地图加载超时（T36）：WebView 创建/SDK 就绪的硬性上限。
pub(crate) const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// T36：Rust 侧超时经错误 IPC 上报时使用的标记；工作区入口把它本地化为
/// 明确的超时文案（ADR-0005 禁止硬编码可见文本）。
pub const MAP_LOAD_TIMEOUT_MARKER: &str = "__t36_map_load_timeout__";

/// IPC 消息处理签名：输入原始消息文本，输出是否已识别处理。
/// 由功能入口注册；返回值仅用于诊断记录。
pub(crate) type IpcHandler = Rc<dyn Fn(&str)>;

/// 地图加载完成状态处理器：true = WebView 创建成功，false = 创建失败。
/// 由功能入口注册；故障只暂停地图相关操作，不阻塞其他页面（ADR-0037）。
pub(crate) type MapStatusHandler = Rc<dyn Fn(bool)>;

/// 地图页面种类：决定弹窗关闭后按哪个页面重建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapPageKind {
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
    /// T36：地图加载失败后禁止弹窗关闭自动重建（避免反复失败弹窗）；
    /// 用户显式切换步骤重新请求时才清除。
    suppress_restore: bool,
    /// 功能入口注册的 IPC 处理器
    ipc_handler: Option<IpcHandler>,
    /// 功能入口注册的地图加载状态处理器
    status_handler: Option<MapStatusHandler>,
    /// 上次使用的密钥（recreate 时复用）
    last_api_key: String,
    last_security_key: String,
    /// 上次使用的校区锚点（recreate 时复用；模态弹窗隐藏后恢复用）
    last_anchor: Option<(f64, f64)>,
    /// 上次显示的地图页面种类（弹窗恢复按此步骤重建）
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
            generation: 0,
            creation_in_flight: false,
            pending_show: None,
            load_timer: None,
            suppress_restore: false,
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

/// T39：评审地图 Rust→JS 回推命令计数（契约测试注入观察；生产为廉价原子
/// 计数，不做业务分支）。一次高亮/三态操作只应产生 1 条回推，不再是
/// clear + 21 批全量。
static REVIEW_PUSH_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 评审地图回推命令累计条数（诊断/验收用）。
#[doc(hidden)]
pub fn review_push_count() -> usize {
    REVIEW_PUSH_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

/// 清零评审地图回推计数（验收测试起点）。
#[doc(hidden)]
pub fn reset_review_push_count() {
    REVIEW_PUSH_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// 无 WebView 测试环境下模拟"评审地图可见"，使回推路径可被计数观察；
/// 生产不设置（恒为实际可见性）。
#[doc(hidden)]
pub fn set_review_push_probe_visible(visible: bool) {
    REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.set(visible));
}

thread_local! {
    static REVIEW_PUSH_PROBE_VISIBLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 评审回推是否应执行：生产 = 活跃 WebView；契约测试可经探针模拟。
pub(crate) fn review_push_visible() -> bool {
    is_visible() || REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.get())
}

/// 记录一条评审地图回推命令（在任何 evaluate_script 前调用，无 WebView
/// 环境同样计数，供 T39 验收断言"一次高亮只产生 1 条回推"）。
pub(crate) fn note_review_push() {
    REVIEW_PUSH_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
fn notify_status(available: bool) {
    log::debug!("map_webview: notify_status(available={available})");
    let handler = STATE.with(|s| s.borrow().status_handler.clone());
    if let Some(handler) = handler {
        handler(available);
    }
}

/// 把 WebView2 IPC 原始载荷转交注册的业务 handler。
///
/// T35 根因（RefCell already borrowed → WebView2 回调内不可 unwind 崩溃，
/// WER 0xc0000409）：不得在 `STATE` 借用存活期间调用 handler。这里先克隆出
/// handler、释放借用，再在回调栈内执行业务（handler 内可能调用 [`hide`]）。
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

/// 当前地图是否为评审页（T38：评审步的 IPC 路由到评审入口而非边界入口）。
pub(crate) fn is_review_page() -> bool {
    page_kind() == Some(MapPageKind::Review)
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
            ..
        } => {
            let config = gaode_client::BoundaryEditPageConfig::new(api_key, security_key)
                .with_anchor(*anchor_lon, *anchor_lat);
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
            ..
        } => {
            let config = gaode_client::ReviewMapPageConfig::new(api_key, security_key)
                .with_anchor(*anchor_lon, *anchor_lat);
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
                Some(PumpAction::HtmlFailed { reports_status })
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
        Some(PumpAction::HtmlFailed { reports_status }) => {
            if reports_status {
                notify_status(false);
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
            return false;
        }
        log::warn!("map_webview: 地图加载超时（{LOAD_TIMEOUT:?}），上报失败并禁止自动重建");
        state.generation += 1;
        state.creation_in_flight = false;
        state.pending_show = None;
        state.load_timer = None;
        state.suppress_restore = true;
        true
    });
    if timed_out {
        // 状态（map_available=false / 处理中清除）由工作区入口在错误 IPC
        // 分支统一处理并弹明确错误对话框；这里只发 IPC，避免重复通知。
        dispatch_ipc(&format!(
            r#"{{"type":"error","message":"{MAP_LOAD_TIMEOUT_MARKER}"}}"#
        ));
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
            .with_bounds(bounds)
            .with_focused(false)
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

        #[cfg(windows)]
        if result.is_ok() {
            windows_focus_guard::install(&*winit_win);
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
            notify_status(true);
            // T40：朝向页创建成功即经可靠通道激活（不依赖 map_ready）；页面
            // 脚本在 SDK/地图未就绪时静默等待自动激活兜底。
            if let Some(script) = creation_activation_script(page_kind) {
                log::debug!("map_webview: 朝向页创建成功，执行激活脚本（创建成功通道）: {script}");
                evaluate_script(script);
            }
        }
        Outcome::Failed => notify_status(false),
        Outcome::Stale => {}
    }
    pump_creation();
}

/// 记录待执行请求并尝试放行（T36 统一 show 入口）。
fn request_show(request: PendingShow) {
    let page_kind = request.page_kind();
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        // 幂等：同页已显示时不重建（键/锚点已被记录供 recreate 复用）。
        if state.webview.is_some() && state.last_page_kind == Some(page_kind) {
            return;
        }
        match &request {
            PendingShow::Boundary {
                api_key,
                security_key,
                anchor_lon,
                anchor_lat,
                ..
            } => {
                state.last_api_key = api_key.clone();
                state.last_security_key = security_key.clone();
                state.last_anchor = Some((*anchor_lon, *anchor_lat));
                state.last_page_kind = Some(MapPageKind::Boundary);
                state.campus_search_mode = false;
            }
            PendingShow::CampusSearch {
                api_key,
                security_key,
                ..
            } => {
                state.last_api_key = api_key.clone();
                state.last_security_key = security_key.clone();
                state.last_page_kind = Some(MapPageKind::CampusSearch);
                state.campus_search_mode = true;
            }
            PendingShow::Orientation { config, .. } => {
                state.last_api_key = config.api_key.clone();
                state.last_security_key = config.security_key.clone();
                state.last_page_kind = Some(MapPageKind::Orientation);
                state.last_orientation_config = Some(config.clone());
                state.campus_search_mode = false;
            }
            PendingShow::Review {
                api_key,
                security_key,
                anchor_lon,
                anchor_lat,
                ..
            } => {
                state.last_api_key = api_key.clone();
                state.last_security_key = security_key.clone();
                state.last_anchor = Some((*anchor_lon, *anchor_lat));
                state.last_page_kind = Some(MapPageKind::Review);
                state.campus_search_mode = false;
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
        // 显式请求清除"失败后禁止自动重建"标记。
        state.suppress_restore = false;
        state.generation += 1;
        state.pending_show = Some(request);
    });
    pump_creation();
}

/// 显示（或重建）边界地图 WebView。
///
/// 幂等：已存在时不重复创建。密钥/锚点被记录供 recreate 复用。
/// 创建失败如实上报（地图不可用 → 人工圈画画布仍在 Slint 侧可用）。
pub(crate) fn show(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
    anchor_lon: f64,
    anchor_lat: f64,
) {
    request_show(PendingShow::Boundary {
        window: window_weak,
        api_key,
        security_key,
        anchor_lon,
        anchor_lat,
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

/// T38: 显示（或重建）评审地图 WebView。
///
/// 候选标注由 Rust 侧在 `map_ready` IPC 后经 `drawReviewCandidates` 推送
/// （候选几何已在 B2 投影中为 GCJ-02，JS 不做坐标转换）。
pub(crate) fn show_review(
    window_weak: Weak<crate::AppWindow>,
    api_key: String,
    security_key: String,
    anchor_lon: f64,
    anchor_lat: f64,
) {
    request_show(PendingShow::Review {
        window: window_weak,
        api_key,
        security_key,
        anchor_lon,
        anchor_lat,
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
}

/// T36：地图加载失败标记——弹窗关闭不得自动重建（避免反复失败弹窗）。
/// 用户显式切换步骤（新的 [`show`] 请求）时清除。
pub(crate) fn mark_map_failed() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.suppress_restore = true;
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

/// 模态弹窗关闭后是否需要恢复地图（T34 统一机制）。
///
/// - 工作区（screen 4）：五个步骤全部显示地图——步骤 ①② 为边界/朝向页，
///   步骤 ③④⑤ 继续让位显示（评审步骤 ④ 显示评审地图，T38）。
/// - 校区搜索页（screen 1）：取消弹窗后停留该页时需要恢复搜索地图。
fn should_restore_after_modal(active_screen: i32, _active_step: i32) -> bool {
    if active_screen == 4 {
        return true;
    }
    active_screen == 1 && page_kind() == Some(MapPageKind::CampusSearch)
}

/// 恢复被模态弹窗隐藏的地图 WebView（仅当应当恢复且锚点/配置已知时）。
///
/// T34：关闭错误/确认/输入弹窗后按**当前步骤模式**重建——
/// 步骤②重建朝向页（复用上次朝向配置，含半透明边界参照），
/// 步骤④重建评审地图页（T38），其余步骤重建边界页；不得恢复错页。
/// T36：地图加载失败（[`mark_map_failed`] / 超时）后跳过自动重建，
/// 用户显式切换步骤才重试。
pub(crate) fn restore_after_modal(window: Weak<crate::AppWindow>) {
    if STATE.with(|s| s.borrow().suppress_restore) {
        log::debug!("map_webview: 地图加载失败后跳过弹窗自动重建（等待显式重新进入）");
        return;
    }
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
        if active_step == 3 {
            // 评审页：用上次密钥/锚点重建评审地图——候选不再内嵌 HTML，
            // 重建后页面 map_ready → Rust 在事件循环安全上下文全量推送。
            if let Some((anchor_lon, anchor_lat)) = anchor {
                show_review(window, api_key, security_key, anchor_lon, anchor_lat);
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

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows subclass callback required to keep Slint keyboard focus instead of WebView2"
)]
mod windows_focus_guard {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GetClassLongPtrW, GCLP_WNDPROC, WM_ENTERSIZEMOVE, WM_SETFOCUS, WNDPROC,
    };

    /// 与 wry 的 `PARENT_SUBCLASS_ID`（`WM_USER + 0x64`）错开的子类 ID。
    const FOCUS_GUARD_SUBCLASS_ID: usize = 0x4d43_0001;

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uidsubclass: usize,
        dwrefdata: usize,
    ) -> LRESULT {
        if bypasses_wry(msg) {
            // 绕过 wry 父窗口子类（它会把焦点 MoveFocus 到 WebView2），直接交回
            // 原始 winit 窗口过程，使 Slint 文本输入保持键盘焦点。
            let original: WNDPROC = unsafe { std::mem::transmute(dwrefdata) };
            unsafe { CallWindowProcW(original, hwnd, msg, wparam, lparam) }
        } else {
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
    }

    /// 只把「抢焦点」的消息绕过 wry 子类；其余消息（含 WM_SIZE / WM_DESTROY /
    /// WM_MOVE）仍走正常链，保证 wry 的尺寸与销毁清理逻辑不受影响。
    fn bypasses_wry(msg: u32) -> bool {
        msg == WM_SETFOCUS || msg == WM_ENTERSIZEMOVE
    }

    pub(crate) fn install(window: &impl HasWindowHandle) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as _);
        unsafe {
            let original = GetClassLongPtrW(hwnd, GCLP_WNDPROC);
            if original == 0 {
                log::debug!("map_webview: 焦点守卫未安装（原始窗口过程为空）");
                return;
            }
            log::debug!(
                "map_webview: 安装焦点守卫子类（原始窗口过程=0x{:x}）",
                original
            );
            let _ = SetWindowSubclass(hwnd, Some(subclass_proc), FOCUS_GUARD_SUBCLASS_ID, original);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::Win32::UI::WindowsAndMessaging::{WM_MOVE, WM_SIZE};

        #[test]
        fn guard_bypasses_only_focus_messages() {
            assert!(bypasses_wry(WM_SETFOCUS));
            assert!(bypasses_wry(WM_ENTERSIZEMOVE));
            assert!(!bypasses_wry(WM_SIZE));
            assert!(!bypasses_wry(WM_MOVE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 清空注册的 handler 与状态（thread_local 在测试线程间共享）。
    fn reset_state() {
        REVIEW_PUSH_PROBE_VISIBLE.with(|s| s.set(false));
        REVIEW_PUSH_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.ipc_handler = None;
            state.status_handler = None;
            state.webview = None;
            state.retiring.clear();
            state.hide_scheduled = false;
            state.generation = 0;
            state.creation_in_flight = false;
            state.pending_show = None;
            state.load_timer = None;
            state.suppress_restore = false;
        });
    }

    #[test]
    fn hide_without_webview_keeps_state_clean_in_headless_environment() {
        // T35：单元测试进程没有运行中的 Slint 事件循环，hide() 必须在
        // 无 WebView 时直接返回（不调用 invoke_from_event_loop、不残留
        // 排定标记），否则无界面环境下会误排定一次永不执行的销毁回调。
        reset_state();
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
    fn orientation_page_creation_success_has_activation_channel() {
        // T40：朝向页创建成功必须走"创建成功回调激活"通道（不依赖
        // map_ready）；其他页种类不得误触发朝向激活。
        assert_eq!(
            creation_activation_script(MapPageKind::Orientation),
            Some("activateOrientationWhenReady();")
        );
        assert_eq!(creation_activation_script(MapPageKind::Boundary), None);
        assert_eq!(creation_activation_script(MapPageKind::CampusSearch), None);
        assert_eq!(creation_activation_script(MapPageKind::Review), None);
    }

    #[test]
    fn ipc_dispatch_releases_borrow_before_invoking_handler() {
        // T35 根因回归（RefCell already borrowed → WebView2 回调内不可 unwind
        // 崩溃，WER 0xc0000409）：错误弹窗路径会在 IPC handler 内调用
        // hide()（对 STATE 做 borrow_mut）。若 dispatch 在 STATE 借用存活期间
        // 调用 handler（旧写法 `if let Some(handler) = s.borrow()...` 的临时
        // Ref 活到 if-let 体结束），这里会直接 panic。
        reset_state();
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
        reset_state();
    }

    #[test]
    fn creation_failure_reports_map_unavailable_immediately() {
        // T36：步骤②切换后必须如实上报 map_available——创建前失败（如非法
        // 密钥）不得保持上一次的 true，也不得静默。HTML 构建为同步路径，
        // 无事件循环也能断言状态回调收到 false。
        reset_state();
        let received: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let captured = Rc::clone(&received);
        register_status_handler(Rc::new(move |available| {
            captured.borrow_mut().push(available);
        }));
        // 非法密钥：gaode_client 要求纯字母数字 → HTML 构建失败。
        let config = gaode_client::BoundaryEditPageConfig::new("bad key!", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_orientation_mode(true);
        show_with_config(Weak::<crate::AppWindow>::default(), config);
        assert_eq!(
            received.borrow().as_slice(),
            &[false],
            "创建前失败必须如实上报 map_available=false"
        );
        STATE.with(|s| {
            let state = s.borrow();
            assert!(state.webview.is_none(), "失败后不得持有 WebView");
            assert!(!state.creation_in_flight, "失败后不得残留在途创建");
            assert!(state.pending_show.is_none(), "失败后不得残留等待请求");
            assert!(state.retiring.is_empty(), "失败后不得残留待销毁队列");
        });
        reset_state();
    }

    #[test]
    fn review_page_skips_rust_load_timeout() {
        // T39：评审地图页不得挂 Rust 侧 10s 加载超时——候选不再内嵌 HTML，
        // 千级候选在 map_ready 后分批上屏（属于创建超时之外的绘制阶段），
        // 10s 兜底会把慢速注入/慢网络的评审地图误杀；页面自身 5s SDK 超时
        // 与 onerror 仍负责失败上报。
        reset_state();
        let weak = Weak::<crate::AppWindow>::default();
        assert!(
            !PendingShow::Review {
                window: weak.clone(),
                api_key: "testapikey123".into(),
                security_key: "testsecurity123".into(),
                anchor_lon: 116.4,
                anchor_lat: 39.9,
            }
            .has_load_timeout(),
            "评审页必须跳过 Rust 侧加载超时"
        );
        assert!(
            PendingShow::Boundary {
                window: weak.clone(),
                api_key: "testapikey123".into(),
                security_key: "testsecurity123".into(),
                anchor_lon: 116.4,
                anchor_lat: 39.9,
            }
            .has_load_timeout(),
            "边界页必须保留 Rust 侧加载超时"
        );
        reset_state();
    }

    #[test]
    fn review_push_counter_and_probe_are_diagnostic_seams() {
        // T39 验收缝：回推计数可注入观察；探针仅测试环境使用，生产恒为
        // 实际可见性（无 WebView 不推送）。
        reset_state();
        set_review_push_probe_visible(true);
        assert!(review_push_visible(), "探针打开后回推路径必须可观察");
        note_review_push();
        assert_eq!(review_push_count(), 1, "一次命令只计一条回推");
        set_review_push_probe_visible(false);
        assert!(!review_push_visible(), "探针关闭后回到实际可见性");
        reset_review_push_count();
        reset_state();
    }
}
