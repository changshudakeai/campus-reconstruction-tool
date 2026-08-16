# T37 工作区地图与朝向交互修复（走查阻塞）

Status: completed（2026-08-17 发布收口）
Blocked by: 无（基于本地稳定版 main 开发）

## What to build（负责人视角）

1. 边界/朝向步骤的地图应上下填满可用区域，不再在下方留大片空白。
2. 设定朝向步骤点击地图应能选点确定朝向（当前点击无反应）。
3. 朝向角度输入框只能输入数字（可小数），不能输入汉字等非数字字符。

## 根因（验收窗口 2026-08-09 定位）

1. 地图下方空白：`core/gaode-client/src/boundary_edit_map_page.rs:283` 的 `#map-container` 高度为固定 `{height}px`（默认 300），而 WebView 槽位按窗口填满（`apps/desktop/ui/main.slint:163` 高度=窗口−128−16）；T32/T34 的 resize 处理只同步宽度与 `map.resize()`，未把容器高度拉到视口高度。
2. 朝向点击无反应：`apps/desktop/src/production/workspace_adapter.rs:721` 收到 `MapReady` 一律走 `start_boundary_fetch()`，从未调用 ORIENTATION_SCRIPT 定义的 `window.initOrientationMode()`（`boundary_edit_map_page.rs:154`），因此朝向步的地图点击处理器从未挂接。
3. 角度输入框：`apps/desktop/ui/workspace_drawer.slint:148` 的 TextInput 未设 `input-type`，任意字符可输入。

## 范围

1. 地图高度：初始化与窗口 resize（含抽屉开合让位）时把 `#map-container` 高度设为 WebView 视口高度（`html/body` 100% 或显式 `innerHeight`），再 `map.resize()`；边界步与朝向步一致。
2. 朝向激活：进入朝向步且收到 `MapReady`（或展示地图后）时，按当前步骤调用 `map_webview::evaluate_script("initOrientationMode();")`；边界步仍走 `start_boundary_fetch()`。
3. 输入框：角度 TextInput 设 `input-type` 为数字（支持小数），Rust 侧保留范围校验与错误提示。
4. 明确不做：改采集/评审/导出逻辑；产品基线；评审工作台（见 T38）。

## 验收标准（逐条证据）

1. 边界/朝向步地图上下填满视口（修复前后截图；窗口 resize 与抽屉开合后仍填满）。
2. 朝向步点击地图可选两点并反馈角度（截图 + 契约测试或可点性断言）。
3. 角度输入框无法输入非数字字符（UI 断言）。
4. 回归：`cargo test -p desktop-shell --test s1_05 --test s1_06 --test s1_08 --test s1_15 --test s1_23 --test s1_26 --test s1_27` 全绿；全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）。
5. 重建便携包（`scripts/build-release.ps1`），刷新开发版快捷方式。

## 分支与合入（当前策略：本地稳定）

- 分支建议 `fix/t37-map-orientation-fixes`，自本地 main。
- 门禁本地全绿后合入本地 main；**不推送 GitHub**，等稳定版本统一推送。

## 交接

- 向验收窗口报告根因、修复、逐条证据、门禁输出；更新主线计划事实段。
