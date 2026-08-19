/*
 * T52 第五步 3D 方块预览渲染器。
 *
 * 数据由 Rust 侧 F9 从与导出同源的 B18 BlockModel 序列化而来：
 *   {v, palette, bounds, count, simplified, runs: [[paletteIndex,x0,x1,y,z],...]}
 * 渲染器只做呈现：隐藏面剔除 + 同色面贪婪合并 + 轨道旋转/缩放/复位。
 * 本文件与 three.min.js（MIT）一起随 desktop-shell 打包，不联网加载资源，
 * 不包含任何用户可见文案（按钮与提示全部在 Slint 侧经 zh-CN.json 注入）。
 */
(function () {
  "use strict";

  // 初始化前到达的数据暂存（HTML 内嵌负载或早到的 evaluate_script 推送）。
  var pendingPayload = window.__previewPending === undefined ? null : window.__previewPending;
  window.__previewError = null;
  window.__previewReady = false;
  window.__previewFps = 0;

  function report(type, payload) {
    try {
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(JSON.stringify({
          type: type,
          payload: payload || null
        }));
      }
    } catch (_) { /* IPC 失败不影响渲染 */ }
  }

  function fail(message) {
    window.__previewError = message;
    report("preview_error", { message: message });
  }

  if (typeof THREE === "undefined") {
    fail("three.js 未加载");
    return;
  }

  // ── 方块类型 → 基础色（平色 + 按面烘焙明暗；不引入 Mojang 版权贴图） ──
  var BLOCK_COLORS = {
    "minecraft:stone_bricks": [0.62, 0.62, 0.64],
    "minecraft:bricks": [0.60, 0.36, 0.29],
    "minecraft:glass_pane": [0.75, 0.90, 1.00],
    "minecraft:glass": [0.75, 0.90, 1.00],
    "minecraft:oak_planks": [0.72, 0.58, 0.37],
    "minecraft:dark_oak_slab": [0.29, 0.22, 0.16],
    "minecraft:dark_oak_door": [0.24, 0.17, 0.12],
    "minecraft:smooth_sandstone": [0.89, 0.85, 0.69],
    "minecraft:smooth_stone": [0.62, 0.62, 0.62],
    "minecraft:stone": [0.55, 0.55, 0.55],
    "minecraft:stone_slab": [0.62, 0.62, 0.62],
    "minecraft:white_concrete": [0.92, 0.92, 0.92],
    "minecraft:light_gray_concrete": [0.78, 0.78, 0.78],
    "minecraft:spruce_planks": [0.55, 0.42, 0.29],
    "minecraft:spruce_door": [0.42, 0.31, 0.20],
    "minecraft:water": [0.25, 0.46, 0.89],
    "minecraft:oak_log": [0.48, 0.35, 0.21],
    "minecraft:oak_leaves": [0.30, 0.55, 0.23],
    "minecraft:grass_block": [0.44, 0.75, 0.31],
    "minecraft:dirt": [0.61, 0.42, 0.26],
    "minecraft:red_concrete": [0.69, 0.18, 0.15],
    "minecraft:rail": [0.55, 0.50, 0.48]
  };

  var TRANSPARENT_IDS = {
    "minecraft:water": true,
    "minecraft:glass": true,
    "minecraft:glass_pane": true
  };

  function colorFor(blockId) {
    var color = BLOCK_COLORS[blockId];
    if (color) {
      return color;
    }
    // 未知方块：确定性灰度兜底（同一 ID 恒同色）
    var hash = 0;
    for (var i = 0; i < blockId.length; i++) {
      hash = (hash * 31 + blockId.charCodeAt(i)) | 0;
    }
    var base = 0.45 + ((hash >>> 0) % 40) / 100;
    return [base, base, base + 0.04];
  }

  // ── 六方向可见面（明暗按面烘焙，无灯光依赖） ──
  var FACES = [
    { n: [1, 0, 0], shade: 0.60, u: 2, v: 1, c: 0, sign: 1 },
    { n: [-1, 0, 0], shade: 0.60, u: 2, v: 1, c: 0, sign: -1 },
    { n: [0, 1, 0], shade: 1.00, u: 0, v: 2, c: 1, sign: 1 },
    { n: [0, -1, 0], shade: 0.46, u: 0, v: 2, c: 1, sign: -1 },
    { n: [0, 0, 1], shade: 0.80, u: 0, v: 1, c: 2, sign: 1 },
    { n: [0, 0, -1], shade: 0.80, u: 0, v: 1, c: 2, sign: -1 }
  ];

  var renderer = null;
  var scene = null;
  var camera = null;
  var canvas = null;
  var modelGroup = null;
  var orbit = {
    theta: Math.PI * 0.25,
    phi: 1.05,
    radius: 80,
    target: [0, 0, 0]
  };
  var minRadius = 1;
  var maxRadius = 4000;
  var drag = null;
  var stats = {
    frames: 0,
    lastFpsAt: 0,
    fps: 0,
    blocks: 0,
    quads: 0,
    lastReportAt: 0
  };

  function cellIndex(x, y, z, dims) {
    return x + z * dims[0] + y * dims[0] * dims[2];
  }

  function resize() {
    if (!renderer || !camera || !canvas) {
      return;
    }
    var width = canvas.clientWidth || window.innerWidth;
    var height = canvas.clientHeight || window.innerHeight;
    if (width === 0 || height === 0) {
      return;
    }
    renderer.setSize(width, height, false);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
  }

  function updateCamera() {
    var sinPhi = Math.sin(orbit.phi);
    camera.position.set(
      orbit.target[0] + orbit.radius * sinPhi * Math.sin(orbit.theta),
      orbit.target[1] + orbit.radius * Math.cos(orbit.phi),
      orbit.target[2] + orbit.radius * sinPhi * Math.cos(orbit.theta)
    );
    camera.lookAt(orbit.target[0], orbit.target[1], orbit.target[2]);
  }

  function fitToBounds(bounds) {
    // 网格使用包围盒本地坐标（x - min_x 等），旋转中心也必须在本地坐标系：
    // 本地中心 = 各轴跨度的一半，而不是绝对坐标中心。
    var center = [
      (bounds[3] - bounds[0]) / 2,
      (bounds[4] - bounds[1]) / 2,
      (bounds[5] - bounds[2]) / 2
    ];
    var spanX = bounds[3] - bounds[0] + 1;
    var spanY = bounds[4] - bounds[1] + 1;
    var spanZ = bounds[5] - bounds[2] + 1;
    var diagonal = Math.sqrt(spanX * spanX + spanY * spanY + spanZ * spanZ);
    orbit.target = center;
    orbit.theta = Math.PI * 0.25;
    orbit.phi = 1.05;
    orbit.radius = Math.max(4, diagonal * 1.25);
    minRadius = Math.max(0.5, diagonal * 0.03);
    maxRadius = Math.max(minRadius * 4, diagonal * 10);
    updateCamera();
  }

  function emitQuad(target, face, component, u0, v0, u1, v1, color, shade) {
    var base = target.count;
    var corners = [
      [u0, v0],
      [u1, v0],
      [u1, v1],
      [u0, v1]
    ];
    for (var i = 0; i < corners.length; i++) {
      component[face.u] = corners[i][0];
      component[face.v] = corners[i][1];
      target.positions.push(component[0], component[1], component[2]);
      target.colors.push(color[0] * shade, color[1] * shade, color[2] * shade);
    }
    target.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    target.count += 4;
  }

  function buildGeometry(grid, dims, palette) {
    var opaque = { positions: [], colors: [], indices: [], count: 0 };
    var transparent = { positions: [], colors: [], indices: [], count: 0 };
    var component = [0, 0, 0];

    for (var f = 0; f < FACES.length; f++) {
      var face = FACES[f];
      var dimU = dims[face.u];
      var dimV = dims[face.v];
      var dimC = dims[face.c];
      var vis = new Uint8Array(dimU * dimV);
      var ids = new Uint32Array(dimU * dimV);
      var done = new Uint8Array(dimU * dimV);

      for (var w = 0; w < dimC; w++) {
        var plane = face.sign > 0 ? w + 1 : w;
        if (plane < 0 || plane > dimC) {
          continue;
        }
        component[face.c] = w;
        for (var v = 0; v < dimV; v++) {
          for (var u = 0; u < dimU; u++) {
            component[face.u] = u;
            component[face.v] = v;
            var present = grid[cellIndex(component[0], component[1], component[2], dims)] !== 0;
            var visible = false;
            if (present) {
              var neighbor = w + face.sign;
              if (neighbor < 0 || neighbor >= dimC) {
                visible = true;
              } else {
                component[face.c] = neighbor;
                visible = grid[cellIndex(component[0], component[1], component[2], dims)] === 0;
                component[face.c] = w;
              }
            }
            var offset = u + v * dimU;
            vis[offset] = visible ? 1 : 0;
            ids[offset] = present ? grid[cellIndex(component[0], component[1], component[2], dims)] : 0;
          }
        }

        done.fill(0);
        component[face.c] = plane;
        for (var row = 0; row < dimV; row++) {
          for (var col = 0; col < dimU; col++) {
            var offset = col + row * dimU;
            if (!vis[offset] || done[offset]) {
              continue;
            }
            var blockId = ids[offset];
            var uEnd = col;
            while (uEnd + 1 < dimU &&
              !done[uEnd + 1 + row * dimU] &&
              vis[uEnd + 1 + row * dimU] &&
              ids[uEnd + 1 + row * dimU] === blockId) {
              uEnd++;
            }
            var vEnd = row;
            var extend = true;
            while (extend && vEnd + 1 < dimV) {
              for (var uu = col; uu <= uEnd; uu++) {
                var probe = uu + (vEnd + 1) * dimU;
                if (done[probe] || !vis[probe] || ids[probe] !== blockId) {
                  extend = false;
                  break;
                }
              }
              if (extend) {
                vEnd++;
              }
            }
            for (var uu = col; uu <= uEnd; uu++) {
              for (var vv = row; vv <= vEnd; vv++) {
                done[uu + vv * dimU] = 1;
              }
            }
            var paletteIndex = blockId - 1;
            var blockName = palette[paletteIndex] || "";
            var color = colorFor(blockName);
            var target = TRANSPARENT_IDS[blockName] ? transparent : opaque;
            emitQuad(target, face, component, col, row, uEnd + 1, vEnd + 1, color, face.shade);
            stats.quads++;
          }
        }
      }
    }
    return { opaque: opaque, transparent: transparent };
  }

  function disposeModel() {
    if (modelGroup) {
      scene.remove(modelGroup);
      modelGroup.traverse(function (object) {
        if (object.geometry) {
          object.geometry.dispose();
        }
        if (object.material) {
          object.material.dispose();
        }
      });
      modelGroup = null;
    }
  }

  function addGeometry(group, built, material) {
    if (built.indices.length === 0) {
      return;
    }
    var geometry = new THREE.BufferGeometry();
    geometry.setAttribute(
      "position",
      new THREE.Float32BufferAttribute(Float32Array.from(built.positions), 3)
    );
    geometry.setAttribute(
      "color",
      new THREE.Float32BufferAttribute(Float32Array.from(built.colors), 3)
    );
    geometry.setIndex(built.indices);
    var mesh = new THREE.Mesh(geometry, material);
    mesh.matrixAutoUpdate = false;
    mesh.updateMatrix();
    group.add(mesh);
  }

  function loadPreviewData(payload) {
    window.__previewError = null;
    if (!payload || typeof payload !== "object" || !payload.palette || !payload.runs) {
      fail("预览数据格式无效");
      return;
    }
    var bounds = payload.bounds || [0, 0, 0, 0, 0, 0];
    window.__lastBounds = bounds;
    var dims = [
      Math.max(1, bounds[3] - bounds[0] + 1),
      Math.max(1, bounds[4] - bounds[1] + 1),
      Math.max(1, bounds[5] - bounds[2] + 1)
    ];
    var total = dims[0] * dims[1] * dims[2];
    // 体素值 = 调色板索引 + 1；调色板远小于 65536，恒可用 Uint16（内存减半）。
    var grid = payload.palette.length > 0xFFFF ? new Uint32Array(total) : new Uint16Array(total);

    var runs = payload.runs;
    var blocks = 0;
    for (var i = 0; i < runs.length; i++) {
      var run = runs[i];
      var paletteIndex = run[0] + 1; // 0 留给空气
      var x0 = run[1] - bounds[0];
      var x1 = run[2] - bounds[0];
      var y = run[3] - bounds[1];
      var z = run[4] - bounds[2];
      for (var x = x0; x <= x1; x++) {
        grid[cellIndex(x, y, z, dims)] = paletteIndex;
        blocks++;
      }
    }

    disposeModel();
    modelGroup = new THREE.Group();
    modelGroup.matrixAutoUpdate = false;
    modelGroup.updateMatrix();
    scene.add(modelGroup);

    stats.blocks = blocks;
    stats.quads = 0;
    var built = buildGeometry(grid, dims, payload.palette);
    addGeometry(
      modelGroup,
      built.opaque,
      // 合并面不依赖绕序：双面渲染避免错误剔除，明暗已按面烘焙。
      new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide })
    );
    addGeometry(
      modelGroup,
      built.transparent,
      new THREE.MeshBasicMaterial({
        vertexColors: true,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.65,
        depthWrite: false
      })
    );
    fitToBounds(bounds);
    report("preview_loaded", {
      blocks: blocks,
      quads: stats.quads,
      simplified: !!payload.simplified
    });
  }

  function resetPreviewView() {
    if (window.__lastBounds) {
      fitToBounds(window.__lastBounds);
    }
  }

  function zoomPreview(delta) {
    orbit.radius = Math.min(maxRadius, Math.max(minRadius, orbit.radius * delta));
    updateCamera();
  }

  // ── 输入：左键拖动旋转 / 滚轮缩放 ──
  function pointerDown(event) {
    if (!canvas || event.button !== 0) {
      return;
    }
    drag = { x: event.clientX, y: event.clientY };
    canvas.setPointerCapture(event.pointerId);
  }

  function pointerMove(event) {
    if (!drag) {
      return;
    }
    var dx = event.clientX - drag.x;
    var dy = event.clientY - drag.y;
    drag.x = event.clientX;
    drag.y = event.clientY;
    orbit.theta -= dx * 0.008;
    orbit.phi = Math.min(Math.PI - 0.05, Math.max(0.05, orbit.phi - dy * 0.008));
    updateCamera();
  }

  function pointerUp(event) {
    if (!drag) {
      return;
    }
    drag = null;
    try {
      canvas.releasePointerCapture(event.pointerId);
    } catch (_) { /* 已释放 */ }
  }

  function wheel(event) {
    event.preventDefault();
    var factor = event.deltaY < 0 ? 0.9 : 1.1;
    zoomPreview(factor);
  }

  function animationFrame(now) {
    requestAnimationFrame(animationFrame);
    stats.frames++;
    if (now - stats.lastFpsAt >= 1000) {
      stats.fps = Math.round(stats.frames * 1000 / (now - stats.lastFpsAt));
      stats.frames = 0;
      stats.lastFpsAt = now;
      window.__previewFps = stats.fps;
    }
    if (now - stats.lastReportAt >= 2000) {
      stats.lastReportAt = now;
      report("preview_stats", {
        fps: stats.fps,
        blocks: stats.blocks,
        quads: stats.quads
      });
    }
    if (renderer && scene && camera) {
      renderer.render(scene, camera);
    }
  }

  function start() {
    canvas = document.getElementById("preview-canvas");
    if (!canvas) {
      fail("预览画布缺失");
      return;
    }
    try {
      renderer = new THREE.WebGLRenderer({ canvas: canvas, antialias: true });
    } catch (error) {
      fail("WebGL 初始化失败：" + String(error && error.message ? error.message : error));
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    scene = new THREE.Scene();
    scene.background = new THREE.Color(0xf4f6f8);
    camera = new THREE.PerspectiveCamera(45, 1, 0.1, 100000);
    camera.position.set(40, 40, 40);
    camera.lookAt(0, 0, 0);

    canvas.addEventListener("pointerdown", pointerDown);
    canvas.addEventListener("pointermove", pointerMove);
    canvas.addEventListener("pointerup", pointerUp);
    canvas.addEventListener("pointercancel", pointerUp);
    canvas.addEventListener("wheel", wheel, { passive: false });
    window.addEventListener("resize", resize);

    resize();
    window.__previewReady = true;
    requestAnimationFrame(animationFrame);

    if (pendingPayload !== null && pendingPayload !== undefined) {
      try {
        loadPreviewData(pendingPayload);
      } catch (error) {
        fail("预览数据装载失败：" + String(error && error.message ? error.message : error));
      }
    }
  }

  // 对外接口（Rust 侧经 evaluate_script 调用；早于本脚本解析时由 page.html
  // 的占位函数收下，本脚本初始化后再消费 pending 数据）
  window.loadPreviewData = function (payload) {
    if (!window.__previewReady) {
      pendingPayload = payload;
      return;
    }
    loadPreviewData(payload);
  };
  window.resetPreviewView = resetPreviewView;
  window.zoomPreview = zoomPreview;

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
