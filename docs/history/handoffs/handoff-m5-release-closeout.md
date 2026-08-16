# 交接：M5 正式版收口（豁免清理 + 门禁证据 + 通知/教程/快捷方式/安装包 + 端到端剧本）

Status: 已实现并提交 `m5-release-closeout` 分支（PR 待合，CI 待跑）。
日期：2026-08-06。验收窗口：M5 实施窗口 → 产品负责人验收窗口。
权威依据：`docs/product-baseline.md`、ADR-0003/0014/0015/0020/0021/0031/
0037/0039/0040/0041/0042/0043、主线计划 M5 段、`docs/developer-guide/
enforcement.md`。

## 完成项

### 1. 豁免与旧接线清理

- `apps/desktop/src/presentation.rs`：1152 → 470 行，页面状态/请求拆到
  `presentation/pages.rs`（699 行），删除行数豁免。
- `apps/desktop/src/production/workspace_boundary.rs`：1404 → 583 行，
  适配器拆到 `workspace_adapter.rs`（854 行）；删除引用了已取消
  "10/10 crate 源文件上限" 的失效豁免理由。
- `apps/desktop/src/production/mod.rs`：2204 → 1281 行，拆出
  `collection.rs` / `review.rs` / `export.rs` / `notification.rs` 四个
  流程适配器；剩余为组合根本体（ProductionEntries 入口持有 + UI 回调 +
  确认路由），保留**有期限豁免**（失效里程碑 v2.1.0 / 2026-12-31）。
- 4 处 `#[allow(clippy::too_many_arguments)]`（candidate_projections.rs:88、
  boundary_export.rs:118/379、enhanced.rs:295）逐条审计后保留，理由改为
  有期限（v2.1.0 / 2026-12-31）；其余 allow（xtask 构建工具、Slint 生成
  代码）带结构性 reason，不适用期限。
- 删除覆盖率占位端口（CoveragePageState/CoveragePresentationEntry/
  CoverageProductionAdapter/show_coverage_for_test）；清理 runtime.rs、
  notice_board.slint、plan_list.slint、trash.slint、main.slint 过时
  "占位/遗留" 注释。半成品扫描（TODO/FIXME/XXX）零命中。

### 2. 门禁证据

- 补齐 `core/project-management`（F3）public-api 快照（此前只有行为测试
  冒充 API 测试、无入库快照）；已生成 `tests/snapshots/public-api.txt`。
- 21 个 workspace crate 的 public-api 快照齐备（12 基础 + 功能/流程/壳）。
- 依赖扇出（正式直接依赖）与 timings 证据归档：
  `docs/developer-guide/m5-release-closeout-evidence.md` +
  `docs/developer-guide/m5-evidence/`（含 timings-report.html）。
- 全部门禁本机全绿：machete / fmt / workspace tests（109 目标） / clippy
  `-D warnings` / deny（advisories/bans/licenses/sources） / xtask ci /
  timings（总 167.4s，最慢单元 27.2s < 120s 预算）。

### 3. 通知与教程

- B7 通知中心页 + 错误弹窗（弹窗铁律）+ 故障资料入口（ADR-0031）已有接线；
  本阶段**新增实现** warn toast 浮层与铃铛未读角标（此前 ShellPresenter
  为 no-op、注释写"T19B 后续"），toast 经 Slint Timer 自动消失、未读数随
  发布增减、点开清零。
- F2 教程：跟练气泡（方案列表 + 工作区步骤条）、知道了/跳过全部、设置
  "重新查看教程"全部接线；进度经 B2 持久化。
- 新增桌面契约测试 `apps/desktop/tests/s1_21_notification_tutorial_contract.rs`：
  公告栏记录/未读角标/诊断入口 + 教程只教一次/跳过全跳/重看可逆。
- 补齐缺失文本键 `common.pending`（采集页状态此前渲染字面量键）。

### 4. 开发版快捷方式（ADR-0014）

- `cargo xtask dev-shortcut` 实测：release 构建 → 更新
  `%LOCALAPPDATA%\MCRebuildV2\dev\campus-rebuild-dev.exe`（上一版本保留
  `previous\`）→ 创建/更新桌面"校园复刻工具 - 开发版.lnk"。
- 验证：.lnk 存在、TargetPath 正确、启动后进程响应正常（证据
  `m5-evidence/08-dev-shortcut.txt`）。

### 5. 安装包（ADR-0003 轻快）

- 新增 `scripts/build-release.ps1`（可复现构建脚本）：release 构建 → 暂存
  exe + resources → `dist/MCRebuild-V2.0.0-dev-portable.zip`（7.74 MB）。
- 验证：解压后 `campus-rebuild-dev.exe` 启动正常、进程响应；基础导出能力
  由 s1_08 集成测试（同一 UI 链路真实写出 .schem + manifest）背书。

### 6. 端到端剧本与真实高德

- 剧本 A（基础导出）与剧本 B（真实高德 + 增强导出）运行手册与人工验收记录
  模板：`.scratch/v2-implementation/m5-e2e/`。
- 剧本 A 自动化等价证据：`s1_08_boundary_export_flow` 集成测试全绿。
- **真实高德在线链路未能在本机走通**：本地演示密钥调用高德 REST API 返回
  `USERKEY_PLAT_NOMATCH`（密钥平台/域名未对本机放行）。应用侧 WebView 使用
  高德 JS API 2.0 + securityJsCode，需要 Web 端(JS API) 密钥且域名白名单
  覆盖验收机器。这是 M5 唯一强制人工环节：需产品负责人提供已授权密钥或
  约定现场验收时间/环境。

## 分支与提交

分支 `m5-release-closeout`，基于最新 origin/main（2b9259b，PR #16 合并后）。
提交按逻辑拆分（见 PR 描述）：豁免/接线清理、门禁证据、通知/教程、快捷方式/
安装包、端到端剧本与验收记录。push 后建 draft PR，等 CI `conclusion` 全绿。

## 验收证据位置

- 门禁原始输出：`docs/developer-guide/m5-evidence/*.txt` + `timings-report.html`
- 豁免审计/依赖扇出/timings 摘要：`docs/developer-guide/m5-release-closeout-evidence.md`
- 契约测试：`apps/desktop/tests/s1_21_notification_tutorial_contract.rs`
- 安装包：`dist/MCRebuild-V2.0.0-dev-portable.zip`；构建脚本 `scripts/build-release.ps1`
- 端到端剧本 + 验收模板：`.scratch/v2-implementation/m5-e2e/`

## 未能验证内容（需产品负责人/环境）

- 真实高德在线候选采集链路（密钥授权 + 现场验收）
- 两条端到端剧本的人工现场确认（负责人签名）
- PR CI 最终结果（以 CI `conclusion` 为准）

## 剩余工作（不属于 M5）

边界持久化（另行工单）、生成规则精细效果（后续规则完善）、已合并分支清理
（另行确认）。

## M5 人工验收结果（2026-08-06，产品负责人密钥现场执行）

**Status 更新**：产品负责人已提供授权 Web 端(JS API) 密钥，验收窗口已按
`docs/developer-guide/m5-e2e/script-a-basic-export.md` 与
`script-b-enhanced-export.md` 执行。结论：**剧本 A 产物通过、剧本 B 候选采集
未通过（代码缺陷 D-1，非密钥/白名单问题）**。完整记录见
`docs/developer-guide/m5-e2e/manual-acceptance-record.md`，证据位于
`docs/developer-guide/m5-e2e/evidence/`。

验收要点：
- 高德密钥与白名单**实际放行**：地图在线瓦片、图源声明、`AMap.PlaceSearch`
  请求均成功（探测返回 `complete/OK`，50 个 POI），未再出现
  `USERKEY_PLAT_NOMATCH`。
- 剧本 A：`.schem`（417B）+ manifest（`exportKind=base`、
  `orientation.source=map_north`、候选空）真实产出。
- 剧本 B：地图在线加载/边界取点/确认通过；**候选采集必失败**——真实 JS API
  v2.0 的 POI `location` 为对象，`core/gaode-client/src/poi.rs` 的
  `RawPoi.location: String`（REST 风格）反序列化失败 → `collection.error_failed`；
  评审/封账/增强导出因此不可执行，应用如实回退基础导出（未伪造 enhanced）。
- 其他缺陷：`common.pending` 字面量键（`flatten` 未注册 `common` 类别，
  80d2aa4 修复不完整）；校区搜索无高德在线链路（T23 取点页未接入）；
  OSM 备用端点无客户端超时导致手动圈画延迟激活；地图工具栏按钮在窗口宽度
  不足时被裁出窗口；便携包 exe 构建时间早于分支最后提交。

**建议**：缺陷修复（优先 D-1）后以分支 HEAD 重新构建 `dist` 便携包，重跑
剧本 B 再做最终验收确认。

## T30 修复窗口交接（2026-08-06，`fix/m5-acceptance-defects`，PR 待合）

- **实际提交**：4 个逻辑提交（D-1 采集解析 / D-3 校区搜索核心 / D-2+D-4+D-5
  捎带 / D-3 桌面接线与 s1_04 重写）+ 本窗口追加提交（type 文本分类兼容、
  plan-list 文案注入、D-5 根因布局修复、验收记录与证据）。push 后开 draft PR
  等 CI。
- **剧本 A 重跑通过**：真实校区“上海交通大学(闵行本部校区)”→ 方案 →
  边界确认 → `exportKind=base`、`orientation.source=map_north`、`.schem`
  453 B（793×1×195）。证据 `docs/developer-guide/m5-e2e/evidence/script-a/
  *-t30.png` 与 `e2e-a-files.txt`/`e2e-a-manifest.json`。
- **剧本 B 重跑**：采集 50 对象（真实 searchNearBy，计数真实）、评审保留 5 +
  封账通过；**增强导出未通过**——真实候选均为非六类 POI（餐饮 30/生活服务 11/
  商务住宅 9），落“其他”无标签，生成引擎 `generate_other` 如实报错（禁止
  静默丢弃）。候选类别映射与“其他”生成规则属 T30 明确不做的“生成规则精细效果”。
- **门禁**：workspace tests 536 通过 / fmt / clippy -D warnings / machete /
  deny / xtask ci / timings 全绿（Windows，`SLINT_BACKEND=software`、
  `CARGO_BUILD_JOBS=2`）。
- **未能验证**：剧本 B 的 `exportKind=enhanced` 产物（被上述生成规则缺口阻塞）；
  负责人现场签名。
- **剩余工作（不在本工单）**：边界持久化（另行工单）、生成规则精细效果、
  已合并分支清理。

## T31 验收窗口走查交接（2026-08-07）

验收窗口用合并后便携包（`dist/MCRebuild-V2.0.0-dev-portable.zip`，HEAD
7d0659e，252ff25 仅更新文档）完成真实 GUI 走查：

- **走通**：首次向导（zh-CN / 26.1.2 / 已知悉）；设置页录入高德 JS API
  Key + 安全密钥（沿用已提供凭据，只经设置页，不落明文；DB 校验值一致）；
  校区搜索“上海交通大学”→ 高德在线返回真实校区列表并确认“闵行本部校区”
  （POI B00155R1D5）；新建方案“M5剧本A走查方案”；OSM 边界自动获取成功
  （“来自 OSM: 上海交通大学（闵行校区）✓”，页面含高德瓦片署名与
  © OpenStreetMap contributors）。
- **新增缺陷 T31-D6（P1）**：边界地图 WebView 内容横向溢出，“确认边界”/
  “改人工圈画”按钮渲染在 WebView 视口右缘之外（UIA (860, 521)/(806, 529)，
  窗口逻辑宽 800），不可见不可点；自动获取成功但无法确认边界，剧本 A 导出
  与剧本 B 全部后续步骤被阻塞。疑似与 T30 D-5 同根因，建议实施窗口核对
  `map_webview::compute_bounds` 与 `boundary_edit.slint` 画布几何、AMap 容器
  初始化宽度，修复后重建便携包重跑剧本 A/B。
- 证据：`docs/developer-guide/m5-e2e/evidence/script-a/*-t31walk.png`（18 张）
  与 `evidence/e2e-t31-walkthrough-files.txt`；验收记录
  `docs/developer-guide/m5-e2e/manual-acceptance-record.md`“T31 走查记录”段。
- 签名栏：负责人签名待 T31-D6 修复重跑后确认。

## T32 实施窗口交接（2026-08-07，`fix/t32-boundary-page-overflow`）

按工单 T32 修复 T31-D6（边界地图页按钮横向溢出），并用修复版
`campus-rebuild-dev.exe` 重跑剧本 A/B。交付验收窗口：

- **根因（实测）**：`apps/desktop/src/map_webview.rs` 把 slint
  `Window::size()` 返回的物理宽当逻辑宽再乘 scale，WebView 物理宽 =
  (1000−32)×1.25 = 1210 超出窗口物理宽 1000（T31-D6 与 T30 D-5 同族但未根治）。
  修复：新增 `logical_window_width()`（物理 ÷ scale），4 处调用点统一改用
  逻辑宽。
- **布局修复**（`core/gaode-client/src/boundary_edit_map_page.rs`）：
  `html/body overflow-x:hidden`、`#map-container max-width:100%`、AMap 初始化前
  钳制容器宽度 + `map.resize()`、resize 监听同步、地图初始化延后到
  `window load`。配套 `workspace_adapter.rs`：步骤 ≥3 隐藏地图 WebView。
- **按钮可见可点**（验收点 1，800/1000 逻辑宽 × 125% 缩放，DPI 120 实测）：
  确认边界/改人工圈画右缘均 < 视口（800 宽下相对右缘 664.8/758.4；1000 宽下
  864.8/958.4），点击可切换到人工圈画模式；截图 + UIA 断言
  `docs/developer-guide/m5-e2e/evidence/t32/`。
- **剧本 A 走通**：OSM 自动获取 → 确认 → 基础导出（planId 3a8baae2，`.schem`
  5,341 B；manifest `exportKind=base`/`map_north`/attribution）。
- **剧本 B 走通**：OSM/Overpass 采集原始 1037（建筑 761/其他 276）、可评审
  1026、隔离 11、修复 1026；评审保留 5（建筑）→ 封账 → 增强导出（`.schem`
  109,263 B = 3461×14×1988，`exportKind=enhanced`，`keepByCategory=Building 5`
  与保留一致，attribution 在）。DB 终态计数见
  `evidence/t32/t32-db-final-state.txt`。
- **门禁全绿**：fmt / machete / clippy -D warnings / workspace tests（全 ok）/
  deny 四连 / xtask ci / timings（Windows，`SLINT_BACKEND=software`、
  `CARGO_BUILD_JOBS=2`）。便携包重建 `dist/MCRebuild-V2.0.0-dev-portable.zip`
  （7.91 MB）。
- **新发现缺陷 T32-D2（P2，未修复，另行工单）**：评审工作台候选列表无滚动
  容器，大候选集（1026 项）下“封账完成评审”操作栏渲染在视口外不可达；走查
  通过切换到空分类（道路 0）使操作栏回到视口内完成封账。小候选集不受影响。
- **密钥合规**：只经设置页录入，仓库 `git status/diff` 无新增含密钥文件。
- 验收记录：`docs/developer-guide/m5-e2e/manual-acceptance-record.md`
  “T32 走查记录”段；签名栏留产品负责人。

## T33 实施窗口交接（2026-08-07，`fix/t33-review-list-scroll`）

按工单 T33 修复 T32-D2（评审候选列表无滚动容器，大量候选下操作栏不可达），
只改 UI 布局，不动评审/封账逻辑。交付验收窗口：

- **根因**：`apps/desktop/ui/review.slint` 候选卡片 `for` 循环直接排在页面
  `VerticalLayout` 中、无滚动容器；候选多时内容整体向下溢出，底部操作栏
  （逐项判定/批量确认/封账）被推出视口，滚轮/键盘均不可达。
- **修复（仅 UI 布局）**：候选卡片移入 `ScrollView`（fluent），页面高度约束
  `parent.height - 128px`，操作栏固定于页面可视底部；`FocusScope` 转发键盘
  滚动（Up/Down/PageUp/PageDown/Home/End）；分类标签点击复位滚动到顶；
  空态布局与文案不变；`review-list-viewport-y` 为 T33 契约观测（双向链上联，
  仅布局观测，不参与业务）。
- **契约测试（先红后绿）**：`apps/desktop/tests/s1_22_review_scroll_contract.rs`
  真实 1026 候选（建筑 1000 + 道路 26）→ 滚轮/键盘滚动有效、分类切换复位、
  “封账完成评审”真实点击落账。修复前红态：无滚动容器下断言失败；修复后
  `s1_22` 通过（27s）。
- **验收证据**：截图
  `docs/developer-guide/m5-e2e/evidence/t33-review-scroll/
  review-1026-scrollbar-actionbar.png`（1026 候选页，右侧滚动条 + 底部
  暂停/恢复/封账按钮可见）+ `s1_22` 可点性断言（真实点击封账成功）。
- **回归**：`s1_16` / `s1_17` / `s1_18` / `s1_22` 全绿；全部门禁全绿
  （machete / workspace tests 全 ok / fmt / clippy -D warnings / deny /
  xtask ci / timings，Windows，`SLINT_BACKEND=software`、
  `CARGO_BUILD_JOBS=2`）；便携包重建 `dist/MCRebuild-V2.0.0-dev-portable.zip`。
- **明确未做**：评审/封账逻辑、F5 核心、产品基线未改；未开 GitHub Issues。
- 验收记录：`docs/developer-guide/m5-e2e/manual-acceptance-record.md`
  “T33 修复记录”段；签名栏留产品负责人。

## T34 实施窗口交接（2026-08-08，`fix/t34-map-first-workspace-layout`）

按工单 T34 把五步工作区改为"地图为主 + 左侧抽屉"（做法 A：地图让位）。
交付验收窗口：

- **根因**：地图 WebView 硬编码坐标叠在 Slint 画面上，与文本/按钮框架互相
  遮挡（`main.slint` y:470/y:500 遗留重复）；朝向页两点/角度模式切换与页内
  罗盘/输入框叠放难懂；地图内 HTML 工具栏与 Slint 框架按钮功能重复，是
  T31-D6/T32 两轮 bug 的来源；Slint 模态弹窗会被地图原生子窗口盖住。
- **布局（做法 A）**：步骤 ①②③⑤ = 顶部五步条 + 地图主画面 + 左侧可收拉
  抽屉（宽 300 逻辑像素，左缘箭头开合）；抽屉展开地图右移让位（+312 逻辑），
  收起恢复；步骤 ④ 评审保持现状整页，不并入抽屉、不改滚动/封账逻辑。
- **地图矩形**：`map_webview` 不再硬编码 (32,184,w-32,340)，改由 Slint
  布局槽位 `workspace-map-slot-x/y/width/height` 上报（逻辑像素），300ms
  轮询 `set_bounds` 跟随（含抽屉开合）；HTML `map.resize` 同步画布；
  125% DPI 物理右缘不越界（T31-D6 回归防护）。
- **交互迁移**：地图 HTML 工具栏按钮全部删除（确认/撤销/清空/改人工圈画/
  添加区域），地图退化为纯画布 + 消息桥；抽屉 ① 点数/撤销/清空/确认边界，
  ② 自动角度/小罗盘/手动输角度/确认两点朝向/重置（覆盖走 F5 重算确认），
  ③ 采集来源/开始/进度/差异摘要/查看报告，⑤ 导出开始/结果/错误；朝向页
  删除模式切换与页内画布/罗盘/输入框；全部文案走 zh-CN.json、颜色走 Theme。
- **弹窗遮挡统一**：错误/确认/输入弹窗前隐藏地图、关闭后按当前步骤模式
  （边界页 vs 朝向页）恢复；`map_webview::restore_after_modal` + 步骤守卫
  （评审页不恢复、校区搜索页取消后恢复）。
- **清理**：`main.slint` y:470/y:500 遗留重复删除；旧 `osm_elements`/
  `convertAndDraw` 死接线删除（B3 解析兼容保留、S1 不再调用、无静默后备）；
  多区域 UI 随工具栏删除（`confirm_boundary` MultiPolygon seam 保留，
  `s1_12` 覆盖；UI 入口不在本工单抽屉清单，后续另行决策）。
- **测试**：新增 `s1_23_workspace_drawer_contract`（抽屉开合让位关系、
  800×666/1000×666 矩形互不相交、朝向覆盖重算确认、旧 IPC 惰性）；
  `s1_15` 改写为真实 WebView 驱动抽屉桥接命令；`s1_05/s1_06` 等存量套件
  全绿；全部门禁全绿（machete / workspace tests / fmt / clippy -D warnings /
  deny / xtask ci / timings，Windows，`SLINT_BACKEND=software`、
  `CARGO_BUILD_JOBS=2`）。
- **便携包与证据**：`scripts/build-release.ps1` 重建
  `dist/MCRebuild-V2.0.0-dev-portable.zip`（7.92 MB，HEAD bee5bdb），
  解压启动验证通过（种子库落地方案列表 → 打开方案 → 工作区布局可见可点）；
  证据见 `docs/developer-guide/m5-e2e/evidence/t34/`（11 张截图 +
  `t34-drawer-rects.txt`）。
- **明确未做**：步骤 ④ 评审并入新布局（另行决策）；做法 B（抽屉覆盖在
  地图上）留作后续版本方向；业务规则不变（边界唯一必填 ADR-0041、朝向
  可选默认正北、采集/评审可跳过、导出资格不变）；S1 只呈现与转发
  （ADR-0037）；未开 GitHub Issues。
- 验收记录：`docs/developer-guide/m5-e2e/manual-acceptance-record.md`
  "T34 实施窗口记录"段；真实密钥走查由验收窗口补充，签名栏留产品负责人。
