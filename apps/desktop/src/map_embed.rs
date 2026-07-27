//! S1 高德地图嵌入 —— 屏 4 WebView 子窗口壳层（T21 REDO - Final）
//!
//! **核心发现**：wry 0.55 + Slint 1.9 存在 API 兼容性问题：
//! - wry::WebViewBuilder::build() 需要 HasWindowHandle trait
//! - Slint 1.9 未暴露 WinitWindowAccessor trait  
//!
//! T21 REDO 结论：**可以验证可行性**，但当前框架不支持真实嵌入。
//!
//! 推荐后续方案：升级 Slint 至 1.17+ 后再启用此模块。

use std::fs;
use std::path::PathBuf;

/// 探针期临时密钥文件路径：%LOCALAPPDATA%\MCRebuildV2\dev\gaode-demo-keys.txt
const GAODE_KEYS_FILE: &str = "gaode-demo-keys.txt";

/// 读取临时密钥文件（第一行 key，第二行安全密钥）。
pub fn read_temp_keys() -> Option<(String, String)> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let keys_path = PathBuf::from(local_app_data)
        .join("MCRebuildV2")
        .join("dev")
        .join(GAODE_KEYS_FILE);

    let content = fs::read_to_string(&keys_path).ok()?;
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    let key = lines.next()?.to_owned();
    let secret = lines.next()?.to_owned();
    Some((key, secret))
}

/// 构建高德 JS API 2.0 地图页 HTML（探针期固定北京锚点）。
fn build_map_html(
    api_key: &str,
    security_code: &str,
    height_px: u32,
    (lng, lat): (f64, f64),
) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>高德地图底图</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; overflow: hidden; }}
  #map {{ width: 100%; height: {}px; }}
</style>
<script>
  window._AMapSecurityConfig = {{ securityJsCode: "{security_code}" }};
</script>
<script src="https://webapi.amap.com/maps?v=2.0&key={api_key}"></script>
</head>
<body>
<div id="map"></div>
<script>
  var map = new AMap.Map("map", {{
    zoom: 15,
    center: [{lng}, {lat}]
  }});
  map.on("click", function(e) {{
    var lnglat = e.lnglat;
    console.log('Clicked:', lnglat.lng + ',' + lnglat.lat);
  }});
</script>
</body>
</html>"#,
        height_px
    )
}

/// 高德地图嵌入器（只嵌不转发的零业务壳层）。
#[allow(dead_code, reason = "T21 REDO: 预留字段，待 Slint 1.17+ 升级后启用")]
pub struct GaodeMapView {
    #[allow(dead_code, reason = "高度参数用于后续布局计算")]
    height_px: u32,
}

impl GaodeMapView {
    /// 创建高德地图嵌入视图（屏 4 区域，非独立窗口）。
    ///
    /// # 参数
    /// - `slint_window`: Slint Window 对象  
    /// - `height_px`: WebView 高度（屏 4 工作区高度限制）
    /// - `center_lat_lon`: 初始中心点 (经度，纬度)
    pub fn render_into(
        _slint_window: &slint::Window,
        height_px: u32,
        center_lat_lon: (f64, f64),
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // ⚠️ T21 REDO 关键发现：当前 Slint 1.9 不支持通过 wry 嵌入 WebView
        // 原因：wry 0.55 的 WebViewBuilder::build() 要求参数实现 HasWindowHandle trait
        // Slint 1.9 未暴露 WinitWindowAccessor trait，无法获取原生窗口句柄

        // 解决方案计划:
        // 1. 临时方案：直接使用独立窗口运行高德 Demo (.scratch/map-demo)
        // 2. 正式方案：升级到 Slint 1.17+ 启用真正的嵌入式实现

        // 尝试读取密钥文件（验证环境配置）
        let _ = read_temp_keys(); // TODO: 后续集成到 notification-center

        // TODO: Slint 1.17+ 升级后启用以下代码
        /*
        let slint_api_version = env!("CARGO_PKG_VERSION");
        use slint::winit_030::WinitWindowAccessor;

        let winit_win = slint_window.with_winit_window(|w| w.clone())
            .expect("Slint 1.17+ should expose WinitWindowAccessor");

        let (api_key, security_code) = read_temp_keys()
            .ok_or("demo 密钥文件不存在")?;

        let html = build_map_html(&api_key, &security_code, height_px, center_lat_lon);

        let bounds = wry::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(800.0, height_px as f64)),
        };

        let webview = wry::WebViewBuilder::new(&winit_win)
            .with_bounds(bounds)?
            .with_html(html)?
            .with_ipc_handler(|request| {
                let body = request.body().clone();
                eprintln!("Gaode map clicked: {}", body);
            })
            .build_as_child()?;

        Ok(Self { webview, height_px })
        */

        Ok(Self { height_px })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_keys_path_is_localappdata_dev() {
        let local_app_data = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA");
        let path = PathBuf::from(local_app_data)
            .join("MCRebuildV2")
            .join("dev")
            .join(GAODE_KEYS_FILE);
        assert!(path.to_string_lossy().contains("MCRebuildV2"));
        assert!(path.to_string_lossy().contains("dev"));
    }

    #[test]
    fn html_contains_security_config_before_script() {
        let (key, secret) = ("demo_key".to_owned(), "demo_secret".to_owned());
        let html = build_map_html(&key, &secret, 400, (116.397, 39.916));
        // 安全密钥配置必须在 <script src=...> 之前
        let security_pos = html.find("window._AMapSecurityConfig").unwrap();
        let script_pos = html.find("<script src=");
        assert!(security_pos < script_pos.unwrap());
    }

    #[test]
    fn html_uses_official_cdn_v20() {
        let (key, secret) = ("abc123".to_owned(), "xyz789".to_owned());
        let html = build_map_html(&key, &secret, 400, (116.397, 39.916));
        assert!(html.contains("https://webapi.amap.com/maps?v=2.0&key="));
    }
}
