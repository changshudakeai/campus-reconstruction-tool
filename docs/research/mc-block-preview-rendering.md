# 3D 方块预览渲染调研（MCBlock 参考与可选实现）

状态：2026-08-18，T52 工单前期调研。目的：确定导出步骤“3D 校园方块预览”的实现
方向——mcblock.top 的预览如何实现，以及可复用的、性能更好的开源方案。

## 一、mcblock.top 的 3D 预览实现（一手检查）

### 已证实的事实

- 站点是 Next.js（React）应用：建筑页 HTML 引用 `/_next/static/chunks/…` 系列脚本
  （2026-08-18 直接抓取页面源码）。
- 建筑查看器由 React 状态驱动，页面 chunk 源码中含：
  `mode:"preview"`、`currentIndex`、`stepCount`、`layerCount`、`totalBlocks`、
  `selectedBlock`、`materialStats`、`stepMeta`、`layerMeta`，以及视图模式
  `orbit`/`isometric`（`useState("orbit")`/`useState("isometric")`）——支持
  逐层教学、材质统计、选中方块查看。

### 推断（标注：未完全证实）

- 抽查到的几个 JS chunk 中未发现 WebGL 特征（无 `WebGLRenderer`、
  `InstancedMesh`、`BufferGeometry`、`regl`、`Babylon`、`webgl` 等标记），因此
  预览很可能用 DOM/CSS（或 2D canvas）等距方块渲染实现。
- 该做法适合单建筑逐层教学，但对“整校园数万~数十万方块”的性能不可行；站点
  也没有公开源码可直接复用。
- 局限：只抽查了部分 chunk，未做完整逆向；如需确证须在浏览器中进一步分析
  网络请求与 DOM。

来源：
- 页面与 chunk 源码抓取：https://mcblock.top/buildings/24bedfe5-428d-4dca-ba8a-619cb6654e81

## 二、可复用的开源实现方案（一手来源）

### Web / Three.js 方案（推荐方向）

1. **deepslate**（misode / jacobsjo，MIT，npm `deepslate` 0.26.0）
   - Three.js 渲染/模拟 Minecraft 的 TypeScript 库；社区项目
     KokeCacao/minecraft-viewer 基于它支持渲染 schematic。
   - 来源：https://github.com/jacobsjo/deepslate 、https://github.com/misode/deepslate-demo
2. **@mattzh72/lodestone**（MIT）
   - deepslate 的优化 Three.js 原生版本：分块网格（chunked meshing）、遮挡剔除
     （occlusion culling）、透明度排序、发光方块支持；加载 `.litematic` 格式；
     提供 CDN/UMD 用法。
   - 来源：https://www.npmjs.com/package/@mattzh72/lodestone
3. **craftmatic**（tribixbite，MIT）
   - Minecraft schematic 工具包：解析/生成/渲染/转换 `.schem` 文件，交互式
     Three.js 3D 查看器，可导出独立 HTML 查看器；含 schem↔Three.js 双向转换。
   - 与本项目导出的 `.schem` 格式直接相关。
   - 来源：https://github.com/tribixbite/craftmatic 、https://www.npmjs.com/package/craftmatic
4. **自研 three.js 渲染**（无外部依赖）
   - three.js `InstancedMesh` 或合并几何 + 隐藏面剔除（只画可见面）足以渲染
     几十万方块（three.js 社区实践：Minecraft 风格只把表面多边形化）。
   - 来源：https://stackoverflow.com/questions/56602575/render-millions-of-cubes-with-non-static-position-in-three-js

### Rust 原生方案（评估后不推荐作首选）

- bevy-voxels（ray marching，WIP）、all-is-cubes（wgpu）、bevy-voxel-engine 等
  多为 WIP 或完整游戏引擎，嵌入 Slint 桌面应用的集成与维护成本明显高于
  WebView + Three.js。
- 来源：https://github.com/okamt/bevy-voxels 、https://crates.io/keywords/voxel

## 三、推荐结论

- 采用 **WebView（wry/WebView2）内嵌 Three.js** 渲染 3D 方块预览：Rust 侧把
  B18 生成引擎的真实 `BlockModel` 序列化为渲染数据（方块 ID + 坐标），前端用
  Three.js 合并几何 + 隐藏面剔除 + OrbitControls 实现旋转/缩放/复位。
- 复用优先级：先评估 **craftmatic**（`.schem` 原生支持）与
  **deepslate / lodestone**（MC 渲染成熟、MIT）；三者许可证均允许商用。
- 性能关键：只渲染可见面、同类方块合并或实例化、屏幕外剔除、必要时分块；
  超大方案自动简化并提示。真机验收帧率（目标 30–60fps）。
- 不引入完整 Rust 游戏引擎。

## 四、来源汇总

- https://mcblock.top/buildings/24bedfe5-428d-4dca-ba8a-619cb6654e81 （页面与 chunk 源码抓取）
- https://github.com/jacobsjo/deepslate （deepslate，MIT）
- https://www.npmjs.com/package/@mattzh72/lodestone （Lodestone，MIT）
- https://github.com/tribixbite/craftmatic 、https://www.npmjs.com/package/craftmatic （craftmatic，MIT）
- https://stackoverflow.com/questions/56602575/render-millions-of-cubes-with-non-static-position-in-three-js （three.js 实例化/合并实践）
- https://github.com/okamt/bevy-voxels 、https://crates.io/keywords/voxel （Rust 体素方案，评估）
