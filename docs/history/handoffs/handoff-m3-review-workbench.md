# 交接：M3 可选候选评审（F5 三态页面接通桌面）

Status: 已实现并提交 `m3-review-workbench` 分支（PR 待合并）。日期：2026-08-05。
验收窗口：M3 实施窗口 → 验收窗口。权威依据：`docs/product-baseline.md`、ADR-0037/
0039/0040/0041、主线计划 M3 段（ADR-0040“交付切分”中“评审页面归 T19B-7/M4”的
旧表述与主线计划冲突，按主线计划执行；ADR-0040 数据契约规则全部适用）。

## 完成项

- 桌面评审页接通 F5：`ReviewProductionAdapter` 由占位改为持有注入器中的
  `ReviewWorkbench` 会话，转发完整用户操作（进入评审台、逐项三态判定、批量确认、
  暂停/恢复、封账）；S1 只呈现 F5 返回的页面状态与通知，不做业务编排。
- `ReviewPageState` 扩展为 F5 `WorkbenchView` 的完整呈现层：六类标签页 + 计数、
  当前类别候选卡片（标题/三态/复选）、已选计数、批量按钮、封账导出摘要、空态。
- 无候选路径：`enter_review` 读不到可评审候选时显示明确空态（`review.empty`），
  不阻塞导出、不伪造“评审完成”；空态下不显示封账入口与摘要。
- 封账语义：`seal` 经 `ViewModelInjector::seal_review` 取 F3 共享数据库连接写回 B2；
  写回失败返回 `Err` 且封账不生效，B7 呈现结构化失败（`review.seal_failed_*`），
  评审状态保持可修改，不出现伪成功产物。
- 批量确认：批量剔除 >=5 项走 F5 `NeedsConfirmation` → 桌面通用确认弹窗；
  确认执行 `confirm_pending`、取消执行 `cancel_pending`（状态原样）。
- 暂停/恢复：`save_session`/`restore_session` 落到系统临时目录
  `campus-rebuild-review-<plan>.json`，成功/失败均有本地化通知。
- 文本键：`core/localization/resources/zh-CN.json` 的 `review.*` 补齐
  （empty/seal/export_summary/seal_failed/enter_failed/session_*），全部用户可见
  文本经 B6 注入，无硬编码、无静默兜底。
- 行为基线：`docs/behavior-baselines/s1-current-user-observable-behavior.md` 评审行
  由“占位页”更新为 M3 真实可观察行为，`s1_contract_baseline.rs` 同步断言。

## 提交（m3-review-workbench，基于 PR #13 合入后的 main 内容，即本地 `1fc634c`）

- `feat(desktop): wire F5 review workbench into the desktop review page`
- `i18n(zh-CN): add review.* keys for M3 review page`
- `test(desktop): cover review grouping, tri-state, batch confirm and seal`
- `docs(baseline): record M3 review observable behavior in contract`
- `docs(plan): mark M3 complete and hand off next step to M4`

## 验收证据（本机，Windows，SLINT_BACKEND=software，CARGO_BUILD_JOBS=2）

- `cargo test -p desktop-shell --test s1_16_review_flow`：六类分组 + 逐项三态 +
  资格边界（Isolated 与无投影原始观测不进评审页）通过。
- `cargo test -p desktop-shell --test s1_17_review_batch_and_seal`：批量确认
  （含取消与 >=5 二次确认阈值）、封账写出导出摘要、DB 终态 (0,2,4)、重新进入
  恢复上一轮封账终态，通过。
- `cargo test -p desktop-shell --test s1_18_review_empty_and_failure`：无候选空态
  不阻塞导出、不伪造完成；封账写回失败 → B7 错误弹窗 + 状态可继续修改 + 恢复后
  封账成功，通过。
- `cargo test --workspace`：exit 0，105 个测试结果全绿。
- `cargo xtask ci`：tidy + arch 通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `cargo fmt --all --check`：通过。
- `cargo machete`：无未使用依赖。
- `cargo deny check advisories bans licenses sources`：全部 ok。
- `cargo xtask timings`：全部编译单元在 120s 预算内。

## 关键文件与位置

- `apps/desktop/src/production/mod.rs`：`ReviewProductionAdapter`（转发 + B7 失败
  呈现）、`ReviewRequest` 处理、`PendingConfirmation::ReviewBatchReject`、评审回调
  绑定。
- `apps/desktop/src/presentation.rs`：`ReviewPageState` / `ReviewRequest` 与渲染。
- `apps/desktop/src/runtime.rs`：`review_mut()` / `seal_review()`。
- `apps/desktop/src/production/workspace_boundary.rs`：`active_plan_id()`。
- `apps/desktop/ui/review.slint`：评审页组件；`apps/desktop/ui/main.slint` 步骤④
  由占位矩形替换为 `ReviewView`（步骤⑤导出占位保留）。
- `apps/desktop/tests/s1_16_review_flow.rs`、`s1_17_review_batch_and_seal.rs`、
  `s1_18_review_empty_and_failure.rs`：桌面评审契约测试。
- `core/localization/resources/zh-CN.json`：`review.*` 新键。
- `docs/behavior-baselines/s1-current-user-observable-behavior.md` 评审行。

## 未能验证 / 剩余工作（不属于 M3）

- 真实高德在线人工链路：桌面采集仍走 WebView IPC 测试桥，真实网络链路留发布前
  人工验证（M2 既有项）。
- M4 增强导出：保留候选进 F9 导出、封账后的身份模型接线、manifest 区分基础/增强。
- 边界持久化（产品负责人另行工单）。
- CI 偶发红的 5 秒等待加固已先行完成（PR #14 `fix/desktop-test-wait-hardening`，
  24 处等待放宽至 30 秒轮询上限，本地 6 个测试文件全绿）。
