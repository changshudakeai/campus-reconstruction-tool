//! 第五步 3D 方块预览页的显示入口与契约测试探针（T52）。
//!
//! 与 `map_webview` 共用同一套 WebView 生命周期；本模块只负责“请求显示
//! 预览页”和测试观测，避免把预览细节继续堆进地图适配器大文件。

use std::cell::Cell;

use slint::Weak;

use crate::map_webview::{request_show, PendingShow};

thread_local! {
    /// 契约测试探针：只记录页面种类，不真实创建 WebView2 子窗口（软件后端
    /// 测试进程内真实创建会在事件循环完成时触发 COM 崩溃）。生产恒为 false。
    static DISABLE_WEBVIEW_CREATION: Cell<bool> = const { Cell::new(false) };
}

/// 显示（或重建）第五步 3D 方块预览页。
///
/// 与其他地图页共用同一 WebView 生命周期：进入其他步骤时由地图会话按现场
/// 重建边界/朝向/评审页，预览负载留在会话内，回到第五步自动恢复。
pub(crate) fn show_block_preview(
    window_weak: Weak<crate::AppWindow>,
    initial_payload: Option<String>,
) {
    request_show(PendingShow::BlockPreview {
        window: window_weak,
        initial_payload,
    });
}

/// 当前记录的 WebView 页面种类（稳定字符串形态，供契约测试观测）。
#[doc(hidden)]
pub fn current_page_kind_name() -> Option<&'static str> {
    crate::map_webview::page_kind().map(|kind| match kind {
        crate::map_webview::MapPageKind::Boundary => "boundary",
        crate::map_webview::MapPageKind::Orientation => "orientation",
        crate::map_webview::MapPageKind::Review => "review",
        crate::map_webview::MapPageKind::CampusSearch => "campus_search",
        crate::map_webview::MapPageKind::BlockPreview => "block_preview",
    })
}

/// 契约测试探针：关闭真实 WebView 创建（页面种类与现场逻辑照常推进）。
#[doc(hidden)]
pub fn set_webview_creation_probe(enabled: bool) {
    DISABLE_WEBVIEW_CREATION.with(|state| state.set(enabled));
}

/// 读取测试探针状态（`map_webview::pump_creation` 消费）。
pub(crate) fn webview_creation_disabled() -> bool {
    DISABLE_WEBVIEW_CREATION.with(|state| state.get())
}
