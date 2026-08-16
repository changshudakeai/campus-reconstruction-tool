# T30 M5 验收缺陷修复（收口前置）

Status: completed（2026-08-17 发布收口）
Blocked by: 无。本工单基于最新 origin/main 开工（建议等 PR #17 合并后再建分支；若 PR #17 未合，先合再开工）。

## What to build（负责人视角）

1. **校区必须来自真实学校**：删除“新建演示校区”入口；用户输入学校名称 → 高德搜索 → 点选真实学校 → 进入该校方案列表；不能绕过搜索进入方案。
2. **候选采集在真实高德数据上必须成功**：点“采集”后能完成候选采集并出报告，不再弹“采集未能完成”。
3. **捎带修复**：待定文案不再显示原始键名；OSM 自动取边界失败时快速回退手动圈画（不再等约 6 分钟）；地图工具栏按钮在小窗口下仍可点击。

## 依据（权威顺序）

- `docs/adr/0008-campus-selected-via-gaode-search.md`：添加校区的唯一途径是搜索并选定真实学校；不设“添加校区”按钮；网络失败停留搜索页、弹窗重试/取消，不得绕过进入方案。
- `docs/adr/0040`：候选资格门槛（F4/B14/B2 契约），F9 只消费保留候选。
- `.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md` M5 段：真实数据人工验收、两条端到端剧本。
- `docs/developer-guide/m5-e2e/manual-acceptance-record.md`（2026-08-06 实测）：缺陷 D-1 P0、D-2/D-3 P1、D-4/D-5 P2。

## 范围

### 主修复 A：校区选择接通真实高德搜索（D-3）

- 接入校区选择页：复用 B3 已就绪的 `build_map_page_html` / `MapPageConfig` / `CampusSearchFlow` / `parse_place_search_response`（`core/gaode-client`），wry 嵌入方式参照 `apps/desktop/src/map_webview.rs`；S1 只转发意图（ADR-0037）。
- 搜索 → 候选（仅学校类 POI、过滤干扰项、同名/近名去重、多校区并列）→ 点选 → `select_campus_with_anchor`（POI 标识/名称/地址/锚点，`core/global-settings/src/settings.rs:361` 已有）→ 直接进入方案列表；重复校区只切换不重复建（ADR-0008 第 5-7 条）。
- 删除“新建演示校区”按钮、`CreateDemoCampus` 请求链路、`create_campus(&name)` 业务入口与 `campus.demo_name`/`new_demo_button` 文案键；空态只显示搜索区（ADR-0008 第 1 条）。
- 失败路径：搜索失败/网络中断 → 停留搜索页 + 弹窗“重试/取消”；取消不创建校区，不能绕过进入方案（ADR-0008 第 9 条）。

### 主修复 B：采集解析兼容真实 JS API 响应（D-1）

- `RawPoi.location`（`core/gaode-client/src/poi.rs:54`）兼容 JS API v2.0 的对象/数组格式，与 REST 风格字符串并存；或在采集脚本侧统一序列化后解析。坐标必须真实进入候选，不得静默丢弃。
- 修复后重跑剧本 B：采集报告计数真实（原始/可评审/修复/隔离），评审/封账/增强导出（`exportKind=enhanced`、`keepByCategory`）全部走通。

### 捎带修复（同 PR 内，逐项小改）

- D-2：`common` 类别注册进 localization flatten（`core/localization/src/lib.rs:140`；80d2aa4 修复不完整，`common.pending` 渲染字面量键）。
- D-4：Overpass 备用端点加客户端超时（`core/gaode-client/src/boundary_edit_map_page.rs` JS fetch），超时快速回退手动圈画。
- D-5：地图工具栏在小窗口下不被裁出，确认边界按钮始终可点。

## 明确不做

- 不新增新功能/新 ADR；不恢复旧 T 工单；不改产品基线；不开 GitHub Issues（GitHub 只用于 PR）。
- 边界持久化、生成规则精细效果、已合并分支清理不在本工单。
- 高德密钥只经应用设置页录入，禁止写入代码/日志/PR/验收记录。

## 验收标准（逐条给出证据）

1. 校区搜索：输入真实学校名 → 结果仅学校类 → 点选 → 建/选校区并直接进方案列表；重复点选只切换；搜索失败可重试/取消、不建校区、不能绕过。
2. “新建演示校区”入口、请求链路与文案键全部删除（grep 零命中）。
3. 剧本 B 真实链路：地图在线加载、边界获取、采集报告计数真实、评审保留/封账、增强导出 `exportKind=enhanced` 且 `keepByCategory` 与保留一致；`.schem` 实际包含候选内容（高度/方块计数 > 基础场地）。
4. 剧本 A 回归：基础导出仍 `exportKind=base`、默认地图正北。
5. D-2/D-4/D-5 修复生效（文案键正常渲染、OSM 超时回退在阈值内、工具栏按钮可点）。
6. 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：`cargo machete` / `cargo test --workspace` / `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo deny check advisories bans licenses sources` / `cargo xtask ci` / `cargo xtask timings`。
7. 以修复后 HEAD 重建便携包（`scripts/build-release.ps1`），重跑剧本 A/B 并更新 `docs/developer-guide/m5-e2e/manual-acceptance-record.md`（截图 + manifest 摘要），由产品负责人签名。

## 分支/PR

- 分支建议 `fix/m5-acceptance-defects`，自最新 origin/main（先 fetch）。
- 提交按逻辑拆分（校区搜索 / 采集解析 / 捎带修复），互不混入；push 后建 draft PR，等 CI 全绿。

## 交接

- 完成后向验收窗口报告：实际提交、逐条验收的测试命令与输出、文件与行号、CI 链接、重跑剧本证据、未能验证内容；更新主线计划 M5 事实段与交接文档。
