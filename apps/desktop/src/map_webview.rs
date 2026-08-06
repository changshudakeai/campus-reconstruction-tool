//! T24: 屏 4 边界地图 WebView 嵌入与生命周期管理
//!
//! 薄壳原则（ADR-0017）：本模块只做嵌入显示与消息桥接，零业务计算——
//! OSM 候选排序在 B3 `gaode_client::BoundarySorter`，校验在 B5
//! `validate_polygon_closure`，HTML 生成在 B3 `gaode_client`。
//!
//! ## 嵌入链路（T21 已验证，禁止推翻）
//! `slint::spawn_local` → `winit_window().await` → `build_as_child`。
//!
//! ## 显隐策略
//! 屏 4 显示、其余屏隐藏。wry 0.55 无 `set_visible` API，采用
//! **drop/recreate** 兜底并诚实声明（验收条款"随屏显隐"以此实现）。
//! 屏幕切换处由注入器调用 [`show`] / [`hide`]。
//!
//! ## resize 跟随
//! slint 1.17 无公开窗口 resize 回调，采用 300ms 轮询兜底：
//! 窗口尺寸或缩放变化时经 `WebView::set_bounds` 重定位。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use slint::{winit_030::WinitWindowAccessor, Weak};

/// IPC 消息处理器签名：输入原始消息文本，输出是否已识别处理。
/// 由功能入口注册；返回值仅用于诊断记录。
pub(crate) type IpcHandler = Rc<dyn Fn(&str)>;

/// 地图加载完成状态处理器：true = WebView 创建成功，false = 创建失败。
/// 由功能入口注册；故障只暂停地图相关操作，不阻塞其他页面（ADR-0037）。
pub(crate) type MapStatusHandler = Rc<dyn Fn(bool)>;

struct WebViewState {
    /// 活跃 WebView（None = 已隐藏/未创建）
    webview: Option<wry::WebView>,
    /// 功能入口注册的 IPC 处理器
    ipc_handler: Option<IpcHandler>,
    /// 功能入口注册的地图加载状态处理器
    status_handler: Option<MapStatusHandler>,
    /// 上次使用的密钥（recreate 时复用）
    last_api_key: String,
    last_security_key: String,
    /// 当前 WebView 是否处于"校区搜索页"模式（D-3：区别于边界地图页）
    campus_search_mode: bool,
    /// resize 跟随轮询定时器（hide 时 drop 即停）
    resize_timer: Option<slint::Timer>,
    /// 上次同步的（窗口逻辑宽度, 缩放因子）——变化才 set_bounds
    last_size_scale: (u32, f32),
}

thread_local! {
    /// 全局唯一边界地图 WebView（单窗口单实例；Slint 单线程 UI 模型）
    static STATE: RefCell<WebViewState> = const {
        RefCell::new(WebViewState {
            webview: None,
            ipc_handler: None,
            status_handler: None,
            last_api_key: String::new(),
            last_security_key: String::new(),
            campus_search_mode: false,
            resize_timer: None,
            last_size_scale: (0, 0.0),
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
    STATE.with(|s| {
        if let Some(handler) = s.borrow().status_handler.clone() {
            handler(available);
        }
    });
}

/// 是否已有活跃 WebView
pub(crate) fn is_visible() -> bool {
    STATE.with(|s| s.borrow().webview.is_some())
}

/// 校区搜索 WebView 是否已就绪（存在且处于校区搜索页模式）。
pub(crate) fn campus_search_ready() -> bool {
    STATE.with(|s| {
        let state = s.borrow();
        state.webview.is_some() && state.campus_search_mode
    })
}

/// 画布区域（boundary_edit.slint）：x:16 y:56, width:parent.width-32, height:340
/// 逻辑像素 × scale factor = 物理像素
fn compute_bounds(window_width_logical: u32, scale: f32) -> wry::Rect {
    let scale = f64::from(scale);
    wry::Rect {
        position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(
            (16.0 * scale) as i32,
            (56.0 * scale) as i32,
        )),
        size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(
            ((f64::from(window_width_logical) - 32.0).max(300.0) * scale) as u32,
            (340.0 * scale) as u32,
        )),
    }
}

/// 校区选择页搜索地图区域：x:16 y:110, width:parent.width-32（兜底 300）, height:300。
/// 与边界地图页同规格：逻辑像素 × scale factor = 物理像素。
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

/// 启动 resize 跟随轮询（slint 1.17 无公开 resize 回调 → 定时器兜底）
fn start_resize_timer(window_weak: Weak<crate::AppWindow>) {
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(300),
        move || {
            let Some(app_window) = window_weak.upgrade() else {
                return;
            };
            let width = app_window.window().size().width;
            let scale = app_window.window().scale_factor();
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                if state.last_size_scale == (width, scale) {
                    return;
                }
                state.last_size_scale = (width, scale);
                if let Some(webview) = state.webview.as_ref() {
                    let _ = webview.set_bounds(compute_bounds(width, scale));
                }
            });
        },
    );
    STATE.with(|s| s.borrow_mut().resize_timer = Some(timer));
}

/// 显示（或重建）边界地图 WebView。
///
/// 幂等：已存在时不重复创建。密钥/锚点被记录供 recreate 复用。
/// 创建失败静默降级（地图不可用 → 人工圈画布仍在 Slint 侧可用）。
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
        let width = app_window.window().size().width;
        let bounds = compute_bounds(width, scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                STATE.with(|s| {
                    if let Some(handler) = s.borrow().ipc_handler.clone() {
                        handler(&body);
                    }
                });
            })
            .build_as_child(&*winit_win);

        let map_created = result.is_ok();
        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_size_scale = (width, scale);
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
        let width = app_window.window().size().width;
        let bounds = compute_campus_search_bounds(width, scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                STATE.with(|s| {
                    if let Some(handler) = s.borrow().ipc_handler.clone() {
                        handler(&body);
                    }
                });
            })
            .build_as_child(&*winit_win);

        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_size_scale = (width, scale);
            });
            start_resize_timer(weak_for_timer);
        }
    });
}

/// T25: 显示（或重建）地图 WebView，使用完整配置（朝向模式/已确认边界）。
///
/// 与 [`show`] 不同：本函数允许调用方传入完整 `BoundaryEditPageConfig`，
/// 用于步骤②朝向模式（显示半透明边界参照）。
pub(crate) fn show_with_config(
    window_weak: Weak<crate::AppWindow>,
    config: gaode_client::BoundaryEditPageConfig,
) {
    // 非幂等：朝向模式需要重建 WebView 以切换 HTML 初始化参数
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.last_api_key = config.api_key.clone();
        state.last_security_key = config.security_key.clone();
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
        let width = app_window.window().size().width;
        let bounds = compute_bounds(width, scale);

        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                STATE.with(|s| {
                    if let Some(handler) = s.borrow().ipc_handler.clone() {
                        handler(&body);
                    }
                });
            })
            .build_as_child(&*winit_win);

        let map_created = result.is_ok();
        if let Ok(webview) = result {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.webview = Some(webview);
                state.last_size_scale = (width, scale);
            });
            start_resize_timer(weak_for_timer);
        }
        // 创建成功或失败都如实回报：故障只暂停地图相关操作
        notify_status(map_created);
    });
}

/// 隐藏边界地图 WebView（离开屏 4 时调用）。
///
/// 诚实声明：wry 0.55 无 `set_visible` API，这里直接 drop WebView；
/// 返回屏 4 时由 [`show`] 重建。重建期间地图状态（缩放/编辑内容）
/// 不保留——已确认边界已由 Rust 侧保管，不影响数据正确性。
pub(crate) fn hide() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.webview = None;
        state.resize_timer = None; // drop 即停止轮询
        state.last_size_scale = (0, 0.0);
        state.campus_search_mode = false;
    });
}

/// 向 JS 发送命令（Rust→JS 单向通道，如 convertAndDraw / enableManualMode）。
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
    fn hide_is_idempotent() {
        hide();
        hide();
        assert!(!is_visible());
    }

    #[test]
    fn bounds_follow_canvas_area() {
        // boundary_edit.slint：x:16 y:56, width:parent.width-32, height:340
        let bounds = compute_bounds(800, 1.0);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!((pos.x, pos.y), (16, 56));
        assert_eq!((sz.width, sz.height), (800 - 32, 340));
    }

    #[test]
    fn bounds_scale_to_physical_pixels() {
        // 2 倍缩放：逻辑像素 × 2 = 物理像素
        let bounds = compute_bounds(800, 2.0);
        let wry::Rect { position, size } = bounds;
        let wry::dpi::Position::Physical(pos) = position else {
            panic!("position 必须是物理像素");
        };
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!((pos.x, pos.y), (32, 112));
        assert_eq!((sz.width, sz.height), ((800 - 32) * 2, 680));
    }

    #[test]
    fn bounds_never_shrink_below_minimum() {
        // 极窄窗口：宽度兜底 300px 物理像素
        let bounds = compute_bounds(200, 1.0);
        let wry::Rect { size, .. } = bounds;
        let wry::dpi::Size::Physical(sz) = size else {
            panic!("size 必须是物理像素");
        };
        assert_eq!(sz.width, 300);
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
    fn campus_search_ready_follows_mode_and_visibility() {
        hide();
        assert!(!campus_search_ready(), "无 WebView 时不算就绪");
        // 模式标记只由 show_campus_search 置位（创建 WebView 需要真实窗口，
        // 单元测试只验证纯函数边界与 hide 复位行为）。
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
