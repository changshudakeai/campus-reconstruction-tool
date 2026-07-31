# Issue Tracker 配置（本地优先）

> **位置**: `docs/agents/issue-tracker.md`
> **用途**: 告知 Agent 工单存放于本地 `.scratch/`，以及如何创建和追踪

---

## 工单存放位置

**工单、PRD、handoff 全部以本地 `.scratch/` 为准**（per-worktree 本地追踪器，gitignore，不入库）。
GitHub Issues 不用于本项目的工单追踪；`gh` CLI 仅用于代码 PR 的创建与合并。

## 本地工单约定

- `.scratch/v2-implementation/issues/TXX-*.md` — V2 实施工单（T01 起）
- `.scratch/s1-thin-shell-repair/issues/NN-*.md` — S1 薄壳批次工单（01 起）
- `.scratch/v2-implementation/issues-index.md` — 工单总索引（含补充批次）
- `.scratch/handoff-*.md` — 交付与状态交接，开工前先读最近一份

每张工单包含：**What to build**（负责人视角）、**Blocked by**、**Status**、验收标准。
状态字段沿用五个 triage 词（needs-triage / needs-info / ready-for-agent / ready-for-human / wontfix），
并允许如实补充 implemented / backlog / retired 等状态。

## 新建或更新工单

1. 先读最近 handoff 与 issues-index，避免重复
2. 新建 `issues/TXX-*.md` 或更新既有工单状态
3. 同步更新 `issues-index.md`
4. 完成交付后补一份 handoff 记录

## 代码改动流程（仍走 PR）

1. 新建独立分支并提交
2. 推送后 `gh pr create --draft`（如仓库已推送 GitHub）
3. 等 CI `conclusion` 全绿，由守门人评审合并
4. 合并后更新本地工单状态并补 handoff

## 相关文档

- [Triage Labels](./triage-labels.md) — 五个状态标签
- [Domain Docs](./domain.md) — 如何阅读领域文档