//! 第五步 3D 方块预览页（T52）的 HTML 组装。
//!
//! 页面、three.js（MIT）与渲染脚本随 desktop-shell 打包：不联网加载资源、
//! 不引入 npm 构建步骤，也不使用 Mojang 版权的方块贴图（平色 + 按面明暗）。
//! 渲染数据由 F9 与导出同源生成，在页面构建时嵌入，或在生成完成后经
//! `window.loadPreviewData(...)` 推送。

/// 组装完整的预览页 HTML；`initial_payload` 为已生成的渲染 JSON（无则 null）。
pub(crate) fn build_page_html(initial_payload: Option<&str>) -> String {
    let template = include_str!("assets/page.html");
    let three_js = include_str!("assets/three.min.js");
    let viewer_js = include_str!("assets/viewer.js");
    let initial_payload = initial_payload.unwrap_or("null");
    template
        .replace("__THREE_JS__", three_js)
        .replace("__VIEWER_JS__", viewer_js)
        .replace("__INITIAL_PAYLOAD__", initial_payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_bundles_renderer_scripts_and_initial_payload() {
        let html = build_page_html(None);
        assert!(html.contains("preview-canvas"), "必须包含预览画布");
        assert!(html.contains("149"), "必须内嵌 pinned 版 three.js");
        assert!(
            html.contains("window.__previewPending = null;"),
            "无数据时初始负载为 null"
        );
        assert!(!html.contains("__THREE_JS__"), "占位符必须全部替换");
        assert!(!html.contains("__VIEWER_JS__"), "占位符必须全部替换");
        assert!(!html.contains("__INITIAL_PAYLOAD__"), "占位符必须全部替换");
    }

    #[test]
    fn page_embeds_payload_without_breaking_script_boundaries() {
        let payload = r#"{"v":1,"palette":["minecraft:air"],"bounds":[0,0,0,0,0,0],"count":0,"simplified":false,"runs":[]}"#;
        let html = build_page_html(Some(payload));
        let marker = "window.__previewPending = ";
        let position = html.find(marker).expect("初始负载注入点存在");
        let injected = &html[position + marker.len()..];
        assert!(injected.starts_with(payload), "负载必须原样注入首段脚本");
    }
}
