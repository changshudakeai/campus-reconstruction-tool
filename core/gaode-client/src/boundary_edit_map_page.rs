//! 屏幕 4 边界编辑地图页生成（T24）+ T25 朝向模式扩展 + T31 Rust 侧直连
//! OSM 自动获取优先 (ADR-0029): **T31 起 Overpass 查询、Nominatim 校名解析、
//! WGS-84 → GCJ-02 转换与候选排序全部在 Rust 侧**（绕开 WebView CORS，
//! 调研根因见 `docs/research/candidate-data-sources-and-naming.md` §4.2），
//! JS 只发 `map_ready` 就绪信号并接收 `drawBoundaryGcj(GCJ-02 坐标)` 直接绘制。
//! 人工调整：PolygonEditor 拖拽顶点 → IPC 回传更新坐标
//! 顶点编辑增强（边界顶点编辑工单）：点击顶点 → 高亮选中（vertex_selected）；
//! 选中后相邻两条边中点各出现 "+" 按钮，点击在对应边上插入新顶点并自动选中
//! 新点；抽屉"删除选中点"经 deleteSelectedVertexFromDrawer 删除选中顶点，
//! 剩余点数 < 3 时明确拒绝（delete_vertex_rejected）且不破坏边界。
//! 人工圈画兜底：点击落点模式（沿用 T23 协议）
//! T25: 同一页面扩展朝向模式 —— 已确认边界半透明显示，点击两点画参考线。
//!
//! **T34: 地图退化为纯画布 + 消息桥**——地图内不再渲染任何 HTML 工具栏按钮
//! （确认/撤销/清空/改人工圈画/添加区域全部迁入 Slint 左侧抽屉）。地图只负责：
//! 1) 画布点击经 IPC 上行（manual_point / orientation_points / map_ready）；
//! 2) 接收 Rust 下行绘制命令（drawBoundaryGcj / enableManualMode）；
//! 3) 提供抽屉按钮调用的 JS 桥接命令（undoManualPointFromDrawer /
//!    clearManualDrawingFromDrawer / submitBoundaryFromDrawer /
//!    submitOrientationFromDrawer / clearOrientationFromDrawer）。
//!
//! **职责**: B3 高德客户端负责生成 HTML，壳层 WebView 加载渲染 —— 零业务计算
//! 在壳内（ADR-0017）。
// ignore-tidy-filelength: 边界编辑地图页单文件承载地图 HTML/JS（顶点编辑增强后
// 超 1000 行）；拆分需先立地图页拆分工单。失效里程碑：v2.1.0（2026-12-31），
// 届时把顶点编辑 JS 段拆出后消除。

use crate::error::{Error, Result};
use crate::MapViewport;

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
    /// T25: 已确认边界坐标（用于在朝向模式下显示半透明参照）
    pub existing_boundary_gcj02: Option<Vec<[f64; 2]>>,
    /// ADR-0045：安全重建 WebView 时恢复的连续校园视野。
    pub initial_viewport: Option<MapViewport>,
}

impl BoundaryEditPageConfig {
    /// 新建配置 (高度取最小值 300px)
    pub fn new(api_key: impl Into<String>, security_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            security_key: security_key.into(),
            // 锚点必须是调用方提供的真实校区坐标；没有真实锚点时构建页面明确失败，
            // 不得以固定坐标作默认点代替用户数据（ADR-0042 §7）。
            anchor_lon: f64::NAN,
            anchor_lat: f64::NAN,
            height_px: 300,
            orientation_mode: false,
            existing_boundary_gcj02: None,
            initial_viewport: None,
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

    pub fn with_initial_viewport(mut self, viewport: MapViewport) -> Self {
        self.initial_viewport = Some(viewport);
        self
    }

    pub fn effective_height_px(&self) -> u32 {
        self.height_px.max(300)
    }
}

/// T25: 朝向模式附加脚本（不经过 format!，因此无需双花括号转义）。
///
/// T34: 删除页内"确认朝向/清除重来"工具栏按钮；两点确认与清除改由左侧
/// 抽屉经 `submitOrientationFromDrawer` / `clearOrientationFromDrawer` 桥接。
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
      editable: false,
      bubble: true
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

  function handleOrientationClick(e) {
    var loc = e.lnglat;
    if (orientationPoints.length === 0) {
      orientationPoints.push([loc.lng, loc.lat]);
      setStatus('已选第 1 个点，请选第 2 个点 → 角度反馈到左侧抽屉', 'error');
      redrawOrientationPreview();
    } else if (orientationPoints.length === 1) {
      orientationPoints.push([loc.lng, loc.lat]);
      setStatus('已选好两点，角度已反馈到左侧抽屉，点"确认"提交', 'success');
      redrawOrientationPreview();
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
    // 模式互斥：真实 AMap v2.0 的 off(type) 不带回调不会移除监听，
    // 统一走 bindExclusiveClick（单一 dispatcher 换 activeClickHandler）。
    window.bindExclusiveClick(handleOrientationClick);
  };

  // T40: Rust 侧"WebView 创建成功"回调调用的可靠激活通道（不依赖
  // map_ready 是否到达）：SDK/地图尚未就绪时静默等待，由
  // onMapReadyForMode 的自动激活兜底；就绪后立即挂接两点点击。
  window.activateOrientationWhenReady = function() {
    if (!(window.__orientationConfig__ && window.__orientationConfig__.orientationMode)) {
      return;
    }
    if (typeof map === 'undefined' || !map) {
      return;
    }
    if (typeof window.initOrientationMode === 'function') {
      window.initOrientationMode();
    }
  };

  // T34: 抽屉"确认"按钮 → 提交两点朝向（沿用 confirm_orientation IPC）
  window.submitOrientationFromDrawer = function() {
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

  // T34: 抽屉"重置"按钮 → 清除两点草稿（沿用 orientation_clear IPC）
  window.clearOrientationFromDrawer = function() {
    orientationPoints = [];
    if (orientationLine) {
      map.remove(orientationLine);
      orientationLine = null;
    }
    setStatus('已清除，重新点击地图选择两点', 'error');
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({ type: 'orientation_clear' }));
    }
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
        // T40: 朝向页在 onMapReadyForMode 之外同样发送 map_ready，启用
        // Rust 侧 T37 兜底（map_ready_for_active_step 按当前步骤显式激活
        // initOrientationMode）；与页面自动激活双保险，重复调用幂等无害。
        if (window.ipc && window.ipc.postMessage) {
          window.ipc.postMessage(JSON.stringify({ type: 'map_ready' }));
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
/// 6. T25: 朝向模式扩展（两点参考线 + 已确认边界半透明参照）
/// 7. T34: 无 HTML 工具栏；撤销/清空/确认经左侧抽屉桥接命令完成
/// 8. IPC 协议扩展:
///    - `map_ready`: 地图就绪（T31 触发 Rust 侧 OSM 自动获取；T40 起朝向页
///      在 onMapReadyForMode 自动激活之外同样发送，启用 Rust 侧 T37 兜底）
///    - `activateOrientationWhenReady`: Rust 侧创建成功回调调用的激活通道
///      （T40，不依赖 map_ready；SDK 未就绪时静默等待页面自动激活）
///    - `boundary_update`: 编辑后的多边形坐标 `{type:"boundary_update", coords:[lng,lat,...]}`
///    - `vertex_selected`: 选中顶点 `{type:"vertex_selected", index, count}`
///    - `vertex_deselected`: 取消选中 `{type:"vertex_deselected"}`
///    - `delete_vertex_rejected`: 删除被拒绝（点数不足）`{type:"delete_vertex_rejected", reason}`
///    - `manual_point`: 人工圈画落点 `"lng,lat"`（含 total 供抽屉显示点数）
///    - `manual_cancel`: 撤销最后一点 `{type:"manual_cancel"}`
///    - `manual_clear`: 清空重画 `{type:"manual_clear"}`
///    - `confirm_boundary`: 确认最终边界 `{type:"confirm_boundary", coords:[...]}`
///    - `orientation_points`: 朝向两点 `{type:"orientation_points", points:[[lng,lat],[lng,lat]]}`
///    - `confirm_orientation`: 确认朝向 `{type:"confirm_orientation", points:[[...],[...]]}`
///    - `orientation_clear`: 清除朝向点 `{type:"orientation_clear"}`
///
/// **红线**:
/// - 密钥只经 F1（通过 map_page.rs 现有校验）；禁止硬编码真实 key
/// - 禁止候选列表 UI（ADR-0029）
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

    // T25: 将已确认边界坐标序列化为 JSON 注入 JS
    let existing_boundary_json = serde_json::to_string(&config.existing_boundary_gcj02)
        .unwrap_or_else(|_| "null".to_string());
    let initial_viewport_json =
        serde_json::to_string(&config.initial_viewport).unwrap_or_else(|_| "null".to_string());

    let base_html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>圈选边界</title>
<style>
  /* T32：禁止 body/文档横向滚动——AMap 初始化若读取到未布局的容器宽度，
     可能把地图画布撑到视口右侧之外（T31-D6）。T34：不再有工具栏按钮，
     保留该约束防止画布溢出。 */
  html, body {{ margin: 0; padding: 0; height: 100%; overflow-x: hidden; }}
  /* T37：地图容器高度随 WebView 视口填满（html/body 100%），不再固定
     300px——WebView 槽位已按 Slint 布局填满窗口，固定高度会在下方留白。 */
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
<div id="osm-attribution">© OpenStreetMap contributors</div>
<script src="{cdn_url}"></script>
<script>
  var map;
  var polygon;          // 当前 OSM/编辑模式多边形（OSM 绘制后赋值；必须显式声明，
                        // 否则人工圈画兜底路径读取未声明变量会抛 ReferenceError）
  var polygonEditor;    // 当前多边形编辑器（同上）
  var manualPoints = [];   // 人工圈画的点序列
  var isEditMode = false;  // true = 编辑模式 (有 OSM 边界), false = 人工圈画模式
  var previewLine = null;  // Manual mode preview line
  // ── 顶点编辑选中态（边界顶点编辑工单）──────────────────────────
  var selectedVertexIndex = -1;  // 当前选中顶点索引；-1 = 未选中
  var vertexHighlight = null;    // 选中顶点的高亮圆标
  var midInsertMarkers = [];     // 相邻两条边中点的 "+" 按钮
  var VERTEX_SELECT_PX = 16;     // 点击命中顶点的像素阈值
  var anchorPoint = {{ lng: {anchor_lon}, lat: {anchor_lat} }};  // 校区锚点 (GCJ-02，来自高德 POI)
  var initialViewport = {initial_viewport_json};

  var statusPanel = document.getElementById('status-panel');

  // 模式互斥统一入口（真实 AMap v2.0 事件语义修复）：
  // `map.off(type)` 不带回调函数不会移除任何监听（官方 off(type, fn) 按函数
  // 移除；clearEvents(type) 虽能清空一类，但可能连带清掉 PolygonEditor 内部
  // 在地图上注册的点击监听，破坏拖拽/加边）。因此采用单一 dispatcher：
  // 地图上始终只挂 dispatchMapClick 这一个监听，模式切换只换
  // activeClickHandler，既不残留旧模式监听，也不触碰 PolygonEditor 的监听。
  var activeClickHandler = null;
  function dispatchMapClick(e) {{
    if (activeClickHandler) {{
      activeClickHandler(e);
    }}
  }}
  window.bindExclusiveClick = function(handler) {{
    map.off('click', dispatchMapClick);
    map.on('click', dispatchMapClick);
    activeClickHandler = handler;
  }};

  function setStatus(text, type) {{
    statusPanel.textContent = text;
    statusPanel.className = type || '';
  }}

  // T37：把地图容器同步到 WebView 视口尺寸（html/body 100% + 显式
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

  // T31：锚点来自高德 POI（GCJ-02），地图中心直接使用锚点，不再二次转换；
  // OSM 边界坐标由 Rust 侧直连获取并转 GCJ-02 后经 drawBoundaryGcj 上屏。
  function initWithAnchor() {{
    try {{
      // T32/T37：AMap 初始化前把容器宽高钳制到当前 WebView 视口（布局
      // 可能尚未稳定，AMap 会按容器当前尺寸创建画布；T37 高度随视口填满）
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

      // T32/T37：布局完成后同步一次画布尺寸（含 map.resize()）
      syncContainerSize();

      // T32/T34/T37：WebView bounds 变化（窗口 resize 或抽屉开合让位）时
      // 同步地图容器尺寸，防止画布残留旧宽高导致横向溢出或下方空白。
      window.addEventListener('resize', syncContainerSize);

      // 显示校区锚点（GCJ-02）
      new AMap.Marker({{
        position: new AMap.LngLat(anchorPoint.lng, anchorPoint.lat),
        label: {{ content: '📍 校区锚点', offset: new AMap.Pixel(0, -20) }}
      }}).addTo(map);

      // T25: 朝向模式入口由 ORIENTATION_SCRIPT 注入的 onMapReadyForMode 接管；
      // 非朝向模式下通知 Rust 侧发起 OSM 边界自动获取（T31：Rust 直连）。
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

  // 已确认的方案边界来自当前应用会话缓存；它可能最初来自 OSM，也可能由
  // 用户人工圈画，因此恢复时不得伪称数据来源，只呈现调用方注入的本地化状态。
  function drawRestoredBoundaryGcj(gcjCoords, statusText) {{
    if (!gcjCoords || gcjCoords.length < 3) {{
      enableManualMode();
      return;
    }}
    drawBoundary(gcjCoords, '', true);
    setStatus(statusText, 'success');
  }}

  function drawBoundary(coords, name, editable) {{
    isEditMode = editable;

    polygon = new AMap.Polygon({{
      path: coords,
      strokeColor: '#3498db',
      strokeWeight: 3,
      fillOpacity: 0.3,
      fillColor: '#3498db',
      editMode: editable,
      // 顶点编辑：多边形本体点击冒泡到地图，供"点击边界标记点选中"命中检测
      bubble: true
    }});

    map.add(polygon);

    if (editable && AMap.PolygonEditor) {{
      polygonEditor = new AMap.PolygonEditor(map, polygon, {{
        template: {{
          dragging: true,
          circleMarkerStyle: {{ fillColor: '#e74c3c', fillOpacity: 0.6, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, bubble: true, fontSize: '12px', fontColor: '#fff', borderRadius: '50%', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }},
          vertexMarkerStyle: {{ fillColor: '#e74c3c', fillOpacity: 0.8, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, bubble: true, fontSize: '12px', fontColor: '#fff', borderRadius: '50%', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }},
          interiorMarkerStyle: {{ fillColor: '#3498db', fillOpacity: 1, strokeColor: '#fff', strokeWidth: 2, strokeOpacity: 0.9, strokeWeight: 2, cursor: 'pointer', clickable: true, bubble: true, fontSize: '12px', fontColor: '#fff', borderRadius: '4px', fontTextAlign: 'center', fontStrokeColor: '#fff', fontStrokeWidth: 1 }}
        }}
      }});

      // 监听编辑事件
      polygonEditor.on('dragnode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('addnode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('removenode', function(e) {{ updateFromEditor(); }});
      polygonEditor.on('adjust', function(e) {{ updateFromEditor(); }});

      polygonEditor.open();

      // 顶点编辑：编辑态地图点击 → 按像素距离命中顶点选中/点击空白取消选中
      window.bindExclusiveClick(editClickHandler);
      selectedVertexIndex = -1;
      removeVertexOverlays();

      setStatus('已加载 OSM 边界 — 拖动顶点可调整 · 经左侧抽屉确认提交', 'success');
    }} else {{
      enableManualMode();
    }}
  }}

  // 当前多边形路径归一化为 [lng, lat] 数组（GCJ-02）
  function normalizedPath() {{
    if (!polygon) {{ return []; }}
    return polygon.getPath().map(function(p) {{ return [p.lng, p.lat]; }});
  }}

  // ========== T24: 编辑回调 ==========
  function updateFromEditor() {{
    refreshVertexOverlays();
    if (polygon) {{
      var coords = normalizedPath();  // GCJ-02 [lng,lat]×N
      var payload = {{ type: 'boundary_update', coords: coords }};
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify(payload));
      }}
    }}
  }}

  // ========== 顶点编辑：选中 / 高亮 / 相邻边中点 "+" / 删除（边界顶点编辑工单）==========

  // 编辑态地图点击：按像素距离命中最近的顶点 → 选中；否则取消选中
  function editClickHandler(e) {{
    if (!isEditMode || !polygon) {{ return; }}
    var path = normalizedPath();
    if (path.length < 3) {{ return; }}
    if (typeof map.lngLatToContainer !== 'function') {{ clearVertexSelection(); return; }}
    var clickPixel = map.lngLatToContainer(e.lnglat);
    var best = -1;
    var bestDistSq = Infinity;
    for (var i = 0; i < path.length; i++) {{
      var pixel = map.lngLatToContainer(new AMap.LngLat(path[i][0], path[i][1]));
      var dx = pixel.x - clickPixel.x;
      var dy = pixel.y - clickPixel.y;
      var distSq = dx * dx + dy * dy;
      if (distSq < bestDistSq) {{ bestDistSq = distSq; best = i; }}
    }}
    if (best >= 0 && Math.sqrt(bestDistSq) <= VERTEX_SELECT_PX) {{
      selectVertex(best);
    }} else {{
      clearVertexSelection();
    }}
  }}

  // 选中顶点：高亮 + 相邻两条边中点各出现 "+" 按钮，并回传 vertex_selected
  function selectVertex(index) {{
    selectedVertexIndex = index;
    refreshVertexOverlays();
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{
        type: 'vertex_selected',
        index: index,
        count: normalizedPath().length
      }}));
    }}
  }}

  // 取消选中：移除高亮与 "+" 按钮，回传 vertex_deselected
  function clearVertexSelection() {{
    if (selectedVertexIndex < 0 && vertexHighlight === null && midInsertMarkers.length === 0) {{
      return;
    }}
    selectedVertexIndex = -1;
    removeVertexOverlays();
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'vertex_deselected' }}));
    }}
  }}

  function removeVertexOverlays() {{
    if (vertexHighlight) {{ map.remove(vertexHighlight); vertexHighlight = null; }}
    midInsertMarkers.forEach(function(marker) {{ map.remove(marker); }});
    midInsertMarkers = [];
  }}

  // 按当前选中态重绘高亮与 "+" 按钮（拖拽/增删点后同步跟随）
  function refreshVertexOverlays() {{
    removeVertexOverlays();
    if (!isEditMode || selectedVertexIndex < 0 || !polygon) {{ return; }}
    var path = normalizedPath();
    var n = path.length;
    if (n < 3 || selectedVertexIndex >= n) {{ selectedVertexIndex = -1; return; }}
    var at = function(i) {{ var p = path[i]; return new AMap.LngLat(p[0], p[1]); }};

    vertexHighlight = new AMap.CircleMarker({{
      center: at(selectedVertexIndex),
      radius: 10,
      strokeColor: '#f39c12',
      strokeWeight: 3,
      strokeOpacity: 1,
      fillColor: '#f1c40f',
      fillOpacity: 0.95,
      cursor: 'pointer',
      zIndex: 200
    }});
    map.add(vertexHighlight);

    // 选中点两端相邻的两条边中点各一个 "+" 按钮
    var prev = (selectedVertexIndex - 1 + n) % n;
    var next = (selectedVertexIndex + 1) % n;
    addMidInsertMarker(prev, selectedVertexIndex);
    addMidInsertMarker(selectedVertexIndex, next);
  }}

  // 在边 (fromIndex → fromIndex+1) 的中点放置 "+" 按钮，点击插入新顶点
  function addMidInsertMarker(fromIndex, toIndex) {{
    var path = normalizedPath();
    var a = path[fromIndex];
    var b = path[toIndex];
    var mid = new AMap.LngLat((a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
    var marker = new AMap.TextMarker({{
      text: '+',
      position: mid,
      offset: new AMap.Pixel(-10, -10),
      style: {{
        backgroundColor: '#2ecc71',
        borderColor: '#ffffff',
        color: '#ffffff',
        fontSize: '14px',
        width: '20px',
        height: '20px',
        textAlign: 'center',
        lineHeight: '20px',
        cursor: 'pointer'
      }},
      zIndex: 300
    }});
    marker.on('click', function() {{ insertVertexAtEdge(fromIndex); }});
    map.add(marker);
    midInsertMarkers.push(marker);
  }}

  // 在 (fromIndex → fromIndex+1) 边的中点插入新顶点；新点自动选中，
  // 可继续选中/拖动/插入（拖动由 PolygonEditor dragnode 事件接管）。
  function insertVertexAtEdge(fromIndex) {{
    if (selectedVertexIndex < 0) {{ return; }}
    var path = normalizedPath();
    var n = path.length;
    if (n < 3 || fromIndex < 0 || fromIndex >= n) {{ return; }}
    var toIndex = (fromIndex + 1) % n;
    var a = path[fromIndex];
    var b = path[toIndex];
    var mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    var newPath = [];
    for (var i = 0; i <= fromIndex; i++) {{ newPath.push(path[i]); }}
    newPath.push(mid);
    for (var j = fromIndex + 1; j < n; j++) {{ newPath.push(path[j]); }}
    setPolygonPath(newPath);
    selectVertex(fromIndex + 1);  // 新点可用、可继续选中/拖动/插入
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'boundary_update', coords: newPath }}));
    }}
  }}

  // 程序化替换多边形路径：关闭编辑器 → setPath → 重开 → 重绘选中叠加层
  function setPolygonPath(coords) {{
    var lngLats = coords.map(function(c) {{ return new AMap.LngLat(c[0], c[1]); }});
    if (polygonEditor) {{ polygonEditor.close(); }}
    polygon.setPath(lngLats);
    if (polygonEditor) {{ polygonEditor.open(); }}
    refreshVertexOverlays();
  }}

  // 抽屉"删除选中点"：有选中点时删除；剩余点数 < 3 时明确拒绝，不破坏边界
  window.deleteSelectedVertexFromDrawer = function() {{
    if (!isEditMode || selectedVertexIndex < 0 || !polygon) {{ return; }}
    var path = normalizedPath();
    if (path.length <= 3) {{
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{
          type: 'delete_vertex_rejected',
          reason: 'too_few_points'
        }}));
      }}
      return;
    }}
    var index = selectedVertexIndex;
    var newPath = [];
    for (var i = 0; i < path.length; i++) {{ if (i !== index) {{ newPath.push(path[i]); }} }}
    selectedVertexIndex = -1;
    setPolygonPath(newPath);
    removeVertexOverlays();
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'vertex_deselected' }}));
      window.ipc.postMessage(JSON.stringify({{ type: 'boundary_update', coords: newPath }}));
    }}
  }};

  // ========== T24: 人工圈画兜底 ==========
  function enableManualMode() {{
    if (polygon) {{ map.remove(polygon); polygon = null; }}
    if (polygonEditor) {{ polygonEditor.close(); polygonEditor = null; }}

    isEditMode = false;
    manualPoints = [];
    selectedVertexIndex = -1;
    removeVertexOverlays();

    setStatus('人工圈画模式：点击地图添加控制点（操作在左侧抽屉）', 'error');

    // 模式互斥：只保留人工圈画模式的点击 handler（dispatcher 会换掉编辑态监听）
    window.bindExclusiveClick(handleMapClick);
  }}

  // T24/T34: 地图点击落点入口（人工圈画模式；供页面自身点击处理与
  // 真实 WebView seam 测试共用，纯画布 + 消息桥语义）
  function handleMapClick(e) {{
    addManualPoint(e.lnglat);
  }}

  function addManualPoint(loc) {{
    manualPoints.push([loc.lng, loc.lat]);

    // 预览连线 + IPC 上报最新状态（含 total 供抽屉显示点数）
    redrawPreviewLine();
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
      window.previewLine = new AMap.Polyline({{
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

  // ========== T34: 抽屉按钮 → JS 桥接命令（纯画布 + 消息桥）==========
  // 撤销上一点：弹出 manualPoints 并重绘预览，回传 manual_cancel 让 Rust 同步点数。
  window.undoManualPointFromDrawer = function() {{
    if (manualPoints.length > 0) {{
      manualPoints.pop();
      redrawPreviewLine();
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify({{ type: 'manual_cancel' }}));
      }}
    }}
  }};

  // 清空重画：清除多边形/编辑器/预览，进入人工圈画模式，回传 manual_clear。
  window.clearManualDrawingFromDrawer = function() {{
    manualPoints = [];
    if (polygon) {{ map.remove(polygon); polygon = null; }}
    if (polygonEditor) {{ polygonEditor.close(); polygonEditor = null; }}
    if (previewLine) {{ map.remove(previewLine); previewLine = null; }}
    isEditMode = false;
    selectedVertexIndex = -1;
    removeVertexOverlays();
    // 清空重画后进入人工圈画模式：点击落点（模式互斥绑定，防止编辑态残留）
    window.bindExclusiveClick(handleMapClick);
    setStatus('已清空，点击地图重新绘制（操作在左侧抽屉）', 'error');
    if (window.ipc && window.ipc.postMessage) {{
      window.ipc.postMessage(JSON.stringify({{ type: 'manual_clear' }}));
    }}
  }};

  // 确认边界：读取当前多边形路径（编辑模式）或人工点序列（圈画模式），
  // 复用 confirm_boundary IPC 交给 Rust/B5 校验与落库。
  window.submitBoundaryFromDrawer = function() {{
    var coords = null;
    if (polygon) {{
      var path = normalizedPath();
      if (path.length >= 3) {{ coords = path; }}
    }} else if (manualPoints.length >= 3) {{
      coords = manualPoints.slice();
    }}
    if (!coords) {{
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

  // 启动（T32：等窗口 load，确保 WebView 布局完成、容器宽度正确，
  // 避免 AMap 在未布局的容器上创建过宽画布）
  function boot() {{
    if (typeof AMap === 'undefined') {{
      setStatus('高德 SDK 加载中...', '');
    }} else {{
      initWithAnchor();  // T31: 锚点 GCJ-02 直接初始化（Rust 侧负责坐标转换）
    }}
  }}
  if (document.readyState === 'complete') {{
    boot();
  }} else {{
    window.addEventListener('load', boot);
  }}
</script>
{orientation_script}
</body>
</html>"#,
        cdn_url = cdn_url,
        security_js_code = config.security_key,
        anchor_lon = config.anchor_lon,
        anchor_lat = config.anchor_lat,
        orientation_mode = config.orientation_mode,
        existing_boundary_json = existing_boundary_json,
        initial_viewport_json = initial_viewport_json,
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
        // T31：Overpass 查询改 Rust 侧直连（绕开 WebView CORS），JS 只发 map_ready；
        // 页面保留 OSM 署名（ODbL）。
        let config =
            BoundaryEditPageConfig::new("abc123DEF456", "xyz789GHI012").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("map_ready"));
        assert!(
            !html.contains("fetchOverpassBoundary"),
            "JS 不再直接 fetch Overpass"
        );
        assert!(!html.contains("overpass-api.de"), "端点只存在于 Rust 侧");
        assert!(
            !html.contains("university|college|school"),
            "JS 不再出现 | 正则"
        );
        assert!(
            html.contains("© OpenStreetMap contributors"),
            "必须保留 OSM 署名"
        );
    }

    #[test]
    fn overpass_queries_are_rust_side_without_webview_fetch() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("AbortController"));
        assert!(!html.contains("fetchWithTimeout"));
        assert!(!html.contains("OSM_FETCH_TIMEOUT_MS"));
        assert!(!html.contains("overpass.kumi.systems"));
    }

    #[test]
    fn html_has_no_toolbar_buttons() {
        // T34：地图内不再出现任何 HTML 工具栏按钮（确认/撤销/清空/改人工圈画
        // /添加区域全部迁入左侧抽屉），地图退化为纯画布 + 消息桥。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("map-toolbar"), "不得再有 #map-toolbar 容器");
        assert!(!html.contains("control-btn"), "不得再有工具栏按钮样式");
        assert!(!html.contains("confirm-edit-btn"), "不得再有编辑确认按钮");
        assert!(!html.contains("confirm-manual-btn"), "不得再有人工确认按钮");
        assert!(!html.contains("add-area-btn"), "不得再有添加区域按钮");
        assert!(
            !html.contains("submit-boundary-btn"),
            "不得再有提交边界按钮"
        );
        assert!(!html.contains("confirm-area-btn"), "不得再有区域确认按钮");
    }

    #[test]
    fn html_forbids_horizontal_overflow_and_clamps_map_container() {
        // T32：body/文档禁止横向滚动；地图容器 max-width 100% 且初始化前
        // 钳制到视口宽，防止 AMap 画布把内容挤出 WebView 视口（T31-D6）。
        // T37：容器高度随 WebView 视口填满（html/body 100% + 显式
        // innerHeight + map.resize()），不再固定 300px 造成下方留白。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(
            html.contains("overflow-x: hidden"),
            "html/body 必须禁止横向滚动"
        );
        assert!(
            html.contains("#map-container { width: 100%; max-width: 100%; box-sizing: border-box;"),
            "地图容器必须钳制在视口内"
        );
        assert!(
            html.contains("container.style.maxWidth = '100%'"),
            "AMap 初始化前必须钳制容器宽度"
        );
        assert!(
            html.contains("#map-container { width: 100%; max-width: 100%; box-sizing: border-box; height: 100%; min-height: 300px;"),
            "地图容器高度必须随视口填满（height: 100%，仅保留最小高度兜底）"
        );
        assert!(
            !html.contains("border-box; height: 300px"),
            "地图容器不得再固定 300px 高度（T37 根因）"
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
        assert!(
            html.contains("window.addEventListener('resize'"),
            "窗口 resize 必须同步地图容器与画布（抽屉开合让位也依赖此机制）"
        );
        assert!(
            html.contains("window.addEventListener('load', boot)"),
            "地图初始化必须等窗口 load（WebView 布局完成）"
        );
    }

    #[test]
    fn html_draws_preconverted_gcj02_without_js_conversion() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("drawBoundaryGcj"));
        assert!(
            html.contains("drawRestoredBoundaryGcj"),
            "方案会话缓存命中后必须能重绘已确认边界"
        );
        assert!(!html.contains("convertAndDraw"));
        assert!(
            !html.contains("AMap.convertFrom(["),
            "JS 不得再发起 convertFrom 调用"
        );
    }

    #[test]
    fn html_restores_and_reports_map_session_viewport() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789")
            .with_anchor(116.4, 39.9)
            .with_initial_viewport(crate::MapViewport::new(121.44, 31.03, 17.0));
        let html = build_boundary_edit_page_html(&config).unwrap();

        assert!(html.contains("initialViewport"));
        assert!(html.contains("type: 'viewport_changed'"));
        assert!(html.contains("map.on('moveend'"));
        assert!(html.contains("map.on('zoomend'"));
    }

    #[test]
    fn html_contains_polygon_editor_plugin() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("plugin=AMap.PolygonEditor"));
        assert!(html.contains("PolygonEditor"));
    }

    #[test]
    fn html_contains_manual_mode_fallback_without_toolbar() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("enableManualMode"));
        assert!(html.contains("manualPoints"));
        assert!(html.contains("manual_point"));
        assert!(html.contains("confirm_boundary"));
    }

    #[test]
    fn html_edit_mode_confirmation_routes_through_drawer_bridge() {
        // T34：编辑模式（OSM 边界已绘制）必须保留确认通道——经抽屉
        // submitBoundaryFromDrawer 读取 getPath 归一化坐标后发 confirm_boundary。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("getPath()"));
        assert!(!html.contains("getCoordinates"));
        assert!(
            html.contains("submitBoundaryFromDrawer"),
            "抽屉确认按钮需要 JS 桥接命令"
        );
    }

    #[test]
    fn html_exposes_drawer_bridge_commands() {
        // T34：抽屉 ① 撤销/清空/确认 对应三个 JS 桥接命令，且不残留工具栏 UI。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("undoManualPointFromDrawer"));
        assert!(html.contains("clearManualDrawingFromDrawer"));
        assert!(html.contains("submitBoundaryFromDrawer"));
        assert!(!html.contains("showEditControls"));
        assert!(!html.contains("showManualControls"));
        assert!(!html.contains("showAreaControls"));
    }

    #[test]
    fn html_exposes_vertex_editing_selection_and_delete_bridge() {
        // 顶点编辑：选中/取消选中/删除拒绝 IPC 与抽屉"删除选中点"桥接命令。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(
            html.contains("deleteSelectedVertexFromDrawer"),
            "抽屉删除选中点按钮需要 JS 桥接命令"
        );
        assert!(html.contains("type: 'vertex_selected'"));
        assert!(html.contains("type: 'vertex_deselected'"));
        assert!(html.contains("type: 'delete_vertex_rejected'"));
        assert!(html.contains("selectedVertexIndex"));
    }

    #[test]
    fn html_selected_vertex_shows_adjacent_midpoint_plus_buttons_only_when_selected() {
        // 选中后相邻两条边中点才出现 "+" 按钮；未选中时无叠加层。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(
            html.contains("addMidInsertMarker(prev, selectedVertexIndex)")
                && html.contains("addMidInsertMarker(selectedVertexIndex, next)"),
            "选中点两端相邻的两条边必须各有一个中点 '+' 按钮"
        );
        assert!(
            html.contains("if (!isEditMode || selectedVertexIndex < 0 || !polygon) { return; }")
                || html.contains("refreshVertexOverlays"),
            "未选中/非编辑态不得绘制 '+' 按钮"
        );
        assert!(html.contains("marker.on('click'"));
        assert!(html.contains("insertVertexAtEdge"));
        assert!(
            html.contains("new AMap.TextMarker"),
            "中点 '+' 按钮使用 TextMarker 叠加"
        );
    }

    #[test]
    fn html_delete_vertex_guards_three_points_and_posts_rejected_payload() {
        // 删除前剩余点数 <= 3 时拒绝并回传 delete_vertex_rejected，不修改路径。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("path.length <= 3"));
        assert!(html.contains("delete_vertex_rejected"));
        assert!(html.contains("reason: 'too_few_points'"));
    }

    #[test]
    fn html_edit_mode_click_selects_nearest_vertex_within_threshold() {
        // 编辑态地图点击按像素距离命中顶点选中；点击空白取消选中。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("editClickHandler"));
        assert!(html.contains("lngLatToContainer"));
        assert!(html.contains("VERTEX_SELECT_PX"));
        assert!(html.contains("selectVertex(best)"));
        assert!(html.contains("clearVertexSelection()"));
    }

    #[test]
    fn html_mode_click_binding_uses_single_dispatcher_without_bare_off_or_clearevents() {
        // 严重 bug 回归：OSM 边界抓取失败进入人工圈画后，若重新获取成功进入
        // 编辑态，旧模式点击监听不得残留（否则点击地图会同时触发 manual_point）。
        // 真实 AMap v2.0 的 off(type) 不带回调不生效，clearEvents 又可能清掉
        // PolygonEditor 内部监听，因此必须走 bindExclusiveClick + dispatcher。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(
            html.contains("window.bindExclusiveClick = function(handler)"),
            "模式互斥必须统一经 bindExclusiveClick 绑定"
        );
        assert!(
            html.contains("function dispatchMapClick(e)"),
            "必须存在单一 dispatcher 转发当前模式点击"
        );
        assert!(
            html.contains("map.off('click', dispatchMapClick)"),
            "解除绑定必须按具体函数移除（AMap v2.0 off(type) 无参不生效）"
        );
        assert!(
            !html.contains("map.off('click');") && !html.contains("map.off('click')"),
            "不得再出现无回调的裸 map.off('click')"
        );
        assert!(
            !html.contains("map.clearEvents("),
            "不得用 clearEvents('click') 清空一类监听（会波及 PolygonEditor 内部监听）"
        );
        assert!(
            html.contains("window.bindExclusiveClick(handleMapClick)")
                && html.contains("window.bindExclusiveClick(editClickHandler)"),
            "人工圈画与编辑态都必须经 bindExclusiveClick 切换"
        );
    }

    #[test]
    fn html_editor_markers_and_polygon_bubble_clicks_to_map() {
        // 顶点/内部标记与多边形本体点击冒泡到地图，供选中命中检测。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(html.contains("bubble: true"));
        assert!(
            html.matches("bubble: true").count() >= 4,
            "多边形 + 三种编辑器标记样式都应冒泡点击"
        );
    }

    #[test]
    fn html_sorting_stays_out_of_js() {
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("osm_elements"), "JS 不再转发 OSM 原始要素");
        assert!(!html.contains("findBestMatch"));
        assert!(html.contains("drawBoundaryGcj"));
    }

    #[test]
    fn invalid_api_key_is_rejected() {
        let config = BoundaryEditPageConfig::new("bad@key", "xyz789");
        assert!(build_boundary_edit_page_html(&config).is_err());
    }

    #[test]
    fn missing_anchor_is_rejected_instead_of_defaulting_to_a_fixed_point() {
        // ADR-0042 §7：地图页不得以固定坐标作默认锚点代替真实校区数据。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789");
        let error = build_boundary_edit_page_html(&config).expect_err("缺少锚点必须明确失败");
        assert!(error.to_string().contains("锚点缺失"));
    }

    #[test]
    fn html_keeps_boundary_seam_without_multi_area_toolbar() {
        // T34：多区域"添加区域/提交边界"工具栏随工具栏整体删除；confirm_boundary
        // IPC 通道保留（s1_15 契约仍可经桌面 seam 提交 MultiPolygon geometry）。
        let config = BoundaryEditPageConfig::new("abc123", "xyz789").with_anchor(116.4, 39.9);
        let html = build_boundary_edit_page_html(&config).unwrap();
        assert!(!html.contains("add-area-btn"));
        assert!(!html.contains("submit-boundary-btn"));
        assert!(!html.contains("confirm-area-btn"));
        assert!(!html.contains("additionalRings"));
        assert!(html.contains("confirm_boundary"));
        assert!(html.contains("geometry") || html.contains("coords"));
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
        assert!(
            html.contains("bubble: true"),
            "朝向参考多边形必须冒泡点击到地图，否则地图两点点击被覆盖物吞掉"
        );
        assert!(
            html.contains("activateOrientationWhenReady"),
            "T40：Rust 侧创建成功激活通道需要页面激活函数"
        );
        assert!(
            html.contains("__orientationConfig__.orientationMode"),
            "T40：激活函数必须只作用于朝向模式页面"
        );
        assert!(
            html.matches("type: 'map_ready'").count() >= 2,
            "T40：朝向页在 onMapReadyForMode 之外也必须发送 map_ready 启用 Rust 侧兜底"
        );
        assert!(
            html.contains("submitOrientationFromDrawer"),
            "朝向抽屉确认需要 JS 桥接命令"
        );
        assert!(
            html.contains("clearOrientationFromDrawer"),
            "朝向抽屉重置需要 JS 桥接命令"
        );
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
