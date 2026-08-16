# 交接：M4 增强导出（基础场地 + 已封账保留候选的初始校园内容）

Status: 已实现并提交 `m4-enhanced-export` 分支（PR 待合并）。日期：2026-08-05。
验收窗口：M4 实施窗口 → 验收窗口。权威依据：`docs/product-baseline.md`、
ADR-0040/0041/0042/0043、主线计划 M4 段。

## 完成项

- **ADR-0043**：增强导出流程归属 A2 `export-flow`（不新建 crate），复用
  Start/轮询/过期/落盘与 `ExportFileSystem` 故障注入管线；不改变 M1 边界
  直出行为；增强导出是独立完整用户操作入口（`start_enhanced`），S1 仍只
  转发一次 `start` 意图，由 A2 依据 B2 封账事实内部路由。
- **F9 增强端口**（`core/export-console`）：`EnhancedExportPort` +
  `CandidateExportReader` 窄读者 seam + `EnhancedExportRequest`；只消费
  封账后保留标识与同一份规范化投影；复核 Reviewable 资格；投影缺失/已隔离/
  摘要不一致均明确失败，不静默跳过、不伪成功。
- **生成与落盘**：B5 `BoundaryProjector` 把候选多边形投影到与边界同一块
  坐标系（外接范围 + 落位）；B18 六类生成器按保留候选产出内容并合并进
  基础场地；staging/备份/发布/恢复与 M1 同一管线。
- **manifest**（B17）：`exportKind`（base/enhanced）+ `candidateFacts`
  `keepByCategory` 类别计数；无候选保持空事实与基础标记。
- **封账语义**：导出路径不调用封账、不制造空封账记录；封账只由 F5 用户
  确认时经 B2 单事务写回；导出摘要如实报保留/待定/剔除。
- **A2 路由**：`BoundaryExportFlow::new_with_candidate_store` 共享壳内 B2
  连接组；`start()` 内部路由增强/边界直出；`start_enhanced()` 独立入口；
  `enhanced_hint()` 提供呈现提示。
- **S1 呈现**：导出页在存在已封账保留候选时显示增强摘要（保留 N 项 +
  类别 + 待定/剔除计数，`export.enhanced_summary` 文本键）；只转发不编排；
  失败仍经 B7 错误弹窗。

## 提交（m4-enhanced-export，基于 origin/main `855eac7`）

（提交列表在 PR 描述中给出，按逻辑拆分：ADR 决策 / B5+B17 基础 /
F9 增强核心 / A2 路由 / S1 呈现 / 测试 / 文档。）

## 验收证据（本机，Windows，SLINT_BACKEND=software，CARGO_BUILD_JOBS=2）

- `cargo test -p export-console --test enhanced_export`：增强导出生成保留
  候选内容 + manifest 类别/数量；资格边界（非 Reviewable、投影缺失、摘要
  不一致）明确失败；manifest/schem staging、发布、发布+恢复注入故障 →
  结构化失败且零残留。
- `cargo test -p export-flow`：`start()` 存在保留候选时路由增强导出、无
  保留候选保持边界直出且不制造空封账记录、`start_enhanced()` 独立入口、
  待定/剔除/隔离不进入计数。
- `cargo test -p desktop-shell --test s1_19_enhanced_export_flow`：导出页
  增强提示 + 一次开始意图 → `.schem` 高度/方块计数大于基础场地 + manifest
  enhanced + 评审终态条数不变。
- `cargo test -p desktop-shell --test s1_20_enhanced_export_failure_flow`：
  manifest staging 注入失败 → `Failed` + B7 错误弹窗 + 无伪成功产物。
- `cargo test -p desktop-shell`（全量，含 s1_08–s1_15 回归）：全绿。
- `cargo test --workspace`、`cargo xtask ci`、`cargo clippy --workspace
  --all-targets -- -D warnings`、`cargo fmt --all --check`、`cargo machete`、
  `cargo deny check advisories bans licenses sources`、`cargo xtask timings`：
  全部通过（最终结果以 PR CI 为准）。

## 关键文件与位置

- `docs/adr/0043-enhanced-export-application-flow.md`：流程归属决策。
- `core/export-console/src/enhanced.rs`：增强端口/请求/读者/生成；`src/
  boundary_export.rs` 的 `export_enhanced` + `guarded_export` +
  `write_and_publish`；`src/error.rs` 新增 `CandidateRead`/
  `CandidateEligibility`/`CandidateFactsMismatch`。
- `core/manifest-generator/src/manifest.rs`：`ExportKind`/`CategoryCount`/
  `CandidateFacts.keep_by_category`。
- `core/foundation-mode/src/boundary_export.rs`：`BoundaryProjector`/
  `CandidateBlockBounds`。
- `apps/export-flow/src/candidates.rs` + `lib.rs`：B2 候选存储、增强输入、
  `start` 路由与 `start_enhanced`。
- `apps/desktop/src/runtime.rs`（注入共享 DB）、`apps/desktop/src/production/
  mod.rs`（增强提示 + 失败呈现）、`core/localization/resources/zh-CN.json`
  （`export.enhanced_summary`）。

## 剩余工作（不属于 M4）

M5 正式版收口（旧接线/豁免清理、通知教程、安装包、真实数据人工验收、端到端
剧本）、真实高德在线人工链路、边界持久化（另行工单）、已合并分支清理（另行
确认）。
