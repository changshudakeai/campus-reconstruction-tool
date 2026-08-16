# 交接：M2 可选候选采集闭环（A1 collection-flow）

Status: 已完成实现并提交 `m2-collection-flow` 分支（PR 待评审/合并）。
日期：2026-08-05。验收窗口：M2 实施窗口 → 验收窗口。

## 完成项

- 新建 `apps/collection-flow`（A1 应用流程模块）：外部接口只表达完整用户操作
  （开始采集、查看采集报告、取消/进度），接口后协调 F4 → B2 → B14 → F7，
  返回已决定的页面状态、进度与通知事实（ADR-0039/0040）。
- 数据链：F4 `AcquisitionPipeline::acquire_batch`（真实 `DataSource` 接口）→
  B14 点/线/面逐对象验证 → B2 原始观测落库（数据粮仓）+ 候选投影批次原子发布
  （prepare → write → carry-forward missing → publish）→ F7 覆盖体检（事实变体）
  → 采集报告。只有原始观测已保存、候选投影完整发布且报告完成后才解锁评审。
- 错误链：F4/B2/B14/F7 结构化错误 → A1 汇总为页面状态 + B7 通知事实
  （`CollectionError::user_message_key` 映射 zh-CN.json 文本键），S1 在 UI 线程
  发布；A1 不吞错，B7 不依赖 F4 内部错误类型。
- 失败语义：采集失败只暂停本次候选采集；已确认边界的基础导出资格不受影响
  （定向测试用 export-flow 实测验证）。
- 取消/切换方案/过期隔离：export-flow 的 Start/Poll/生命周期过期模式；
  取消后旧结果不拉回，切换方案后旧采集结果按方案隔离。
- S1 迁移：`CollectionCoordinator` 删除，采集页退化为 `CollectionRequest`
  （Open/Start/Poll/ShowReport/Abandon）转发 A1；`runtime.rs` 中 F4/F7 编排与
  `collect_and_audit`、session 的 `has_collection`/`generated_category_counts`
  业务状态全部删除；评审解锁由 A1 `is_review_unlocked` 派生。
- 架构登记：`xtask/src/arch.rs` 的 `APPLICATION_FLOW_CRATES` 与壳白名单加入
  `collection-flow`；`cargo xtask ci`（tidy+arch）绿。
- F7 新增非呈现事实变体 `QuietSentinel::after_collection_facts`（A1 后台执行
  时弹窗事实经 S1 在 UI 线程发布）；`ProjectManager` 改为共享
  `Arc<Mutex<Database>>`（A1 worker 与 F 模块共用同一 B2 连接）。
- 全部用户可见文本走 `zh-CN.json`（新增 collection.report_* 与
  collection.error_* 键）。

## 提交

（`m2-collection-flow`，基于 origin/main `45d6193`，即 PR #12 合入后）

- `7d28f8b` feat(collection-flow): add A1 candidate acquisition use case crate
- `9f6179c` feat(coverage-audit): expose non-presenting audit facts for A1 popup aggregation
- `b62bb2e` refactor(project-management): expose shared database handle for A1 background collection
- `9e5de0e` feat(desktop): migrate collection orchestration to A1 collection-flow
- `2f49d55` chore(xtask): register collection-flow as application flow crate

## 验收证据（本机，Windows，SLINT_BACKEND=software，CARGO_BUILD_JOBS=2）

- `cargo test -p collection-flow`：14 个定向测试全绿（成功闭环/隔离/失败/导出资格/取消/切换/空集合/报告）。
- `cargo test -p desktop-shell --test s1_07_collection_flow`：S1 契约测试绿（转发意图 + 呈现 A1 结果 + 疑点弹窗）。
- `cargo test --workspace`：exit 0，102 个测试二进制全绿。
- `cargo xtask ci`：tidy 与 arch 通过（含 collection-flow 登记）。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`：通过。
- `cargo machete`：无未使用依赖。
- `cargo deny check advisories bans licenses sources`：全部 ok。
- `cargo xtask timings`：全部编译单元在 120s 预算内。

## 关键文件与位置

- `apps/collection-flow/src/flow.rs`：A1 完整用例（start/worker/run_inner 协调链）。
- `apps/collection-flow/src/operation.rs`：Start/Poll/过期隔离。
- `apps/collection-flow/src/error.rs`：`CollectionError` 汇总与文本键映射。
- `apps/collection-flow/tests/collection_flow.rs`：14 个定向测试。
- `core/coverage-audit/src/report.rs`：`after_collection_facts`（非呈现事实变体）。
- `core/project-management/src/view_models.rs`：`database()`/`shared_database()`。
- `apps/desktop/src/runtime.rs`：组合根构造 A1 + 生产 DataSource 桥（WebView 通道）。
- `apps/desktop/src/production/mod.rs`：采集适配器只转发意图并呈现 A1 状态。
- `apps/desktop/src/production/workspace_boundary.rs`：评审解锁改由 A1 派生。
- `apps/desktop/tests/s1_07_collection_flow.rs`：S1 契约测试（后台 worker 化）。
- `xtask/src/arch.rs`：`APPLICATION_FLOW_CRATES` + 壳白名单。

## 未能验证 / 剩余工作（不属于 M2）

- 真实高德在线人工链路：桌面生产 DataSource 桥已接线（WebView IPC 通道 +
  AMap PlaceSearch 脚本），但真实网络链路留发布前人工验证；M2 用真实
  `DataSource` 接口 + 测试桩。
- M3 候选评审（F5 三态页面、批量确认、恢复状态）——本次只解锁评审入口。
- M4 增强导出（保留候选进导出、封账、manifest 区分基础/增强）。
- 边界持久化（产品负责人另行工单）。
- WebView2 环境说明：本机 Edge WebView 运行时在测试进程正常退出时存在
  `EmbeddedBrowserWebView.dll` 清理崩溃；s1_07 契约测试通过不设置高德密钥
  避免创建 WebView2（采集页本身不需要地图）。
