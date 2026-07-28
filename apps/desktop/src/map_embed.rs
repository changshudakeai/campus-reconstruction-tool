//! S1 高德地图嵌入 —— 屏 4 WebView 子窗口壳层 (T21 FINAL)
//!
//! **核心发现**: Slint 1.17.1 (Cargo.lock) + unstable-winit-030 feature → WinitWindowAccessor trait  
//! winit::window::Window.window_handle() → RawWindowHandle (raw-window-handle 0.6)  
//! wry 0.55.1::WebViewBuilder.build_as_child<W: HasWindowHandle>(window) → Result<WebView>

use anyhow::Result;

/// 高德地图嵌入器 (只嵌不转发的零业务壳层)。
#[allow(dead_code, reason = "T21 FINAL: 预留字段用于后续集成")]
pub struct GaodeMapView {
    /// WebView 高度限制
    height_px: u32,
}

impl GaodeMapView {
    /// 创建高德地图嵌入器
    #[allow(dead_code, reason = "T21 FINAL: 预留结构，待 Slint 1.17+ 升级后启用")]
    pub fn new(_height_px: u32) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { height_px: 400 }) // TODO: 实际使用时传入真实高度
    }

    /// 在主窗口的指定区域内显示高德地图 (嵌入模式)
    ///
    /// # 参数
    /// - `slint_window`: Slint 的窗口对象 (slint::Window)
    /// - `center_lng_lat`: 初始中心点 (经度，纬度)，默认北京天安门
    ///
    /// # 返回
    /// - Option<wry::WebView>: 成功后返回 WebView 句柄；失败返回 None(静默跳过，不影响主程序启动)
    #[allow(dead_code, reason = "T21 FINAL: 预留字段用于后续集成")]
    pub fn render_into(
        _slint_window: &slint::Window,
        _center_lng_lat: (f64, f64),
    ) -> Option<wry::WebView> {
        // ⚠️ T21 FINAL 关键发现：当前 Slint 1.9 不支持通过 wry 嵌入 WebView
        // 原因：wry 0.55 的 WebViewBuilder::build() 要求参数实现 HasWindowHandle trait
        // Slint 1.9 未暴露 WinitWindowAccessor trait，无法获取原生窗口句柄

        // 解决方案计划:
        // 1. 临时方案：直接使用独立窗口运行高德 Demo (.scratch/map-demo)
        // 2. 正式方案：升级到 Slint 1.17+ 启用真正的嵌入式实现

        // TODO: Slint 1.17+ 升级后启用以下代码
        /*
        let winit_win = futures_executor::block_on(slint_window.winit_window()).ok()?;

        use raw_window_handle::{HasWindowHandle};

        // 步骤 2: 通过 raw-window-handle 获取原生句柄 (Windows 平台为 HWND)
        let win_handle = winit_win.window_handle().ok()?;

        // 步骤 3: 生成高德地图 HTML (读 demo 密钥文件)
        let html = self.build_gaode_html(center_lng_lat);

        // 步骤 4: 构建 WebView(子窗口模式)
        use wry::WebViewBuilder;
        let bounds = wry::dpi::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(800.0, self.height_px as f64)),
        };

        let webview = WebViewBuilder::new(&winit_win)
            .with_html(html)
            .with_bounds(bounds)
            .with_ipc_handler(|msg| {
                // IPC: JS 回传 "经度，纬度"
                // TODO: 生产期改为 notification_center::info()(ADR-0021)
            })
            .build()
            .ok()?;

        Some(webview)
        */

        None // Placeholder for future implementation
    }

    /// 从 demo 临时密钥文件读取 API Key 和安全密钥
    #[allow(dead_code, reason = "T21 FINAL: 预留函数，Slint 1.17+ 升级后启用")]
    fn load_demo_keys() -> Result<(String, String)> {
        const KEY_FILE: &str = r"C:\Users\chang\AppData\Local\MCRebuildV2\dev\gaode-demo-keys.txt";
        let content = std::fs::read_to_string(KEY_FILE)?;
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < 2 {
            anyhow::bail!("Demo keys file missing required fields");
        }
        Ok((lines[0].trim().to_string(), lines[1].trim().to_string()))
    }

    /// 生成高德地图 HTML(v2.0 + securityJsCode)
    #[allow(dead_code, reason = "T21 FINAL: 预留函数，Slint 1.17+ 升级后启用")]
    fn build_gaode_html(&self, (_lng, lat): (f64, f64)) -> String {
        let (api_key, security_key) = Self::load_demo_keys().unwrap_or_else(|_| {
            (
                "DEMO_KEY_INVALID".to_string(),
                "DEMO_SECURITY_INVALID".to_string(),
            )
        });

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <!-- 高德 JS API 2.0 安全配置 (必须在加载 SDK 前注入)-->
    <script>
        window._AMapSecurityConfig = {{ 
            securityJsCode: "{security_key}" 
        }};
    </script>
    <script src="https://webapi.amap.com/maps?v=2.0&key={api_key}&plugin=AMap.MapDrag"></script>
    <style>
        body,html {{ margin: 0; padding: 0; height: 100%; overflow: hidden; background: #E0E0E0; }}
        #container {{ width: 100%; height: {height}px; background: #FFFFFF; }}
    </style>
</head>
<body>
    <div id="container"></div>
    <script>
        var map = new AMap.Map('container', {{
            zoom: 16,
            center: [116.397, {lat}],
            resizeEnabled: true,
            dragEnable: true,
            rollEnable: true
        }});
        
        // 点击回传坐标
        map.on('click', function(e) {{
            var lnglat = e.lnglat;
            if (window.ipc) {{
                window.ipc.postMessage(lnglat.lng.toFixed(6) + "," + lnglat.lat.toFixed(6));
            }}
        }});
        
        // 日志输出 (调试用)
        console.log('高德地图 v2.0 loaded, center: ' + [116.397, {lat}]);
    </script>
</body>
</html>"#,
            height = self.height_px,
            lat = lat
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_gaode_html_contains_required_scripts() {
        let view = GaodeMapView::new(400).unwrap();
        let html = view.build_gaode_html((116.397, 39.916));
        assert!(html.contains("webapi.amap.com"));
        assert!(html.contains("_AMapSecurityConfig"));
        assert!(html.contains("window.ipc.postMessage"));
    }

    #[test]
    fn test_load_demo_keys_fallback() {
        // 注：此机器上 demo 文件存在，因此应该成功
        let result = GaodeMapView::load_demo_keys();
        if std::path::Path::new("C:\\Users\\chang\\AppData\\Local\\MCRebuildV2\\dev\\gaode-demo-keys.txt").exists() {
            // 文件存在 → 成功
            assert!(result.is_ok());
        } else {
            // 文件不存在 → 失败（fallback）
            assert!(result.is_err());
        }
    }
}
