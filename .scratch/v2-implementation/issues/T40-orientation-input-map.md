# T40 朝向输入框被呈现清空 + 地图两点点击无反馈（P1）

Status: **已实现并合入本地 main**（commit 95b75ad，分支
`fix/t40-orientation-input-map`，自本地 main d24bbf2；2026-08-10 在干净
worktree `D:\MCRebuild_Renovation\.t40-worktree` 验证后合并）。
验收窗口：真机 100%/125% DPI 走查（剧本见
`docs/developer-guide/m5-e2e/evidence/t40/README.md`）。

## What to build（负责人视角）

1. 步骤②"手动输角度"输入框：键入后任意呈现（map_status、orientation_points
   IPC、切步）都不得把输入框重置成空。
2. 步骤②地图两点点击要可靠挂接，页面就绪后点击即反馈角度与参考线。
3. 步骤②地图 WebView 创建失败 / SDK 加载失败：明确错误提示，仍可退回
   "方位角手动输入"完成朝向，不得呈现无声空白地图。
4. 125% DPI（先按 T35 记录刷新 Windows 会话）走查：抽屉展开时 WebView
   矩形不覆盖抽屉、输入框可聚焦可输入、地图可点击。

## 根因

1. 输入框被清空：`OrientationViewState::render` 每次呈现都执行
   `set_workspace_orientation_input_text(session.orientation_input_text)`，
   而会话里的副本只在提交时更新——任何一次无关呈现（map_status /
   orientation_points IPC / 切步）都会把窗口输入覆盖回旧值/空值。
2. 朝向点击无反应：激活只依赖 T37 的 map_ready 兜底，但朝向页在
   onMapReadyForMode 之外从不发送 map_ready，Rust 侧兜底路径实际不触发；
   且激活没有"创建成功即触发"的独立通道（T36 真机证据：initOrientationMode
   已运行但真实鼠标输入不达 WebView2 子窗口，需日志钉死确切环节）。
3. 失败兜底：T36 已有创建失败 → 提示 + 手动输入兜底（s1_27），本单回归锁定。

## 修复

1. `presentation/pages.rs`：`OrientationViewState` 移除 `input_text` 回写，
   改为一次性 `clear_input` 请求；渲染只在 clear_input 时写一次空串。
   `workspace_boundary.rs`：会话字段 `orientation_input_text` 删除（输入值只
   活在窗口，提交时读取）。
   `workspace_adapter.rs`：显式清空入口 = 重置（OrientationReset）、
   地图清除重来（orientation_clear IPC）、切换方案（open_plan）——三个入口
   直接在返回的页面状态上置 `clear_input`（一次呈现即消费，天然一次性）。
2. `map_webview.rs`：朝向页 WebView 创建成功回调立即
   `evaluate_script("activateOrientationWhenReady();")`（不依赖 map_ready）；
   `boundary_edit_map_page.rs`：新增 `window.activateOrientationWhenReady`
   （SDK/地图未就绪时静默等待自动激活兜底）；朝向页在 onMapReadyForMode
   之外同样发送 `map_ready`，启用 T37 的 map_ready_for_active_step 兜底。
3. 失败兜底沿用 T36（status=false → 明确提示 + map_available=false；
   error IPC → 错误弹窗 + hide + 手动输入可用），s1_28 回归锁定。

## 验证（2026-08-10，Windows，SLINT_BACKEND=software、CARGO_BUILD_JOBS=2）

- 定向回归全绿：`cargo test -p desktop-shell --test s1_06_orientation_flow
  --test s1_26_orientation_map_two_click_chain
  --test s1_28_orientation_input_persistence_contract`（3 项各 1 用例）。
- 全部门禁全绿：machete / `cargo test --workspace`（120 组 ok）/ fmt --check /
  clippy `-D warnings` / deny（advisories/bans/licenses/sources）/ xtask ci
  （tidy + arch）/ timings（120s 预算内）。
- 合入本地 main（fast-forward，commit 95b75ad）；**未推送 GitHub**。
- 便携包经 `scripts/build-release.ps1` 重建（见交接报告）。

## 待验收窗口（真机）

- 100%/125% DPI（125% 先按 T35 记录刷新 Windows 会话）：步骤②点两点 →
  抽屉角度 + 参考线 → 确认两点朝向生效；输入框键入数字并提交；抽屉展开时
  WebView 矩形不覆盖抽屉、输入框可聚焦可输入、地图可点击。
- `MCREBUILD_LOG_FILE` 抓日志，钉死"地图点击无反馈"在真机上的确切环节
  （创建成功 → 激活通道脚本 → 页面 map_ready IPC → 兜底激活 → 两次点击
  orientation_points IPC → F5 角度回填）。

## 验收（逐条证据）

1. 真机（真实 Key + 100%/125% DPI）：步骤②点两点 → 抽屉出现角度与参考线 →
   确认两点朝向生效；输入框键入数字并提交。
2. 回归测试（s1_28 新增 + s1_06/s1_23/s1_26/s1_27 + map_webview 单测）全绿；
   门禁全绿；重建便携包并覆盖安装 `MCRebuildV2\dev`。
3. `MCREBUILD_LOG_FILE` 抓真机日志，证据顺序：
   创建成功 → notify_status(true) → 创建成功通道激活脚本 →
   页面 map_ready IPC → Rust 侧 map_ready_for_active_step 激活 →
   两次点击 orientation_points IPC → F5 角度回填。

## 阻塞（2026-08-10 11:54-11:55 观测）

- 并发 T39 会话在同一工作树活动：`review_map_page.rs` 最近修改时间
  11:54:06；`map_webview.rs`/`pages.rs`/`lib.rs` 也被 T39 增量编辑
  （未提交），工作树呈 T40+T39 混编且 `gaode-client` 当前无法编译
  （`ReviewMapPageConfig` 缺 `candidates`/`effective_height_px`，T39
  重构未收口）。因此无法干净提交 T40、无法跑门禁。
- 处理结果：T39 于 d24bbf2 收口后，本工单改在干净 worktree
  `D:\MCRebuild_Renovation\.t40-worktree` 重做并验证；主 checkout 混编树
  已按负责人指示废弃（快照
  `.t39t40-conflict-snapshot-20260810-121029/working-tree.patch` 留存）。
