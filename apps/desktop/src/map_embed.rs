use slint::{winit_030::WinitWindowAccessor, ComponentHandle};
use slint::Weak;

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
<style>body,html{{margin:0;height:100%}}#m{{width:100%;height:100%}}</style>
</head><body><div id="m"></div><script>
var map=new AMap.Map('m',{{zoom:16,center:[116.397,39.916]}});
map.on('click',function(e){{
  if(window.ipc)window.ipc.postMessage(e.lnglat.lng+","+e.lnglat.lat);
}});
</script></body></html>"#
    )
}

/// 探针入口：在 app.run() 之前调用一次。
/// 通过 spawn_local 等事件循环启动后拿 winit 窗口并嵌入 WebView。
pub(crate) fn embed_probe(window_weak: Weak<crate::AppWindow>) {
    let Some((api_key, security_key)) = load_demo_keys() else {
        return;
    };
    let _ = slint::spawn_local(async move {
        let Some(app_window) = window_weak.upgrade() else {
            return;
        };
        let Ok(winit_win) = app_window.window().winit_window().await else {
            return;
        };
        let html = build_html(&api_key, &security_key);
        let result = wry::WebViewBuilder::new()
            .with_html(html)
            .with_ipc_handler(|request: wry::http::Request<String>| {
                let _ = std::fs::write(
                    concat!(
                        r"C:\Users\chang\Desktop\MCRebuild_Renovation\",
                        r"New-branch-v2\.scratch\t21-clicks.log"
                    ),
                    format!("{}\n", request.body()),
                );
            })
            .build_as_child(&*winit_win);
        if let Ok(webview) = result {
            std::mem::forget(webview); // 探针阶段保活
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
        if std::path::Path::new(
            r"C:\Users\chang\AppData\Local\MCRebuildV2\dev\gaode-demo-keys.txt",
        )
        .exists()
        {
            assert!(result.is_some());
        } else {
            assert!(result.is_none());
        }
    }
}
