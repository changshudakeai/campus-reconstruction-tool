//! 第五步 3D 方块预览页（T52）的 HTML 组装。
//!
//! 页面、three.js（MIT）、分块 Worker、社区纹理图集（Pixel Perfection
//! Legacy，CC BY 4.0，见 `assets/textures/THIRD_PARTY_NOTICES.md`）与渲染
//! 脚本随 desktop-shell 打包：不联网加载资源、不引入 npm 构建步骤。
//! 渲染数据由 F9 与导出同源生成，在页面构建时嵌入，或在生成完成后经
//! `window.loadPreviewData(...)` 推送。

/// 组装完整的预览页 HTML；`initial_payload` 为已生成的渲染 JSON（无则 null）。
pub(crate) fn build_page_html(initial_payload: Option<&str>) -> String {
    let template = include_str!("assets/page.html");
    let three_js = include_str!("assets/three.min.js");
    let viewer_js = include_str!("assets/viewer.js");
    let worker_js = include_str!("assets/worker.js");
    let texture_map = include_str!("assets/textures/texture_map.json");
    let atlas = include_bytes!("assets/textures/atlas.png");
    let initial_payload = initial_payload.unwrap_or("null");
    let worker_js_literal =
        serde_json::to_string(worker_js).expect("worker.js 可序列化为 JS 字符串字面量");
    template
        .replace("__THREE_JS__", three_js)
        .replace("__VIEWER_JS__", viewer_js)
        .replace("__WORKER_JS__", &worker_js_literal)
        .replace("__TEXTURE_MAP__", texture_map)
        .replace("__ATLAS_BASE64__", &base64_encode(atlas))
        .replace("__INITIAL_PAYLOAD__", initial_payload)
}

/// RFC 4648 base64（无依赖实现；仅用于内嵌 10KB 级纹理图集，不引入 crate）。
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
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
        assert!(
            html.contains("window.__PREVIEW_WORKER_SRC__ = \"/*"),
            "必须内嵌分块 Worker 源码"
        );
        assert!(
            html.contains("window.__PREVIEW_ATLAS_DATA__ = \"data:image/png;base64,iVBOR"),
            "必须内嵌纹理图集"
        );
        assert!(
            html.contains("\"minecraft:stone_bricks\""),
            "必须内嵌方块面纹理映射"
        );
        assert!(!html.contains("__THREE_JS__"), "占位符必须全部替换");
        assert!(!html.contains("__VIEWER_JS__"), "占位符必须全部替换");
        assert!(!html.contains("__WORKER_JS__"), "占位符必须全部替换");
        assert!(!html.contains("__TEXTURE_MAP__"), "占位符必须全部替换");
        assert!(!html.contains("__ATLAS_BASE64__"), "占位符必须全部替换");
        assert!(!html.contains("__INITIAL_PAYLOAD__"), "占位符必须全部替换");
    }

    #[test]
    fn base64_encoder_matches_rfc4648_examples() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn page_embeds_payload_without_breaking_script_boundaries() {
        let payload = r#"{"v":2,"palette":["minecraft:air"],"bounds":[0,0,0,0,0,0],"count":0,"runs":[],"features":[]}"#;
        let html = build_page_html(Some(payload));
        let marker = "window.__previewPending = ";
        let position = html.find(marker).expect("初始负载注入点存在");
        let injected = &html[position + marker.len()..];
        assert!(injected.starts_with(payload), "负载必须原样注入首段脚本");
    }
}
