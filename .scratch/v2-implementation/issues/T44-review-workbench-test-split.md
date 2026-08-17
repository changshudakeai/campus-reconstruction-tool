# T44 — 按场景拆分 ReviewWorkbench 集成测试

**Status:** completed（2026-08-17，产品负责人验收并授权并入本地主线）

**What to build:** 将 `review_workbench.rs` 按核心评审/批量确认、会话与封账、轻量
建议与撤销场景拆成独立测试目标或共享私有 fixture，删除文件长度期限豁免；测试仍
通过 ReviewWorkbench 外部接口观察行为，不越过 seam 断言内部调用顺序。

**Blocked by:** T41.

## 验收与验证

- [x] 原文件低于 1000 行并删除豁免；共享 fixture 不形成新的巨型或浅层公开模块。
- [x] `.\scripts\cargo-managed.ps1 -- test -p review-workbench --tests` +
  `.\scripts\cargo-managed.ps1 -- xtask tidy` + fmt。
- [x] 纯测试文件搬分不触发 timings/machete/deny；PR 收口完整门禁一次。

## 实施证据（2026-08-17）

- 仅重组 `core/review-workbench/tests/`：核心评审/批量确认 11 个、会话与封账
  5 个、轻量建议与撤销 9 个；拆分前后 25 个集成测试名称完全一致。
- 原 `review_workbench.rs` 由 1353 行降为 334 行；新场景文件分别为 320 行与
  510 行；私有共享 fixture 为 173 行。测试目录内原文件长度期限豁免已删除。
- `test -p review-workbench --tests`：模块单测 18、公开接口测试 2、三组集成测试
  11/5/9，全部通过；`xtask tidy` 与 `fmt --all --check` 通过。
- 按 T41 风险分级，本次未运行 timings、machete、deny 或完整 workspace 门禁；
  它们留待 PR/版本收口执行一次。
