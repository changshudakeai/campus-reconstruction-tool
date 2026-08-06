//! WebView 地图页生成
//!
//! 高德地图 JS SDK 必须通过**官方 CDN** 加载（决策记忆：v2.0，
//! `https://webapi.amap.com/maps?v=2.0&key=...`，无内部镜像）；
//! 地图容器最小高度 **300px**（决策记忆：适用于所有嵌入式地图场景）。
//!
//! 本模块只生成静态 HTML 文本；由壳层 WebView 加载渲染。
//! - 校区搜索页：JS 侧通过 `window.ipc.postMessage(json)`（wry 标准桥）把地点
//!   搜索结果回传宿主，宿主再交 [`crate::parse_place_search_response`] 解析。
//! - 取点页：点击地图 → `window.ipc.postMessage(lng + "," + lat)`；错误检测脚本上报异常。
//!
//! T21/T23: 升级为 JS API 2.0 + securityJsCode（高德 2.0 强制要求）；新增取点页协议。
//! D-3: 搜索脚本携带请求序号回传信封（防止旧响应串台），location 三格式归一。
//!
//! ## 桥协议
//!
//! - `window.ipc.postMessage(msg)`: 唯一新通道，msg 格式：
//!   - 坐标：`"经度，纬度"` (字符串)
//!   - 错误：`{"type":"error","message":"..."}` (JSON)

use crate::error::{Error, Result};

/// 官方 CDN 地址模板（{key}处填入高德 Web API key，T21 起 v1.2→v2.0）
pub const GAODE_CDN_URL_TEMPLATE: &str =
    "https://webapi.amap.com/maps?v=2.0&key={key}&plugin=AMap.PlaceSearch";

/// 地图容器最小高度（像素；所有嵌入式地图场景硬约束）
pub const MAP_MIN_HEIGHT_PX: u32 = 300;

/// 地图页配置（T21: JS API 2.0 + securityJsCode）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPageConfig {
    /// 高德 Web API key（由部署配置注入，不入库不入 git）
    pub api_key: String,
    /// 高德安全密钥（T21: securityJsCode，用于 window._AMapSecurityConfig）
    pub security_key: String,
    /// 地图容器高度（像素）；低于 300 会被钳制到 300
    pub height_px: u32,
}

impl MapPageConfig {
    /// 用 API key + 安全密钥构造默认配置（高度取最小值 300px）
    pub fn new(api_key: impl Into<String>, security_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            security_key: security_key.into(),
            height_px: MAP_MIN_HEIGHT_PX,
        }
    }

    /// 生效高度：不低于 300px
    pub fn effective_height_px(&self) -> u32 {
        self.height_px.max(MAP_MIN_HEIGHT_PX)
    }
}

/// 生成校区搜索地图页 HTML（官方 CDN + PlaceSearch 插件 + 结果桥接）
///
/// API key 含引号、尖括号、空白等字符时拒绝（防注入；高德 key 是纯十六进制）。
/// T21: 同时注入 securityJsCode（必须在 SDK script 之前）。
pub fn build_map_page_html(config: &MapPageConfig) -> Result<String> {
    // 双重校验：API key + 安全密钥均为纯字母数字
    if config.api_key.is_empty() || !config.api_key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::MalformedResponse(
            "高德 API key 只能是字母或数字".to_owned(),
        ));
    }
    if config.security_key.is_empty()
        || !config
            .security_key
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
    {
        return Err(Error::MalformedResponse(
            "高德安全密钥只能是字母或数字".to_owned(),
        ));
    }
    let cdn_url = GAODE_CDN_URL_TEMPLATE.replace("{key}", &config.api_key);
    let height = config.effective_height_px();
    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>校区搜索</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; }}
  /* 决策记忆：地图最小高度 300px，自适应缩放不得低于该值 */
  #map-container {{ width: 100%; height: {height}px; min-height: {min_height}px; }}
</style>
<script>
  // T21: securityJsCode 必须在 AMap SDK 加载前注入（高德 2.0 强制要求）
  window._AMapSecurityConfig = {{ securityJsCode: "{security_js_code}" }};
</script>
</head>
<body>
<div id="map-container"></div>
<script src="{cdn_url}"></script>
<script>
  var map = new AMap.Map("map-container", {{ zoom: 15 }});
  // 错误检测：JS 异常经桥回传宿主，失败路径可见（D-3 排障）
  window.onerror = function(msg, url, line) {{
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'campus_search_error', message: msg + ' (line ' + line + ')' }}));
    }}
    return false;
  }};
  // location 三种格式归一为 "经度,纬度" 文本：REST 字符串 / JS API v2.0
  // 对象（lng/lat 两字段）/ 数组 [lng,lat]（D-1 同源兼容）。
  function locToText(loc) {{
    if (typeof loc === 'string') return loc;
    if (Array.isArray(loc)) return loc.join(',');
    if (loc && typeof loc === 'object' &&
        typeof loc.lng === 'number' && typeof loc.lat === 'number') {{
      return loc.lng + ',' + loc.lat;
    }}
    return '';
  }}
  // 学校名称关键词搜索：结果 JSON 经桥接回传宿主，由 Rust 侧筛选学校类目
  function searchCampus(requestId, keyword) {{
    try {{
      // JS API v2.0 插件必须经 AMap.plugin 就绪后再构造（采集脚本同款用法）
      AMap.plugin('AMap.PlaceSearch', function() {{
        var placeSearch = new AMap.PlaceSearch({{ city: "全国" }});
        placeSearch.search(keyword, function (status, result) {{
          var payload = {{ status: status === "complete" ? "1" : "0", info: status, pois: [] }};
          if (status === "complete" && result.poiList) {{
            payload.pois = result.poiList.pois.map(function (poi) {{
              return {{
                id: poi.id,
                name: poi.name,
                address: poi.address,
                location: locToText(poi.location),
                typecode: poi.typeCode || poi.typecode || poi.type_code || "",
                type: poi.type || ""
              }};
            }});
          }}
          window.ipc.postMessage(JSON.stringify({{
            type: 'campus_search_response',
            request_id: requestId,
            payload: JSON.stringify(payload)
          }}));
        }});
      }});
    }} catch (error) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'campus_search_error', message: String(error) }}));
    }}
  }}
  // 页面脚本就绪后回传握手，宿主等 ready 再求值搜索（避免加载竞态）
  window.ipc.postMessage(JSON.stringify({{ type: 'campus_search_ready' }}));
  // 选定校区后地图定位到坐标锚点（此后画边界直接从锚点开始，ADR-0008）
  function centerOn(longitude, latitude) {{
    map.setCenter([longitude, latitude]);
  }}
</script>
</body>
</html>
"#,
        height = height,
        min_height = MAP_MIN_HEIGHT_PX,
        cdn_url = cdn_url,
        security_js_code = config.security_key,
    ))
}

/// 生成取点地图页 HTML（点击地图 → window.ipc.postMessage("经度，纬度")）
///
/// API key 含引号、尖括号、空白等字符时拒绝；同时注入 securityJsCode 与错误检测脚本。
pub fn build_pick_point_page_html(config: &MapPageConfig) -> Result<String> {
    // 双重校验：API key + 安全密钥均为纯字母数字
    if config.api_key.is_empty() || !config.api_key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::MalformedResponse(
            "高德 API key 只能是字母或数字".to_owned(),
        ));
    }
    if config.security_key.is_empty()
        || !config
            .security_key
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
    {
        return Err(Error::MalformedResponse(
            "高德安全密钥只能是字母或数字".to_owned(),
        ));
    }
    let cdn_url = GAODE_CDN_URL_TEMPLATE.replace("{key}", &config.api_key);
    let height = config.effective_height_px();
    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>圈选边界</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; }}
  /* 决策记忆：地图最小高度 300px，自适应缩放不得低于该值 */
  #map-container {{ width: 100%; height: {height}px; min-height: {min_height}px; }}
</style>
<script>
  // T21: securityJsCode 必须在 AMap SDK 加载前注入（高德 2.0 强制要求）
  window._AMapSecurityConfig = {{ securityJsCode: "{security_js_code}" }};
  
  // T23: 错误检测脚本：window.onerror → ipc 回传结构化错误 JSON
  var errorMessage = "";
  window.onerror = function(msg, url, line) {{
    errorMessage = msg + " (line " + line + ")";
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage({{"type":"error","message":msg}});
    }}
    return false;
  }};
  
  // T23: SDK 加载超时心跳（5 秒无 AMap 对象 → 回传超时错误）
  var sdkCheckTimer = setInterval(function() {{
    if (typeof AMap === 'undefined') {{
      // 仍无 AMap 对象，继续尝试
    }} else {{
      // AMap 已加载，清除定时器
      clearInterval(sdkCheckTimer);
      window.sdkLoaded = true;
    }}
  }}, 1000);
  setTimeout(function() {{
    if (!window.sdkLoaded) {{
      clearInterval(sdkCheckTimer);
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage({{"type":"error","message":"SDK 加载超时"}});
      }}
    }}
  }}, 5000);
</script>
</head>
<body>
<div id="map-container"></div>
<script src="{cdn_url}"></script>
<script>
  var map = new AMap.Map("map-container", {{ zoom: 15 }});
  
  // T23: 点击地图 → window.ipc.postMessage("经度，纬度")
  map.on('click', function(e) {{
    var location = e.lnglat;
    var msg = location.lng + "," + location.lat;
    window.ipc.postMessage(msg);
  }});
  
  // 选定校区后地图定位到坐标锚点（此后画边界直接从锚点开始，ADR-0008）
  function centerOn(longitude, latitude) {{
    map.setCenter([longitude, latitude]);
  }}
</script>
</body>
</html>
"#,
        height = height,
        min_height = MAP_MIN_HEIGHT_PX,
        cdn_url = cdn_url,
        security_js_code = config.security_key,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_uses_official_cdn_v20() {
        let html =
            build_map_page_html(&MapPageConfig::new("abc123DEF456", "xyz789GHI012")).unwrap();
        assert!(html.contains("https://webapi.amap.com/maps?v=2.0&key=abc123DEF456"));
        assert!(html.contains("AMap.PlaceSearch"));
    }

    #[test]
    fn map_height_never_below_300px() {
        let mut config = MapPageConfig::new("abc123", "xyz789");
        config.height_px = 120;
        assert_eq!(config.effective_height_px(), 300);
        let html = build_map_page_html(&config).unwrap();
        assert!(html.contains("height: 300px"));
        assert!(html.contains("min-height: 300px"));

        config.height_px = 720;
        assert_eq!(config.effective_height_px(), 720);
        let html = build_map_page_html(&config).unwrap();
        assert!(html.contains("height: 720px"));
    }

    #[test]
    fn security_key_is_injected_before_sdk_script() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_map_page_html(&config).unwrap();

        // Find positions of security config and SDK script
        let security_pos = html.find("window._AMapSecurityConfig").unwrap();
        let sdk_script_pos = html.find("<script src=");

        // Security config must appear before SDK script
        assert!(sdk_script_pos.is_some());
        assert!(security_pos < sdk_script_pos.unwrap());

        // Verify exact content
        assert!(html.contains("window._AMapSecurityConfig = { securityJsCode: \"xyz789GHI012\" }"));
    }

    #[test]
    fn search_campus_posts_envelope_with_request_id_via_wry_bridge() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_map_page_html(&config).unwrap();

        // D-3：搜索经 wry 标准桥回传带请求序号的信封，宿主按序号配对，
        // 旧响应不得串台；不得再依赖未注入的 mcrebuildBridge。
        assert!(html.contains("function searchCampus(requestId, keyword)"));
        assert!(html.contains("campus_search_response"));
        assert!(html.contains("request_id: requestId"));
        assert!(html.contains("window.ipc.postMessage"));
        assert!(!html.contains("mcrebuildBridge"));
    }

    #[test]
    fn search_page_handshakes_ready_and_reports_errors() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_map_page_html(&config).unwrap();

        // 页面就绪握手：宿主等 campus_search_ready 再求值，避免加载竞态
        assert!(html.contains("campus_search_ready"));
        // 错误回传：异常经桥上报，失败路径可见
        assert!(html.contains("window.onerror"));
        assert!(html.contains("campus_search_error"));
        // v2.0 插件就绪用法：AMap.plugin 后才构造 PlaceSearch
        assert!(html.contains("AMap.plugin('AMap.PlaceSearch'"));
    }

    #[test]
    fn search_page_normalizes_all_location_formats() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_map_page_html(&config).unwrap();

        // D-1/D-3：location 对象/数组/文本统一归一为 "经度,纬度"。
        assert!(html.contains("function locToText(loc)"));
        assert!(html.contains("Array.isArray(loc)"));
        assert!(html.contains("loc.lng + ',' + loc.lat"));
        assert!(html.contains("location: locToText(poi.location)"));
    }

    #[test]
    fn suspicious_security_key_is_rejected() {
        // API key valid, security key bad
        let config = MapPageConfig::new("abc123", "key\"onload=");
        assert!(build_map_page_html(&config).is_err());

        // Both bad
        let config = MapPageConfig::new("bad@key", "bad#key");
        assert!(build_map_page_html(&config).is_err());

        // Empty security key rejected
        let config = MapPageConfig::new("abc123", "");
        assert!(build_map_page_html(&config).is_err());
    }

    #[test]
    fn pick_point_page_has_ipc_postmessage_protocol() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_pick_point_page_html(&config).unwrap();

        // 验证 IPC 协议存在
        assert!(html.contains("window.ipc.postMessage"));
        assert!(html.contains("location.lng + \",\" + location.lat"));
    }

    #[test]
    fn pick_point_page_has_error_detection_scripts() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_pick_point_page_html(&config).unwrap();

        // window.onerror
        assert!(html.contains("window.onerror"));
        assert!(html.contains("\"type\":\"error\""));

        // SDK 超时检测
        assert!(html.contains("sdkCheckTimer"));
        assert!(html.contains("SDK 加载超时"));

        // 安全密钥注入
        assert!(html.contains("window._AMapSecurityConfig = { securityJsCode: \"xyz789GHI012\" }"));
    }

    #[test]
    fn pick_point_page_centeron_capability_preserved() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_pick_point_page_html(&config).unwrap();

        // centerOn 能力存在（ADR-0008）
        assert!(html.contains("function centerOn(longitude, latitude)"));
        assert!(html.contains("map.setCenter([longitude, latitude])"));
    }

    #[test]
    fn pick_point_page_title_is_chinese() {
        let config = MapPageConfig::new("abc123DEF456", "xyz789GHI012");
        let html = build_pick_point_page_html(&config).unwrap();

        assert!(html.contains("<title>圈选边界</title>"));
    }
}
