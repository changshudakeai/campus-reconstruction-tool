# Issue Tracker Configuration

> **位置**: `docs/agents/issue-tracker.md`  
> **用途**: 告诉 Agent 工单存在哪里、如何创建和追踪

---

## Issue Tracker: GitHub Issues

**工单存放在 GitHub Issues**。本技能集（特别是 `triage`, `to-tickets`, `to-spec`）会使用 `gh` CLI 与 GitHub 集成。

### 核心配置

- **仓库地址**: `https://github.com/[your-org]/MCRebuild_V2`
- **CLI 工具**: [`gh`](https://cli.github.com/) (GitHub CLI)
- **本地引用**: `.scratch/<feature>/` 目录下存储工单的本地 markdown 副本（方便离线查阅）

### 工作流程

1. **创建工单**: 
   ```bash
   gh issue create --title "TXX: 功能描述" --body .scratch/TXX-[feature].md
   ```

2. **链接工单到 PR**:
   ```bash
   gh issue edit <number> --add-label "in-progress"
   git commit -m "[TXX] 提交内容"
   git push
   gh pr create --draft --body "Closes #<number>"
   ```

3. **更新状态**:
   - `needs-triage` → 初始分类
   - `needs-info` → 需要补充信息
   - `ready-for-agent` → 准备就绪，Agent 可开始
   - `ready-for-human` → 待人工审核/确认
   - `wontfix` → 不予修复

### 本地 Markdown 约定

每个工单在 `.scratch/` 下维护一个 markdown 文件：
```
.scratch/
└── T05-global-settings-and-campus-search/
    ├── T05-global-settings-and-campus-search.md  # 工单主文件
    └── acceptance-criteria.md                    # 验收标准
```

这些文件是**参考副本**，权威源始终是 GitHub Issues。

---

## PRs as a Request Surface

**默认关闭**: 不自动将 PR 作为 triage queue 的一部分。PR 仅用于代码审查和合并，不会触发 triage 流程。

如需开启此功能，编辑本文件并设置 `prs_as_request_surface = true`。

---

## 相关文档

- [Triage Labels](./triage-labels.md) — 标签映射词汇表
- [Domain Docs](./domain.md) — 如何阅读领域文档
