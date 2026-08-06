# M5 正式版收口——门禁与豁免证据归档

状态：M5 实施窗口留档（2026-08-06）。所有命令在 Windows PowerShell 下执行，
`SLINT_BACKEND=software`、`CARGO_BUILD_JOBS=2`；原始输出见
[`docs/developer-guide/m5-evidence/`](m5-evidence/)。

## 一、豁免审计结果

### 1. 文件行数豁免（3 处 → 1 处）

| 文件 | M4 现状 | M5 处置 | 结果 |
|------|---------|---------|------|
| `apps/desktop/src/presentation.rs` | 1152 行 + 豁免 | 页面状态与请求拆到 `presentation/pages.rs`，核心接缝保留 | 470 行，**删除豁免** |
| `apps/desktop/src/presentation/pages.rs` | 新建 | 页面状态/请求集中于此 | 699 行，无豁免 |
| `apps/desktop/src/production/workspace_boundary.rs` | 1404 行 + 失效豁免（引用已取消的 10/10 上限） | 适配器拆到 `workspace_adapter.rs`，并删除失效理由 | 583 行，**删除豁免** |
| `apps/desktop/src/production/workspace_adapter.rs` | 新建 | 工作区适配器 | 854 行，无豁免 |
| `apps/desktop/src/production/mod.rs` | 2204 行 + 豁免 | 采集/评审/导出/通知四个流程适配器拆到 `collection.rs`/`review.rs`/`export.rs`/`notification.rs` | 1281 行，**保留有期限豁免**（失效里程碑 v2.1.0 / 2026-12-31） |

mod.rs 保留豁免的理由：剩余体量为组合根本体（`ProductionEntries` 全部入口持有、
UI 回调绑定与确认路由，属 ADR-0037 允许的构造期接线）；流程适配器已全部按入口
拆出，组合根不再包含任何功能模块的呈现翻译。

### 2. `#[allow]` 逐条审计（核心 4 处 → 全部带期限）

| 位置 | lint | 结论 | 理由 |
|------|------|------|------|
| `core/data-persistence/src/candidate_projections.rs:88` | `too_many_arguments` | 保留 + 有期限 | ADR-0040 完整来源与资格事实构造；失效里程碑 v2.1.0 |
| `core/export-console/src/boundary_export.rs:118` | `too_many_arguments` | 保留 + 有期限 | ADR-0042 F9 稳定端口显式展开；expiry v2.1.0 |
| `core/export-console/src/boundary_export.rs:379` | `too_many_arguments` | 保留 + 有期限 | ADR-0042/0043 双文件落盘尾段；失效里程碑 v2.1.0 |
| `core/export-console/src/enhanced.rs:295` | `too_many_arguments` | 保留 + 有期限 | ADR-0043 增强输入稳定端口；失效里程碑 v2.1.0 |

其余 `#[allow]`（`xtask/src/shortcut.rs:54/91`、`xtask/src/timings.rs:66` 的
`disallowed_methods`，`apps/desktop/src/lib.rs:38` 的 Slint 生成代码豁免）均
带 reason，属构建自动化/生成代码的结构性豁免，不适用业务期限。

## 二、遗留接线与占位清理

- 删除覆盖率占位端口：`CoveragePageState`/`CoveragePresentationEntry`/
  `CoverageProductionAdapter`/`show_coverage_for_test` 全部移除（覆盖率事实由
  A1 采集报告承载，独立覆盖率页非产品屏、仅测试可达）。
- 清理过时"占位/遗留"注释：`runtime.rs`、`notice_board.slint`、
  `plan_list.slint`、`trash.slint`、`main.slint`、`workspace_boundary.rs`。
- 半成品扫描（TODO/FIXME/XXX）：源码零命中；tidy 半成品禁令通过。

## 三、public-api 快照审计

按 `docs/developer-guide/enforcement.md` 审计全部 21 个已立户 workspace crate：

- 12 个基础 crate（B1/B2/B3/B4/B5/B6/B7/B10/B13/B14/B17/B18）快照齐备。
- 功能模块与流程模块（F1-F5/F7/F9、A1/A2、S1）快照齐备。
- **补齐缺失项**：`core/project-management`（F3）此前只有行为测试冒充 API
  测试、无入库快照；已按模板改写为真实 `rustdoc-json` 快照测试并生成
  `tests/snapshots/public-api.txt`（2026-08-06，40 KB）。
- 全量清单与依赖扇出见下表。

### 依赖扇出（正式直接依赖，2026-08-06 cargo metadata）

| crate | 直接依赖（workspace 内） |
|-------|--------------------------|
| collection-flow (A1) | coverage-audit, data-acquisition, data-persistence, geometry-validator, localization, notification-center, shared-domain-types |
| coverage-audit (F7) | data-persistence, localization, notification-center, shared-domain-types |
| data-acquisition (F4) | data-persistence, data-transformers, gaode-client, shared-domain-types |
| data-persistence (B2) | shared-domain-types |
| data-transformers (B13) | shared-domain-types |
| desktop-shell (S1) | collection-flow, data-acquisition, data-persistence, export-flow, foundation-mode, gaode-client, global-settings, localization, notification-center, onboarding-tutorial, project-management, review-workbench, shared-domain-types |
| export-console (F9) | foundation-mode, generation-engine, localization, manifest-generator, notification-center, shared-domain-types, sponge-export |
| export-flow (A2) | data-persistence, export-console, global-settings, project-management, shared-domain-types |
| foundation-mode (B5) | shared-domain-types |
| gaode-client (B3) | shared-domain-types |
| generation-engine (B18) | manifest-generator, shared-domain-types |
| geometry-validator (B14) | 无 |
| global-settings (F1) | data-persistence, shared-domain-types |
| localization (B6) | 无 |
| manifest-generator (B17) | shared-domain-types |
| notification-center (B7) | 无 |
| onboarding-tutorial (F2) | data-persistence, localization, notification-center |
| project-management (F3) | data-persistence, shared-domain-types |
| review-workbench (F5) | data-persistence, shared-domain-types |
| shared-domain-types (B1) | 无 |
| sponge-export (B4) | 无 |
| theming (B10) | 无 |
| xtask | 无 |

## 四、timings 证据

`cargo xtask timings`（触碰全 workspace 源文件强制重建后执行，M5 最终状态）：

- 总耗时 167.4s（2m 47.4s），全部单元低于 120s 预算。
- 最慢单元 `desktop-shell` s1_19_enhanced_export_flow(test) 27.2s，lib 本体 9.6s。
- 报告原件已归档：`docs/developer-guide/m5-evidence/timings-report.html`。

## 五、全部门禁（命令与输出文件）

| 门禁 | 命令 | 结果 | 输出 |
|------|------|------|------|
| machete | `cargo machete` | 通过 | `m5-evidence/01-machete.txt` |
| 格式 | `cargo fmt --all --check` | 通过 | `m5-evidence/02-fmt-check.txt` |
| 许可证/漏洞 | `cargo deny check advisories bans licenses sources` | 通过 | `m5-evidence/03-deny.txt` |
| 全量测试 | `cargo test --workspace` | 通过 | `m5-evidence/04-workspace-tests.txt` |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | `m5-evidence/05-clippy.txt` |
| tidy + arch | `cargo xtask ci` | 通过 | `m5-evidence/06-xtask-ci.txt` |
| timings | `cargo xtask timings` | 通过 | `m5-evidence/07-timings.txt` + HTML |

以上结果以 PR CI 的 `conclusion` 为准。
