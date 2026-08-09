# M5 人工验收记录

验收日期：2026-08-06　验收人（产品负责人）：____________（待负责人签名）

---

## T33 修复记录（2026-08-07，实施窗口 fix/t33-review-list-scroll）

工单：`.scratch/v2-implementation/issues/T33-review-list-scroll-fix.md`
（缺陷 T32-D2：评审候选列表无滚动容器，大量候选下“封账完成评审”操作栏
渲染在视口外不可达）。

### 根因

`apps/desktop/ui/review.slint` 的评审候选卡片 `for` 循环直接排在页面
`VerticalLayout` 中、无滚动容器；候选多时（走查 1026）内容整体向下溢出，
底部操作栏（逐项判定/批量确认/封账）被推出视口，滚轮/键盘均不可达。

### 修复（仅 UI 布局，不动评审/封账逻辑与 F5 入口）

- 候选卡片移入 `ScrollView`（fluent，滚动条 as-needed），页面高度约束为
  `parent.height - 128px`，操作栏固定于页面可视底部。
- 键盘滚动：`FocusScope`（点击列表区域获得焦点）转发 Up/Down/PageUp/
  PageDown/Home/End 到滚动容器；滚轮由 ScrollView 原生处理。
- 分类标签点击复位滚动到顶；空态（无候选）布局与文案不变。
- 契约观测 `review-list-viewport-y`（in-out 双向链上联滚动位置，仅布局观测）。

### 自动化契约测试（先红后绿）

新增 `apps/desktop/tests/s1_22_review_scroll_contract.rs`：真实构造 1026 个
可评审候选（建筑 1000 + 道路 26，贴近 T32 走查）进入评审工作台，窗口按默认
800×600 显示并完成真实布局后断言：

1. 滚轮向下滚动有效（真实 `PointerScrolled` 事件后 `viewport-y < 0`）；
2. 键盘滚动有效（点击列表获得焦点后 DownArrow 使 `viewport-y` 继续下降）；
3. 分类切换后滚动状态合理（真实点击“建筑”标签复位到顶）；
4. “封账完成评审”真实可点：对底部操作栏多点真实点击后 F5 封账落账
   （1026 条决定一次性写回），呈现 sealed + 导出摘要。

修复前红态：同一契约断言在无滚动容器布局下失败（`viewport-height=0 >
visible-height=0`、滚轮/键盘 viewport-y 恒 0、封账按钮点击无效）。
修复后绿态：`s1_22` 通过（27s）。

### 验收点证据

- 大量候选（1026）列表可滚动、操作栏可见可点：截图
  `docs/developer-guide/m5-e2e/evidence/t33-review-scroll/
  review-1026-scrollbar-actionbar.png`（1000×750 物理像素，右侧滚动条 +
  底部暂停/恢复/封账按钮可见）+ `s1_22` 可点性断言（真实点击封账成功）。
- 滚轮与键盘滚动均有效、分类切换后行为一致：`s1_22` 契约断言。
- 回归全绿：`cargo test -p desktop-shell --test s1_16_review_flow --test
  s1_17_review_batch_and_seal --test s1_18_review_empty_and_failure --test
  s1_22_review_scroll_contract`（4 个测试文件全 ok）。
- 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：
  `cargo machete` ✓ / `cargo test --workspace`（110 个结果行全 ok）✓ /
  `cargo fmt --all --check` ✓ / `cargo clippy --workspace --all-targets --
  -D warnings` ✓ / `cargo deny check advisories bans licenses sources` ✓ /
  `cargo xtask ci`（tidy + arch）✓ / `cargo xtask timings`（120s 预算内）✓。
- 便携包已按修复后 HEAD 重建（`dist/MCRebuild-V2.0.0-dev-portable.zip`）。

### 明确未做

评审/封账业务逻辑、F5 核心、产品基线均未改动；未开 GitHub Issues。

---

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

## T35 走查记录（2026-08-09，实施窗口 fix/t35-boundary-confirm-crash）

工单：`.scratch/v2-implementation/issues/T35-boundary-confirm-crash-fix.md`
（P0：真实 OSM 边界确认失败时程序直接退出、无任何反馈；对应 T19B-5B
破坏性验收项“画自相交多边形 → 弹窗报错而非崩溃”，此前为崩溃）。
最终二进制：`dist/MCRebuild-V2.0.0-dev-portable.zip`（7.95 MB），
`campus-rebuild-dev.exe` SHA256 `120C99857665465D276BD6BF7549D077D3BBBFD35FBACB60EE2202EC2526F8B4`。
走查实例：`dist/t35-walkthrough/`（新 exe + dev DB 副本，DB 未经改动）。

### 根因（实测定位，2026-08-09）

1. **RefCell already borrowed panic（WER 0xc0000409 主因）**：
   `apps/desktop/src/map_webview.rs` wry IPC 回调闭包
   `if let Some(handler) = s.borrow().ipc_handler.clone() { handler(&body); }`
   的临时 `Ref` 活到 if-let 体结束，业务 handler 执行期间 `STATE` 一直被
   借用；错误弹窗路径 `hide()` 对 `STATE` 做 `borrow_mut` → panic。panic 在
   WebView2 COM 回调栈内不可 unwind → 进程 abort（BEX64 / 0xc0000409）。
   debug 复现 stderr：`thread 'main' panicked at
   apps\desktop\src\map_webview.rs:490:27: RefCell already borrowed` +
   `thread caused non-unwinding panic. aborting.`（Windows 事件日志
   Application Error：0xc0000409，faulting module campus-rebuild-dev.exe，
   时间与蝴蝶结确认 IPC 一致）。
2. **同步 drop WebView 的 COM 重入（WER 0xc0000005 combase.dll）**：
   即使借用不冲突，WebView2 IPC 回调栈内同步 drop 也会 COM 重入；wry 0.55
   无 `set_visible`，`hide()` 是 drop/recreate 兜底。

### 修复（仅呈现层，ADR-0037；业务规则不变）

- `map_webview.rs`：IPC 回调先克隆 handler 释放 `STATE` 借用再执行业务
  （新增 `dispatch_ipc`，3 处 `with_ipc_handler` 与 `notify_status` 统一）；
  `hide()` 成为**唯一延迟销毁入口**：活跃 WebView 立即移入 `retiring`
  （`is_visible()` 即刻为 false），经 `slint::invoke_from_event_loop` 排定
  下一拍回调，IPC 回调返回后才真正 drop；事件循环不可用（无界面/单测）时
  fallback 立即销毁；同轮多次 hide 只排一次。
- `presenter.rs`（ShellPresenter::present）与 `presentation.rs`
  （render_presentation 两处）的弹窗 hide() 调用点注释标明统一走延迟入口，
  行为不变。
- 校验失败仍经 B7 error 通知 + 错误弹窗 + 公告栏留底（ADR-0021），不静默
  吞错；T33 `normalize_closed_ring` + validation 共享端点跳过保留未动。
- `main.rs` 新增 std-only 文件日志（`MCREBUILD_LOG_FILE` 环境变量启用，
  log::* 现可落盘），用于验收 3 的 drop 顺序证据；零新依赖。

### 自动化回归（去 zz_ 前缀，s1_ 契约风格）

- `apps/desktop/tests/s1_24_boundary_confirm_error_contract.rs`：自相交
  蝴蝶结 → 错误弹窗可见、`边界面积过小…; 边界自相交：第 0 条边与第 2 条边
  相交`、边界未确认、操作状态 Failed、B7 留底；点“知道了”后程序存活、
  边界仍未确认；同一会话内有效环确认成功、五步解锁。
- `apps/desktop/tests/s1_25_osm_boundary_confirm_canned_contract.rs`：
  真实 OSM 环确认成功（罐头 Overpass/Nominatim，无网络）——华东师大普陀
  relation 6179557（90 点、第 87 点中途闭合 + 2 尾点，T33 场景）、上交闵行
  way 288249651（39 点），断言 `normalize_closed_ring` 截断尾点后确认成功。
- `map_webview::tests::ipc_dispatch_releases_borrow_before_invoking_handler`：
  RefCell 借用根因回归（handler 内调用 hide() 不得 panic）；另有
  `hide_without_webview_keeps_state_clean_in_headless_environment`（无事件
  循环环境不误排定）。
- 删除 `zz_t33_diag_boundary_confirm.rs` / `zz_t33_diag_osm_boundary.rs`
  （临时诊断已由 s1_24 / s1_25 正式固化）。

### 真机走查（真实 WebView2 + 真实 OSM/Nominatim/Overpass/高德，100% DPI）

驱动：`scripts/t35-walkthrough-drive.py`（pywinauto UIA + WebView2 CDP
`--remote-debugging-port=9222 --remote-allow-origins=*`）；截图见
`docs/developer-guide/m5-e2e/evidence/t35/`。

1. **华东师大普陀（真实 OSM）**：dev DB 已有校区
   “华东师范大学普陀校区”（中山北路 3663 号）与方案“新方案 1”。打开方案 →
   地图页“来自 OSM: 华东师范大学 ✓”自动绘制（高德瓦片 + OSM 署名可见）→
   抽屉“当前点数：87” → 点“确认边界” → 状态“边界已确认，可点'重置'
   重新绘制”（`t35-A-confirmed-095258.png`）→ 程序不退出。
2. **上交闵行（真实 OSM）**：切换校区 → 高德在线搜索“上海交通大学”→
   真实结果列表（闵行本部/徐汇/长宁/七宝/医学院等，
   `t35-B-search-results-095306.png`）→ 点选“上海交通大学(闵行本部校区)”
   → 确认添加 → 新建方案“T35走查方案” → 打开 → OSM amenity 级联自动选中
   并绘制（“当前点数：87”，真实 Overpass amenity 查询命中）→ 点“确认边界”
   → “边界已确认”（`t35-B-confirmed-095322.png`）→ 程序不退出。
3. **自相交蝴蝶结（对应 T19B-5B 破坏性验收项，修复前为崩溃）**：CDP 在真实
   边界页执行 `enableManualMode()` + 4 个 `handleMapClick`（蝴蝶结坐标）+
   `submitBoundaryFromDrawer()` → 真实 wry IPC 回调 → B5 校验失败 → 错误弹窗
   可见：`错误 / 应用 / 边界面积过小：0.0 平方米…; 边界自相交：第 0 条边与
   第 2 条边相交` + “知道了”按钮（`t35-C-error-modal-095329.png`）→ 点
   “知道了” → 程序存活（`t35-C-after-dismiss-095331.png`）、边界保持未确认
   → CDP 画有效矩形再确认 → “边界已确认”（`t35-C-recovered-095338.png`），
   会话内恢复能力成立。
4. **error 通知在地图可见时发布（验收 3）**：蝴蝶结确认即为地图可见时的
   error 通知（B7 弹窗 + 留底），弹窗正常显示、程序不退出；日志
   `dist/t35-walkthrough/t35-walkthrough.log` 证明 drop 顺序：

   ```text
   WebView2 IPC 回调进入（body=104 字节）
   hide() 已排定下一拍销毁 1 个 WebView（IPC 回调返回后 drop）
   WebView2 IPC 回调返回
   事件循环下一拍，销毁 1 个待退休 WebView（IPC 回调已返回）
   ```

   即 WebView 实际 drop 发生在 IPC 回调返回之后的事件循环下一拍。

### 125% DPI（待会话刷新补充）

本机当前 100% DPI（96）。Windows 11（build 26200）更改显示缩放需登出/登入
才生效（已实测：注册表 `LogPixels=120` + `Win8DpiScaling=1` +
`UpdatePerUserSystemParameters` + 重启 Explorer 后 `GetDpiForSystem()` 仍为
96；随后已恢复原值）。125% 走查步骤（验收窗口执行，完成后请恢复 100%）：

```powershell
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name LogPixels -Value 120 -Type DWord
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Win8DpiScaling -Value 1 -Type DWord
# 登出 → 登入 → 运行 scripts\t35-dpi-walkthrough.py（华东师大普陀方案确认 + UIA 矩形 + 截图）
# 完成后恢复：
Remove-ItemProperty 'HKCU:\Control Panel\Desktop' -Name LogPixels
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Win8DpiScaling -Value 0 -Type DWord
# 再登出/登入一次
```

125% 布局正确性已有自动化覆盖：`map_webview` 单测在 scale 1.25 下断言物理
右缘不越界（T32/T34 回归），`s1_23` 断言 800×666 / 1000×666 抽屉让位。

### 门禁与便携包

- 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：
  `cargo machete` ✓ / `cargo test --workspace`（113 结果行全 ok）✓ /
  `cargo fmt --all --check` ✓ / `cargo clippy --workspace --all-targets --
  -D warnings` ✓ / `cargo deny check advisories bans licenses sources` ✓ /
  `cargo xtask tidy` ✓ / `cargo xtask arch` ✓（`xtask ci` 同为绿）。
- 便携包重建 `dist/MCRebuild-V2.0.0-dev-portable.zip`（7.95 MB）；
  `C:\Users\chang\AppData\Local\MCRebuildV2\dev\campus-rebuild-dev.exe`
  已替换为最终二进制（旧版备份至 `previous/campus-rebuild-dev.exe.
  20260809-pre-t35.exe`；dev DB 与 WebView2 数据未动），启动验证通过。
  三处 exe（staging / dev / 走查实例）SHA256 一致。

### 剩余风险与交接

- 仍有其他校验失败入口可能走到同一隐藏弹窗路径（如朝向两点重合、校区搜索
  失败重试、评审/导出失败等，地图可见时弹窗都会触发 `hide()`）；本修复将
  `hide()` 统一收口为延迟销毁入口 + 修复 IPC 借用，覆盖全部入口，但未逐条
  人工复核，建议验收窗口抽查 1–2 条（如朝向页两点重合弹窗）。
- 125% DPI 窗口走查待 Windows 会话刷新后执行（步骤见上）。
- T19B-5B 破坏性验收项需负责人勾掉并注明此前为崩溃。
- 若现场发现其他校区边界仍校验失败，作为新缺陷单独开单（先证据后修）。

### 跟进（2026-08-09 上午，负责人反馈“用新版本无法从 OSM 获取边界”）

排查结论（含复现证据，`diagnosing-bugs` 流程）：

1. **最终二进制未破坏 OSM 获取**：干净单实例下在 dev 安装与桌面安装各复验
   一次，OSM 自动获取均成功（`evidence/t35/t35-osm-fetch-check-111134.png`、
   `t35-osm-fetch-check-111225.png`，抽屉点数 > 0，无崩溃）。本修复只涉及
   `hide()`/IPC 回调（弹窗路径），不触碰 OSM 获取链路
   （workspace_adapter `start_boundary_fetch`/`poll`/`apply`）。
2. **桌面安装“取不到 OSM 边界”的根因是全新空库**：刚解压的桌面便携包
   `first_run_completed=false`、**无高德密钥**、无校区/方案——在线功能
   （校区搜索/地图页/OSM 自动获取）必须先经设置页录入密钥（T22/README
   既有设计），不是 T35 引入。已把 dev 的 `campus-rebuild.db`（含密钥与
   华东师大普陀校区）复制到桌面安装使其直接可用，并复验 OSM 成功。
3. **dev 安装 11:04 有一次 combase.dll 0xc0000005 崩溃**（事件日志证据；
   WER 临时 dump 已被系统清理，无法取栈、无法确定性复现）。崩溃发生在
   旧二进制遗留的 WebView2 缓存 + 可能的多实例环境下；已将旧 WebView2
   缓存目录改名（`campus-rebuild-dev.exe.WebView2.t35-stale-20260809`，
   可再生缓存）后单实例复验，OSM 正常、无崩溃。运维注意项：**升级/替换
   二进制后，若旧版本崩溃过，先清理该 exe 旁的 `*.WebView2` 缓存目录再
   启动**；复现需提供崩溃时间、是否多开、以及 `MCREBUILD_LOG_FILE` 日志。

负责人签名：____________

---

## T34 实施窗口记录（2026-08-08，fix/t34-map-first-workspace-layout）

工单：五步工作区"地图为主 + 左侧抽屉"布局改造（做法 A：地图让位）。
便携包 `dist/MCRebuild-V2.0.0-dev-portable.zip` 已按 HEAD bee5bdb 重建
（7.92 MB）。本段为实施窗口记录；验收窗口复核与真实密钥走查另行补充。

### 布局与矩形证据（验收标准 1）

- 步骤 ①②③⑤ = 顶部五步条 + 地图主画面 + 左侧可收拉抽屉（做法 A）；
  步骤 ④ 评审保持现状整页，不并入抽屉、不改滚动/封账逻辑。
- 地图矩形不再硬编码 (32,184,w-32,340)：改由 Slint 布局槽位
  `workspace-map-slot-*` 上报（逻辑像素），`map_webview` 300ms 轮询
  `set_bounds` 跟随；抽屉开合改变槽位 → WebView 内 `map.resize` 同步。
- 截图与 UIA 矩形见 `evidence/t34/t34-drawer-rects.txt` 与
  `evidence/t34/01..11-*.png`：800×600 逻辑 @125% DPI 下实测
  箭头条 (0..20)、抽屉 (20..320)、地图槽位（收起 x=20 / 展开 x=332）、
  五步条 y 64..128 互不相交；契约测试 `s1_23` 覆盖 800×666 与
  1000×666 的让位关系与 125% 物理右缘不越界。

### 逐条验收对照（实施窗口证据）

1. **抽屉开合 → 地图让位/恢复**：`s1_23` 断言展开槽位 x=20+300+12、
   宽度 = 收起宽−312、收回恢复；无闪烁/遮挡由 WebView 常驻 + 布局让位
   保证（截图 02/03）。
2. **圈边界经抽屉完成**：地图兜底画布点画 → 抽屉显示"当前点数：4 /
   已添加 4 个点"→ 撤销/重置/确认经抽屉按钮完成（截图 03/05）；
   地图 HTML 工具栏按钮已删除（gaode-client 单测断言无 map-toolbar /
   confirm-edit-btn / add-area-btn 等残留）。
3. **定朝向**：地图点两点 → 角度/罗盘反馈到抽屉（截图 06）；手动输角度
   覆盖已有朝向 → F5 重算确认弹窗（截图 07），确认后新角度生效
   （截图 08，270.0°）；页内无模式切换、无重叠元素。
4. **采集/导出**：抽屉 ③ 显示来源/类别/可跳过（截图 09）；抽屉 ⑤ 显示
   导出开始/结果/错误状态位（截图 10）。
5. **弹窗遮挡统一**：错误/确认/输入弹窗前隐藏地图、关闭后按当前步骤
   模式（边界页 vs 朝向页）恢复（`map_webview::restore_after_modal` +
   步骤守卫，单元测试覆盖守卫与种类书签）；错误弹窗在地图步骤可见可点
   （截图 04），重算确认弹窗在朝向步骤可见可点（截图 07）。
6. **800×666 / 1000×666 / 125% DPI**：`s1_23` 以 800×666 与 1000×666
   逻辑断言让位；`map_webview` 单测断言 125% 下物理右缘 ≤ 窗口物理宽；
   步骤 ④ 评审页与现状一致（截图 11）。
7. **全部门禁全绿**（Windows，`SLINT_BACKEND=software`、
   `CARGO_BUILD_JOBS=2`）：`cargo machete` ✓ / `cargo test --workspace`
   ✓ / `cargo fmt --all --check` ✓ / `cargo clippy --workspace
   --all-targets -- -D warnings` ✓ / `cargo deny check advisories bans
   licenses sources` ✓ / `cargo xtask ci` ✓ / `cargo xtask timings` ✓。
   定向契约：新增 `s1_23_workspace_drawer_contract`；存量
   `s1_05/s1_06/s1_15/s1_16/s1_17/s1_18/s1_22` 等全绿；`s1_15`
   改写为驱动抽屉桥接命令（多区域 UI 随工具栏删除，MultiPolygon seam
   由 `s1_12` 覆盖）。
8. **便携包**：`scripts/build-release.ps1` 重建成功，解压启动验证通过
   （种子库落地方案列表 → 打开方案 → 工作区布局可见可点）。

### 本工单明确不做

- 步骤 ④ 评审并入新布局（另行决策）。
- 做法 B（抽屉覆盖在地图上）留作后续版本方向。
- 地图内多区域（添加区域）UI 随 HTML 工具栏删除；`confirm_boundary`
  MultiPolygon seam（IPC）保留（`s1_12`/`s1_15` 覆盖），UI 入口不在本
  工单抽屉清单内。
- 不改变业务规则：边界唯一必填（ADR-0041）、朝向可选默认正北、采集/
  评审可跳过、导出资格不变；S1 只呈现与转发（ADR-0037）。

### 待验收窗口补充

- 已授权高德密钥下真实地图瓦片 + WebView 矩形 UIA 断言（本证据为无密钥
  兜底画布路径，矩形数值相同）。
- 800×666 / 1000×666 两档真实截图（实施窗口为 800×600 @125%）。

## T31 走查记录（2026-08-07，验收窗口）

验收窗口按 `script-a-basic-export.md` / `script-b-enhanced-export.md`，用合并后
便携包（`dist/MCRebuild-V2.0.0-dev-portable.zip`，解压后
`campus-rebuild-dev.exe` 便携模式）执行真实 GUI 走查。本段为验收窗口独立
记录；实现窗口 T31 修复记录见上文。

### 环境与前置

- 便携包：`dist/MCRebuild-V2.0.0-dev-portable.zip`（8,288,325 B，构建时间
  2026-08-07 09:42，HEAD 7d0659e；252ff25 仅更新验收记录文档，不影响二进制）。
- 解压目录：`dist/t31-walkthrough/`，DB 落在解压目录内
  （`campus-rebuild.db`）。
- 首次向导：语言 zh-CN、Minecraft 26.1.2、勾选“请确认…一致”（截图
  `evidence/script-a/00-wizard-t31walk.png`）。
- 设置页录入高德 Web 端(JS API) Key 与安全密钥：只经设置页输入，凭据沿用
  已提供值（`evidence/script-a/05-settings-apikey-t31walk.png`、
  `06-settings-seckey-t31walk.png`）；本记录不写明文。保存后 DB
  `app_settings` 中 `gaode_api_key` / `gaode_security_key` 均为 32 位十六进制
  且与已提供凭据一致（读取校验，未落盘明文）。

### 剧本 A：基础导出（走查结果）

| 验收点 | 证据 | 结果 |
|--------|------|------|
| 首次设置 → 校区 → 方案 | 首次向导完成；校区搜索输入“上海交通大学”→ 高德在线搜索返回真实校区列表（闵行本部校区/徐汇/长宁/闵行人文/七宝/医学院黄浦，`evidence/script-a/11-campus-search-results-t31walk.png`）；点选并确认“上海交通大学(闵行本部校区)”（POI B00155R1D5，东川路800号，锚点 121.436882/31.025626；`12-campus-selected-t31walk.png`、`13-campus-confirmed-t31walk.png`）→ 直接进入方案列表（`14-plan-list-t31walk.png`） | ☑ |
| 新建方案 | 新建方案对话框输入“M5剧本A走查方案”→ 确认 → 方案卡片“新方案 1 M5剧本A走查方案 / 尚未确定范围”（`15-create-plan-t31walk.png`、`16-create-plan-dialog-t31walk.png`） | ☑ |
| 边界步骤：OSM 自动获取（验收点） | 打开方案 → 边界地图 WebView 显示“来自 OSM: 上海交通大学（闵行校区）✓”，自动绘制边界；页面含高德瓦片署名（© 2026 AutoNavi - GS(2025)5996号）与 OSM 署名（© OpenStreetMap contributors）（`20-osm-boundary-t31walk.png`） | ☑（自动获取成功） |
| 确认边界 | **未通过（缺陷 T31-D6）**：WebView 内“确认边界”按钮（UIA `confirm-edit-btn`，坐标 (860, 521)）位于 WebView 视口右缘之外约 60px（窗口逻辑 800×666），物理与逻辑坐标均超出窗口，按钮不可见不可点；“改人工圈画”按钮同理（(806, 529)）。自动获取成功但无法确认边界 → 后续导出步骤被阻塞。Slint 层“确认边界”按钮保持 disabled（边界未确认时设计如此） | ☒（缺陷 T31-D6） |
| `.schem` + manifest（exportKind=base / map_north / attribution） | 因“确认边界”被 T31-D6 阻塞，本次 GUI 走查未产出导出物；导出链路本身由 `s1_08_boundary_export_flow` 集成测试覆盖（T30 已人工跑通：453 B .schem、`exportKind=base`、`orientation.source=map_north`） | ☒（被 T31-D6 阻塞） |

### 剧本 B：增强导出（走查结果）

剧本 B 依赖剧本 A 完成边界确认。因 T31-D6 阻塞，采集/评审/封账/增强导出
步骤在本次 GUI 走查中不可达，未执行；不产生任何伪成功产物（界面停留在边界
步骤，无导出物、无错误弹窗之外的异常状态）。

| 验收点 | 结果 |
|--------|------|
| 采集报告计数与来源标注 | 未执行（被 T31-D6 阻塞） |
| 评审保留/剔除/封账 | 未执行（同上） |
| 增强导出 `.schem` + manifest（exportKind=enhanced / keepByCategory） | 未执行（同上） |

### 缺陷清单（验收窗口发现，未修复）

- **T31-D6（P1，阻塞剧本 A/B 完成）**：边界地图 WebView 内容横向溢出，
  “确认边界”/“改人工圈画”按钮渲染在 WebView 视口右缘之外（UIA 坐标
  (860, 521) / (806, 529)，窗口逻辑宽 800；物理像素同样超出窗口右缘），
  按钮不可见、不可点击。OSM 自动获取边界成功（状态面板
  “来自 OSM: 上海交通大学（闵行校区）✓”），但无法确认边界，剧本 A 的
  导出与剧本 B 的全部后续步骤被阻塞。疑似与 T30 D-5（WebView 尺寸/布局）
  同根因，建议实施窗口核对 `map_webview::compute_bounds` 与
  `boundary_edit.slint` 画布几何（WebView 物理宽度 vs Slint 画布宽度、AMap
  容器初始化宽度），修复后以 HEAD 重建便携包重跑剧本 A/B。

### 结论（T31 走查窗口）

- ☐ 剧本 A/B 通过，v2.0.0 进入正式版候选（待 T31-D6 修复后重跑）。
- ☑ 真实 GUI 走查已完成：首次向导、设置密钥、高德在线校区搜索与点选、
  方案创建、OSM 边界自动获取（来源标注、自动绘制）均真实走通；剧本 A 在
  “确认边界”步骤被 T31-D6 阻塞，剧本 B 因此不可执行。
- 密钥合规：只经设置页录入，未写入任何仓库文件/日志；DB 内为设置页保存值。
- 待负责人确认 T31-D6 修复策略与重跑安排后，再行剧本 A/B 完成确认。

负责人签名：____________

---

## T32 走查记录（2026-08-07，实施窗口 fix/t32-boundary-page-overflow）

实施窗口按工单 T32 修复边界地图页按钮横向溢出（T31-D6），并用修复版
`campus-rebuild-dev.exe`（`dist/t32-diag/`，与重建后便携包同 HEAD）重跑剧本
A/B。本段为实施窗口的验收证据；**负责人签名栏在段末**。

### 缺陷事实与根因（实测数据）

修复前（T31 走查实测）：窗口逻辑 800×666，“确认边界”UIA (860, 521)、“改人工
圈画”(806, 529) 超出窗口逻辑宽 800 右缘，物理坐标同样越界，按钮不可见不可点。
`compute_bounds` 期望视口 x:32、宽 window−32（右缘 800），按钮却在内容层更宽
位置，属内容横向溢出。

根因（T32 实测修复）：

1. `apps/desktop/src/map_webview.rs`：slint `Window::size()` 返回**物理宽**，
   原实现把物理宽当逻辑宽再乘 scale，导致 WebView 物理宽 = (1000−32)×1.25 =
   1210，超出窗口物理宽 1000。新增 `logical_window_width()`（物理 ÷ scale），
   4 处调用点统一改用逻辑宽，`compute_bounds` 物理尺寸不再越界。
2. `core/gaode-client/src/boundary_edit_map_page.rs`：`html/body overflow-x:
   hidden`；`#map-container` `max-width:100%` + `box-sizing`；AMap 初始化前把
   容器宽度钳制到视口并 `map.resize()`；`resize` 监听同步容器；地图初始化延后
   到 `window load`（避免 AMap 在未布局容器上创建过宽画布）。
3. `apps/desktop/src/production/workspace_adapter.rs`：步骤 ≥3（采集/评审/导出）
   时 `map_webview::hide()`，避免地图子窗口覆盖 Slint 操作区导致按钮不可点。

### 按钮可见/可点断言（验收点 1）

修复版实测（DPI 120 = 125%，UIA 物理像素；截图
`evidence/t32/c01~c04-*.png`，断言原文 `evidence/t32/t32-button-rects.txt`）：

| 窗口（逻辑） | WebView 容器 | 确认边界（confirm-edit-btn） | 改人工圈画 | 可点性 |
|--------------|--------------|------------------------------|------------|--------|
| 800×666（客户区 1000×833 物理） | (109,328)-(1069,753)，右缘=窗口右缘 | (849,702)-(940,741)，相对右缘 664.8 < 800 | (949,702)-(1057,741)，相对右缘 758.4 < 800 | 点击切换“人工圈画模式” ✓ |
| 1000×666（客户区 1250×833 物理） | (89,308)-(1299,733)，右缘=窗口右缘 | (1079,682)-(1170,721)，相对右缘 864.8 < 1000 | (1179,682)-(1287,721)，相对右缘 958.4 < 1000 | 点击切换“人工圈画模式” ✓ |

两张尺寸下页面均显示“来自 OSM: 上海交通大学（闵行校区）✓”、高德署名
“© 2026 AutoNavi - GS(2025)5996号”与 OSM 署名“© OpenStreetMap contributors”，
OSM 署名在 WebView 内可见。一次真实观察：1000 逻辑宽首次打开时 OSM 自动获取
失败并正确回退“人工圈画模式”（非静默成功，符合剧本 A 验收点），重开方案后
自动获取成功。

### 剧本 A：基础导出（T32 修复后重跑）

| 验收点 | 证据 | 结果 |
|--------|------|------|
| 校区→方案→OSM 边界自动获取 | 高德在线搜索“上海交通大学”点选“闵行本部校区”（POI B00155R1D5）→ 新建方案（`evidence/t32/10-campus-search.png`、`14-plan-list.png`、`17-plan-card.png`）；边界页自动获取“来自 OSM: 上海交通大学（闵行校区）✓”并自动绘制（`20-osm-boundary.png`） | ☑ |
| 确认边界 | 修复后“确认边界”按钮在视口内可点，点击后步骤 1 显示 ✓ 与“边界已确认，可点'重置'重新绘制”（`21-boundary-confirmed.png`） | ☑ |
| 基础导出 `.schem` + manifest | 新建方案（planId `3a8baae2-8cf2-4e6a-b88c-3a2a49579eed`）导出：`.schem` 5,341 B；manifest `exportKind="base"`、`orientation.source="map_north"`、`attribution="© OpenStreetMap contributors"`、`candidateFacts` 全 0（`evidence/t32/t32-manifest-base.json`、`30-export-page.png`、`31-export-done.png`） | ☑ |

### 剧本 B：增强导出（T32 修复后走通）

| 验收点 | 证据 | 结果 |
|--------|------|------|
| 采集：真实 OSM 建筑候选 | 步骤 3 点“采集”→ 真实 Overpass 拉取：**原始 1037 项（建筑 761 / 其他 276）**，**可评审 1026**、**已隔离 11**、**自动修复 1026**，来源标注 OSM（页面标题“OSM（OpenStreetMap）”；`evidence/t32/b00-collect-page.png`、`b01-collect-report.png`；DB 终态 `t32-db-final-state.txt`） | ☑ |
| 评审保留/封账 | 建筑分类保留 5 项（way/283411238、way/545419223、way/288254676、实验C楼、way/283411263）→ 封账 → 摘要“保留 5 项，待定 1021 项，剔除 0 项 / 建筑 5”（`b02-review-page.png`、`b03-review-sealed.png`） | ☑ |
| 增强导出 `.schem` + manifest | 导出页显示“增强导出：基础场地 + 保留候选 5 项（建筑 5）”（`b04-export-enhanced-page.png`）→ 导出完成（`b05-export-enhanced-done.png`）：`.schem` **109,263 B（尺寸 3461×14×1988，含候选内容，远大于基础 5,341 B）**；manifest `exportKind="enhanced"`、`orientation.source="map_north"`、`keepByCategory=[{"category":"Building","count":5}]` 与保留一致、`attribution="© OpenStreetMap contributors"`（`evidence/t32/t32-manifest-enhanced.json`） | ☑ |

DB 评审终态（`dist/t32-diag/campus-rebuild.db`，封账后不可再改）：Building
keep 5 / pending 745；Other pending 276；合计 1026 = 可评审数。密钥只经设置页
录入，仓库内无任何含密钥文件（`git status/diff` 核验）。

### 门禁与便携包

- 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：
  `cargo fmt --all --check`、`cargo machete`、`cargo clippy --workspace
  --all-targets -- -D warnings`、`cargo test --workspace`（109 个 test result 全
  ok）、`cargo deny check advisories/bans/licenses/sources`、`cargo xtask ci`
  （tidy + arch）、`cargo xtask timings`。
- 便携包重建：`scripts/build-release.ps1` → `dist/MCRebuild-V2.0.0-dev-portable.zip`
  （7.91 MB，HEAD 即本分支提交）。

### 新发现缺陷（如实记录，未擅自修复）

- **T32-D2（评审页可操作性，P2，不属本工单范围）**：评审工作台候选列表无滚动
  容器（`review.slint` 直接 `for card in root.cards` 纵向排布），1026 个候选时
  “暂停/继续/封账完成评审”操作栏渲染在窗口视口之外且滚轮/键盘均无法到达；
  本走查通过切换到空分类“道路 (0)”使操作栏回到视口内完成封账。小候选集不受
  影响。建议后续工单给评审列表加滚动/分页，不在本工单修改。
- 备注：剧本 B 增强导出期间未出现错误弹窗或伪成功产物；导出消息与落盘文件
  一致。

### 结论（T32 实施窗口）

- ☑ 边界页按钮 800/1000 逻辑宽 × 125% 缩放下可见可点（截图 + 坐标/可点性断言）。
- ☑ 剧本 A 走通：OSM 边界自动获取 → 确认 → 基础导出（`exportKind=base`、
  `map_north`、`attribution`）。
- ☑ 剧本 B 走通：真实 OSM 建筑候选（可评审 1026 > 0）→ 评审/封账 → 增强导出
  （`exportKind=enhanced`、`keepByCategory=Building 5`、`.schem` 含候选内容）。
- ☑ 全部门禁全绿；便携包已按 HEAD 重建。
- 遗留：T32-D2 评审页滚动问题待负责人决策（另行工单）；负责人签名后 v2.0.0
  进入正式版候选。

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

---

## T31 修复后重跑记录（2026-08-07）

修复分支 `fix/t31-real-outline-boundary-sources`（基于 origin/main @ a929a5e，
T30 合入后）。便携包重建：`dist/MCRebuild-V2.0.0-dev-portable.zip`（7.9 MB，
构建时间 2026-08-07 09:42，HEAD 7d0659e）。高德 Web 端(JS API) 密钥沿用设置页
录入值（记录不写明文）。**增强导出与 regeo 补名的在线人工环节仍需负责人现场
操作（见“未能验证项”）**。

### 候选数据源口径（负责人 2026-08-07 明确）

- 高德只当地图底图 / 校区身份 / 坐标转换 / 命名（regeo）；候选几何与边界一律
  来自 OSM（实时 Overpass）；Overture 留作离线补充包（本工单不实现）。
- 生产采集源已从 `GaodeDataSource`（高德 PlaceSearch POI 点位）撤下：
  `apps/desktop/src/runtime.rs` 现注入 `OverpassDataSource`（union `building=*`、
  de→kumi→mail.ru 回退、每端点 12s 超时、结构化 `SourceUnreachable`）；
  WebView 采集脚本与 `collection_response` 通道已删除。

### 三硬伤修复证据（修复前后实测，见 `docs/research/t31-overpass-hard-defects-evidence.md`）

1. URL 缺 `data=`：修复前 `interpreter?<query>` 返回
   `parse error: Unknown type "%"`（存档 `t31-overpass-evidence/before-missing-data-param.html`）；
   修复后一律 `interpreter?data=<编码查询>`。
2. `amenity~"university|college|school"` 的 `|` 正则：版本相关（调研当日 de 0.7.62.11
   拒绝；复查时端点已接受编码形式）→ 一律 union 写法，三端点验证可用。
3. WebView CORS：de/kumi 曾无 ACAO、端点策略会变 → 边界与候选查询全部 Rust 侧直连
   （ureq + native-tls），JS 不再 fetch Overpass（`boundary_edit_map_page.rs` 无
   `fetchOverpassBoundary`/`AbortController`/端点字符串）。

### 真实校区边界自动获取（验收点 1）——实测通过

真实链路冒烟（`data-acquisition` `#[ignore]` 测试，2026-08-07 上海网络）：

```text
fetch_campus("上海交通大学(闵行本部校区)", 121.433, 31.028)
→ AutoSelected name=上海交通大学（闵行校区）
  source=Overpass amenity=university|college|school
  candidates=9 points=39（GCJ-02，闭环）
```

级联行为：高德校区名带“(闵行本部校区)”后缀 → Nominatim 精确/去括号均无
`class=amenity` 命中 → 自动回退 Overpass `amenity=university` 锚点近域查询
（ADR-0029 主路径）→ 按“锚点包含 → 名称匹配 → 距离最近”排序自动选中；
`landuse=education` 与人工圈画为后续兜底。徐汇校区经 Nominatim 可解析到
way/144183801（按 ID 拉取路径，实测 39 点闭合）。

### 候选采集真实轮廓（验收点 3）——实测通过

真实采集链路冒烟（`OverpassDataSource` + 生产 transport，2026-08-07）：

```text
boundary bbox(31.02,121.41,31.04,121.46) → union building=* → 590 元素
→ 面候选 > 0、带 OSM name > 0、WGS-84→GCJ-02 已在入口转换（首点偏移 >50m 断言）
```

- 点位不扩面（ADR-0040 红线）：`source.rs`
  `overpass_node_stays_a_point_never_expanded_to_polygon` 断言 node 保持
  `Point`；regeo/补名器只作用于 Polygon。
- 采集报告来源标签：`collection.source_osm`（“OSM（OpenStreetMap）”）。

### 坐标转换与命名（验收点 7）——代码 + 单测

- `gaode-client/src/coords.rs`：WGS-84→GCJ-02 开源批量转换（~1m 精度，不做反向）；
  `OverpassDataSource` 采集入口就地转换并保留原始 WGS-84 载荷（`source_payload`
  断言）。
- 命名两级：OSM `name` 优先（`RawEntity::name`）；缺名关键建筑由
  `data-acquisition::regeo::RegeoNamer` 补名 + 会话缓存（同坐标只调一次，测试断言）。
  regeo Web 服务 Key 只经设置页录入（新增 `GaodeWebServiceKey` 设置项，ADR-0004
  “开发人员使用”）。
- 署名：边界地图页保留 `© OpenStreetMap contributors`（ODbL）。
- 导出物署名：`foundation_manifest.json` 新增 `attribution` 字段
  （“© OpenStreetMap contributors”，`manifest-generator/src/manifest.rs`）。

### 剧本 A/B 自动化等价证据与门禁（验收点 4/5/6）

- 剧本 A 基础导出：`s1_08_boundary_export_flow` 真实写出 `.schem` +
  `foundation_manifest.json`（`exportKind=base`、`orientation.source=map_north`）；
  T30 已人工跑通。
- 剧本 B 增强导出管线：`s1_19_enhanced_export_flow` /
  `s1_20_enhanced_export_failure_flow`（Building 候选 → `exportKind=enhanced`、
  `keepByCategory` 与保留一致、.schem 含候选高度/方块计数；失败路径不伪造）。
  真实采集→评审→封账→增强导出的**在线人工环节**待负责人现场执行。
- 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：
  `cargo machete` ✓ / `cargo test --workspace` 573 通过 0 失败 ✓ /
  `cargo fmt --all --check` ✓ / `cargo clippy --workspace --all-targets -- -D warnings` ✓ /
  `cargo deny check advisories bans licenses sources` ✓ /
  `cargo xtask ci` ✓ / `cargo xtask timings`（120s 预算内）✓。

### 未能验证项（需负责人环境/密钥）

1. 剧本 A/B 的完整 GUI 人工走查（便携包已按 HEAD 重建；校区搜索/边界确认/
   采集/评审/封账/导出点击流需现场操作）。
2. regeo 真实补名调用：需要负责人在设置页录入高德 **Web 服务 Key**
   （与 JS API Key 不同）；未配置时缺名建筑保持“未命名建筑 #id”，不阻塞采集。
3. 增强导出在真实 OSM 候选（Building 类别）下的 `.schem` 内容核对（自动化等价
   `s1_19/20` 已覆盖管线；真实数据规模需现场确认）。

负责人签名：____________
