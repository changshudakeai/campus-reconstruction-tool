//! 屏 4 边界编辑地图页生成 (T24) + T25 朝向模式扩展
//!
//! OSM 自动获取优先 (ADR-0029): Overpass fetch → 多边形提取 → GCJ-02 转换 → 绘制
//! 人工调整：PolygonEditor 拖拽顶点 → IPC 回传更新坐标
//! 人工圈画兜底：点击落点模式 (沿用 T23 协议)
//! T25: 同一页面扩展朝向模式 —— 已确认边界半透明显示，点击两点画参考线。
//!
//! **职责**: B3 高德客户端负责生成 HTML，壳层 WebView 加载渲染 —— 零业务计算
//! 在壳内 (ADR-0017)

use crate::error::{Error, Result};

/// 官方 CDN URL 模板 (v2.0 + PlaceSearch + PolygonEditor)
pub const GAODE_CDN_URL_TEMPLATE: &str =
    "https://webapi.amap.com/maps?v=2.0&key={key}&plugin=AMap.PolygonEditor";

/// 边界地图页配置
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryEditPageConfig {
    /// 高德 Web API key
    pub api_key: String,
    /// 高德安全密钥 (securityJsCode)
    pub security_key: String,
    /// 校区锚点坐标 (WGS-84; Overpass 查询中心)
    pub anchor_lon: f64,
    pub anchor_lat: f64,
    /// 地图容器高度 (像素)
    pub height_px: u32,
    /// T25: 朝向模式标识
    pub orientation_mode: bool,
    /// T25: 已确认边界坐标（用于在朝向模式下显示半透明参考）
    pub existing_boundary_gcj02: Option<Vec<[f64; 2]>>,
}

impl BoundaryEditPageConfig {
    /// 新建配置 (高度取最小值 300px)
    pub fn new(api_key: impl Into<String>, security_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            security_key: security_key.into(),
            anchor_lon: 116.397, // 默认北京 (会被实际调用方覆盖)
            anchor_lat: 39.916,
            height_px: 300,
            orientation_mode: false,
            existing_boundary_gcj02: None,
        }
    }

    pub fn with_anchor(mut self, lon: f64, lat: f64) -> Self {
        self.anchor_lon = lon;
        self.anchor_lat = lat;
        self
    }

    /// T25: 设置朝向模式
    pub fn with_orientation_mode(mut self, enabled: bool) -> Self {
        self.orientation_mode = enabled;
        self
    }

    /// T25: 设置已确认边界坐标（GCJ-02）
    pub fn with_existing_boundary(mut self, coords: Option<Vec<[f64; 2]>>) -> Self {
        self.existing_boundary_gcj02 = coords;
        self
    }

    pub fn effective_height_px(&self) -> u32 {
        self.height_px.max(300)
    }
}

/// T25: 朝向模式附加脚本（不经过 format!，因此无需双花括号转义）。
const ORIENTATION_SCRIPT: &str = r#"
<script>
(function() {
  var orientationPoints = [];
  var orientationLine = null;
  var orientationPolygon = null;
  var existingBoundaryCoords = window.__orientationConfig__.existingBoundary || null;

  function setStatus(text, type) {
    var statusPanel = document.getElementById('status-panel');
    if (statusPanel) {
      statusPanel.textContent = text;
      statusPanel.className = type || '';
    }
  }

  function drawExistingBoundary(coords) {
    if (orientationPolygon) {
      map.remove(orientationPolygon);
      orientationPolygon = null;
    }
    var gcjCoords = coords.map(function(c) { return new AMap.LngLat(c[0], c[1]); });
    orientationPolygon = new AMap.Polygon({
      path: gcjCoords,
      strokeColor: '#95a5a6',
      strokeWeight: 2,
      fillOpacity: 0.15,
      fillColor: '#95a5a6',
      editable: false
    });
    map.add(orientationPolygon);
    setStatus('已加载已确认边界作半透明参照', '');
  }

  function redrawOrientationPreview() {
    if (orientationLine) {
      map.remove(orientationLine);
      orientationLine = null;
    }
    if (orientationPoints.length >= 2) {
      var path = orientationPoints.map(function(p) { return new AMap.LngLat(p[0], p[1]); });
      orientationLine = new AMap.Polyline({
        path: path,
        strokeColor: '#e74c3c',
        strokeWeight: 3,
        lineDash: [8, 4]
      });
      map.add(orientationLine);
    }
  }

  function showOrientationControls() {
    var toolbar = document.getElementById('map-toolbar');
    if (!toolbar) return;
    toolbar.innerHTML = '';

    var clearBtn = document.createElement('button');
    clearBtn.className = 'control-btn';
    clearBtn.textContent = '清除重来';
    clearBtn.onclick = function() {
      orientationPoints = [];
      if (orientationLine) { map.remove(orientationLine); orientationLine = null; }
      setStatus('已清除，重新点击地图选择两点', 'error');
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({ type: 'orientation_clear' }));
      }
    };
    toolbar.appendChild(clearBtn);

    var confirmBtn = document.createElement('button');
    confirmBtn.className = 'control-btn primary';
    confirmBtn.textContent = '确认朝向';
    confirmBtn.onclick = function() {
      if (orientationPoints.length >= 2) {
        setStatus('已提交两点坐标 → 等待计算角度', 'success');
        if (window.ipc && window.ipc.postMessage) {
          window.ipc.postMessage(JSON.stringify({
            type: 'confirm_orientation',
            points: orientationPoints
          }));
        }
      }
    };
    toolbar.appendChild(confirmBtn);
  }

  function handleOrientationClick(e) {
    var loc = e.lnglat;
    if (orientationPoints.length === 0) {
      orientationPoints.push([loc.lng, loc.lat]);
      setStatus('已选第 1 个点，请选第 2 个点 → 点"清除"按钮重来', 'error');
      redrawOrientationPreview();
    } else if (orientationPoints.length === 1) {
      orientationPoints.push([loc.lng, loc.lat]);
      setStatus('已选好两点，点击 "确认朝向" 提交', 'success');
      redrawOrientationPreview();
      showOrientationControls();
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({
          type: 'orientation_points',
          points: orientationPoints
        }));
      }
    }
  }

  window.initOrientationMode = function() {
    setStatus('朝向模式：点击地图选择两点确定参考线', 'success');
    if (existingBoundaryCoords && existingBoundaryCoords.length > 0) {
      drawExistingBoundary(existingBoundaryCoords);
    }
    map.off('click');
    map.on('click', handleOrientationClick);
  };

  window.onMapReadyForMode = function() {
    if (window.__orientationConfig__ && window.__orientationConfig__.orientationMode) {
      setStatus('正在进入朝向模式...', '');
      setTimeout(function() {
        if (typeof window.initOrientationMode === 'function') {
          window.initOrientationMode();
        } else {
          setStatus('朝向模式脚本未加载', 'error');
        }
      }, 500);
    } else {
      setTimeout(fetchOverpassBoundary, 1000);
    }
  };
})();
</script>
"#;

/// 生成边界编辑地图页 HTML
///
/// **功能清单**:
/// 1. Overpass fetch: 锚点为中心半径 1000m 查询 `amenity~university|college|school`
/// 2. 多边形提取：way/relation 的 geometry 转坐标数组
/// 3. GCJ-02 转换：AMap.convertFrom(WGS-84 → GCJ-02)
/// 4. 自动绘制：converted boundary 作为初始多边形
/// 5. 编辑支持：PolygonEditor 插件启用后，用户拖拽顶点
/// 6. 人工圈画兜底：查询失败/无数据 → 提示 → 点击落点模式
/// 7. T25: 朝向模式扩展
/// 8. IPC 协议扩展:
///    - `osm_elements`: 原始要素 JSON(经度，纬度数组×N)
///    - `boundary_update`: 编辑后的多边形坐标 `{type:"boundary_update", coords:[lng,lat,...]}`
///    - `manual_point`: 人工圈画落点 `"lng,lat"`
///    - `manual_cancel`: 撤销最后一点 `{type:"manual_cancel"}`
///    - `confirm_boundary`: 确认最终边界 `{type:"confirm_boundary", coords:[...]}`
///    - `orientation_points`: 朝向两点 `{type:"orientation_points", points:[[lng,lat],[lng,lat]]}`
///    - `confirm_orientation`: 确认朝向 `{type:"confirm_orientation", points:[[...],[...]]}`
///    - `orientation_clear`: 清除朝向点 `{type:"orientation_clear"}`
///
/// **红线**:
/// - 密钥只经 F1(通过 map_page.rs 现有校验);禁止硬编码真实 key
/// - 禁止候选列表 UI(ADR-0029)
/// - 未做 GCJ-02 转换的坐标禁止上屏 (必须调用 convertFrom)
pub fn build_boundary_edit_page_html(config: &BoundaryEditPageConfig) -> Result<String> {
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

    // T25: 将已确认边界坐标序列化为 JSON 注入 JS
    let existing_boundary_json = serde_json::to_string(&config.existing_boundary_gcj02)
        .unwrap_or_else(|_| "null".to_string());

    let base_html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>圈选边界</title>
<style>
  html, body {{ margin: 0; padding: 0; height: 100%; }}
  #map-container {{ width: 100%; height: {height}px; min-height: 300px; }}
  #status-panel {{
    position: absolute; top: 10px; left: 10px; z-index: 1000;
    background: white; padding: 12px; border-radius: 6px;
    box-shadow: 0 2px 8px rgba(0,0,0,0.15); font-size: 13px; max-width: 320px;
  }}
  #status-panel.error {{ border-left: 4px solid #e74c3c; }}
  #status-panel.success {{ border-left: 4px solid #2ecc71; }}
  #map-toolbar {{
    position: absolute; bottom: 10px; right: 10px; z-index: 1000;
    display: flex; gap: 8px;
  }}
  button.control-btn {{
    padding: 6px 12px; font-size: 12px;
    background: #f0f0f0; border: 1px solid #ccc; border-radius: 4px; cursor: pointer;
    box-shadow: 0 1px 4px rgba(0,0,0,0.2);
  }}
  button.control-btn:hover {{ background: #e0e0e0; }}
  button.control-btn:disabled {{ opacity: 0.5; cursor: not-allowed; }}
  button.control-btn.primary {{ background: #2ecc71; color: white; border: none; }}
</style>
<script>
  // T21: securityJsCode 必须在 AMap SDK 加载前注入
  window._AMapSecurityConfig = {{ securityJsCode: "{security_js_code}" }};

  // T25: 朝向模式配置（由 Rust 注入）
  window.__orientationConfig__ = {{
    orientationMode: {orientation_mode},
    existingBoundary: {existing_boundary_json}
  }};

  // T23: 错误检测脚本
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
<div id="map-toolbar"></div>
<script src="{cdn_url}"></script>
<script>
  var map;
  var manualPoints = [];   // 人工圈画的点序列
  var isEditMode = false;  // true = 编辑模式 (有 OSM 边界), false = 人工圈画模式
  var previewLine = null;  // Manual mode preview line
  var anchorPoint = {{ lng: {anchor_lon}, lat: {anchor_lat} }};  // 校区锚点 (WGS-84)
  var osmBoundaryData = null;  // 从 Overpass 获取的原始数据

  var statusPanel = document.getElementById('status-panel');

  function setStatus(text, type) {{
    statusPanel.textContent = text;
    statusPanel.className = type || '';
  }}

  // 红线：未做 GCJ-02 转换的坐标禁止上屏——先转换锚点，再用 GCJ-02 中心创建地图
  function initWithConvertedAnchor() {{
    AMap.convertFrom([anchorPoint.lng, anchorPoint.lat], 'gps', function(status, result) {{
      var gcjCenter;
      if (status === 'complete' && result.info === 'ok') {{
        gcjCenter = result.locations[0];
      }} else {{
        // Conversion fails offline: fallback to anchor anyway (map will be ~500m offset)
        // No unconverted coordinate data is drawn; this is just view centering.
        gcjCenter = {{ lng: anchorPoint.lng, lat: anchorPoint.lat }};
      }}

      try {{
        map = new AMap.Map('map-container', {{
          zoom: 16,
          center: [gcjCenter.lng, gcjCenter.lat],
          viewMode: '3D'
        }});

        // Display anchor (now with converted position)
        new AMap.Marker({{
          position: new AMap.LngLat(gcjCenter.lng, gcjCenter.lat),
          label: {{ content: '📍 校区锚点', offset: new AMap.Pixel(0, -20) }}
        }}).addTo(map);

        // T25: 朝向模式入口由 ORIENTATION_SCRIPT 注入的 onMapReadyForMode 接管，
        // 非朝向模式下直接走 OSM 边界查询，主脚本不出现朝向入口标识符。
        if (typeof window.onMapReadyForMode === 'function') {{
          window.onMapReadyForMode();
        }} else {{
          // Enter auto Overpass query after 1 second
          setTimeout(fetchOverpassBoundary, 1000);
        }}
      }} catch (e) {{
        setStatus('地图初始化失败：' + e.message, 'error');
        enableManualMode();  // Fallback → manual drawing
      }}
    }});
  }}

  // ========== T24: Overpass 查询 ==========
  function fetchOverpassBoundary() {{
    setStatus('正在从 OSM 查询边界...', '');

    var radius = 1000;  // 半径 1000 米
    var query = buildOverpassQuery(anchorPoint.lng, anchorPoint.lat, radius);

    fetch('https://overpass-api.de/api/interpreter?' + encodeURIComponent(query))
      .then(function(response) {{
        if (!response.ok) throw new Error('OSM HTTP error: ' + response.status);
        return response.json();
      }})
      .then(function(data) {{
        var elements = data.elements.filter(function(e) {{
          return (e.type === 'way' || e.type === 'relation') &&
                 e.tags &&
                  (e.tags.amenity === 'university' ||
                   e.tags.amenity === 'college' ||
                   e.tags.amenity === 'school');
        }});

        if (elements.length === 0) {{
          setStatus('OSM 无此校区边界数据 → 已切换至人工圈画', 'error');
          enableManualMode();
          return;
        }}

        // Normalize & send to Rust for sorting
        var normalized = normalizeElements(elements);
        setStatus('正在按规则自动选取最匹配边界...', '');
        window.ipc.postMessage(JSON.stringify({{ type: 'osm_elements', elements: normalized }}));
      }})
      .catch(function(err) {{
        setStatus('查询失败，备用端点：' + err.message,
 'error');
        // Fallback to kumi.systems
        var query2 = buildOverpassQuery(anchorPoint.lng, anchorPoint.lat, radius);
        return fetch('https://overpass.kumi.systems/api/interpreter?' + encodeURIComponent(query2))
          .then(function(response) {{
            if (!response.ok) throw new Error('备用端点失败：' + response.status);
            return response.json();
          }});
      }})
      .catch(function(err) {{
        setStatus('查询失败 (' + err.message + ') → 已切换至人工圈画', 'error');
        setTimeout(enableManualMode, 1500);
      }});
  }}

  function buildOverpassQuery(lng, lat, radius) {{
    var latDelta = radius / 111320.0;
    var lngDelta = radius / (111320.0 * Math.abs(Math.cos(lat * Math.PI / 180)));
    return '[out:json][timeout:45];' +
      '(way["amenity"~"university|college|school"](' +
        (lat - latDelta) + ',' + (lng - lngDelta) + ',' +
        (lat + latDelta) + ',' + (lng + lngDelta) + ');' +
       'relation["amenity"~"university|college|school"](' +
        (lat - latDelta) + ',' + (lng - lngDelta) + ',' +
        (lat + latDelta) + ',' + (lng + lngDelta) + '););out geom;';
  }}

  // Extract polygon from way or relation element (for overpass out geom)
  function extractPolygon(element) {{
    if (!element.geometry) return null;
    if (element.type === 'way') {{
      // [{{lat:,lon:}},...] → [[lon,lat],...]
      return element.geometry.map(function(p) {{ return [p.lon, p.lat]; }});
    }}
    if (element.type === 'relation') {{
      // Stitch outer members (concatenate their geometries)
      var coords = [];
      element.members.forEach(function(m) {{
        if ((m.role === 'outer' || m.role === '') && m.geometry) {{
          coords.push.apply(coords, m.geometry.map(function(p) {{ return [p.lon, p.lat]; }}));
        }}
      }});
      return coords.length > 0 ? coords : null;
    }}
    return null;
  }}

  // Normalize OSM elements before sending to Rust
  function normalizeElements(elements) {{
    return elements.map(function(e) {{
      var normalized = {{ type: e.type, id: e.id, tags: e.tags, members: [], geometry: extractPolygon(e) }};
      if (!normalized.geometry) return null;
      return normalized;
    }}).filter(function(e) {{ return e !== null; }});
  }}

  // ========== T24: GCJ-02 坐标转换 ==========
  // 由 Rust 排序选定后经 evaluate_script 调用 (Rust→JS 通道，
  // 传入 WGS-84 原始坐标；本函数负责 convertFrom 转换后才上屏——
  // 红线：未做 GCJ-02 转换的坐标禁止上屏)
  function convertAndDraw(wgsCoords, sourceName) {{
    setStatus('正在转换 WGS-84 到 GCJ-02 坐标系...', '');

    // Convert WGS-84 to GCJ-02 in chunks of 40 pairs
    function convertChunked(startIdx, acc) {{
      if (startIdx >= wgsCoords.length) {{
        // All done
        drawBoundary(acc, sourceName, true);
        setStatus('来自 OSM: ' + sourceName + ' ✓', 'success');
        return;
      }}
      var endIdx = Math.min(startIdx + 40, wgsCoords.length);
      var chunk = wgsCoords.slice(startIdx, endIdx);
      AMap.convertFrom(chunk, 'gps', function(status, result) {{
        if (status !== 'complete' || result.info !== 'ok') {{
          setStatus('坐标转换失败 → 人工圈画', 'error');
          enableManualMode();
          return;
        }}
        convertChunked(endIdx, acc.concat(result.locations));
      }});
    }}

    convertChunked(0, []);
  }}

  function drawBoundary(coords, name, editable) {{
    isEditMode = editable;

    polygon = new AMap.Polygon({{
      path: coords,
      strokeColor: '#3498db',
      strokeWeight: 3,
      fillOpacity: 0.3,
      fillColor: '#3498db',  // Fixed: was fillStyle
      editMode: editable
    }});

    map.add(polygon);

    if (editable && AMap.PolygonEditor) {{
      // Enable editor
      polygonEditor = new AMap.PolygonEditor(map, polygon, {{
        template: {{
          dragging: true,
          circleMarkerStyle: {{ fillColor: '#e74c3c', fillOpacity: 0.6, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, fontSize: '12px', fontColor: '#fff', borderRadius: '50%', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }},
          vertexMarkerStyle: {{ fillColor: '#e74c3c', fillOpacity: 0.8, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, fontSize: '12px', fontColor: '#fff', borderRadius: '50%', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }},
          interiorMarkerStyle: {{ fillColor: '#3498db', fillOpacity: 1, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, fontSize: '12px', fontColor: '#fff', borderRadius: '4px', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }}
        }}
      }});

      // 监听编辑事件
      polygonEditor.on('dragnode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('addnode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('removenode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('adjust', function(e) {{ updateFromEditor(); }});

      // Open editor to enable drag mode
      polygonEditor.open();

      setStatus('已加载 OSM 边界 — 拖动顶点可调整 · 确认后提交', 'success');
      showEditControls();
    }} else {{
      enableManualMode();
    }}
  }}

  // 编辑模式工具栏：确认边界（上交当前多边形）/ 改人工圈画
  function showEditControls() {{
    var toolbar = document.getElementById('map-toolbar');
    toolbar.innerHTML = '';

    var confirmBtn = document.createElement('button');
    confirmBtn.className = 'control-btn primary';
    confirmBtn.textContent = '确认边界';
    confirmBtn.onclick = function() {{
      if (!polygon) {{ return; }}
      var coords = normalizedPath();
      if (coords.length < 3) {{
        setStatus('边界至少需要 3 个点', 'error');
        return;
      }}
      setStatus('已提交边界 → 等待验证', 'success');
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{
          type: 'confirm_boundary',
          coords: coords
        }}));
      }}
    }};
    toolbar.appendChild(confirmBtn);

    var manualBtn = document.createElement('button');
    manualBtn.className = 'control-btn';
    manualBtn.textContent = '改人工圈画';
    manualBtn.onclick = function() {{ enableManualMode(); }};
    toolbar.appendChild(manualBtn);
  }}

  // 当前多边形路径归一化为 [lng, lat] 数组（GCJ-02）
  function normalizedPath() {{
    if (!polygon) {{ return []; }}
    return polygon.getPath().map(function(p) {{ return [p.lng, p.lat]; }});
  }}

  // ========== T24: 编辑回调 ==========
  function updateFromEditor() {{
    if (polygon) {{
      var coords = normalizedPath();  // GCJ-02 [lng,lat]×N
      var payload = {{ type: 'boundary_update', coords: coords }};
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify(payload));
      }}
    }}
  }}

  // ========== T24: 人工圈画兜底 ==========
  function enableManualMode() {{
    if (polygon) {{ map.remove(polygon); polygon = null; }}
    if (polygonEditor) {{ polygonEditor.close(); polygonEditor = null; }}

    isEditMode = false;
    manualPoints = [];

    setStatus('人工圈画模式：点击地图添加控制点', 'error');

    // Remove any previous click handler to prevent accumulation
    map.off('click');

    // 地图点击事件 → 落点
    map.on('click', function(e) {{
      var loc = e.lnglat;
      manualPoints.push([loc.lng, loc.lat]);

      // 预览连线 + 刷新确认按钮计数
      redrawPreviewLine();
      showManualControls();

      // IPC 上报最新状态
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{
          type: 'manual_point',
          point: [loc.lng, loc.lat],
          total: manualPoints.length
        }}));
      }}
    }});

    showManualControls();
  }}

  function redrawPreviewLine() {{
    if (!window.previewLine) {{
      window.previewLine = new AMap.Polyline({{  // Fixed: was LineString
        path: manualPoints,
        strokeColor: '#e74c3c',
        strokeWeight: 2,
        lineDash: [5, 5]
      }});
      map.add(window.previewLine);
    }} else {{
      window.previewLine.setPath(manualPoints);
    }}
  }}

  function showManualControls() {{
    var toolbar = document.getElementById('map-toolbar');
    toolbar.innerHTML = '';

    var undoBtn = document.createElement('button');
    undoBtn.className = 'control-btn';
    undoBtn.textContent = '撤销上一个点';
    undoBtn.onclick = function() {{
      if (manualPoints.length > 0) {{
        manualPoints.pop();
        redrawPreviewLine();
        showManualControls();
        if (window.ipc && window.ipc.postMessage) {{
          window.ipc.postMessage(JSON.stringify({{ type: 'manual_cancel' }}));
        }}
      }}
    }};
    toolbar.appendChild(undoBtn);

    var clearBtn = document.createElement('button');
    clearBtn.className = 'control-btn';
    clearBtn.textContent = '清空重画';
    clearBtn.onclick = function() {{
      manualPoints = [];
      redrawPreviewLine();
      showManualControls();
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{ type: 'manual_clear' }}));
      }}
    }};
    toolbar.appendChild(clearBtn);

    var confirmBtn = document.createElement('button');
    confirmBtn.className = 'control-btn primary';
    confirmBtn.textContent = '确认边界 (' + manualPoints.length + '个点)';
    confirmBtn.disabled = manualPoints.length < 3;
    confirmBtn.onclick = function() {{
      if (manualPoints.length >= 3) {{
        setStatus('已确认 ' + manualPoints.length + ' 个点 → 待验证', 'success');
        if (window.ipc && window.ipc.postMessage) {{
          window.ipc.postMessage(JSON.stringify({{
            type: 'confirm_boundary',
            coords: manualPoints
          }}));
        }}
      }}
    }};
    toolbar.appendChild(confirmBtn);
  }}

  // 启动
  if (typeof AMap === 'undefined') {{
    setStatus('高德 SDK 加载中...', '');
  }} else {{
    initWithConvertedAnchor();  // Convert anchor first, then init map
  }}
</script>
{orientation_script}
</body>
</html>"#,
        height = height,
        cdn_url = cdn_url,
        security_js_code = config.security_key,
        anchor_lon = config.anchor_lon,
        anchor_lat = config.anchor_lat,
        orientation_mode = config.orientation_mode,
        existing_boundary_json = existing_boundary_json,
        orientation_script = if config.orientation_mode {
            ORIENTATION_SCRIPT
        } else {
            ""
        },
    );

    Ok(base_html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_contains_overpass_query_logic() {
        let config =
            BoundaryEditPageConfig::new("abc123DEF456", "xyz789GHI012").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("fetchOverpassBoundary"));
        assert!(html.contains("overpass-api.de"));
        assert!(html.contains("amenity\"~\"university|college|school\""));
    }

    #[test]
    fn html_contains_gcj02_conversion() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("AMap.convertFrom"));
        assert!(html.contains("convertAndDraw"));
    }

    #[test]
    fn html_contains_polygon_editor_plugin() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("plugin=AMap.PolygonEditor"));
        assert!(html.contains("PolygonEditor"));
    }

    #[test]
    fn html_contains_manual_mode_fallback() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("enableManualMode"));
        assert!(html.contains("manualPoints"));
        assert!(html.contains("confirm_boundary"));
    }

    #[test]
    fn html_contains_edit_mode_confirm_button() {
        // 验收③：编辑模式（OSM 边界已绘制）必须有确认路径——
        // 确认后步骤条打勾依赖此按钮发出 confirm_boundary
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("showEditControls"));
        assert!(html.contains("map-toolbar"));
        // 编辑事件坐标必须用 getPath 归一化（JS API 2.0 无 getCoordinates）
        assert!(html.contains("getPath()"));
        assert!(!html.contains("getCoordinates"));
    }

    #[test]
    fn html_sorting_stays_out_of_js() {
        // 红线：排序逻辑必须在 Rust 且可单测，禁止只写进 JS——
        // JS 只发 osm_elements 等 Rust 回复 convertAndDraw/enableManualMode
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("osm_elements"));
        assert!(!html.contains("findBestMatch"));
    }

    #[test]
    fn invalid_api_key_is_rejected() {
        let config = BoundaryEditPageConfig::new("bad@key", "xyz789");
        assert!(build_boundary_edit_page_html(&config).is_err());
    }

    #[test]
    fn valid_config_generates_complete_html() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("<html"));
        assert!(html.contains("<body"));
        assert!(html.contains("116.4"));
        assert!(html.contains("39.9"));
    }

    // T25: 朝向模式相关测试
    #[test]
    fn orientation_mode_includes_orientation_script() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_orientation_mode(true);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("initOrientationMode"));
        assert!(html.contains("confirm_orientation"));
        assert!(html.contains("orientation_points"));
        assert!(html.contains("orientationMode: true"));
    }

    #[test]
    fn non_orientation_mode_excludes_orientation_script() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_orientation_mode(false);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("initOrientationMode"));
        assert!(html.contains("orientationMode: false"));
    }

    #[test]
    fn orientation_mode_injects_existing_boundary() {
        let boundary = vec![[116.4, 39.9], [116.5, 39.9], [116.5, 40.0]];
        let config = BoundaryEditPageConfig::new("abc123", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_orientation_mode(true)
            .with_existing_boundary(Some(boundary));
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("116.4"));
        assert!(html.contains("39.9"));
        assert!(html.contains("existingBoundary"));
    }
}
