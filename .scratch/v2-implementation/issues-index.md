# MCRebuild V2 历史工单索引

Status: historical-reference

> [!CAUTION]
> 本文件不再包含可执行的 frontier、依赖顺序或“下一张工单”。当前产品见 `docs/product-baseline.md`；当前唯一执行入口见 [`v0.1-end-to-end-mainline-plan.md`](./v0.1-end-to-end-mainline-plan.md)。两者与旧工单冲突时，旧工单冲突段落自动失效。

## 仍需知道的历史映射

- T01–T18：早期核心模块批次，多数已经完成；不得从 T01 重新执行。
- T19 / T19B：旧桌面壳批次；未完成的采集、评审、导出范围已经重新吸收到当前主线。
- T20：旧贯穿弹验收；由当前主线 M1 和 M5 的两次端到端验收取代。
- T21–T25：高德接入批次，已经完成；T21 已被后续实现取代。
- T26–T29 与 s1-12 之后：历史技术债候选；只有当前主线重新纳入时才能启动。
- s1-07 的候选契约未完成部分：只保留当前主线 P0 明确列出的收尾范围。

`.scratch/v2-implementation/issues/` 和 `.scratch/s1-thin-shell-repair/issues/` 中的旧文件是历史实现材料，不具有独立开工权。禁止仅因文件状态写着 backlog / ready-for-agent 就创建新任务。

> **2026-08-17（v2.0.0 发布）**：全部主线工单已完成并验收；本跟踪器随仓库提交，
> 旧 s1 批次已归档至 `docs/history/s1-thin-shell-repair/`。
