//! 高德地图嵌入实现（T21）
//!
//! 使用 wry WebView 嵌入主窗口，与 Slint UI 共存。
//! 探针任务：验证 winit 版本桥接与坐标回传协议。

use slint::Weak;
use slint::{winit_030::WinitWindowAccessor, ComponentHandle};

fn debug_log(msg: &str) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(
            r"C:\Users\chang\Desktop\MCRebuild_Renovation\",
            r"New-branch-v2\.scratch\t21-debug.log"
        ))
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(
                f,
                "{}: {}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                msg
            )
        });
}

/// 从 demo 临时密钥文件读取（第一行 key，第二行安全密钥）
fn load_demo_keys() -> Option<(String, String)> {
    let path = concat!(
        r"C:\Users\chang\AppData\Local\MCRebuildV2\dev\",
        "gaode-demo-keys.txt"
    );
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    Some((
        lines.next()?.trim().to_owned(),
        lines.next()?.trim().to_owned(),
    ))
}

fn build_html(api_key: &str, security_key: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<script>window._AMapSecurityConfig={{securityJsCode:"{security_key}"}};</script>
<script src="https://webapi.amap.com/maps?v=2.0&key={api_key}"></script>
<style>
body,html{{margin:0;height:100%;position:relative;overflow:hidden}}
#m{{width:100%;height:100%;position:absolute;top:0;left:0;z-index:1}}
#test-btn{{position:absolute;top:10px;left:10px;z-index:999;padding:12px 24px;font-size:16px;background:#e74c3c;color:white;border:none;cursor:pointer;border-radius:6px}}
</style>
</head><body>
<div id="m"></div>
<button id="test-btn">Test Click</button>
<script>
var map=new AMap.Map('m',{{zoom:16,center:[116.397,39.916]}});
if(window.ipc){{window.ipc.postMessage("IPC_READY");}}else{{document.title="NO_IPC";}}
map.on('click',function(e){{
  if(window.ipc)window.ipc.postMessage("MAP_CLICK,"+e.lnglat.lng+","+e.lnglat.lat);
}});
document.getElementById('test-btn').onclick=function(){{
  if(window.ipc)window.ipc.postMessage("BTN_CLICK,test-data,116.4,39.92");
}};
</script></body></html>"#
    )
}

/// 探针入口：在 app.run() 之前调用一次。
/// 通过 spawn_local 等事件循环启动后拿 winit 窗口并嵌入 WebView。
pub(crate) fn embed_probe(window_weak: Weak<crate::AppWindow>) {
    debug_log("embed_probe called");
    let Some((api_key, security_key)) = load_demo_keys() else {
        debug_log("FAIL: load_demo_keys returned None");
        return;
    };
    debug_log("keys loaded OK");
    let _ = slint::spawn_local(async move {
        debug_log("spawn_local future started");
        let Some(app_window) = window_weak.upgrade() else {
            debug_log("FAIL: window_weak.upgrade() returned None");
            return;
        };
        debug_log("weak upgrade OK");
        let Ok(winit_win) = app_window.window().winit_window().await else {
            debug_log("FAIL: winit_window().await returned Err");
            return;
        };
        debug_log("winit_window OK, building WebView");
        let html = build_html(&api_key, &security_key);
        let bounds = wry::Rect {
            position: wry::dpi::Position::Physical(wry::dpi::PhysicalPosition::new(0, 30)),
            size: wry::dpi::Size::Physical(wry::dpi::PhysicalSize::new(1200, 600)),
        };
        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let body = request.body().to_string();
                debug_log(&format!("IPC received: {}", body));
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(concat!(
                        r"C:\Users\chang\Desktop\MCRebuild_Renovation\",
                        r"New-branch-v2\.scratch\t21-clicks.log"
                    ))
                    .and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, "{}", body)
                    });
            })
            .build_as_child(&*winit_win);
        match result {
            Ok(webview) => {
                debug_log("WebView created OK");
                std::mem::forget(webview);
            }
            Err(e) => {
                debug_log(&format!("FAIL: build_as_child error: {:?}", e));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_html_contains_required_scripts() {
        let html = build_html("test_key", "test_security");
        assert!(html.contains("webapi.amap.com"));
        assert!(html.contains("_AMapSecurityConfig"));
        assert!(html.contains("window.ipc.postMessage"));
        assert!(html.contains("test_key"));
        assert!(html.contains("test_security"));
    }

    #[test]
    fn test_load_demo_keys() {
        let result = load_demo_keys();
        if std::path::Path::new(r"C:\Users\chang\AppData\Local\MCRebuildV2\dev\gaode-demo-keys.txt")
            .exists()
        {
            assert!(result.is_some());
        } else {
            assert!(result.is_none());
        }
    }
}
