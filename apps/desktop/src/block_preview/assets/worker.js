/*
 * T52 3D 预览分块网格化 Worker。
 *
 * 主线程把 RLE 游程按 64^3 分块切片后分发给本 Worker；每个 Worker 填充
 * 自己的块体素网格并做六方向隐藏面剔除 + 同面贪婪合并，输出按材质分类的
 * 几何缓冲（transferable）。块边界不做跨块剔除（相邻块边界面重复渲染，
 * 占总体面数约 9%，换取每块可独立并行与懒加载）。
 */
"use strict";

var CHUNK = 64;
var ATLAS = 512;
var TILE = 16;

var FACES = [
  { n: [1, 0, 0], u: 2, v: 1, c: 0, sign: 1, face: "side" },
  { n: [-1, 0, 0], u: 2, v: 1, c: 0, sign: -1, face: "side" },
  { n: [0, 1, 0], u: 0, v: 2, c: 1, sign: 1, face: "top" },
  { n: [0, -1, 0], u: 0, v: 2, c: 1, sign: -1, face: "bottom" },
  { n: [0, 0, 1], u: 0, v: 1, c: 2, sign: 1, face: "side" },
  { n: [0, 0, -1], u: 0, v: 1, c: 2, sign: -1, face: "side" }
];

var TRANSPARENT = {
  "minecraft:water": true,
  "minecraft:glass": true,
  "minecraft:glass_pane": true
};

var CUTOUT = {
  "minecraft:oak_leaves": true
};

function materialClass(blockId) {
  if (TRANSPARENT[blockId]) {
    return 1;
  }
  if (CUTOUT[blockId]) {
    return 2;
  }
  return 0;
}

function tileUv(tile) {
  // 图集 512x512，nearest 采样，像素中心 UV；v 按图像行倒转。
  var u0 = (tile[0] * TILE + 0.5) / ATLAS;
  var u1 = (tile[0] * TILE + TILE - 0.5) / ATLAS;
  var v0 = 1.0 - (tile[1] * TILE + TILE - 0.5) / ATLAS;
  var v1 = 1.0 - (tile[1] * TILE + 0.5) / ATLAS;
  return [u0, v0, u1, v1];
}

function emitQuad(target, face, component, u0, v0, u1, v1, origin) {
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
    // 顶点必须落在统一本地坐标（块原点偏移），否则所有块会重叠在原点。
    target.positions.push(
      component[0] + origin[0],
      component[1] + origin[1],
      component[2] + origin[2]
    );
    target.uvs.push(corners[i][0], corners[i][1]);
  }
  target.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
  target.count += 4;
}

function buildChunk(spec) {
  var ox = spec.origin[0];
  var oy = spec.origin[1];
  var oz = spec.origin[2];
  var sx = spec.size[0];
  var sy = spec.size[1];
  var sz = spec.size[2];
  var dims = [sx, sy, sz];
  var palette = spec.palette;
  var textureMap = spec.textureMap;
  var runs = spec.runs;

  var grid = new Uint16Array(sx * sy * sz);
  for (var i = 0; i < runs.length; i++) {
    var run = runs[i];
    var paletteIndex = run[0] + 1; // 0 = air
    var x0 = Math.max(0, run[1] - ox);
    var x1 = Math.min(sx - 1, run[2] - ox);
    var y = run[3] - oy;
    var z = run[4] - oz;
    if (x0 < 0 || x1 < x0 || y < 0 || y >= sy || z < 0 || z >= sz) {
      continue;
    }
    for (var x = x0; x <= x1; x++) {
      grid[cellIndex(x, y, z, dims)] = paletteIndex;
    }
  }

  var output = [
    { positions: [], uvs: [], indices: [], count: 0 },
    { positions: [], uvs: [], indices: [], count: 0 },
    { positions: [], uvs: [], indices: [], count: 0 }
  ];
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
          ids[offset] = present
            ? grid[cellIndex(component[0], component[1], component[2], dims)]
            : 0;
        }
      }

      done.fill(0);
      component[face.c] = w + (face.sign > 0 ? 1 : 0);
      for (var row = 0; row < dimV; row++) {
        for (var col = 0; col < dimU; col++) {
          var offset = col + row * dimU;
          if (!vis[offset] || done[offset]) {
            continue;
          }
          var paletteIndex = ids[offset];
          var uEnd = col;
          while (
            uEnd + 1 < dimU &&
            !done[uEnd + 1 + row * dimU] &&
            vis[uEnd + 1 + row * dimU] &&
            ids[uEnd + 1 + row * dimU] === paletteIndex
          ) {
            uEnd++;
          }
          var vEnd = row;
          var extend = true;
          while (extend && vEnd + 1 < dimV) {
            for (var uu = col; uu <= uEnd; uu++) {
              var probe = uu + (vEnd + 1) * dimU;
              if (done[probe] || !vis[probe] || ids[probe] !== paletteIndex) {
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
          var blockId = palette[paletteIndex - 1] || "";
          var tile = textureMap[blockId] && textureMap[blockId][face.face];
          if (!tile) {
            continue;
          }
          var uv = tileUv(tile);
          var target = output[materialClass(blockId)];
          emitQuad(target, face, component, uv[0], uv[1], uv[2], uv[3], [
            ox,
            oy,
            oz
          ]);
        }
      }
    }
  }
  return output;
}

function cellIndex(x, y, z, dims) {
  return x + z * dims[0] + y * dims[0] * dims[2];
}

self.onmessage = function (event) {
  var message = event.data;
  if (!message || message.type !== "build") {
    return;
  }
  var output = buildChunk(message.spec);
  var buffers = [];
  var payload = { type: "chunk_ready", key: message.key, quads: 0, classes: [] };
  for (var i = 0; i < 3; i++) {
    var positions = new Float32Array(output[i].positions);
    var uvs = new Float32Array(output[i].uvs);
    var indices = new Uint32Array(output[i].indices);
    payload.classes.push({
      positions: positions,
      uvs: uvs,
      indices: indices,
      count: output[i].count
    });
    payload.quads += output[i].count;
    buffers.push(positions.buffer, uvs.buffer, indices.buffer);
  }
  self.postMessage(payload, buffers);
};
