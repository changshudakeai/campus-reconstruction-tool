//! 评审地图页生成（T38）：地图为主区 + 左侧抽屉评审工作台。
//!
//! 本页是高德 JS API 的"评审画布"：
//! 1) 只发 `map_ready` 就绪信号，不发起任何网络业务查询（候选已在 Rust 侧）；
//! 2) 接收 Rust 下行绘制命令：
//!    - `drawReviewCandidates(candidatesJson)`：按候选三态标注（待定虚线 /
//!      保留实线 / 剔除隐藏），剔除候选在地图隐藏但卡片保留；
//!    - `locateReviewCandidate(candidateId)`：地图中心跳转到该候选并高亮；
//!    - `highlightReviewCandidate(candidateId)` / `clearReviewHighlight()`：
//!      地图↔卡片双向联动高亮（共用 F5 同一份高亮状态）。
//! 3) 点击地图对象经 IPC 上行 `review_object_clicked` → 高亮对应卡片。
//!
//! **红线**：
//! - 密钥只经 F1 校验注入，禁止硬编码真实 key；
//! - 坐标一律 GCJ-02（候选投影已在采集入口转换，JS 不再做坐标转换）；
//! - 地图内不渲染任何 HTML 工具栏按钮（薄壳 + 抽屉模式，ADR-0017/T34 同纪律）；
//! - 署名：OSM 数据 ODbL，页面保留 `© OpenStreetMap contributors`。

use crate::error::{Error, Result};

/// 官方 CDN URL 模板（v2.0；评审页不需要 PolygonEditor 等插件）
pub const REVIEW_MAP_CDN_URL_TEMPLATE: &str = "https://webapi.amap.com/maps?v=2.0&key={key}";

/// 评审地图页配置
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewMapPageConfig {
    /// 高德 Web API key
    pub api_key: String,
    /// 高德安全密钥 (securityJsCode)
    pub security_key: String,
    /// 校区锚点坐标 (GCJ-02；来自高德 POI)
    pub anchor_lon: f64,
    pub anchor_lat: f64,
    /// 地图容器高度 (像素)
    pub height_px: u32,
}

impl ReviewMapPageConfig {
    /// 新建配置（高度取最小值 300px）
    pub fn new(api_key: impl Into<String>, security_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            security_key: security_key.into(),
            anchor_lon: f64::NAN,
            anchor_lat: f64::NAN,
            height_px: 300,
        }
    }

    pub fn with_anchor(mut self, lon: f64, lat: f64) -> Self {
        self.anchor_lon = lon;
        self.anchor_lat = lat;
        self
    }

    pub fn effective_height_px(&self) -> u32 {
        self.height_px.max(300)
    }
}

/// 评审标注与定位的 JS 桥接脚本（不经过 format!，因此无需双花括号转义）。
const REVIEW_SCRIPT: &str = r#"
<script>
(function() {
  // 当前在图的候选对象与基准样式（用于高亮后还原）
  var reviewObjects = {};
  var reviewBaseStyle = {};
  var reviewCentroids = {};
  var reviewHighlightId = null;

  function postReviewClick(candidateId) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({
        type: 'review_object_clicked',
        candidate_id: candidateId
      }));
    }
  }

  // 多边形坐标兼容两种形态：平铺 [[lon,lat],...] 或带环 [[[lon,lat],...],...]
  function toPath(coords) {
    if (coords && coords.length > 0 && Array.isArray(coords[0]) &&
        Array.isArray(coords[0][0])) {
      return coords[0];
    }
    return coords;
  }

  function centroidOf(path) {
    if (!path || path.length === 0) return null;
    var sum = 0;
    var lon = 0;
    var lat = 0;
    for (var i = 0; i < path.length; i++) {
      lon += path[i][0];
      lat += path[i][1];
      sum += 1;
    }
    if (sum === 0) return null;
    return [lon / sum, lat / sum];
  }

  function clearReviewHighlightInner() {
    if (!reviewHighlightId) return;
    var id = reviewHighlightId;
    reviewHighlightId = null;
    var overlay = reviewObjects[id];
    var base = reviewBaseStyle[id];
    if (overlay && overlay.setOptions && base) {
      overlay.setOptions({ strokeColor: base.strokeColor, strokeWeight: base.strokeWeight });
    }
  }

  // 按候选三态全量重绘：待定=虚线、保留=实线、剔除=地图隐藏（卡片保留可改回）
  window.drawReviewCandidates = function(candidatesJson) {
    var candidates = [];
    try { candidates = JSON.parse(candidatesJson) || []; } catch (e) { candidates = []; }
    // 清除旧标注
    Object.keys(reviewObjects).forEach(function(id) {
      map.remove(reviewObjects[id]);
    });
    reviewObjects = {};
    reviewBaseStyle = {};
    reviewCentroids = {};
    clearReviewHighlightInner();

    candidates.forEach(function(c) {
      if (!c || c.state === 'remove') return; // 剔除候选在地图隐藏，卡片仍保留
      var baseColor = c.state === 'pending' ? '#95a5a6' : '#3498db';
      var dash = c.state === 'pending' ? [6, 4] : null;
      var overlay = null;
      var centroid = null;
      if (c.kind === 'point') {
        centroid = [c.coordinates[0], c.coordinates[1]];
        overlay = new AMap.CircleMarker({
          center: c.coordinates,
          radius: 8,
          strokeColor: baseColor,
          strokeWeight: 2,
          fillColor: baseColor,
          fillOpacity: 0.45,
          cursor: 'pointer',
          clickable: true
        });
      } else if (c.kind === 'line_string') {
        centroid = centroidOf(c.coordinates);
        overlay = new AMap.Polyline({
          path: c.coordinates,
          strokeColor: baseColor,
          strokeWeight: 3,
          lineDash: dash,
          cursor: 'pointer',
          clickable: true
        });
      } else {
        var path = toPath(c.coordinates);
        centroid = centroidOf(path);
        overlay = new AMap.Polygon({
          path: path,
          strokeColor: baseColor,
          strokeWeight: 3,
          lineDash: dash,
          fillColor: baseColor,
          fillOpacity: 0.25,
          cursor: 'pointer',
          clickable: true
        });
      }
      if (!overlay) return;
      overlay.on('click', function() { postReviewClick(c.candidate_id); });
      map.add(overlay);
      reviewObjects[c.candidate_id] = overlay;
      reviewBaseStyle[c.candidate_id] = { strokeColor: baseColor, strokeWeight: 3 };
      reviewCentroids[c.candidate_id] = centroid;
    });
  };

  // "定位到地图"：地图中心跳转到该候选并高亮
  window.locateReviewCandidate = function(candidateId) {
    var centroid = reviewCentroids[candidateId];
    if (!centroid) return;
    map.setZoomAndCenter(18, centroid);
    window.highlightReviewCandidate(candidateId);
  };

  // 高亮一个候选（地图对象 ↔ 卡片双向联动；卡片高亮也经此同步）
  window.highlightReviewCandidate = function(candidateId) {
    clearReviewHighlightInner();
    var overlay = reviewObjects[candidateId];
    if (!overlay) return;
    reviewHighlightId = candidateId;
    if (overlay.setOptions) {
      overlay.setOptions({ strokeColor: '#e74c3c', strokeWeight: 5 });
    }
  };

  window.clearReviewHighlight = function() {
    clearReviewHighlightInner();
  };

  window.initReviewMap = function() {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({ type: 'map_ready' }));
    }
  };
})();
</script>
"#;

/// 生成评审地图页 HTML
///
/// **功能清单**:
/// 1. 地图加载（高德 JS API；锚点 GCJ-02 直接作为中心）
/// 2. `map_ready` IPC：Rust 侧收到后回传候选标注（`drawReviewCandidates`）
/// 3. 候选标注：待定虚线 / 保留实线 / 剔除隐藏
/// 4. 定位跳转 + 双向高亮（`locateReviewCandidate` /
///    `highlightReviewCandidate` / `clearReviewHighlight`）
/// 5. 点击地图对象 → `review_object_clicked` IPC（高亮对应卡片）
pub fn build_review_map_page_html(config: &ReviewMapPageConfig) -> Result<String> {
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
    if !config.anchor_lon.is_finite() || !config.anchor_lat.is_finite() {
        return Err(Error::MalformedResponse(
            "校区锚点缺失或无效：必须提供真实校区坐标".to_owned(),
        ));
    }

    let cdn_url = REVIEW_MAP_CDN_URL_TEMPLATE.replace("{key}", &config.api_key);
    let height = config.effective_height_px();

    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>评审地图</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; overflow-x: hidden; }}
  #map-container {{ width: 100%; max-width: 100%; box-sizing: border-box; height: {height}px; min-height: 300px; }}
  #status-panel {{
    position: absolute; top: 10px; left: 10px; z-index: 1000;
    background: white; padding: 12px; border-radius: 6px;
    box-shadow: 0 2px 8px rgba(0,0,0,0.15); font-size: 13px; max-width: 320px;
  }}
  #status-panel.error {{ border-left: 4px solid #e74c3c; }}
  #status-panel.success {{ border-left: 4px solid #2ecc71; }}
  #osm-attribution {{
    position: absolute; bottom: 4px; left: 8px; z-index: 999;
    font-size: 10px; color: #666;
    background: rgba(255,255,255,0.72); padding: 2px 6px; border-radius: 3px;
  }}
</style>
<script>
  // T21: securityJsCode 必须在 AMap SDK 加载前注入
  window._AMapSecurityConfig = {{ securityJsCode: "{security_js_code}" }};

  window.onerror = function(msg, url, line) {{
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage({{"type":"error","message":msg}});
    }}
    return false;
  }};

  // SDK 超时检测 (5 秒)
  var sdkCheckTimer = setInterval(function() {{
    if (typeof AMap !== 'undefined') {{
      clearInterval(sdkCheckTimer);
      window.sdkLoaded = true;
    }}
  }}, 1000);
  setTimeout(function() {{
    if (!window.sdkLoaded && typeof AMap === 'undefined') {{
      clearInterval(sdkCheckTimer);
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage({{"type":"error","message":"SDK 加载超时"}});
      }}
    }}
  }}, 5000);
</script>
</head>
<body>
<div id="status-panel">正在初始化...</div>
<div id="map-container"></div>
<div id="osm-attribution">© OpenStreetMap contributors</div>
<script src="{cdn_url}"></script>
<script>
  var map;
  var anchorPoint = {{ lng: {anchor_lon}, lat: {anchor_lat} }};

  function initWithAnchor() {{
    try {{
      var container = document.getElementById('map-container');
      var viewportW = Math.max(document.documentElement.clientWidth || 1, 1);
      if (container) {{
        container.style.width = '100%';
        container.style.maxWidth = '100%';
        if (container.offsetWidth > viewportW) {{
          container.style.width = viewportW + 'px';
        }}
      }}
      map = new AMap.Map('map-container', {{
        zoom: 16,
        center: [anchorPoint.lng, anchorPoint.lat],
        viewMode: '3D'
      }});
      if (typeof map.resize === 'function') {{
        map.resize();
      }}
      window.addEventListener('resize', function() {{
        var c = document.getElementById('map-container');
        var vw = document.documentElement.clientWidth || 1;
        if (c) {{
          c.style.width = '100%';
          c.style.maxWidth = '100%';
          if (c.offsetWidth > vw) {{
            c.style.width = vw + 'px';
          }}
        }}
        if (map && typeof map.resize === 'function') {{
          map.resize();
        }}
      }});
      if (typeof window.initReviewMap === 'function') {{
        window.initReviewMap();
      }}
    }} catch (e) {{
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{ type: 'error', message: '评审地图初始化失败: ' + e.message }}));
      }}
    }}
  }}

  function boot() {{
    if (typeof AMap === 'undefined') {{
      var statusPanel = document.getElementById('status-panel');
      if (statusPanel) statusPanel.textContent = '高德 SDK 加载中...';
    }} else {{
      initWithAnchor();
    }}
  }}
  if (document.readyState === 'complete') {{
    boot();
  }} else {{
    window.addEventListener('load', boot);
  }}
</script>
{review_script}
</body>
</html>"#,
        height = height,
        cdn_url = cdn_url,
        security_js_code = config.security_key,
        anchor_lon = config.anchor_lon,
        anchor_lat = config.anchor_lat,
        review_script = REVIEW_SCRIPT,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ReviewMapPageConfig {
        ReviewMapPageConfig::new("abc123DEF456", "xyz789GHI012").with_anchor(116.4, 39.9)
    }

    #[test]
    fn html_exposes_review_annotation_and_locate_commands() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(html.contains("drawReviewCandidates"));
        assert!(html.contains("locateReviewCandidate"));
        assert!(html.contains("highlightReviewCandidate"));
        assert!(html.contains("clearReviewHighlight"));
        assert!(html.contains("review_object_clicked"));
        assert!(html.contains("map_ready"));
    }

    #[test]
    fn html_annotates_pending_dashed_keep_solid_and_hides_remove() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("c.state === 'remove'"),
            "剔除候选必须在地图隐藏（卡片保留可改回）"
        );
        assert!(
            html.contains("var dash = c.state === 'pending' ? [6, 4] : null"),
            "待定=虚线、保留=实线必须由状态决定"
        );
        assert!(html.contains("lineDash"), "折线/面标注必须支持虚线");
    }

    #[test]
    fn html_keeps_osm_attribution_and_no_toolbar() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("© OpenStreetMap contributors"),
            "候选数据 OSM ODbL，必须保留署名"
        );
        assert!(!html.contains("map-toolbar"));
        assert!(!html.contains("control-btn"));
        assert!(!html.contains("confirm-edit-btn"));
    }

    #[test]
    fn html_forbids_horizontal_overflow() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(html.contains("overflow-x: hidden"));
        assert!(html.contains("window.addEventListener('load', boot)"));
    }

    #[test]
    fn html_accepts_point_line_and_polygon_kinds() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(html.contains("c.kind === 'point'"));
        assert!(html.contains("c.kind === 'line_string'"));
        assert!(html.contains("new AMap.Polygon"));
        assert!(html.contains("new AMap.CircleMarker"));
        assert!(html.contains("new AMap.Polyline"));
    }

    #[test]
    fn invalid_keys_and_missing_anchor_are_rejected() {
        assert!(
            build_review_map_page_html(&ReviewMapPageConfig::new("bad@key", "xyz789")).is_err()
        );
        assert!(
            build_review_map_page_html(&ReviewMapPageConfig::new("abc123", "bad key!")).is_err()
        );
        assert!(build_review_map_page_html(&ReviewMapPageConfig::new("abc123", "xyz789")).is_err());
    }
}
