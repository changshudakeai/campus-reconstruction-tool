# T44 — 按场景拆分 ReviewWorkbench 集成测试

**Status:** ready-for-agent

**What to build:** 将 `review_workbench.rs` 按核心评审/批量确认、会话与封账、轻量
建议与撤销场景拆成独立测试目标或共享私有 fixture，删除文件长度期限豁免；测试仍
通过 ReviewWorkbench 外部接口观察行为，不越过 seam 断言内部调用顺序。

**Blocked by:** T41.

## 验收与验证

- [ ] 原文件低于 1000 行并删除豁免；共享 fixture 不形成新的巨型或浅层公开模块。
- [ ] `.\scripts\cargo-managed.ps1 -- test -p review-workbench --tests` +
  `.\scripts\cargo-managed.ps1 -- xtask tidy` + fmt。
- [ ] 纯测试文件搬分不触发 timings/machete/deny；PR 收口完整门禁一次。
