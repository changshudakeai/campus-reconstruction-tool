//! 多区域（MultiPolygon）边界提交脚本（ADR-0042 §3）。
//!
//! 与主模板 `boundary_edit_map_page` 同页协作：共享 `map`、`polygon`、
//! `manualPoints`、`normalizedPath`、`setStatus` 等全局状态；独立成文件是为了
//! 让 HTML 模板文件保持在千行红线内。首个区域仍以 Polygon 直交；用户“添加区域”
//! 后，提交时把所有外环组成 MultiPolygon 一次性交给 F9，不在壳层重建几何。
pub(crate) const MULTI_AREA_SCRIPT: &str = r#"
<script>
  var firstSubmittedRing = null; // 首次作为 Polygon 提交的环
  var additionalRings = [];      // 已确认的附加区域（每个都是 [lng,lat]×N 环）
  var additionalPoints = [];     // 附加区域正在绘制的点序列
  var additionalMode = false;    // 是否正在绘制附加区域

  function showAreaControls() {
    var toolbar = document.getElementById('map-toolbar');
    toolbar.innerHTML = '';

    var addBtn = document.createElement('button');
    addBtn.className = 'control-btn';
    addBtn.id = 'add-area-btn';
    addBtn.textContent = '添加区域';
    addBtn.onclick = function() { startAdditionalArea(); };
    toolbar.appendChild(addBtn);

    var submitBtn = document.createElement('button');
    submitBtn.className = 'control-btn primary';
    submitBtn.id = 'submit-boundary-btn';
    submitBtn.textContent = '提交边界（' + (1 + additionalRings.length) + ' 个区域）';
    submitBtn.onclick = function() { submitBoundary(); };
    toolbar.appendChild(submitBtn);
  }

  function startAdditionalArea() {
    additionalMode = true;
    additionalPoints = [];
    if (polygon) { map.remove(polygon); polygon = null; }
    if (polygonEditor) { polygonEditor.close(); polygonEditor = null; }
    isEditMode = false;
    setStatus('添加区域：点击地图添加控制点', '');
    map.off('click');
    map.on('click', function(e) { addAdditionalPoint(e.lnglat); });
    showAdditionalControls();
  }

  function addAdditionalPoint(loc) {
    additionalPoints.push([loc.lng, loc.lat]);
    redrawAdditionalPreview();
    showAdditionalControls();
  }

  function redrawAdditionalPreview() {
    if (!window.additionalPreviewLine) {
      window.additionalPreviewLine = new AMap.Polyline({
        path: additionalPoints,
        strokeColor: '#e67e22',
        strokeWeight: 2,
        lineDash: [5, 5]
      });
      map.add(window.additionalPreviewLine);
    } else {
      window.additionalPreviewLine.setPath(additionalPoints);
    }
  }

  function showAdditionalControls() {
    var toolbar = document.getElementById('map-toolbar');
    toolbar.innerHTML = '';

    var undoBtn = document.createElement('button');
    undoBtn.className = 'control-btn';
    undoBtn.textContent = '撤销上一个点';
    undoBtn.onclick = function() {
      if (additionalPoints.length > 0) {
        additionalPoints.pop();
        redrawAdditionalPreview();
        showAdditionalControls();
      }
    };
    toolbar.appendChild(undoBtn);

    var cancelBtn = document.createElement('button');
    cancelBtn.className = 'control-btn';
    cancelBtn.textContent = '取消添加';
    cancelBtn.onclick = function() {
      additionalPoints = [];
      additionalMode = false;
      if (window.additionalPreviewLine) {
        map.remove(window.additionalPreviewLine);
        window.additionalPreviewLine = null;
      }
      showAreaControls();
    };
    toolbar.appendChild(cancelBtn);

    var confirmBtn = document.createElement('button');
    confirmBtn.className = 'control-btn primary';
    confirmBtn.id = 'confirm-area-btn';
    confirmBtn.textContent = '确认区域（' + additionalPoints.length + ' 个点）';
    confirmBtn.disabled = additionalPoints.length < 3;
    confirmBtn.onclick = function() {
      if (additionalPoints.length >= 3) {
        additionalRings.push(additionalPoints.slice());
        additionalPoints = [];
        additionalMode = false;
        if (window.additionalPreviewLine) {
          map.remove(window.additionalPreviewLine);
          window.additionalPreviewLine = null;
        }
        setStatus('已确认附加区域，可继续添加或提交', 'success');
        showAreaControls();
      }
    };
    toolbar.appendChild(confirmBtn);
  }

  function buildMultiRingPayload() {
    var rings = [];
    if (polygon) {
      var path = normalizedPath();
      if (path.length >= 3) rings.push(path);
    } else if (firstSubmittedRing && firstSubmittedRing.length >= 3) {
      rings.push(firstSubmittedRing);
    }
    for (var i = 0; i < additionalRings.length; i++) {
      if (additionalRings[i].length >= 3) rings.push(additionalRings[i]);
    }
    return rings;
  }

  function submitBoundary() {
    var rings = buildMultiRingPayload();
    if (rings.length === 0) {
      setStatus('没有可提交的边界', 'error');
      return;
    }
    for (var i = 0; i < rings.length; i++) {
      if (rings[i].length < 3) {
        setStatus('每个区域至少需要 3 个点', 'error');
        return;
      }
    }
    if (rings.length === 1) {
      setStatus('已提交边界 → 等待验证', 'success');
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({
          type: 'confirm_boundary',
          coords: rings[0]
        }));
      }
      return;
    }
    setStatus('已提交 ' + rings.length + ' 个区域 → 等待验证', 'success');
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({
        type: 'confirm_boundary',
        geometry: {
          type: 'MultiPolygon',
          coordinates: rings.map(function(ring) { return [ring]; })
        }
      }));
    }
  }
</script>
"#;
