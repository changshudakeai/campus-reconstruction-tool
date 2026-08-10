# T40 真机走查与日志取证计划（朝向输入框持久化 + 两点点击反馈）

构建：`fix/t40-orientation-input-map`（自本地 main），release 便携包经
`scripts/build-release.ps1` 重建后覆盖安装 `%LOCALAPPDATA%\MCRebuildV2\dev`，
桌面"校园复刻工具 - 开发版"快捷方式即新版。

## 已验证（2026-08-10，代码与门禁；commit 95b75ad 已合入本地 main）

- 定向回归全绿：s1_06 / s1_26（真实朝向页经 `activateOrientationWhenReady`
  驱动两点点击链）/ s1_28（键入后三种呈现不重置、重置清空、创建失败后手动
  输入生效、切方案清空）。
- 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`，
  干净 worktree `D:\MCRebuild_Renovation\.t40-worktree`）：machete /
  workspace tests（120 组 ok）/ fmt --check / clippy `-D warnings` / deny /
  xtask ci / timings。
- 真机交互（真实 Key、100%/125% DPI、MCREBUILD_LOG_FILE 日志顺序）仍在
  验收窗口执行，按下方剧本取证后在本段追加结论。

## 前置

- 高德 JS API Key + securityJsCode（域名白名单覆盖验收机器）。
- 125% DPI 走查前按 T35 记录先刷新 Windows 会话（登出/登入），再启动应用。
- 抓日志：`$env:MCREBUILD_LOG_FILE='D:\MCRebuild_Renovation\New-branch-v2\docs\developer-guide\m5-e2e\evidence\t40\map-t40-walkthrough.log'`
  后启动应用（*.log 被 gitignore，取证后另存 .txt）。

## 剧本（100% 与 125% DPI 各一遍）

1. 打开方案 → 边界页 WebView 创建 → `notify_status(available=true)`。
2. 地图就绪 → Rust 侧 OSM 自动获取 → 确认边界。
3. 进入步骤②（朝向页 WebView 创建，page=Orientation）→ 日志应依次出现：
   - `开始异步创建 WebView（page=Orientation…）`
   - `WebView 创建成功`
   - `朝向页创建成功，执行激活脚本（创建成功通道）`
   - 页面 `map_ready` IPC 到达 → `map_ready_for_active_step` 兜底激活
   - 页面自动激活（onMapReadyForMode 500ms）
4. 地图点两点 → 每条点击应产出 `orientation_points` IPC → 抽屉出现角度、
   参考线、方向箭头；点"确认两点朝向"生效。
5. 抽屉 ② 输入框键入 `123.5` → 依次触发 map_status / orientation_points
   IPC / 切步（②→①→②）→ 输入框仍为 `123.5`（不得被呈现重置）；
   点"重置" → 输入框清空。
6. 失败兜底（可选）：断网/坏 Key 重建朝向页 → 明确错误提示或超时弹窗，
   输入框仍可键入方位角并提交完成朝向。
7. 截图：抽屉展开时地图矩形与抽屉不交叠、输入框聚焦光标、地图两点参考线。

## 证据归档

- `map-t40-walkthrough.log.txt`：完整日志（创建/加载/IPC/激活顺序）。
- `NN-*.png`：关键状态截图（100% 与 125% DPI 各一份）。
- 本文档追加"已验证/未通过"结论段（逐条对照验收标准）。
