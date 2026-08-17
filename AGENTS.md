# AGENTS.md — MCRebuild V2（重写项目）

## 项目状态

**实施期（ADR 驱动）**。当前产品行为以 `docs/product-baseline.md` 为唯一基线，决策记录于 `docs/adr/`（ADR-0001 起连续编号至 0044），施工顺序以 `.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md` 为唯一入口。

## 开工前必读（按此顺序）

1. **`README.md`** — 项目现状与入口导航
2. **`docs/product-baseline.md`** — 当前产品行为；冲突时取代旧 PRD、工单和交接
3. **`CONTEXT.md`** — 领域术语表（48 个中英对照术语；引用术语以它为准）
4. **`.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md`** — 唯一施工顺序与下一步
5. **与当前工作相关的最新有效 ADR** — 决策缘由和架构边界
6. **`docs/agents/` 与 `docs/module-decisions.md`** — 执行规则和按模块索引

标记为 historical / superseded / retired 的文档只用于追溯。Agent 不得以“最近一份 handoff”或旧 issues-index 重新决定执行顺序。

- 决策总览与"尚未决定"清单：见 `README.md`
- v1.x 源码已按 ADR-0003 完成迁入（导出引擎/Arnis 规则/地图逻辑），原目录已删除

## Agent skills

### Issue tracker

当前主线计划 + `.scratch/v2-implementation/` 工单（随仓库提交）；历史归档见
`docs/history/`。GitHub 只用于 PR。详见 `docs/agents/issue-tracker.md`。

### Triage labels

五个标准化标签：`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`。详见 `docs/agents/triage-labels.md`。

### Domain docs

**Single-context** —— 单一 `CONTEXT.md` + `docs/adr/` 在仓库根目录。详见 `docs/agents/domain.md`。

## 硬性工程纪律

1. **界面文本外置**（ADR-0005）：所有用户可见文字放语言资源文件，代码只引用文本键，禁止硬编码；带变量文案用占位符插值，禁止字符串拼接组句。
2. **配置集中**（ADR-0009/0011）：标签映射表、类目筛选规则集中定义，禁止散落硬编码。
3. **禁止抢跑**：未经 ADR 确认的功能不得实施；基于猜测的代码宁缺毋滥。

## 本地验证流程（按改动风险分级）

Windows 本地所有 Cargo 命令使用 `scripts/cargo-managed.ps1`，由它隔离各
worktree 的 target，并在总缓存达到 24 GiB 时自动回收到 16 GiB。30 GiB 是
容量预算，不是人为报错门禁。完整规则见
`docs/developer-guide/cargo-cache-discipline.md`。

### 三层验证纪律

1. **开发循环（默认）**：只跑能直接证明当前改动的最小测试目标；Rust 改动再跑
   受影响 crate 的 Clippy，Rust 格式改动通过 cargo-managed 包装脚本跑
   `fmt --all --check`。测试失败后
   必须继续在这一层修复，禁止反复跑全量门禁碰运气。
2. **工单收口（按风险扩圈）**：同 crate 私有重构跑该 crate 全部测试；跨 crate
   流程、公共接口、正式数据/schema、共享领域类型、构建脚本或架构规则变化，才
   升级到对应的 workspace/xtask/依赖门禁。
3. **PR、合并与版本收口（完整兜底）**：最后一次代码改动后运行一次全套门禁。
   每项证据绑定 `HEAD + 该门禁受检范围的 tracked/untracked diff fingerprint`；完整
   门禁才绑定整个 diff。范围未变时不得重复运行；后续改动只使受影响的证据失效，
   先重跑受影响项，最终收口时再补一次全套。

纯 Markdown/说明文档改动默认只跑 `git diff --check` 并人工核对链接与事实；若
改的是门禁、CI、Cargo、ADR/产品基线等治理文件，则追加该文件直接控制的专项
检查。不得因为“准备提交”自动把纯文档改动升级为本地全套 Cargo 门禁。

### 专项门禁触发条件

| 门禁 | 本地必须运行的改动 | 可不运行的典型改动 |
|------|--------------------|--------------------|
| 定向测试 / crate 测试 | 相关行为、状态、错误路径或 crate 内部实现 | 纯文档 |
| workspace tests | 跨 crate 流程、共享类型、公共 API、schema/正式数据、测试基础设施；PR/版本收口 | 单 crate 私有机械重构的开发循环 |
| fmt | Rust 源码改动 | 纯 Markdown/JSON/YAML |
| Clippy | Rust 源码；开发循环优先 `-p <crate> --all-targets`，收口再 workspace | 纯文档 |
| machete | `Cargo.toml` 依赖增删、feature/成员变化；完整收口 | 不涉及依赖声明的源码/文档改动 |
| deny | `Cargo.toml`、`Cargo.lock`、依赖来源/许可证策略变化；完整收口 | 不涉及依赖图的私有重构 |
| `xtask tidy` | 文件规模豁免、模块文档、源码布局或 tidy 规则变化 | 普通函数内部改动 |
| `xtask arch` | crate 成员、依赖边、白名单、公共 API 快照规则或架构执法变化 | 同 crate 私有文件搬分 |
| `xtask timings` | crate 拓扑、依赖/feature、`Cargo.toml`/`Cargo.lock`、`build.rs`、宏/泛型生成、编译配置变化，或明确怀疑编译回归；版本候选 | 纯文档、文案、测试断言、小函数整理、同 crate 文件搬分 |

每张工单必须写明“定向验证”“升级门禁触发项”和“最终收口证据”。Agent 报告门禁
时必须分别列出实际命令与结果，不得用“全绿”代替证据，也不得把未触发的门禁写成
已运行。

### 全套门禁命令（仅完整收口，按顺序执行）

```powershell
# 1. 依赖分析（第一道门，检测未使用依赖）
.\scripts\cargo-managed.ps1 -- machete

# 2. 单元测试 + 执法测试（含 tidy/arch 内嵌测试）
.\scripts\cargo-managed.ps1 -- test --workspace

# 3. 格式检查（本地格式化，CI 用 --check 阻断）
.\scripts\cargo-managed.ps1 -- fmt --all --check

# 4. 代码规范检查（本地 warn 档；CI 提为 deny）
.\scripts\cargo-managed.ps1 -- clippy --workspace --all-targets -- -D warnings

# 5. 许可证与漏洞扫描
.\scripts\cargo-managed.ps1 -- deny check advisories bans licenses sources

# 6. xtask 独立检查（单独运行特定子命令）
.\scripts\cargo-managed.ps1 -- xtask ci          # tidy + arch 组合
.\scripts\cargo-managed.ps1 -- xtask timings     # 编译时间预算报告
```

### 全套门禁说明

| 步骤 | 命令 | 作用 | 失败后果 |
|------|------|------|----------|
| 1 | `cargo machete` | 检测未使用的直接依赖 | 依赖白名单腐化，二进制体积膨胀 |
| 2 | `cargo test --workspace` | 运行全部单元测试 + 执法测试 | 功能或架构违规，不准合并 |
| 3 | `cargo fmt --all --check` | 代码格式校验 | 格式不一致，CI 红灯 |
| 4 | `cargo clippy ...` | 代码质量 lint | 警告提升至 deny，阻止编译 |
| 5 | `cargo deny check ...` | 许可证/漏洞/禁用包扫描 | 法律风险或安全漏洞 |
| 6 | `cargo xtask ci` | 规模红线 + 架构 DAG 断言 | 违反 ADR 架构决策 |

### 执法测试特点

- **xtask 内嵌测试**：`xtask/src/main.rs`、`tidy.rs`、`arch.rs` 中的 `#[test]` 函数随 `cargo test` 自动执行
- **public-api 快照**：每个基础 crate (B1-B18) 的 `tests/public_api.rs` 确保公开 API 任何变更显形于 PR diff
- **架构断言**：`cargo xtask arch` 验证依赖 DAG，禁止横向依赖（功能模块间零直连）

### CI 流水线对照

完整 CI 流程见 `.github/workflows/ci.yml`，包含 7 个并行 job：
- `rustfmt` / `clippy` / `test` / `xtask` / `timings` / `machete` / `dependencies`
- 聚合 job `conclusion` 作为唯一的 required status check（分支保护规则）

CI 当前仍在每次 PR 更新、merge queue 与 `main` push 上运行完整兜底；本地分级
验证的目的，是避免 Agent 在一个工单的每个微小编辑后重复执行同一套全量门禁，
不是用定向测试替代合并前的最终证据。

**注意**：PowerShell 中执行多条命令需用分号 `;` 分隔，不可使用 `&&`。

# 非技术产品负责人协作规则

本项目的用户是产品负责人，但不是专业程序员。

在访谈、规划、领域建模和规格制定过程中，必须严格区分产品决策与技术决策。

## 一、用户负责的决策

只有下列问题可以优先询问用户：

1. 用户要解决什么问题；
2. 功能对最终用户应该如何表现；
3. 页面、按钮、步骤和操作流程是否容易理解；
4. 用户看到的名称、术语和中文表达；
5. 功能的优先级；
6. 删除、覆盖、恢复、跳过等行为规则；
7. 哪些情况属于成功、失败或不可接受；
8. 不同方案会产生明显不同的用户体验时，用户偏好哪一种。

提问时必须使用生活化、场景化语言，不得要求用户理解代码、框架、设计模式或底层实现。

## 二、Agent 负责的决策

以下问题由 Agent 阅读代码库后自行决定，不得要求用户审批：

1. 使用什么 Rust 类型；
2. crate、模块、文件和函数如何划分；
3. 使用回调、消息、状态机还是其他实现机制；
4. 测试框架、测试替身和测试目录的选择；
5. 内部数据结构和接口命名；
6. 普通依赖库的选择；
7. 可逆的代码组织和重构细节；
8. 不会改变产品行为的实现方案。

Agent 应选择与现有代码库最一致、风险最低、最容易测试和维护的方案，并在结果中简要记录理由。

## 三、需要共同决定的重大事项

只有满足以下至少一项时，才可以向用户提出技术方案选择：

1. 会改变用户可见行为；
2. 会造成数据删除或不可逆迁移；
3. 会显著增加费用；
4. 会引入外部云服务、账号或隐私风险；
5. 会改变支持的平台；
6. 会推翻现有核心架构；
7. 会显著影响性能、安装体积或硬件要求；
8. 不同方案会影响项目未来能够实现的功能。

提出此类问题时，必须按以下格式解释：

* 我们实际在决定什么；
* 为什么必须由用户参与；
* 方案 A 对普通用户意味着什么；
* 方案 B 对普通用户意味着什么；
* 对现有功能、数据和开发工作量的影响；
* 是否可以以后更改；
* Agent 推荐哪个方案；
* 推荐理由和信心程度。

不得只展示技术名词让用户选择。

## 四、问题过滤规则

在提出任何问题前，依次检查：

1. 能否通过阅读代码、文档或运行项目获得答案？
2. 能否根据现有架构采用安全默认值？
3. 这个决定以后是否容易修改？
4. 不同答案是否真的会改变产品行为？
5. 用户是否具备回答该问题所需的信息？

如果前三个问题中任意一个答案为"是"，并且不会明显改变产品体验，则不要询问用户。

不得询问"Rust 好不好""是否采用某设计模式"这类笼统技术问题。

项目已经采用 Rust 和 Slint（ADR-0003）。除非发现明确、严重且有证据的阻塞问题，否则不得重新讨论是否更换技术栈。

## 五、访谈方式

一次只讨论一个明确主题。不得在同一轮同时讨论 UI、数据库、架构、测试和部署。

不要使用"是否同意推荐方案"作为唯一问题。优先提供具体用户场景，让用户描述自己希望发生什么。

例如不要问："是否同意使用软删除？"
应该问："用户误删项目后，是否应该能够在一段时间内恢复？"

## 六、理解确认

每解决一个重要产品主题后，必须用普通中文总结：

1. 用户将看到什么；
2. 用户可以做什么；
3. 系统会自动做什么；
4. 出错时会发生什么；
5. 哪些内容尚未决定。

然后要求用户判断这段描述是否符合他心中的产品，而不是要求用户审批技术实现。

如果用户无法用自己的话解释功能，则说明当前方案尚未形成真正的共同理解，不得直接进入实施。

## 七、默认行为

当用户表示"不懂技术""没有偏好"或"按推荐方案"时：

* 对技术问题：Agent 自行负责选择；
* 对可逆问题：采用最保守、最符合现有项目的方案；
* 对产品问题：换成具体场景继续帮助用户表达需求；
* 对不可逆问题：解释实际影响后再询问；
* 不得把用户的技术不了解记录成对某个产品方案的主动认可。
