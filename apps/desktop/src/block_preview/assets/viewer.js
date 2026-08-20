/*
 * T52 第五步 3D 方块预览渲染器（分块并行版）。
 *
 * 数据由 Rust 侧 F9 从与导出同源的 B18 BlockModel 序列化而来：
 *   {v, palette, bounds, count, runs, features}
 * 渲染流程：
 *   1. 主线程把 RLE 游程按 64^3 分块切片；
 *   2. Web Worker 池并行填充块体素网格并做隐藏面剔除 + 同面贪婪合并；
 *   3. 主线程逐块组装 BufferGeometry，使用社区纹理图集（Pixel Perfection
 *      Legacy）与 Lambert 光照，块边界面不做跨块剔除；
 *   4. 保留候选定位：`locatePreviewFeature(id)` 飞行取景 + 高亮包围盒。
 *
 * 本文件与 three.min.js（MIT）、worker.js、atlas.png 一起随 desktop-shell
 * 打包，不联网加载资源，不包含任何用户可见文案（ADR-0005）。
 */
(function () {
  "use strict";

  var CHUNK = 64;
  var WORKER_COUNT = 4;

  // 初始化前到达的数据暂存（HTML 内嵌负载或早到的 evaluate_script 推送）。
  var pendingPayload =
    window.__previewPending === undefined ? null : window.__previewPending;
  window.__previewError = null;
  window.__previewReady = false;
  window.__previewFps = 0;

  function report(type, payload) {
    try {
      if (window.ipc && window.ipc.postMessage) {
        window.ipc.postMessage(
          JSON.stringify({ type: type, payload: payload || null })
        );
      }
    } catch (_) {
      /* IPC 失败不影响渲染 */
    }
  }

  function fail(message) {
    window.__previewError = message;
    report("preview_error", { message: message });
  }

  if (typeof THREE === "undefined") {
    fail("three.js 未加载");
    return;
  }

  var renderer = null;
  var scene = null;
  var camera = null;
  var canvas = null;
  var modelGroup = null;
  var atlasTexture = null;
  var atlasReady = false;
  var textureMap = window.__PREVIEW_TEXTURE_MAP__ || {};
  var materialByClass = [null, null, null];
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
    chunks: 0,
    lastReportAt: 0
  };

  var workers = [];
  var chunkQueue = [];
  var chunkIndex = 0;
  var idleWorkers = 0;
  var buildPending = 0;
  var buildDone = false;
  var highlight = null;
  var pendingLocate = null;
  var activeFly = null;
  var currentSliced = null;

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
    if (!camera) {
      return;
    }
    var sinPhi = Math.sin(orbit.phi);
    camera.position.set(
      orbit.target[0] + orbit.radius * sinPhi * Math.sin(orbit.theta),
      orbit.target[1] + orbit.radius * Math.cos(orbit.phi),
      orbit.target[2] + orbit.radius * sinPhi * Math.cos(orbit.theta)
    );
    camera.lookAt(orbit.target[0], orbit.target[1], orbit.target[2]);
  }

  function fitToBounds(bounds) {
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

  function disposeModel() {
    if (modelGroup) {
      scene.remove(modelGroup);
      modelGroup.traverse(function (object) {
        if (object.geometry) {
          object.geometry.dispose();
        }
      });
      modelGroup = null;
    }
    if (highlight) {
      scene.remove(highlight);
      if (highlight.geometry) {
        highlight.geometry.dispose();
      }
      highlight = null;
    }
    terminateWorkers();
    chunkQueue = [];
    chunkIndex = 0;
    buildPending = 0;
    buildDone = false;
    stats.quads = 0;
    stats.chunks = 0;
  }

  function terminateWorkers() {
    for (var i = 0; i < workers.length; i++) {
      try {
        workers[i].terminate();
      } catch (_) {
        /* 已终止 */
      }
    }
    workers = [];
    idleWorkers = 0;
  }

  function cellIndex(x, y, z, dims) {
    return x + z * dims[0] + y * dims[0] * dims[2];
  }

  function chunkKey(cx, cy, cz, chunkDims) {
    // 维度顺序 x（低位）、z（中位）、y（高位）：任一维度为 1 时 key 仍单射，
    // 避免 y 只有一块（如校园高度 14 格）时解码把 cz 误当 cy。
    return cx + cz * chunkDims[0] + cy * chunkDims[0] * chunkDims[2];
  }

  // 把 RLE 游程按 64^3 分块切片（run 沿 x 水平，只可能在 x 上跨块）。
  function sliceRuns(runs, bounds, chunkDims) {
    var sliced = new Map();
    for (var i = 0; i < runs.length; i++) {
      var run = runs[i];
      var x0 = run[1] - bounds[0];
      var x1 = run[2] - bounds[0];
      var y = run[3] - bounds[1];
      var z = run[4] - bounds[2];
      var cx0 = Math.floor(x0 / CHUNK);
      var cx1 = Math.floor(x1 / CHUNK);
      var cy = Math.floor(y / CHUNK);
      var cz = Math.floor(z / CHUNK);
      for (var cx = cx0; cx <= cx1; cx++) {
        var key = chunkKey(cx, cy, cz, chunkDims);
        var pieces = sliced.get(key);
        if (!pieces) {
          pieces = [];
          sliced.set(key, pieces);
        }
        var pieceStart = Math.max(x0, cx * CHUNK);
        var pieceEnd = Math.min(x1, (cx + 1) * CHUNK - 1);
        // 统一为本地坐标（bounds 平移后），与 Worker 的块原点同坐标系；
        // 若混用绝对/本地坐标，非零 bounds 会造成整体偏移。
        pieces.push([run[0], pieceStart, pieceEnd, y, z]);
      }
    }
    return sliced;
  }

  function makeMaterials() {
    var base = {
      map: atlasTexture,
      side: THREE.FrontSide
    };
    materialByClass[0] = new THREE.MeshLambertMaterial(base);
    materialByClass[1] = new THREE.MeshLambertMaterial({
      map: atlasTexture,
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.65,
      depthWrite: false
    });
    materialByClass[2] = new THREE.MeshLambertMaterial({
      map: atlasTexture,
      side: THREE.DoubleSide,
      alphaTest: 0.5
    });
  }

  function addChunkMesh(key, classes) {
    for (var i = 0; i < classes.length; i++) {
      var data = classes[i];
      if (!data.indices || data.indices.length === 0) {
        continue;
      }
      var geometry = new THREE.BufferGeometry();
      geometry.setAttribute(
        "position",
        new THREE.BufferAttribute(data.positions, 3)
      );
      geometry.setAttribute("uv", new THREE.BufferAttribute(data.uvs, 2));
      geometry.setIndex(new THREE.BufferAttribute(data.indices, 1));
      geometry.computeVertexNormals();
      var mesh = new THREE.Mesh(geometry, materialByClass[i]);
      mesh.matrixAutoUpdate = false;
      mesh.updateMatrix();
      modelGroup.add(mesh);
    }
    stats.chunks++;
    stats.quads += classes.reduce(function (sum, data) {
      return sum + data.count;
    }, 0);
  }

  function startBuild(payload) {
    if (!atlasReady) {
      pendingPayload = payload;
      return;
    }
    disposeModel();
    var bounds = payload.bounds || [0, 0, 0, 0, 0, 0];
    window.__lastBounds = bounds;
    var dims = [
      Math.max(1, bounds[3] - bounds[0] + 1),
      Math.max(1, bounds[4] - bounds[1] + 1),
      Math.max(1, bounds[5] - bounds[2] + 1)
    ];
    var chunkDims = [
      Math.ceil(dims[0] / CHUNK),
      Math.ceil(dims[1] / CHUNK),
      Math.ceil(dims[2] / CHUNK)
    ];
    var sliced = sliceRuns(payload.runs, bounds, chunkDims);
    currentSliced = sliced;
    var chunkOrigins = new Map();
    sliced.forEach(function (_, key) {
      var cy = Math.floor(key / (chunkDims[0] * chunkDims[2]));
      var rest = key - cy * chunkDims[0] * chunkDims[2];
      var cz = Math.floor(rest / chunkDims[0]);
      var cx = rest - cz * chunkDims[0];
      chunkOrigins.set(key, [cx, cy, cz]);
    });
    // 队列按 y 升序（地面层先出现），再按 x/z 字典序保证确定性。
    chunkQueue = Array.from(sliced.keys()).sort(function (a, b) {
      var oa = chunkOrigins.get(a);
      var ob = chunkOrigins.get(b);
      return oa[1] - ob[1] || oa[0] - ob[0] || oa[2] - ob[2];
    });
    chunkIndex = 0;
    buildPending = chunkQueue.length;
    buildDone = false;

    modelGroup = new THREE.Group();
    modelGroup.matrixAutoUpdate = false;
    modelGroup.updateMatrix();
    scene.add(modelGroup);
    stats.blocks = payload.count || 0;
    stats.quads = 0;
    stats.chunks = 0;

    var workerCount = Math.max(1, Math.min(WORKER_COUNT, buildPending || 1));
    if (!window.__PREVIEW_WORKER_SRC__) {
      fail("分块 Worker 源码缺失");
      return;
    }
    try {
      var blob = new Blob([window.__PREVIEW_WORKER_SRC__], {
        type: "application/javascript"
      });
      var workerUrl = URL.createObjectURL(blob);
      for (var i = 0; i < workerCount; i++) {
        var worker = new Worker(workerUrl);
        worker.onmessage = handleWorkerMessage;
        worker.onerror = function (event) {
          fail("分块 Worker 出错：" + (event && event.message ? event.message : "未知错误"));
        };
        workers.push(worker);
      }
    } catch (error) {
      fail("Web Worker 初始化失败：" + (error && error.message ? error.message : error));
      return;
    }

    fitToBounds(bounds);
    idleWorkers = workers.length;
    assignWork();
  }

  function assignWork() {
    while (idleWorkers > 0 && chunkIndex < chunkQueue.length) {
      var key = chunkQueue[chunkIndex++];
      var spec = makeChunkSpec(key);
      if (!spec) {
        buildPending--;
        if (buildPending === 0) {
          finishBuild();
        }
        continue;
      }
      var worker = workers[workers.length - idleWorkers];
      idleWorkers--;
      worker.postMessage({ type: "build", key: key, spec: spec });
    }
  }

  function makeChunkSpec(key) {
    var bounds = window.__lastBounds;
    var dims = [
      bounds[3] - bounds[0] + 1,
      bounds[4] - bounds[1] + 1,
      bounds[5] - bounds[2] + 1
    ];
    var chunkDims = [
      Math.ceil(dims[0] / CHUNK),
      Math.ceil(dims[1] / CHUNK),
      Math.ceil(dims[2] / CHUNK)
    ];
    var cy = Math.floor(key / (chunkDims[0] * chunkDims[2]));
    var rest = key - cy * chunkDims[0] * chunkDims[2];
    var cz = Math.floor(rest / chunkDims[0]);
    var cx = rest - cz * chunkDims[0];
    var origin = [cx * CHUNK, cy * CHUNK, cz * CHUNK];
    var size = [
      Math.max(0, Math.min(CHUNK, dims[0] - origin[0])),
      Math.max(0, Math.min(CHUNK, dims[1] - origin[1])),
      Math.max(0, Math.min(CHUNK, dims[2] - origin[2]))
    ];
    return {
      origin: origin,
      size: size,
      palette: window.__lastPalette || [],
      textureMap: textureMap,
      runs: currentSliced ? currentSliced.get(key) || [] : []
    };
  }

  function handleWorkerMessage(event) {
    var message = event.data;
    if (!message) {
      return;
    }
    if (message.type === "chunk_ready") {
      idleWorkers++;
      addChunkMesh(message.key, message.classes);
      buildPending--;
      if (buildPending === 0) {
        finishBuild();
      }
      assignWork();
    }
  }

  function finishBuild() {
    buildDone = true;
    terminateWorkers();
    report("preview_loaded", {
      blocks: stats.blocks,
      quads: stats.quads,
      chunks: stats.chunks
    });
    if (pendingLocate) {
      var featureId = pendingLocate;
      pendingLocate = null;
      locatePreviewFeature(featureId);
    }
  }

  function flyTo(bounds, durationMs) {
    var center = [
      (bounds[3] + bounds[0]) / 2,
      (bounds[4] + bounds[1]) / 2,
      (bounds[5] + bounds[2]) / 2
    ];
    var spanX = bounds[3] - bounds[0] + 1;
    var spanY = bounds[4] - bounds[1] + 1;
    var spanZ = bounds[5] - bounds[2] + 1;
    var diagonal = Math.sqrt(spanX * spanX + spanY * spanY + spanZ * spanZ);
    var targetRadius = Math.max(3, diagonal * 2.0);
    var from = {
      theta: orbit.theta,
      phi: orbit.phi,
      radius: orbit.radius,
      target: orbit.target.slice()
    };
    var to = {
      theta: Math.PI * 0.25,
      phi: 1.05,
      radius: targetRadius,
      target: center
    };
    activeFly = {
      from: from,
      to: to,
      start: performance.now(),
      duration: Math.max(200, durationMs || 600)
    };
  }

  function updateFly() {
    if (!activeFly) {
      return;
    }
    var elapsed = performance.now() - activeFly.start;
    var t = Math.min(1, elapsed / activeFly.duration);
    var eased = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
    var from = activeFly.from;
    var to = activeFly.to;
    orbit.theta = from.theta + (to.theta - from.theta) * eased;
    orbit.phi = from.phi + (to.phi - from.phi) * eased;
    orbit.radius = from.radius + (to.radius - from.radius) * eased;
    orbit.target = [
      from.target[0] + (to.target[0] - from.target[0]) * eased,
      from.target[1] + (to.target[1] - from.target[1]) * eased,
      from.target[2] + (to.target[2] - from.target[2]) * eased
    ];
    updateCamera();
    if (t >= 1) {
      activeFly = null;
    }
  }

  function setHighlight(bounds) {
    if (highlight) {
      scene.remove(highlight);
      if (highlight.geometry) {
        highlight.geometry.dispose();
      }
      if (highlight.material) {
        highlight.material.dispose();
      }
      highlight = null;
    }
    if (!bounds) {
      return;
    }
    var min = new THREE.Vector3(bounds[0], bounds[1], bounds[2]);
    var max = new THREE.Vector3(bounds[3], bounds[4], bounds[5]);
    var box = new THREE.Box3(min, max);
    var boxHelper = new THREE.Box3Helper(box, 0xff3b30);
    boxHelper.matrixAutoUpdate = false;
    boxHelper.updateMatrix();
    scene.add(boxHelper);
    highlight = boxHelper;
  }

  function locatePreviewFeature(featureId) {
    if (!featureId || !window.__lastFeatures) {
      return;
    }
    var feature = null;
    for (var i = 0; i < window.__lastFeatures.length; i++) {
      if (window.__lastFeatures[i].id === featureId) {
        feature = window.__lastFeatures[i];
        break;
      }
    }
    if (!feature || !feature.bounds) {
      return;
    }
    if (!buildDone) {
      // 构建中：先记录，全部块完成后再飞行（保证精细检查时建筑完整）。
      pendingLocate = featureId;
      return;
    }
    setHighlight(feature.bounds);
    flyTo(feature.bounds, 600);
  }

  function loadPreviewData(payload) {
    window.__previewError = null;
    if (!payload || typeof payload !== "object" || !payload.palette || !payload.runs) {
      fail("预览数据格式无效");
      return;
    }
    window.__lastFeatures = payload.features || [];
    window.__lastPalette = payload.palette;
    pendingLocate = null;
    startBuild(payload);
  }

  function resetPreviewView() {
    if (window.__lastBounds) {
      activeFly = null;
      setHighlight(null);
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
    } catch (_) {
      /* 已释放 */
    }
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
      stats.fps = Math.round((stats.frames * 1000) / (now - stats.lastFpsAt));
      stats.frames = 0;
      stats.lastFpsAt = now;
      window.__previewFps = stats.fps;
    }
    if (now - stats.lastReportAt >= 2000) {
      stats.lastReportAt = now;
      report("preview_stats", {
        fps: stats.fps,
        blocks: stats.blocks,
        quads: stats.quads,
        chunks: stats.chunks,
        build_done: buildDone
      });
    }
    updateFly();
    if (renderer && scene && camera) {
      renderer.render(scene, camera);
    }
  }

  function loadAtlas(callback) {
    var image = new Image();
    image.onload = function () {
      atlasTexture = new THREE.CanvasTexture(image);
      atlasTexture.flipY = true;
      // 线性过滤 + mipmap：远景不再闪烁/噪点（图集已带 1px padding）。
      atlasTexture.magFilter = THREE.LinearFilter;
      atlasTexture.minFilter = THREE.LinearMipmapLinearFilter;
      atlasTexture.generateMipmaps = true;
      atlasTexture.needsUpdate = true;
      atlasReady = true;
      makeMaterials();
      callback();
    };
    image.onerror = function () {
      fail("纹理图集加载失败");
      callback();
    };
    image.src = window.__PREVIEW_ATLAS_DATA__;
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
    scene.add(new THREE.HemisphereLight(0xffffff, 0xc8d2dc, 0.9));
    var sun = new THREE.DirectionalLight(0xffffff, 0.95);
    sun.position.set(80, 140, 60);
    scene.add(sun);

    canvas.addEventListener("pointerdown", pointerDown);
    canvas.addEventListener("pointermove", pointerMove);
    canvas.addEventListener("pointerup", pointerUp);
    canvas.addEventListener("pointercancel", pointerUp);
    canvas.addEventListener("wheel", wheel, { passive: false });
    window.addEventListener("resize", resize);

    resize();
    window.__previewReady = true;
    requestAnimationFrame(animationFrame);

    loadAtlas(function () {
      if (pendingPayload !== null && pendingPayload !== undefined) {
        var payload = pendingPayload;
        pendingPayload = null;
        try {
          loadPreviewData(payload);
        } catch (error) {
          fail("预览数据装载失败：" + String(error && error.message ? error.message : error));
        }
      }
    });
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
  window.locatePreviewFeature = function (featureId) {
    if (!window.__previewReady) {
      return;
    }
    locatePreviewFeature(featureId);
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
