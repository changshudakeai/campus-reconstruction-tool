# T31 真实轮廓与边界来源：OSM/Overture 接通修复（M5 收口前置）

Status: completed（2026-08-17 发布收口）
Blocked by: T30（已合入 main，merge a929a5e2）

## What to build（负责人视角）

1. **校区边界应能自动获取**：打开真实校区（如上海交通大学）的边界步骤时，应自动从 OSM/Overture 获取校园边界并绘制；只有数据源不可用或该校确无数据时才提示并切换到手动圈画。当前 OSM 查询实际不可用，需要先诊断再修复。
2. **候选采集应拿到真实轮廓**：候选对象必须来自能提供真实轮廓/类别的数据源（OSM 建筑面 / Overture），让真实数据下增强导出能产出内容；高德 POI 点位只作名称/位置/来源证据，不得伪造面。
3. **验收目标**：剧本 B 在真实数据下走通 采集 → 评审 → 封账 → 增强导出（`exportKind=enhanced`、`keepByCategory` 一致、`.schem` 含候选内容）。

## 数据源口径（产品负责人 2026-08-07 明确，修正既有实现偏差）

- **高德只当地图与身份来源**：地图底图、校区搜索身份（POI 名称/位置/锚点，ADR-0008）、GCJ-02 坐标转换。高德不提供免费的建筑立体几何；AOI 为付费服务，不作为候选几何来源。
- **候选采集数据源 = OSM / Overture**：ADR-0013 首发内置 OSM、Overture 适配器；ADR-0004 明确高德 Web 服务 Key“不属于正式 OSM/Overture 采集流程的必要配置”。当前生产把 `GaodeDataSource`（高德 PlaceSearch POI 点位）接为候选采集源（`apps/desktop/src/runtime.rs:562` 注入 `CollectionFlow`）属偏离规划的落地错误，本工单必须撤下。
- **边界 = OSM 自动获取优先**（ADR-0029）：Overpass `amenity~university|college|school` way/relation，排序（锚点包含优先 → 名称匹配 → 距离最近）后自动绘制，人工仅调整；查询失败或无数据才切手动圈画兜底；WGS-84 须经 GCJ-02 转换后上屏。
- `docs/adr/0040`：候选资格门槛；点位不得用固定半径/包围盒/模板扩展成面（不猜测真实形状）。
- 主线计划 M5 段；`docs/developer-guide/m5-e2e/manual-acceptance-record.md`（T30 重跑：真实数据下增强导出未产出 enhanced）。

## 依据（权威顺序）

- `docs/adr/0008` / `0013` / `0004` / `0029` / `0040` 与本节“数据源口径”。
- 说明：`docs/adr/0017` 模块目录中 F4 行“从高德/Overture 拉取候选对象”与本口径冲突，以 ADR-0013/0004 与产品负责人明确口径为准；交接时记录该修正，不改动 ADR-0017 原文（如需修正另行确认）。
- 调研报告：`docs/research/candidate-data-sources-and-naming.md`（2026-08-07，上海网络实测：Overpass 三硬伤根因、端点可用性/CORS 实测、OSM 建筑覆盖 277 面、Nominatim 校名解析、高德配额表、坐标转换与命名方案）。

## 调研根因（2026-08-07 实测，直接决定修复方式）

“Overpass 不可用”不是数据源问题，是应用查询的三个硬伤叠加（`core/gaode-client/src/boundary_edit_map_page.rs:411/421`）：

1. 请求 URL 少了 `data=` 参数（`interpreter?<查询>` → 服务器必然报 `parse error: Unknown type "%"`）；正确写法是 `interpreter?data=<编码查询>`。
2. `university|college|school` 的 `|` 正则被新版 Overpass（overpass-api.de 0.7.62.11）解析器拒绝（旧版镜像接受所以一直未暴露）；改用 union 写法（`(way["amenity"="university"];way["amenity"="college"];way["amenity"="school"];)`）。
3. overpass-api.de / kumi 不给浏览器 CORS 头（WebView 内 fetch 会被同源策略拦截；只有 maps.mail.ru 带 `Access-Control-Allow-Origin: *`）→ **候选采集与边界查询从 Rust 侧直连，绕开 WebView CORS**。

端点实测（上海网络）：de 最快但负载高（偶发 504）、kumi 慢（偶发 504）、mail.ru 有 CORS 且较稳定 → 按 de → kumi → mail.ru 回退，每端点 10–15 秒超时。

## 范围

### A. 校区边界自动获取修复（首要）

- 修复三硬伤：URL 补 `data=`；`amenity~"university|college|school"` 正则改 union；Overpass 查询从 Rust 侧直连（不再依赖 WebView fetch 与 CORS）。
- 校名 → 校区元素：用 Nominatim 解析校名得到 `osm_type/osm_id`（实测“上海交通大学（徐汇/七宝校区）”可解析到 way 144183801/538416377；按 OSMF 政策 ≤1 次/秒并带 User-Agent），再按 ID 从 Overpass 拉取边界；失败回退 `landuse=education` 查询，再回退手动圈画。
- 恢复 ADR-0029 排序与自动绘制（锚点包含优先 → 名称匹配 → 距离最近）；手动圈画仅兜底且提示明确。
- 验收：真实校区自动获取边界成功（来源标注、自动绘制、排序正确）。

### B. 候选轮廓来源（OSM/Overpass 生产接线）

- 把已有 `OverpassDataSource`（`core/data-acquisition/src/source.rs:119`）接入生产采集路径（`apps/desktop/src/runtime.rs:562` 的 `GaodeDataSource` 撤下）；候选建筑查询用 union 写法 `building=*`（面几何 + name/building:levels 标签）。
- Overpass 传输走 Rust 侧直连：端点 de → kumi → mail.ru 回退，每端点 10–15 秒超时，失败返回结构化 `SourceUnreachable`。
- Overture Buildings 作为后续“离线补充包”（月更 GeoParquet，本工单不实现实时查询；如实现下载切片本地查询另行评估）。
- 把 `GaodeDataSource` 从生产采集路径撤下：高德点位不再作为候选采集数据源；校区身份仍走 ADR-0008 高德搜索（名称/位置/锚点），但不产生候选几何。
- 明确点位语义（写进实现与测试）：点位只作名称/位置/来源证据，不参与候选面几何；禁止固定半径/包围盒/模板扩面（ADR-0040 红线）；采集报告如实区分“点位证据”与“可评审面候选”。

### C. 坐标转换与候选命名

- 坐标转换：应用工作坐标系保持 GCJ-02（已入库边界即 GCJ-02）；OSM 的 WGS-84 面数据在采集入口转 GCJ-02（官方 `AMap.convertFrom` 单次 ≤40 点分批，或开源转换批量转、精度约 1 米），同时保留原始 WGS-84 字段备查；不做 GCJ→WGS 反向。
- 命名两级：优先 OSM `name` 标签（零成本、与几何同源）；无名字的关键建筑（教学楼/图书馆/宿舍等）用高德逆地理编码（regeo）补名并缓存。配额：regeo 个人 5000 次/日、企业 300 万次/日，一所学校几百栋建筑够用；regeo 需独立 Web 服务 Key（与 JS API Key 不同）；高德搜索类接口个人仅 100 次/日，生产采集不得依赖高德搜索。
- 合规：OSM/Overture 建筑数据为 ODbL 许可，界面与导出物需署名 © OpenStreetMap contributors。

### D. 明确不做

## 范围

### A. 校区边界自动获取修复（首要）

- 诊断当前 Overpass 实际不可用的根因并留证据：endpoint（`overpass-api.de`）可达性/网络、CORS、查询半径与数据缺失、响应解析（含 T30 前约 6 分钟延迟与 12s 超时后的表现）。
- 修复：多公共 Overpass 端点回退或按区域选择（必要时含 Overture 备源，ADR-0013 已规划）；恢复 ADR-0029 的排序与自动绘制；手动圈画仅作为兜底且提示明确。
- 验收：真实校区自动获取边界成功（截图 + OSM/Overture 来源标注），人工调整与确认正常。

### B. 候选轮廓来源（选项 B 落地）

- 候选采集切换到 OSM/Overture：复用已有 `OverpassDataSource`（`core/data-acquisition/src/source.rs:119`，能解析 node/way 面几何，当前未接线）接入生产采集路径；按 ADR-0013 补 Overture 适配器（B12，如需要）。
- 把 `GaodeDataSource` 从生产采集路径撤下（`apps/desktop/src/runtime.rs:562`）：高德点位不再作为候选采集数据源；校区身份仍走 ADR-0008 高德搜索（名称/位置/锚点），但不产生候选几何。
- 明确点位语义（写进实现与验收）：点位只作名称/位置/来源证据，不参与候选面几何；禁止固定半径/包围盒/模板扩面（ADR-0040 红线）；采集报告如实区分“点位证据”与“可评审面候选”。
- 验收：真实校区采集后存在可评审面候选（数量 > 0），评审保留 → 封账 → 增强导出 `exportKind=enhanced` 走通，`keepByCategory` 与保留一致，`.schem` 实际包含候选内容（高度/方块计数 > 基础场地）。

### C. 明确不做

- 生成规则精细效果（建筑样式等）仍属后续规则完善；不新增新功能/新 ADR（除非诊断需要登记数据源回退决策）；不改产品基线；不开 GitHub Issues。
- 边界持久化另行工单；已合并分支清理另行确认。

## 验收标准（逐条给出证据）

1. 真实校区（如上海交通大学）边界自动获取成功：来源标注、自动绘制、排序正确；手动圈画仅兜底且提示明确。
2. Overpass 不可用根因与修复的证据（端点/网络/查询/解析，实测命令与输出）。
3. 采集候选含真实轮廓：可评审面候选 > 0 且来自面几何；高德点位不扩面（代码审计 + 测试断言）。
4. 剧本 B 真实走通：`exportKind=enhanced`、`keepByCategory` 与保留一致、`.schem` 含候选内容。
5. 剧本 A 回归：`exportKind=base`、默认正北不变。
6. 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：machete / workspace tests / fmt / clippy -D warnings / deny / xtask ci / timings。
7. 坐标转换与命名：采集入口 WGS-84→GCJ-02 且保留原始 WGS-84 字段；OSM name 优先、regeo 补名有缓存；界面含 OSM 署名。
8. 以修复后 HEAD 重建便携包（`scripts/build-release.ps1`），重跑剧本 A/B，更新 `docs/developer-guide/m5-e2e/manual-acceptance-record.md`，由产品负责人签名。

## 分支/PR

- 分支建议 `fix/t31-real-outline-boundary-sources`，自最新 origin/main（先 fetch）。
- 提交按逻辑拆分（Overpass 诊断与端点回退 / 边界排序恢复 / 候选面源接入与点位语义 / 剧本重跑证据），互不混入；push 后 draft PR，等 CI 全绿。

## 交接

- 完成后向验收窗口报告：实际提交、逐条验收命令与输出、文件与行号、CI 链接、重跑剧本证据、未能验证项；更新主线计划 M5 事实段与交接文档。
