# Issue Tracker 配置（本地优先）

> **位置**: `docs/agents/issue-tracker.md`
> **用途**: 告知 Agent 工单存放于本地 `.scratch/`，以及如何创建和追踪

---

## 工单存放位置

当前排期以 `.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md` 为唯一执行入口；
工单跟踪器 `.scratch/v2-implementation/` 随仓库提交（v2.0.0 起）。历史归档见
`docs/history/`（旧 s1 批次、旧 PRD、历史 handoff）与 `.scratch/archive/`（不入库）。
GitHub Issues 不用于本项目的工单追踪；`gh` CLI 仅用于代码 PR 的创建与合并。

## 本地工单约定

- `.scratch/v2-implementation/issues/TXX-*.md` — V2 实施工单（T01 起）
- `docs/history/s1-thin-shell-repair/issues/NN-*.md` — S1 薄壳批次工单（历史归档）
- `.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md` — 唯一执行顺序
- `.scratch/v2-implementation/issues-index.md` — 历史状态账本，不用于判断下一步

每张工单包含：**What to build**（负责人视角）、**Blocked by**、**Status**、验收标准。
状态字段沿用五个 triage 词（needs-triage / needs-info / ready-for-agent / ready-for-human / wontfix），
并允许如实补充 implemented / backlog / retired 等状态。

## 新建或更新工单

1. 先读产品基线和主线计划，确认工作属于当前里程碑
2. 只有主线计划明确需要局部工单时才新建或更新工单
3. 工单不得改写产品行为或执行优先级
4. 完成后直接更新主线计划的事实状态；不再要求新增 handoff

## 代码改动流程（仍走 PR）

1. 新建独立分支并提交
2. 推送后 `gh pr create --draft`（如仓库已推送 GitHub）
3. 等 CI `conclusion` 全绿，由守门人评审合并
4. 合并后更新本地工单状态并补 handoff

## 相关文档

- [Triage Labels](./triage-labels.md) — 五个状态标签
- [Domain Docs](./domain.md) — 如何阅读领域文档
