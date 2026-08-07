//! 屏 4 边界编辑地图页生成 (T24) + T25 朝向模式扩展 + T31 Rust 侧直连
//!
//! OSM 自动获取优先 (ADR-0029): **T31 起 Overpass 查询、Nominatim 校名解析、
//! WGS-84 → GCJ-02 转换与候选排序全部在 Rust 侧**（绕开 WebView CORS；
//! 调研根因见 `docs/research/candidate-data-sources-and-naming.md` §4.2），
//! JS 只发 `map_ready` 就绪信号并接收 `drawBoundaryGcj(GCJ-02 坐标)` 直接绘制。
//! 人工调整：PolygonEditor 拖拽顶点 → IPC 回传更新坐标
//! 人工圈画兜底：点击落点模式 (沿用 T23 协议)
//! T25: 同一页面扩展朝向模式 —— 已确认边界半透明显示，点击两点画参考线。
//!
//! **职责**: B3 高德客户端负责生成 HTML，壳层 WebView 加载渲染 —— 零业务计算
//! 在壳内 (ADR-0017)

use crate::boundary_edit_multipolygon_script::MULTI_AREA_SCRIPT;
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
            // 锚点必须是调用方提供的真实校区坐标；没有真实锚点时构建页面明确失败，
            // 不得以固定坐标/默认点代替用户数据（ADR-0042 §7）。
            anchor_lon: f64::NAN,
            anchor_lat: f64::NAN,
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
      // T31：非朝向模式由主脚本 initWithAnchor 发送 map_ready，Rust 侧直连 OSM
      setStatus('正在从 OSM 自动获取边界...', '');
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({ type: 'map_ready' }));
      }
    }
  };
})();
</script>
"#;

/// 生成边界编辑地图页 HTML
///
/// **功能清单**:
/// 1. 地图加载（高德 JS API；锚点 GCJ-02 直接作为中心）
/// 2. `map_ready` IPC：Rust 侧发起 Nominatim → Overpass（端点回退）→
///    WGS-84 → GCJ-02 → ADR-0029 排序（T31，不再依赖 WebView fetch/CORS）
/// 3. 自动绘制：Rust 回传的 GCJ-02 坐标经 `drawBoundaryGcj` 直接上屏
/// 4. 编辑支持：PolygonEditor 插件启用后，用户拖拽顶点
/// 5. 人工圈画兜底：查询失败/无数据 → 提示 → 点击落点模式
/// 6. T25: 朝向模式扩展
/// 7. IPC 协议扩展:
///    - `map_ready`: 地图就绪（T31 触发 Rust 侧 OSM 自动获取）
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
/// - 未做 GCJ-02 转换的坐标禁止上屏（T31：转换在 Rust 侧完成，JS 不再 convertFrom）
/// - 署名：OSM 数据 ODbL，页面保留 `© OpenStreetMap contributors`
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
    if !config.anchor_lon.is_finite() || !config.anchor_lat.is_finite() {
        return Err(Error::MalformedResponse(
            "校区锚点缺失或无效：必须提供真实校区坐标".to_owned(),
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
    /* D-5：小窗口下工具栏可换行、不超出 WebView 右缘，按钮始终可点 */
    display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end;
    max-width: calc(100% - 20px); box-sizing: border-box;
  }}
  #osm-attribution {{
    position: absolute; bottom: 4px; left: 8px; z-index: 999;
    font-size: 10px; color: #666;
    background: rgba(255,255,255,0.72); padding: 2px 6px; border-radius: 3px;
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
<div id="osm-attribution">© OpenStreetMap contributors</div>
<script src="{cdn_url}"></script>
<script>
  var map;
  var polygon;          // 当前 OSM/编辑模式多边形（仅 OSM 绘制后赋值；必须显式声明，
                        // 否则人工圈画兜底路径读取未声明变量会抛 ReferenceError）
  var polygonEditor;    // 当前多边形编辑器（同上）
  var manualPoints = [];   // 人工圈画的点序列
  var isEditMode = false;  // true = 编辑模式 (有 OSM 边界), false = 人工圈画模式
  var previewLine = null;  // Manual mode preview line
  var anchorPoint = {{ lng: {anchor_lon}, lat: {anchor_lat} }};  // 校区锚点 (GCJ-02，来自高德 POI)

  var statusPanel = document.getElementById('status-panel');

  function setStatus(text, type) {{
    statusPanel.textContent = text;
    statusPanel.className = type || '';
  }}

  // T31：锚点来自高德 POI（GCJ-02），地图中心直接使用锚点，不再二次转换；
  // OSM 边界坐标由 Rust 侧直连获取并转 GCJ-02 后经 drawBoundaryGcj 上屏。
  function initWithAnchor() {{
    try {{
      map = new AMap.Map('map-container', {{
        zoom: 16,
        center: [anchorPoint.lng, anchorPoint.lat],
        viewMode: '3D'
      }});

      // 显示校区锚点（GCJ-02）
      new AMap.Marker({{
        position: new AMap.LngLat(anchorPoint.lng, anchorPoint.lat),
        label: {{ content: '📍 校区锚点', offset: new AMap.Pixel(0, -20) }}
      }}).addTo(map);

      // T25: 朝向模式入口由 ORIENTATION_SCRIPT 注入的 onMapReadyForMode 接管，
      // 非朝向模式下通知 Rust 侧发起 OSM 边界自动获取（T31：Rust 直连，
      // 绕开 WebView CORS；JS 不再 fetch Overpass）。
      if (typeof window.onMapReadyForMode === 'function') {{
        window.onMapReadyForMode();
      }} else {{
        setStatus('正在从 OSM 自动获取边界...', '');
        if (window.ipc && window.ipc.postMessage) {{
          window.ipc.postMessage(JSON.stringify({{ type: 'map_ready' }}));
        }}
      }}
    }} catch (e) {{
      setStatus('地图初始化失败：' + e.message, 'error');
      enableManualMode();  // Fallback → manual drawing
    }}
  }}

  // ========== T31: GCJ-02 直接绘制 ==========
  // Rust 侧完成 Nominatim 校名解析 → Overpass 端点回退查询 → WGS-84 批量转
  // GCJ-02（开源转换，~1m 精度）→ ADR-0029 排序后，经 evaluate_script 调本
  // 函数直接上屏（坐标已是 GCJ-02，不再调用 AMap.convertFrom）。
  function drawBoundaryGcj(gcjCoords, sourceName) {{
    if (!gcjCoords || gcjCoords.length < 3) {{
      setStatus('OSM 边界坐标无效 → 人工圈画', 'error');
      enableManualMode();
      return;
    }}
    drawBoundary(gcjCoords, sourceName, true);
    setStatus('来自 OSM: ' + sourceName + ' ✓', 'success');
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
    confirmBtn.id = 'confirm-edit-btn';
    confirmBtn.textContent = '确认边界';
    confirmBtn.onclick = function() {{
      if (!polygon) {{ return; }}
      var coords = normalizedPath();
      if (coords.length < 3) {{
        setStatus('边界至少需要 3 个点', 'error');
        return;
      }}
      setStatus('已提交边界 → 等待验证', 'success');
      firstSubmittedRing = coords.slice();
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{
          type: 'confirm_boundary',
          coords: coords
        }}));
      }}
      showAreaControls();
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

    // 地图点击事件 → 落点（区分首区域与附加区域）
    map.on('click', function(e) {{ handleMapClick(e); }});

    showManualControls();
  }}

  function handleMapClick(e) {{
    if (additionalMode) {{
      addAdditionalPoint(e.lnglat);
      return;
    }}
    addManualPoint(e.lnglat);
  }}

  function addManualPoint(loc) {{
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
    confirmBtn.id = 'confirm-manual-btn';
    confirmBtn.textContent = '确认边界 (' + manualPoints.length + '个点)';
    confirmBtn.disabled = manualPoints.length < 3;
    confirmBtn.onclick = function() {{
      if (manualPoints.length >= 3) {{
        setStatus('已确认 ' + manualPoints.length + ' 个点 → 待验证', 'success');
        firstSubmittedRing = manualPoints.slice();
        if (window.ipc && window.ipc.postMessage) {{
          window.ipc.postMessage(JSON.stringify({{
            type: 'confirm_boundary',
            coords: manualPoints
          }}));
        }}
        showAreaControls();
      }}
    }};
    toolbar.appendChild(confirmBtn);
  }}

  // 启动
  if (typeof AMap === 'undefined') {{
    setStatus('高德 SDK 加载中...', '');
  }} else {{
    initWithAnchor();  // T31: 锚点 GCJ-02 直接初始化（Rust 侧负责坐标转换）
  }}
</script>
{multi_area_script}
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
        multi_area_script = MULTI_AREA_SCRIPT,
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
    fn html_triggers_rust_side_overpass_and_keeps_osm_attribution() {
        // T31：Overpass 查询改 Rust 侧直连（绕开 WebView CORS），
        // JS 只发 map_ready；页面保留 OSM 署名（ODbL）。
        let config =
            BoundaryEditPageConfig::new("abc123DEF456", "xyz789GHI012").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("map_ready"));
        assert!(!html.contains("fetchOverpassBoundary"), "JS 不再直接 fetch Overpass");
        assert!(!html.contains("overpass-api.de"), "端点只存在于 Rust 侧");
        assert!(!html.contains("university|college|school"), "JS 不再出现 | 正则");
        assert!(html.contains("© OpenStreetMap contributors"), "必须保留 OSM 署名");
    }

    #[test]
    fn overpass_queries_are_rust_side_without_webview_fetch() {
        // T31：查询与端点回退全部在 Rust 侧（data-acquisition::overpass），
        // WebView 不再承担 fetch/超时/回退职责。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("AbortController"));
        assert!(!html.contains("fetchWithTimeout"));
        assert!(!html.contains("OSM_FETCH_TIMEOUT_MS"));
        assert!(!html.contains("overpass.kumi.systems"));
    }

    #[test]
    fn toolbar_wraps_inside_small_windows() {
        // D-5：工具栏按钮在小窗口下换行排列，不超出 WebView 右缘，始终可点。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("flex-wrap: wrap"));
        assert!(html.contains("max-width: calc(100% - 20px)"));
        assert!(html.contains("justify-content: flex-end"));
    }

    #[test]
    fn html_draws_preconverted_gcj02_without_js_conversion() {
        // T31：WGS-84 → GCJ-02 转换在 Rust 侧完成，JS 直接绘制 GCJ-02 坐标，
        // 不再依赖 AMap.convertFrom（40 点/片官方接口留在 WebView 已无必要）。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("drawBoundaryGcj"));
        assert!(!html.contains("convertAndDraw"));
        assert!(!html.contains("AMap.convertFrom(["), "JS 不得再发起 convertFrom 调用");
    }

    #[test]
    fn html_contains_polygon_editor_plugin() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("plugin=AMap.PolygonEditor"));
        assert!(html.contains("PolygonEditor"));
    }

    #[test]
    fn html_contains_manual_mode_fallback() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("enableManualMode"));
        assert!(html.contains("manualPoints"));
        assert!(html.contains("confirm_boundary"));
    }

    #[test]
    fn html_contains_edit_mode_confirm_button() {
        // 验收③：编辑模式（OSM 边界已绘制）必须有确认路径——
        // 确认后步骤条打勾依赖此按钮发出 confirm_boundary
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
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
        // T31 起查询/排序全部在 Rust 侧，JS 只接收 drawBoundaryGcj/enableManualMode
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("osm_elements"), "JS 不再转发 OSM 原始要素");
        assert!(!html.contains("findBestMatch"));
        assert!(html.contains("drawBoundaryGcj"));
        assert!(!html.contains("findBestMatch"));
    }

    #[test]
    fn invalid_api_key_is_rejected() {
        let config = BoundaryEditPageConfig::new("bad@key", "xyz789");
        assert!(build_boundary_edit_page_html(&config).is_err());
    }

    #[test]
    fn missing_anchor_is_rejected_instead_of_defaulting_to_a_fixed_point() {
        // ADR-0042 §7：地图页不得以固定坐标/默认锚点代替真实校区数据。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let error = build_boundary_edit_page_html(&config).expect_err("缺少锚点必须明确失败");
        assert!(error.to_string().contains("锚点缺失"));
    }

    #[test]
    fn html_supports_multi_area_multipolygon_submission() {
        // ADR-0042 §3：地图 seam 必须能产生完整 MultiPolygon geometry。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("add-area-btn"));
        assert!(html.contains("submit-boundary-btn"));
        assert!(html.contains("confirm-area-btn"));
        assert!(html.contains("MultiPolygon"));
        assert!(html.contains("geometry"));
        assert!(html.contains("submitBoundary"));
        assert!(html.contains("handleMapClick"));
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
