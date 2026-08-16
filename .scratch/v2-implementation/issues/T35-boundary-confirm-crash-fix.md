# T35 边界确认失败即崩溃修复（P0）

Status: completed（2026-08-17 发布收口）
Blocked by: 无（基于 fix/t33-review-list-scroll 分支；T33 的
`normalize_closed_ring` + 共享端点跳过修复必须保留，本工单不得回退）

## 缺陷事实（负责人视角）

真实 OSM 校区边界在地图上自动获取成功后，若“确认边界”校验失败（如自相交
蝴蝶结），程序会**直接崩溃退出**，没有任何错误提示——用户画错边界想看到
“哪里错了、怎么改”时，应用直接消失（WER 0xc0000005 combase.dll /
0xc0000409）。对应 T19B-5B 破坏性验收项“画自相交多边形 → 弹窗报错而非崩溃”。

## 根因（实施窗口 2026-08-09 实测定位，见 manual-acceptance-record T35 段）

1. **RefCell already borrowed panic（WER 0xc0000409 主因）**：
   `map_webview.rs` 的 wry IPC 回调闭包写作
   `if let Some(handler) = s.borrow().ipc_handler.clone() { handler(&body); }`，
   `s.borrow()` 的临时 `Ref` 会活到 if-let 体结束——`handler()` 执行业务期间
   `STATE` 一直被借用。错误弹窗路径（校验失败 → B7 error → `ShellPresenter`
   → `hide()`）会对 `STATE` 做 `borrow_mut` → `RefCell already borrowed`
   panic。panic 发生在 WebView2 COM 回调栈内、不可 unwind → 进程 abort
   （BEX64 / 0xc0000409）。实测 stderr：`thread 'main' panicked at
   apps\desktop\src\map_webview.rs:490:27: RefCell already borrowed`。
2. **同步 drop WebView 的 COM 重入（WER 0xc0000005 combase.dll）**：
   即使借用不冲突，在 WebView2 自身 IPC 回调栈内同步 drop WebView 也会触发
   COM 重入崩溃。wry 0.55 无 `set_visible`，`hide()` 是 drop/recreate 兜底。

## 范围（只改呈现层，不改业务规则）

1. `apps/desktop/src/map_webview.rs`：
   - IPC 回调先克隆 handler 释放 `STATE` 借用，再执行业务（`dispatch_ipc`）；
   - `hide()` 改为**唯一延迟销毁入口**：活跃 WebView 移入 `retiring`（逻辑
     隐藏立即生效），经 `slint::invoke_from_event_loop` 排定下一拍回调，
     IPC 回调返回后才真正 drop；
   - 事件循环不可用（无界面/单元测试）时 fallback 立即销毁。
2. `presenter.rs` / `presentation.rs` 的弹窗 hide() 调用点统一走该延迟入口
   （注释标明，行为不变）。
3. 校验失败仍必须经 B7 error 通知 + 错误弹窗（ADR-0021 弹窗铁律），只修
   “崩溃”，不修“报错”；不静默吞错。
4. T33 既有修复（overpass `normalize_closed_ring` + validation 共享端点跳过）
   原样保留。

## 验收标准（逐条证据，见 manual-acceptance-record T35 段）

1. 真实 OSM 边界（华东师大普陀、上交闵行各一条）→ 抽屉“确认边界”→
   “边界已确认，可点'重置'重新绘制”，程序不退出（真机 WebView2 走查，
   100% DPI 已通过；125% DPI 需 Windows 登出/登入后复核，见记录）。
2. 自相交蝴蝶结边界 → 错误弹窗可见可点、边界保持未确认、程序不退出。
3. 地图可见时发布 error 通知 → 弹窗正常显示、程序不退出；日志证明 WebView
   drop 发生在 IPC 回调返回之后（事件循环下一拍）。
4. 回归测试（去 zz_ 前缀，s1_ 契约风格）：
   - `s1_24_boundary_confirm_error_contract`：无效边界确认 → 错误弹窗可见 +
     不退出 + B7 留底 + 关闭后可恢复有效确认；
   - `s1_25_osm_boundary_confirm_canned_contract`：真实 OSM 环确认成功
     （罐头 Overpass/Nominatim 响应，华东师大普陀 90 点中途闭合 + 上交闵行
     39 点，去掉网络依赖）；
   - `map_webview::ipc_dispatch_releases_borrow_before_invoking_handler`：
     RefCell 借用根因回归（handler 内调用 hide() 不得 panic）。
5. 全部门禁全绿（Windows，`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`）：
   workspace tests / xtask tidy / xtask arch / clippy `-D warnings` /
   fmt --check / machete / deny。
6. 重建便携包并替换 `C:\Users\chang\AppData\Local\MCRebuildV2\dev` 下旧
   二进制（旧版备份至 `previous/`）。

## 分支/PR

- 分支 `fix/t35-boundary-confirm-crash`，基于 fix/t33-review-list-scroll
  （T33 未合入 origin/main，本分支基座含 T33 三个提交以保留其修复；
   T33 PR #21 先合后本 PR 可顺接）。
- 单一逻辑提交；draft PR 到 main；PR 描述写根因（RefCell 借用 panic +
   WebView2 回调栈内同步 drop 的 COM 重入）与修复方式。

## 交接（剩余风险）

- **仍有其他校验失败入口可能走到同一隐藏弹窗路径**（如朝向两点重合、校区
  搜索失败重试弹窗、评审/导出失败弹窗等，只要地图可见时弹窗都会触发
  `hide()`）。本修复把 `hide()` 统一收口为延迟销毁入口 + IPC 借用修复，
  覆盖全部入口；但未逐条人工复核其他入口的真机行为，建议后续验收窗口
  抽查 1–2 条（如朝向页两点重合弹窗）作为回归。
- 125% DPI 窗口走查需 Windows 会话刷新（登出/登入）后执行，步骤见
  manual-acceptance-record T35 段“待补充”。
- T19B-5B 破坏性验收项“画自相交多边形 → 弹窗报错而非崩溃”已在本工单
  真机复现并修复，需负责人勾掉该项并注明之前为崩溃。
- 若现场发现其他校区边界仍校验失败，作为新缺陷单独开单（先证据后修）。
