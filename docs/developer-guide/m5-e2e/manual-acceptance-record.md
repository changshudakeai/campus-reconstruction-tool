# M5 人工验收记录

验收日期：2026-08-06　验收人（产品负责人）：____________（待负责人签名）

## 环境

- 机器：本机（Windows 10+，PowerShell/Windows 桌面）　操作系统：Windows
- 产物：便携 zip（`dist/MCRebuild-V2.0.0-dev-portable.zip`，解压后
  `campus-rebuild-dev.exe`）——**注意**：包内 exe 构建时间 2026-08-06
  01:14，早于分支最后两个提交（B7 toast/common.pending 键 01:29、剧本文档），
  建议正式验收前重新构建安装包。
- 高德密钥：已授权 Web 端(JS API) 密钥（验收记录不写明文）。白名单实际验证：
  WebView 来源域名已放行——地图在线瓦片、图源声明（© AutoNavi / GS 号）与
  `AMap.PlaceSearch` 请求均成功返回（探测页返回 `complete/OK`，50 个 POI），
  未出现 `USERKEY_PLAT_NOMATCH`。应用侧 WebView 来源为 `http://wry.localhost`
  （wry `with_html` 默认 origin）。

## 剧本 A：基础导出

| 验收点 | 证据（文件/尺寸/manifest 摘要/截图） | 通过 |
|--------|--------------------------------------|------|
| 首次设置 → 校区 → 方案 | 首次向导（zh-CN、26.1.2、勾选已知悉）→ 校区选择页 → 新建方案。截图：`evidence/script-a/01-wizard.png`、`02-campus-select.png`。**缺陷：校区搜索框只搜本地已保存校区，无高德在线校区搜索；剧本步骤"输入真实校区名（上海交通大学）搜索"返回空结果（`03-campus-search-empty.png`），只能经"新建演示校区"建校区（`演示大学`）** | ☑（校区名称与剧本预期不一致，见缺陷 D-3） |
| 边界确认后导出入口可用 | 地图 WebView 真实加载高德瓦片（`04-workspace-boundary-map.png`），地图点击取点 4 个（`05-boundary-drawn.png`），"确认边界"后状态"边界已确认，可点'重置'重新绘制"（`06-boundary-confirmed.png`） | ☑ |
| `.schem` 文件 | 名称：`fc8b4e2b-5cbf-40f3-b3ca-39fd56cdf7b1.schem`　尺寸：417 字节 | ☑ |
| manifest（exportKind=base） | `fc8b4e2b-...foundation_manifest.json`：`exportKind=base`；candidateFacts 全 0、keepByCategory 空；campusName=演示大学；planName=M5剧本A验收方案新方案 1。证据：`evidence/e2e-a-files.txt`、`e2e-a-manifest.json`、`08-export-done.png` | ☑ |
| 默认正北朝向 | manifest 朝向字段：`orientation.source = "map_north"`、`degree = 0.0`（未设置自定义朝向） | ☑ |

## 剧本 B：真实高德 + 增强导出

| 验收点 | 证据（截图/计数/manifest 摘要） | 通过 |
|--------|----------------------------------|------|
| 高德地图真实在线加载 | 截图 `evidence/script-b/10-collection-page.png` 及工作区截图：WebView 显示真实高德瓦片、图源声明（© AutoNavi、GS(2025)5996号）、校区锚点标记 | ☑ |
| 校区搜索/边界获取（真实链路） | 边界获取：地图点击取点 4 个并确认成功（真实链路）。校区搜索：同剧本 A 缺陷 D-3，无高德在线校区搜索 | ☑（边界）/ ☒（校区搜索，缺陷 D-3） |
| 候选采集报告（原始/可评审/隔离） | **采集失败**：连续 2 次点击"采集"均返回错误弹窗"采集未能完成，请检查地图连接后重试。"（`10-collection-page.png` 与截图内错误模态）。根因见缺陷 D-1：真实 PlaceSearch 返回的 POI `location` 为对象，应用解析器只接受 REST 风格字符串，反序列化失败 | ☒（缺陷 D-1） |
| 评审保留 + 封账 | 不可执行（无候选）。评审页如实显示空态"暂无候选评审…评审可跳过，不阻塞导出"（`11-review-empty.png`），未伪造完成 | ☒（被 D-1 阻塞；空态行为正确） |
| 增强导出 `.schem` + manifest（exportKind=enhanced、keepByCategory） | **未产出 enhanced**：导出完成但 manifest 为 `exportKind=base`（candidateFacts 全 0），`e0b804d6-...foundation_manifest.json`。证据：`evidence/e2e-b-files.txt`、`e2e-b-manifest.json`、`12-export-done-base.png`。应用未伪造 enhanced | ☒（被 D-1 阻塞；base 兜底行为正确） |

## 缺陷清单（证据与位置）

- **D-1（P0，阻塞剧本 B 核心链路）采集解析与真实高德 JS API 响应格式不兼容**：
  应用采集经 WebView 执行 `AMap.PlaceSearch.searchNearBy('', center, 3000)`
  （`apps/desktop/src/runtime.rs` `collection_request_script`），真实响应
  `status=complete`（探测页实测 50 个 POI），但 JS API v2.0 的 POI
  `location` 为对象（`[lng,lat]` 数组），而解析器
  `core/gaode-client/src/poi.rs` `RawPoi.location: String` 只接受 REST 风格
  "经度,纬度" 文本 → serde 反序列化失败 → `collection.error_failed`
  （"采集未能完成，请检查地图连接后重试。"）。探测证据：localhost 探测页
  返回 `empty|complete|OK|50`、`poi0|type:object`、
  `loc|[116.395284,39.917514]|string?false`（探测脚本与日志仅存于临时目录，
  不含密钥）。这导致评审/封账/增强导出全部无法执行。
- **D-2（P1）`common.pending` 渲染为字面量键**：采集页六类状态显示
  "common.pending · 可跳过"（`10-collection-page.png`）。根因：
  `core/localization/src/lib.rs` `ResourceBundle::flatten` 的类别清单
  未注册 `"common"`，`zh-CN.json` 中的 `common.pending` 键永远不会展开；
  提交 80d2aa4 只补了键值未补类别注册，修复不完整。
- **D-3（P1）校区搜索无高德在线链路**：T23 地图取点页
  （`gaode_client::build_map_page_html`）未接入桌面 UI；校区搜索
  （`apps/desktop/src/production/campus_plan_trash.rs` `present_campus_search`）
  仅匹配本地已保存校区。剧本步骤"输入真实校区名 → 回车搜索 → 点选结果"
  无法执行（空结果，见 `03-campus-search-empty.png`），与 T19B-3/ADR-0008
  "高德搜索选定校区"不符。
- **D-4（P2）OSM 边界自动获取无客户端超时**：边界页先请求
  `overpass-api.de`（快速失败"Failed to fetch"）再请求备用端点
  `overpass.kumi.systems`，备用端点挂起数分钟（首轮约 6 分钟）后才
  切换"人工圈画模式"，期间地图点击无效；手动圈画模式延迟激活。
  （`core/gaode-client/src/boundary_edit_map_page.rs` `fetchOverpassBoundary`）
- **D-5（P2）地图工具栏按钮被裁出窗口**：WebView 内"撤销/清空/确认边界"
  工具栏位于地图右下，窗口宽度不足时按钮横坐标超出窗口右缘（实测 UIA
  坐标 1244–1387 超出 1000px 窗口），普通用户无法点击；本次验收经
  UIA InvokePattern 触发"确认边界"。建议核对 WebView 尺寸计算
  （`apps/desktop/src/map_webview.rs` `compute_bounds` 与 DPI 缩放）。

## 结论

- ☐ 两条端到端剧本通过，v2.0.0 进入正式版候选
- ☑ 未通过（原因与补救）：
  - 剧本 A 核心产物（`.schem` + manifest，exportKind=base、map_north）真实
    产出并通过；但"真实校区名搜索"步骤不可执行（缺陷 D-3），只能使用
    "新建演示校区"。
  - 剧本 B 地图在线加载与边界获取通过；候选采集因缺陷 D-1 必失败，评审/
    封账/增强导出被阻塞；应用如实回退基础导出（未伪造 enhanced），空态与
    回退行为正确。
  - 补救建议（交由实施窗口，非本次验收修复）：按 D-1 修复采集解析
    （兼容 JS API POI 结构或改用 REST 响应）；补 `flatten` 的 `common`
    类别；接通 T23 校区在线搜索；为 OSM 备用端点加客户端超时；核对地图
    工具栏可见性；以分支 HEAD 重新构建便携包后重跑剧本 B。

负责人签名：____________

---

## T30 修复后重跑记录（2026-08-06）

修复分支 `fix/m5-acceptance-defects`（基于 origin/main @ 9540dba，PR #17 合并后）。
便携包重建：`dist/MCRebuild-V2.0.0-dev-portable.zip`（7.78 MB，构建时间 2026-08-06
21:27）。高德 Web 端(JS API) 密钥沿用应用设置页录入值（记录不写明文；本地库仅存
于 `New-branch-v2/campus-rebuild.db` 与便携包 stage 目录，均已加入 `.gitignore`）。

### 修复与新增证据（文件/行号）

- D-1：`core/gaode-client/src/poi.rs` `RawPoi.location: Value` +
  `parse_location_value`（字符串/对象/数组三格式），`data-acquisition/src/source.rs`
  `parse_all_pois` 复用；新增 `js_api_v2_object_and_array_locations_are_accepted` 等测试。
- D-3 在线搜索：`core/gaode-client/src/map_page.rs` `searchCampus`（typeCode/
  typecode/type_code/type 四字段归一 + 页面就绪握手 + 错误回传），
  `apps/desktop/src/production/campus_search.rs` 生产传输（就绪握手/25s 超时/错误信封），
  `apps/desktop/src/runtime.rs` 采集脚本同源归一；F1 `select_campus_by_poi_id`
  （`core/global-settings/src/settings.rs`）返回 `already_added`，重复只切换；
  B2 迁移 `006_add_campus_poi_id.sql`；F3 删除 `create_campus`/`search_campuses`。
- 演示校区：`ui/campus_select.slint`、`ui/main.slint`、桌面请求枚举与 zh-CN 键全部删除，
  仓库 grep 零命中。
- D-2：`core/localization/src/lib.rs` flatten 注册 `common` 类别。
- D-4：`core/gaode-client/src/boundary_edit_map_page.rs` Overpass 主/备端点
  AbortController 12s 超时。
- D-5 根因：`apps/desktop/ui/main.slint` 工作区步骤条下移至工具栏之下（y 64/128），
  `apps/desktop/src/map_webview.rs` `compute_bounds` 修正为画布实际窗口位置
  （x:32 y:184 逻辑像素），WebView 不再盖住步骤条、不再横向越界。
- 新增阻塞缺陷（重跑发现，随本 PR 修复）：`apps/desktop/src/presentation/pages.rs`
  `CampusPlanPageState::render` 补齐 `plan-list-title/campus-name/create-button-text/
  back-button-text/empty-text/rename/duplicate/delete` 注入（此前方案列表标题与
  “新建方案”按钮文案为空，UI 无法建方案）；`tests/presentation_seams.rs` 增加断言。

### 剧本 A 重跑（基础导出）——通过

| 验收点 | 证据 | 通过 |
|--------|------|------|
| 校区在线搜索→学校类候选→点选→确认→进方案列表 | 输入“上海交通大学”→真实候选
  （闵行本部/徐汇/长宁/七宝/医学院等，截图 `03-campus-search-results-t30.png`）→
  点选→“添加并切换”确认→方案列表（截图同页） | ☑ |
| “新建演示校区”零命中 | `rg -n "新建演示校区|CreateDemoCampus|create_campus|new_demo"` 全仓 0 结果 | ☑ |
| 创建方案 | “新建方案”→输入“M5剧本A验收方案新方案1”→确认（DB `plans` 1 行） | ☑ |
| 边界确认 | 真实高德地图（© AutoNavi、GS(2025)5996号）人工圈画 4 点→确认；
  Slint 状态“边界已确认，可点'重置'重新绘制”（`06-boundary-confirmed-t30.png`） | ☑ |
| `.schem` + manifest（exportKind=base、默认正北） | `1b450f0d-….schem`（453 B，
  793×1×195）；`foundation_manifest.json`：`exportKind=base`、`orientation.source=
  map_north`、`degree=0.0`、candidateFacts 全 0、campusName=上海交通大学(闵行本部校区)
  （`08-export-done-t30.png`、`evidence/e2e-a-files.txt`、`e2e-a-manifest.json`） | ☑ |

### 剧本 B 重跑（真实高德 + 增强导出）

| 验收点 | 证据 | 通过 |
|--------|------|------|
| 地图在线加载 + 边界 | 真实高德瓦片与 POI 标注（同剧本 A）；边界已确认 | ☑ |
| 候选采集报告计数真实 | 点“采集”→真实 `searchNearBy` 完成 50 个对象：
  原始 50 / 可评审 50 / 已隔离 0 / 自动修复 0；类别 餐饮 30、生活服务 11、
  商务住宅 9（`10-collection-report-t30.png`；DB `raw_observations` 50 行） | ☑ |
| 评审保留 + 封账 | 评审工作台保留 5 项→“封账完成评审”；
  摘要“保留 5 项，待定 45 项，剔除 0 项”（`11-review-sealed-t30.png`；
  DB `review_decisions` keep 5 / pending 45） | ☑ |
| 增强导出 exportKind=enhanced | **未通过**：导出页正确提示“增强导出：基础场地 +
  保留候选 5 项（其他 5）”（`12-export-enhanced-ready-t30.png`），但点击导出后
  “场地生成失败。导出未完成；已确认的边界保持不变，请修正后重试。”
  （`13-export-enhanced-failed-t30.png`）。根因：真实 searchNearBy 返回的候选
  均为非六类 POI（餐饮/生活/商务，type 文本无 typecode），落入“其他”且无标签；
  生成引擎 `generate_other` 对无标签族如实报错（`core/generation-engine/src/rules.rs`，
  禁止静默丢弃）。候选类别映射与“其他”生成规则属工单明确不做的
  “生成规则精细效果”。增强导出管线本身由 `s1_19/s1_20` 集成测试验证
  （Building/Road/Water 候选可产出 `exportKind=enhanced`） | ☒ |

### 结论（T30 窗口）

- D-1~D-5 全部修复并生效；剧本 A 端到端通过（真实校区 + 基础导出 + 默认正北）。
- 剧本 B 采集/评审/封账通过；增强导出被“生成规则精细效果”（工单明确不做）阻塞，
  引擎如实报错不伪造结果。
- 全部门禁全绿（见 PR 描述：workspace tests 536 通过 / fmt / clippy -D warnings /
  machete / deny / xtask ci / timings）。
- 待负责人签名确认：____________
