# ADR-0026：用料配置表居 B17，B18 只读调用（修订 ADR-0024 的模块归属）

## 状态

已接受（2026-07-26，T10 实施核查）。本决策登记基础层白名单依赖边，明确 B18 → B17 是唯一合法的横向只读调用。

## 背景

ADR-0024 原文将"生成规则与用料配置表"统一归入新立模块 B18 `generation-engine`。实际代码实施时（T08 T09），发现以下事实：

- 用料配置表（material-table.mjs / material-{mc-version}.rs）与 manifest-generator 职责同族：**如实记录本次导出的方块用量**；
- 版本校验器（version-check.rs）同样属于 B17 的"输出质量门禁"能力；
- 这两者与 foundation_manifest.json 同居一处，便于 "manifest 即最终用料快照" 的产品承诺；
- 实施中（T10），B18 通过公开 API **只读调用** B17 的查询接口获取用料数据，自身不做任何文件 IO；
- xtask arch 断言已自动登记 B18 → B17 为白名单边。

依据"每次决策都落文档"的纪律，新增本 ADR 修正 ADR-0024 的模块归属表述。

## 决定

### 一、职责划分

✅ **维持现状**——按 T10 实施形态拍板：

| 模块 | crate | 职责 |
|-----|-------|------|
| **B17 Manifest 生成器** `manifest-generator` | `manifest-generator` | 用料配置表 + 版本校验器 + foundation_manifest.json（如实记录本次导出用量） |
| **B18 初始校园生成引擎** `generation-engine` | `generation-engine` | 生成规则（Arnis 建筑/道路/水体/植被/体育/其他）、用料查询接口（只读） |

### 二、唯一的白名单横向边

✅ **B18 → B17 只读调用** —— 这是**唯一允许的基础层横向依赖边**之一：

| 使用场景 | 允许的操作 | 禁止操作 |
|---------|-----------|---------|
| B18 查询用料 | 调用 B17 公开的 `get_materials(mc_version)` 接口 | 不得直接读取 B17 配置文件、不得做文件系统 IO |
| 版本校验 | 消费 B17 的 `validate_version()` 结果 | 不得绕过校验、不得降级 MC 版本检测 |

约束：**B18 零文件 IO**。所有存储责任在 B17，B18 仅为计算引擎。

### 三、与其他模块的关系

| 模块 | 允许？ | 说明 |
|-----|-------|------|
| F9 导出控制台 | ✅ | F9 组装 manifest(从 B17) + schem(从 B4) |
| B4 Sponge 导出引擎 | ✅ | B4 仅接收 B18 生成的方块模型，不碰 manifest |
| B17 Manifest 生成器 | ❌ | 反向禁止：B17 不得依赖 B18（单向边） |
| S1 主程序壳 | ❌ | S1 不得跨层访问 B17/B18 内部实现 |

## 后果

### 正面后果

- **manifest 与用料表同居**：foundation_manifest.json 如实反映"本次导出的总用量"，无需额外聚合；
- **B18 保持纯逻辑**：零文件 IO、可独立单元测试、无外部依赖假设；
- **开发边界清晰**：产品负责人验收时，只需关注 B17 的配置表是否与目标 MC 版本匹配；
- **xtask arch 已带断言**：B18 → B17 白名单边已在 CI 层落地，越权边自动拦截。

### 负面后果

- **心智负担转移**：未来有人修改 B17 配置格式时，需同时检查 B18 调用接口的兼容性；
- **版本核对分散**：用料表的版本一致性由 B17 承担，B18 只信任接口返回值。

### 缓解措施

- manifest-generator 公开 API 加稳定性标注（`#[stable]` 注释标记），打破需大版本通知；
- generation-engine 的用料查询接口写 integration test（固定输入 → 固定输出）；
- 每 PR 评审 check 是否改动 B17 配置结构（.mjs/.rs 表），若有则强制拉会 B18 负责人签字。

---

## 附录：module-decisions.md 引用同步

本文档发布后，须在 `docs/module-decisions.md` 的 B17/B18 小节各追加一行引用：

```markdown
### B17 Manifest 生成器 `manifest-generator`
- ✅ foundation_manifest.json 如实记录包含/缺失类别（ADR-0012）
+ ✅ 用料配置表与版本校验器同居此处（ADR-0026，修订 ADR-0024）

### B18 初始校园生成引擎 `generation-engine`
- ✅ 新立模块（ADR-0024，模块总数 30）：评审保留数据 → 方块模型；承载全部生成规则（Arnis 建筑规则：height 优先/层数×4+2 估高/屋顶规则；六类生成规则）与用料配置表
+ ✅ 复用来源：v1.x arnis-core crate 迁入作地基（ADR-0024）；用料表只读调用 B17（ADR-0026，白名单横向边）
```

注意：本 ADR 仅更新文档，不改任何代码。
