//! 评审地图页生成（T38）：地图为主区 + 左侧抽屉评审工作台。
//!
//! 本页是高德 JS API 的"评审画布"：
//! 1) 只发 `map_ready` 就绪信号，不发起任何网络业务查询（候选已在 Rust 侧）；
//! 2) 接收 Rust 下行绘制命令：
//!    - `setReviewCandidates(candidates)`：全量候选标注（T39：进入评审/
//!      map_ready 后只推一次；JS 缓冲 + 定时分批上屏，避免一次创建上千
//!      多边形过载）；剔除候选在地图隐藏但卡片保留；
//!    - `updateReviewCandidate(candidate)`：单候选增量更新（T39：之后
//!      state/highlight/locate 只推对应候选，不再 clear + 全量重推）；
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
use crate::MapViewport;

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
    /// “显示地图文字”开关文案（由壳层经 B6 l10n 注入，禁止在此硬编码）。
    pub map_text_toggle_label: String,
    /// 评审会话内地图文字是否可见（默认 false：隐藏易遮挡轮廓的地标/POI 文字）。
    pub map_text_visible: bool,
    /// ADR-0045：评审页单独记住的位置与缩放。
    pub initial_viewport: Option<MapViewport>,
}

impl ReviewMapPageConfig {
    /// 新建配置（地图容器高度随 WebView 视口填满，T37 同纪律）。
    pub fn new(api_key: impl Into<String>, security_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            security_key: security_key.into(),
            anchor_lon: f64::NAN,
            anchor_lat: f64::NAN,
            map_text_toggle_label: String::new(),
            map_text_visible: false,
            initial_viewport: None,
        }
    }

    pub fn with_anchor(mut self, lon: f64, lat: f64) -> Self {
        self.anchor_lon = lon;
        self.anchor_lat = lat;
        self
    }

    /// 设置地图文字开关文案与当前会话内的文字可见状态。
    pub fn with_map_text_toggle(mut self, label: impl Into<String>, visible: bool) -> Self {
        self.map_text_toggle_label = label.into();
        self.map_text_visible = visible;
        self
    }

    pub fn with_initial_viewport(mut self, viewport: MapViewport) -> Self {
        self.initial_viewport = Some(viewport);
        self
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
  // 最近一次全量/增量推送的候选载荷（含剔除态），用于定位时按需补绘目标。
  var reviewCandidateData = {};
  var reviewHighlightId = null;
  var pendingHighlightId = null;
  var pendingLocateId = null;
  var pendingCandidates = [];
  var drawTimer = null;
  // 评审模式默认隐藏地标/POI 文字；初始状态由 Rust 侧经全局注入（会话内保持），
  // 用户可经右下角开关恢复。
  var reviewMapTextVisible = !!(window.__reviewMapTextVisible);

  // 只上报安全阶段码：候选 ID、坐标与原始异常详情不得进入 IPC/用户弹窗。
  function postReviewMapFailure(stage) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({
        type: 'error',
        message: 'review_map_draw_failed:' + stage
      }));
    }
  }

  function postReviewLocateFailure(reason) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({
        type: 'error',
        message: 'review_map_locate_' + reason
      }));
    }
  }

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

  function locateDrawnReviewCandidate(candidateId) {
    var centroid = reviewCentroids[candidateId];
    if (!centroid) return false;
    try {
      map.setZoomAndCenter(18, centroid);
      window.highlightReviewCandidate(candidateId);
      return true;
    } catch (e) {
      postReviewMapFailure('locate');
      return false;
    }
  }

  // 隐藏/恢复高德地图文字（地标/POI 标签）。隐藏时只保留底图/道路/建筑，
  // 避免文字压盖评审轮廓；恢复时重新启用 point 标签。
  function applyReviewMapText() {
    if (!map || typeof map.setFeatures !== 'function') return;
    if (reviewMapTextVisible) {
      map.setFeatures(['bg', 'point', 'road', 'building']);
    } else {
      map.setFeatures(['bg', 'road', 'building']);
    }
  }

  window.setReviewMapText = function(visible) {
    reviewMapTextVisible = !!visible;
    applyReviewMapText();
  };

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

  function clearReviewOverlaysInner() {
    if (drawTimer) { clearInterval(drawTimer); drawTimer = null; }
    pendingCandidates = [];
    pendingHighlightId = null;
    Object.keys(reviewObjects).forEach(function(id) {
      map.remove(reviewObjects[id]);
    });
    reviewObjects = {};
    reviewBaseStyle = {};
    reviewCentroids = {};
    reviewCandidateData = {};
    // pendingLocateId 跨重绘保留：若新可见集合仍包含该候选，绘制后自动定位；
    // 若已不在新集合，下一批绘制结束时会被清理，避免残留。
    clearReviewHighlightInner();
  }

  // 分批绘制（每批 50、间隔 150ms）：一次同步创建上千个 AMap 多边形会
  // 触发 WebView2/GPU 过载卡顿甚至崩溃；当前可见集合有明确上限，分批只
  // 是让 WebView2 有喘息窗口，标注逐个上屏。
  function startChunkedDrawing() {
    if (drawTimer) { clearInterval(drawTimer); drawTimer = null; }
    drawTimer = setInterval(function() {
      var count = 0;
      var locatedThisTick = false;
      while (pendingCandidates.length > 0 && count < 50) {
        var c = pendingCandidates.shift();
        if (c) addReviewCandidateInner(c);
        count += 1;
      }
      if (pendingHighlightId && reviewObjects[pendingHighlightId]) {
        window.highlightReviewCandidate(pendingHighlightId);
        pendingHighlightId = null;
      }
      if (pendingLocateId) {
        if (reviewObjects[pendingLocateId]) {
          locatedThisTick = locateDrawnReviewCandidate(pendingLocateId);
          pendingLocateId = null;
        } else if (pendingCandidates.length === 0 &&
                   reviewCandidateData[pendingLocateId] &&
                   reviewCandidateData[pendingLocateId].state === 'remove') {
          // ADR-0016：剔除候选始终隐藏；定位请求给出明确反馈，不得补绘。
          postReviewLocateFailure('hidden');
          pendingLocateId = null;
        } else if (pendingCandidates.length === 0) {
          postReviewLocateFailure('unavailable');
          pendingLocateId = null;
        }
      }
      if (pendingCandidates.length === 0) {
        clearInterval(drawTimer);
        drawTimer = null;
        // 全部候选上屏后自动框住候选范围，避免地图仍停留在初始化锚点
        // 而候选在其视野之外（看起来像"没有画出来"）。
        if (!locatedThisTick && !window.__reviewPreserveInitialViewport &&
            map && typeof map.setFitView === 'function') {
          try { map.setFitView(); } catch (e) { postReviewMapFailure('fit_view'); }
        }
      }
    }, 150);
  }

  // 全量候选（可见集合；缓冲 + 分批绘制）
  window.setReviewCandidates = function(candidates) {
    if (!Array.isArray(candidates)) {
      postReviewMapFailure('payload_validation');
      return;
    }
    clearReviewOverlaysInner();
    candidates.forEach(function(c) {
      if (c && c.candidate_id) {
        reviewCandidateData[c.candidate_id] = c;
        pendingCandidates.push(c);
      }
    });
    startChunkedDrawing();
  };

  // 单个候选标注：待定=虚线、保留=实线、剔除始终隐藏（卡片保留可改回）。
  function addReviewCandidateInner(c) {
    var stage = 'payload_validation';
    try {
      if (!c || !c.candidate_id) {
        postReviewMapFailure(stage);
        return;
      }
      if (c.state === 'remove') return;
      var baseColor = c.state === 'pending' ? '#95a5a6' : '#3498db';
      var dash = c.state === 'pending' ? [6, 4] : null;
      var overlay = null;
      var centroid = null;
      stage = 'centroid_build';
      if (c.kind === 'point') {
        centroid = [c.coordinates[0], c.coordinates[1]];
        stage = 'overlay_construct';
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
        stage = 'overlay_construct';
        overlay = new AMap.Polyline({
          path: c.coordinates,
          strokeColor: baseColor,
          strokeWeight: 3,
          strokeStyle: dash ? 'dashed' : 'solid',
          strokeDasharray: dash || [],
          cursor: 'pointer',
          clickable: true
        });
      } else {
        var path = toPath(c.coordinates);
        centroid = centroidOf(path);
        stage = 'overlay_construct';
        overlay = new AMap.Polygon({
          path: path,
          strokeColor: baseColor,
          strokeWeight: 3,
          strokeStyle: dash ? 'dashed' : 'solid',
          strokeDasharray: dash || [],
          fillColor: baseColor,
          fillOpacity: 0.25,
          cursor: 'pointer',
          clickable: true
        });
      }
      if (!overlay) {
        postReviewMapFailure('overlay_construct');
        return;
      }
      stage = 'overlay_bind';
      overlay.on('click', function() {
        // 点击地图对象：JS 自高亮（双向联动的地图侧），Rust 只同步卡片与详情，
        // 不在 WebView2 IPC 回调栈内回推 evaluate_script（避免 COM 通道时序竞争）。
        window.highlightReviewCandidate(c.candidate_id);
        postReviewClick(c.candidate_id);
      });
      stage = 'map_add';
      map.add(overlay);
      reviewObjects[c.candidate_id] = overlay;
      reviewBaseStyle[c.candidate_id] = { strokeColor: baseColor, strokeWeight: 3 };
      stage = 'centroid_index';
      reviewCentroids[c.candidate_id] = centroid;
    } catch (e) {
      // 单候选失败不拖垮整批，但必须经 IPC 进入 B7；只发送安全阶段码。
      postReviewMapFailure(stage);
    }
  }

  // 清空全部标注（用于重绘前；分批添加见 addReviewCandidate）
  window.clearReviewOverlays = function() {
    clearReviewOverlaysInner();
  };

  // 单候选标注（Rust 分批推送小载荷；入缓冲由 JS 定时分批上屏）
  window.addReviewCandidate = function(c) {
    if (!c || typeof c !== 'object') {
      postReviewMapFailure('payload_validation');
      return;
    }
    if (c && c.candidate_id) {
      reviewCandidateData[c.candidate_id] = c;
      pendingCandidates.push(c);
      startChunkedDrawing();
    }
  };

  // T39：单候选增量更新（state 变更只推对应候选）——剔除=地图隐藏、
  // 改回保留/待定=按状态改样式；已在图只改样式，不在图按入缓冲上屏。
  window.updateReviewCandidate = function(c) {
    if (!c || !c.candidate_id) {
      postReviewMapFailure('payload_validation');
      return;
    }
    var stage = 'candidate_update';
    try {
    reviewCandidateData[c.candidate_id] = c;
    var existing = reviewObjects[c.candidate_id];
    var queuedIndex = pendingCandidates.findIndex(function(candidate) {
      return candidate && candidate.candidate_id === c.candidate_id;
    });
    if (!existing && queuedIndex >= 0) {
      // 全量候选尚在绘制队列时，增量状态直接替换队列项；不得提前绘制后
      // 又被旧队列项重复绘制。
      pendingCandidates[queuedIndex] = c;
      return;
    }
    if (c.state === 'remove') {
      if (existing) {
        map.remove(existing);
        delete reviewObjects[c.candidate_id];
        delete reviewBaseStyle[c.candidate_id];
        delete reviewCentroids[c.candidate_id];
      }
      if (reviewHighlightId === c.candidate_id) {
        reviewHighlightId = null;
      }
      return;
    }
    var baseColor = c.state === 'pending' ? '#95a5a6' : '#3498db';
    var dash = c.state === 'pending' ? [6, 4] : null;
    if (existing && existing.setOptions) {
      existing.setOptions({
        strokeColor: baseColor,
        strokeWeight: 3,
        strokeStyle: dash ? 'dashed' : 'solid',
        strokeDasharray: dash || [],
        fillColor: baseColor
      });
      reviewBaseStyle[c.candidate_id] = { strokeColor: baseColor, strokeWeight: 3 };
      if (reviewHighlightId === c.candidate_id) {
        existing.setOptions({ strokeColor: '#e74c3c', strokeWeight: 5 });
      }
      return;
    }
    addReviewCandidateInner(c);
    } catch (e) {
      postReviewMapFailure(stage);
    }
  };

  // 按候选三态全量重绘（兼容入口：缓冲 + 分批绘制）
  window.drawReviewCandidates = function(candidates) {
    window.setReviewCandidates(candidates);
  };

  // "定位到地图"：地图中心跳转到该候选并高亮。目标尚未绘制完成时进入
  // pending 队列，绘制后自动执行；剔除候选保持隐藏并给出明确反馈。
  window.locateReviewCandidate = function(candidateId) {
    var centroid = reviewCentroids[candidateId];
    if (!centroid) {
      var candidate = reviewCandidateData[candidateId];
      if (candidate && candidate.state === 'remove') {
        postReviewLocateFailure('hidden');
        return;
      }
      if (!candidate) {
        postReviewLocateFailure('unavailable');
        return;
      }
      pendingLocateId = candidateId;
      return;
    }
    locateDrawnReviewCandidate(candidateId);
  };

  // 高亮一个候选（地图对象 ↔ 卡片双向联动；卡片高亮也经此同步）
  window.highlightReviewCandidate = function(candidateId) {
    clearReviewHighlightInner();
    var overlay = reviewObjects[candidateId];
    if (!overlay) {
      // 候选尚未分批绘制完成：暂存，待其上屏后补高亮
      pendingHighlightId = candidateId;
      return;
    }
    reviewHighlightId = candidateId;
    if (overlay.setOptions) {
      overlay.setOptions({ strokeColor: '#e74c3c', strokeWeight: 5 });
    }
  };

  window.clearReviewHighlight = function() {
    clearReviewHighlightInner();
  };

  function bindMapTextToggle() {
    var checkbox = document.getElementById('map-text-toggle-checkbox');
    if (!checkbox) return;
    checkbox.checked = reviewMapTextVisible;
    checkbox.addEventListener('change', function() {
      reviewMapTextVisible = checkbox.checked;
      applyReviewMapText();
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({
          type: 'review_map_text_toggled',
          visible: reviewMapTextVisible
        }));
      }
    });
  }

  window.initReviewMap = function() {
    var statusPanel = document.getElementById('status-panel');
    if (statusPanel) statusPanel.style.display = 'none';
    // 评审模式默认隐藏地标/POI 文字；地图创建后按会话状态应用一次。
    applyReviewMapText();
    // T39：候选不再内嵌 HTML——Rust 侧收到 map_ready 后在事件循环安全
    // 上下文分批推送 setReviewCandidates（不在 WebView2 IPC 回调栈内
    // evaluate_script，T35/T38 同纪律），JS 缓冲 + 定时分批上屏。
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({ type: 'map_ready' }));
    }
  };

  bindMapTextToggle();
})();
</script>
"#;

/// 生成评审地图页 HTML
///
/// **功能清单**:
/// 1. 地图加载（高德 JS API；锚点 GCJ-02 直接作为中心；容器高度随视口填满）
/// 2. `map_ready` IPC：Rust 侧收到后分批回传候选标注（`setReviewCandidates`）
/// 3. 候选标注：待定虚线 / 保留实线 / 剔除隐藏
/// 4. 增量更新：`updateReviewCandidate`（state 变更只推对应候选）
/// 5. 定位跳转 + 双向高亮（`locateReviewCandidate` /
///    `highlightReviewCandidate` / `clearReviewHighlight`）
/// 6. 点击地图对象 → `review_object_clicked` IPC（高亮对应卡片）
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
    let initial_viewport_json =
        serde_json::to_string(&config.initial_viewport).unwrap_or_else(|_| "null".to_string());

    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>评审地图</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; overflow-x: hidden; }}
  /* T39：评审地图容器应用 T37 高度填满（html/body 100% + innerHeight）——
     WebView 槽位已按 Slint 布局填满窗口，固定高度会在下方留白。 */
  #map-container {{ width: 100%; max-width: 100%; box-sizing: border-box; height: 100%; min-height: 300px; }}
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
  #map-text-toggle {{
    position: absolute; right: 10px; bottom: 10px; z-index: 1001;
    background: rgba(255,255,255,0.92); padding: 5px 8px; border-radius: 4px;
    font-size: 12px; color: #333; box-shadow: 0 2px 6px rgba(0,0,0,0.15);
  }}
  #map-text-toggle label {{ display: flex; align-items: center; gap: 4px; margin: 0; }}
  #map-text-toggle input {{ margin: 0; }}
</style>
<script>
  // T21: securityJsCode 必须在 AMap SDK 加载前注入
  window._AMapSecurityConfig = {{ securityJsCode: "{security_js_code}" }};

  window.onerror = function(msg, url, line) {{
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{"type":"error","message":"review_map_page_error"}}));
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
        window.ipc.postMessage(JSON.stringify({{"type":"error","message":"review_map_sdk_timeout"}}));
      }}
    }}
  }}, 5000);
</script>
</head>
<body>
<div id="status-panel">正在初始化...</div>
<div id="map-container"></div>
<div id="osm-attribution">© OpenStreetMap contributors</div>
<div id="map-text-toggle">
  <label><input type="checkbox" id="map-text-toggle-checkbox" /><span>{map_text_toggle_label}</span></label>
</div>
<script src="{cdn_url}"></script>
<script>
  var map;
  var anchorPoint = {{ lng: {anchor_lon}, lat: {anchor_lat} }};
  var initialViewport = {initial_viewport_json};
  var preserveInitialViewport = !!initialViewport;
  window.__reviewPreserveInitialViewport = preserveInitialViewport;
  // 评审地图文字初始可见性（false = 默认隐藏地标/POI 文字）
  window.__reviewMapTextVisible = {map_text_visible};

  // T37/T39：把地图容器同步到 WebView 视口尺寸（html/body 100% + 显式
  // innerHeight），再 map.resize()；窗口 resize 与抽屉开合让位（WebView
  // bounds 变化）均依赖此机制，保证地图上下填满可用区域。
  function syncContainerSize() {{
    var container = document.getElementById('map-container');
    var viewportW = Math.max(document.documentElement.clientWidth || 1, 1);
    var viewportH = Math.max(window.innerHeight || document.documentElement.clientHeight || 1, 1);
    if (container) {{
      container.style.width = '100%';
      container.style.maxWidth = '100%';
      if (container.offsetWidth > viewportW) {{
        container.style.width = viewportW + 'px';
      }}
      container.style.height = viewportH + 'px';
    }}
    if (map && typeof map.resize === 'function') {{
      map.resize();
    }}
  }}

  function initWithAnchor() {{
    try {{
      // T37/T39：AMap 初始化前把容器宽高钳制到当前 WebView 视口
      syncContainerSize();
      map = new AMap.Map('map-container', {{
        zoom: initialViewport ? initialViewport.zoom : 16,
        center: initialViewport
          ? [initialViewport.longitude, initialViewport.latitude]
          : [anchorPoint.lng, anchorPoint.lat],
        viewMode: '3D'
      }});
      function reportViewport() {{
        if (!(window.ipc && window.ipc.postMessage)) return;
        var center = map.getCenter();
        window.ipc.postMessage(JSON.stringify({{
          type: 'viewport_changed',
          longitude: center.lng,
          latitude: center.lat,
          zoom: map.getZoom()
        }}));
      }}
      map.on('moveend', reportViewport);
      map.on('zoomend', reportViewport);
      // T37/T39：布局完成后同步一次画布尺寸（含 map.resize()）
      syncContainerSize();
      // WebView bounds 变化（窗口 resize 或抽屉开合让位）时同步容器尺寸
      window.addEventListener('resize', syncContainerSize);
      if (typeof window.initReviewMap === 'function') {{
        window.initReviewMap();
      }}
    }} catch (e) {{
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{ type: 'error', message: 'review_map_init_failed' }}));
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
        cdn_url = cdn_url,
        security_js_code = config.security_key,
        anchor_lon = config.anchor_lon,
        anchor_lat = config.anchor_lat,
        map_text_toggle_label = config.map_text_toggle_label,
        map_text_visible = config.map_text_visible,
        initial_viewport_json = initial_viewport_json,
        review_script = REVIEW_SCRIPT,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ReviewMapPageConfig {
        ReviewMapPageConfig::new("abc123DEF456", "xyz789GHI012")
            .with_anchor(116.4, 39.9)
            .with_map_text_toggle("显示地图文字", false)
    }

    #[test]
    fn html_exposes_review_annotation_and_locate_commands() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(html.contains("drawReviewCandidates"));
        assert!(html.contains("clearReviewOverlays"));
        assert!(html.contains("addReviewCandidate"));
        assert!(
            html.contains("window.updateReviewCandidate = function"),
            "T39：单候选增量更新命令必须存在（state 变更只推对应候选）"
        );
        assert!(html.contains("setReviewCandidates"));
        assert!(html.contains("startChunkedDrawing"));
        assert!(html.contains("locateReviewCandidate"));
        assert!(html.contains("highlightReviewCandidate"));
        assert!(html.contains("clearReviewHighlight"));
        assert!(html.contains("review_object_clicked"));
        assert!(html.contains("map_ready"));
        assert!(
            !html.contains("__embedded_candidates__"),
            "T39：候选不得再内嵌进 HTML（map_ready 后由 Rust 分批推送）"
        );
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
        assert!(
            html.contains("strokeStyle: dash ? 'dashed' : 'solid'")
                && html.contains("strokeDasharray: dash || []"),
            "折线/面标注必须使用高德 v2 支持的虚线字段"
        );
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
    fn html_forbids_horizontal_overflow_and_fills_viewport_height() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(html.contains("overflow-x: hidden"));
        assert!(html.contains("window.addEventListener('load', boot)"));
        // T37/T39：评审地图容器必须随 WebView 视口填满（不再固定高度留白）
        assert!(
            html.contains("#map-container { width: 100%; max-width: 100%; box-sizing: border-box; height: 100%;"),
            "地图容器高度必须随视口填满（height: 100%）"
        );
        assert!(
            !html.contains("border-box; height: 300px") && !html.contains("height: {height}px"),
            "地图容器不得再固定像素高度"
        );
        assert!(
            html.contains("container.style.height = viewportH + 'px'"),
            "初始化与 resize 必须把容器高度显式同步到 WebView 视口高"
        );
        assert!(
            html.contains("window.addEventListener('resize', syncContainerSize)"),
            "resize/抽屉开合必须经同一函数同步宽高并 map.resize()"
        );
        assert!(
            html.contains("syncContainerSize();"),
            "AMap 初始化前后必须调用尺寸同步（含 map.resize()）"
        );
    }

    #[test]
    fn html_restores_review_viewport_without_refitting_candidates() {
        let config = config().with_initial_viewport(crate::MapViewport::new(121.46, 31.05, 19.0));
        let html = build_review_map_page_html(&config).unwrap();

        assert!(html.contains("initialViewport"));
        assert!(html.contains("preserveInitialViewport"));
        assert!(html.contains("type: 'viewport_changed'"));
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
    fn html_hides_initializing_status_before_map_ready() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("statusPanel.style.display = 'none'")
                && html.find("statusPanel.style.display = 'none'")
                    < html.find("window.ipc.postMessage(JSON.stringify({ type: 'map_ready' }))"),
            "高德地图就绪后必须先隐藏初始化状态，再通知 Rust 推送候选"
        );
    }

    #[test]
    fn html_hides_poi_labels_by_default_and_exposes_text_toggle() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("map.setFeatures(['bg', 'road', 'building'])"),
            "评审模式默认必须隐藏地标/POI 文字（省略 point 标签）"
        );
        assert!(
            html.contains("map.setFeatures(['bg', 'point', 'road', 'building'])"),
            "用户恢复文字时必须重新启用 point 标签"
        );
        assert!(html.contains("window.setReviewMapText = function"));
        assert!(html.contains("review_map_text_toggled"));
        assert!(html.contains("map-text-toggle-checkbox"));
        assert!(html.contains("显示地图文字"));
        assert!(
            html.contains("window.__reviewMapTextVisible = false"),
            "默认文字状态必须经全局注入为 false"
        );
    }

    #[test]
    fn html_queues_locate_until_target_is_drawn_and_keeps_remove_hidden() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("pendingLocateId = candidateId"),
            "目标尚未绘制时定位必须进入 pending 队列"
        );
        assert!(
            html.contains("locateDrawnReviewCandidate(pendingLocateId)"),
            "pending 目标绘制后必须设置中心与缩放"
        );
        assert!(
            html.contains("postReviewLocateFailure('hidden')"),
            "剔除候选必须保持地图隐藏并给出明确反馈"
        );
        assert!(
            !html.contains("addReviewCandidateInner(reviewCandidateData[pendingLocateId], true)"),
            "定位不得强制补绘剔除候选"
        );
        assert!(
            html.contains("pendingCandidates[queuedIndex] = c"),
            "全量绘制队列中的候选收到增量状态时必须替换队列项"
        );
    }

    #[test]
    fn html_stringifies_safe_ipc_errors() {
        let html = build_review_map_page_html(&config()).unwrap();
        for marker in [
            "review_map_page_error",
            "review_map_sdk_timeout",
            "review_map_init_failed",
            "review_map_draw_failed:",
        ] {
            assert!(html.contains(marker), "缺少安全错误标记：{marker}");
        }
        assert!(
            !html.contains("window.ipc.postMessage({\"type\":\"error\"")
                && !html.contains("message: '评审地图初始化失败: ' + e.message"),
            "错误 IPC 必须 JSON.stringify，且不得把原始异常详情直接上送"
        );
    }

    #[test]
    fn html_uses_high_contrast_highlight_style() {
        let html = build_review_map_page_html(&config()).unwrap();
        assert!(
            html.contains("strokeColor: '#e74c3c', strokeWeight: 5"),
            "定位/选中必须使用高对比、高层级轮廓"
        );
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
