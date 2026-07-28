# Domain Documentation Consumer Rules

> **位置**: `docs/agents/domain.md`  
> **用途**: 定义如何阅读、理解和使用领域文档（CONTEXT.md, ADRs, module-decisions）

---

## 文档体系布局

### Single-Context（当前采用）

```
New-branch-v2/
├── CONTEXT.md                    ← 领域术语表（glossary）
├── docs/
│   ├── adr/                      ← 架构决策记录（ADR）
│   │   ├── 0001-...md
│   │   └── 0029-...md
│   ├── module-decisions.md       ← 按模块分类的决策索引
│   ├── research/                 ← 深度技术研究文档
│   └── agents/                   ← 本目录（Agent 协作规则）
└── .scratch/                     ← 工单和 handoff 文档
```

---

## 📋 文档阅读优先级

Agent 在开始任何任务前，应该按以下顺序阅读：

### 1️⃣ 顶层导航（根目录）
```
读取: AGENTS.md
目的: 
  ✓ 了解项目定位和协作规则
  ✓ 找到所有相关文档的位置
  ✓ 按照建议顺序继续阅读
```

### 2️⃣ 领域术语表
```
读取: CONTEXT.md
目的:
  ✓ 理解核心术语（中英文对照）
  ✓ 掌握"校区"、"方案"等关键概念的精确定义
  ✓ 为后续阅读建立共同语言
```

### 3️⃣ 架构决策记录（按主题）
```
读取: docs/adr/
策略:
  ✓ 根据任务主题，优先阅读相关 ADR
  ✓ 例如：边界相关任务 → 先读 ADR-0029
  ✓ 例如：通知相关任务 → 先读 ADR-0021
  ✓ 不确定时 → 从 ADR-0001 开始按顺序读

关键 ADR:
  - ADR-0001: 模块化单体架构（必读）
  - ADR-0017: 模块化十戒 + 29 模块目录（必读）
  - ADR-0029: 边界 OSM 获取方案（最新）
```

### 4️⃣ 按模块的决策索引
```
读取: docs/module-decisions.md
目的:
  ✓ 快速定位特定模块的决策
  ✓ 查看模块间的依赖关系
  ✓ 了解全局约束

特别注意: B1 共享领域类型 → 术语定义引用 CONTEXT.md
```

### 5️⃣ 深度研究文档
```
读取: docs/research/
内容:
  ✓ gaode-map-integration-options.md — T21-T25 地图集成方案
  ✓ module-boundary-enforcement.md — 模块边界强制执行
  ✓ data-pipeline-modules.md — 数据采集管道设计
  ✓ modular-desktop-architecture.md — 桌面应用架构

使用时机: 需要理解技术选型的"为什么"时
```

### 6️⃣ 工单和交接文档
```
读取: .scratch/handoff-*.md
目的:
  ✓ 了解最近的交付状态
  ✓ 查看已知限制和遗留问题
  ✓ 追踪工单完成进度

示例: handoff-2026-07-28-t25-complete.md 记录了 T25 的完整交付
```

---

## 🎯 CONTEXT.md 的作用

### 当前状态

**已创建**: `New-branch-v2/CONTEXT.md`（见下）

**内容**:
- 核心术语中英文对照表
- 每个术语的定义、作用域、来源 ADR
- 示例：校区（Campus）、方案（Plan）、边界（Boundary）等

### 维护规则

1. **新增术语** → 在 CONTEXT.md 中添加条目，注明来源 ADR
2. **修改定义** → 同步更新 CONTEXT.md 和 ADR 原文
3. **引用规范** → 所有代码和文档引用术语时，使用 CONTEXT.md 中的标准术语

---

## 📚 推荐阅读路径（按任务类型）

### 类型 1: 新功能开发
```
1. AGENTS.md → 了解协作规则
2. CONTEXT.md → 掌握术语
3. 相关 ADR → 理解设计决策
4. module-decisions.md → 确认模块归属
5. .scratch/handoff-*.md → 检查最新状态
```

### 类型 2: Bug 修复
```
1. AGENTS.md → 硬性工程纪律
2. 相关 ADR → 理解模块边界
3. module-decisions.md → 确认模块依赖
4. 代码文件 → 定位问题
```

### 类型 3: 架构调整
```
1. 全部 ADR → 确保不违反现有决策
2. module-decisions.md → 更新决策索引
3. CONTEXT.md → 同步术语变化
4. .scratch/handoff-*.md → 记录变更原因
```

---

## ⚠️ 常见误区

### ❌ 错误做法
- 直接从 module-decisions.md 复制术语，不查 CONTEXT.md
- 跳过 ADR 原文，只看标题就下结论
- 不看 .scratch/handoff 就开始工作

### ✅ 正确做法
- **术语统一**: 所有引用都指向 CONTEXT.md
- **决策溯源**: 每个决定都能追溯到具体 ADR
- **状态感知**: 开工前先读最新 handoff

---

## 🔄 文档同步机制

### 每次添加新 ADR 后
```bash
# 1. 更新 README.md 的 ADR 列表
# 2. 更新 module-decisions.md 对应模块章节
# 3. 如有新术语，更新 CONTEXT.md
# 4. 在 .scratch/ 中记录变更
```

### 每次修改术语后
```bash
# 1. 更新 CONTEXT.md
# 2. 全局搜索旧术语引用，批量替换
# 3. 更新 ADR 原文中的定义
# 4. 通知团队（通过 handoff 文档）
```

---

## 📖 快速参考

| 场景 | 应该阅读 |
|------|----------|
| 术语不理解 | `CONTEXT.md` |
| 设计为什么这样 | `docs/adr/` |
| 模块如何分工 | `docs/module-decisions.md` |
| 最新进展 | `.scratch/handoff-*.md` |
| 技术选型理由 | `docs/research/` |
| Agent 协作规则 | `AGENTS.md` |
| Issue 如何管理 | `docs/agents/issue-tracker.md` |
| Triage 标签 | `docs/agents/triage-labels.md` |

---

## 相关文档

- [Issue Tracker](./issue-tracker.md) — 工单追踪配置
- [Triage Labels](./triage-labels.md) — 标签词汇表
- `CONTEXT.md` — 领域术语表
- `docs/adr/` — 架构决策记录
- `docs/module-decisions.md` — 模块决策索引
