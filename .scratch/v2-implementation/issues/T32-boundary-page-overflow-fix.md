# T32 边界地图页按钮横向溢出修复（T31-D6，M5 收口前置）

Status: completed（2026-08-17 发布收口）
Blocked by: T31（已合入 main，merge dbed1efc）

## What to build（负责人视角）

真实走查发现：边界步骤里 OSM 自动获取边界已经成功，但“确认边界”和“改人工圈画”按钮渲染在地图视口右缘之外，不可见、不可点，导致无法确认边界、后续导出被阻塞。修复后应能在常规窗口大小下看到并点击这些按钮。

## 缺陷事实（走查实测）

- WebView 内“确认边界”（confirm-edit-btn）UIA 坐标 (860, 521)、“改人工圈画” (806, 529)；窗口逻辑宽 800×666，物理坐标同样超窗。
- 视口期望：`compute_bounds` 为 x:32、宽 window−32（右缘 800）；`boundary_edit.slint` 画布宽 parent.width−32。说明按钮渲染在 WebView 内容层比视口更宽的位置，属内容横向溢出。
- 与 T30 D-5 同族但未根治（D-5 修了 y 与宽度，横向溢出在边界页仍复现）。

## 范围

1. 诊断：在 WebView 内输出 `window.innerWidth`/`devicePixelRatio`、AMap 容器宽度、body 溢出情况、按钮 rect，与 `compute_bounds` 期望视口对比；在 800、1000 逻辑宽与 100%/150% 缩放下复测。
2. 修复边界页 HTML/CSS 布局：AMap 容器与工具栏宽度 ≤ 视口，禁止 body 横向滚动/溢出；必要时同步 `map_webview::compute_bounds` 与 `boundary_edit.slint` 几何，确保“确认边界”“改人工圈画”等关键按钮在任何受支持窗口宽度下可见可点。
3. 不改 OSM 边界获取、采集、评审、导出逻辑；不改产品基线；不开 GitHub Issues。

## 验收标准（逐条证据）

1. 800×666 及常见窗口尺寸下按钮可见可点（截图 + 坐标/可点性断言）。
2. 剧本 A 走通：OSM 边界自动获取 → 确认 → 基础导出（exportKind=base、map_north、attribution）。
3. 剧本 B 走通：采集（OSM 建筑候选，可评审面候选 > 0）→ 评审/封账 → 增强导出（exportKind=enhanced、keepByCategory 一致、.schem 含候选内容）。
4. 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）。
5. 重建便携包（`scripts/build-release.ps1`）并更新 `docs/developer-guide/m5-e2e/manual-acceptance-record.md`，留负责人签名栏。

## 分支/PR

- 分支建议 `fix/t32-boundary-page-overflow`，自最新 origin/main；单一逻辑提交；draft PR，CI 全绿。

## 交接

- 向验收窗口报告根因、修复、逐条验收证据、CI 链接、重跑剧本 A/B 产物；更新主线计划 M5 事实段与交接文档。
