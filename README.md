# MCRebuild V2 —— 校园复刻工具

MCRebuild V2 是一款面向 Minecraft 校园复刻场景的 Windows 桌面工具。项目希望把真实校园的地图数据转化为可检查、可调整，并最终可导入 Minecraft 的 Sponge `.schem` 文件。

V2 是对 v1.x 的彻底重写，核心目标是：**轻便、高效、好维护，并让产品决策和代码实现都可追溯**。

> [!IMPORTANT]
> 当前版本为 **`2.0.0-dev`（实施期）**，不是可直接交付的稳定版本。核心业务模块已经实现并通过自动化测试；桌面端已接通边界和朝向步骤，采集、评审、导出三个步骤的界面与端到端编排仍在开发中。

## 产品流程

```text
首次设置
  → 选择校区
  → 创建或打开复刻方案
  → 圈定边界
  → 设定朝向
  → 采集校园对象（可选）
  → 人工评审（可选）
  → 生成并导出 .schem
```

项目中的数据层级为：

```text
应用（全局设置：语言、Minecraft 版本）
 └─ 校区（共享知识：校区名称、位置锚点等）
     ├─ 方案 A（边界、朝向、采集与评审数据）
     └─ 方案 B
```

## 当前完成情况

截至 2026-07-30，仓库真实状态如下：

| 范围 | 状态 | 已实现内容 |
|---|---|---|
| 工程基础 | ✅ 已完成 | Cargo Workspace、依赖白名单、架构 DAG 断言、CI、许可证与漏洞扫描、公开 API 快照 |
| 全局设置与持久化 | ✅ 已完成 | 首次运行设置、语言与 Minecraft 版本、高德 Key 配置、上次校区记录、SQLite schema 与迁移 |
| 校区与方案管理 | ✅ 基础流程已完成 | 校区入口、方案创建/改名/复制、软删除、30 天回收站及最近使用排序 |
| 地图边界 | ✅ 已接入桌面端 | 在线高德地图、OSM 边界自动获取、候选排序、顶点编辑与手动画边界兜底 |
| 校区朝向 | ✅ 已接入桌面端 | 在真实地图上用两点或角度设置朝向，按方案隔离保存，并提示修改后的重算影响 |
| 数据采集 | 🟡 核心模块完成 | 可插拔数据源、原始观测落库、增量变化检测与进度模型已实现；桌面步骤页仍是占位 |
| 数据转换与审计 | ✅ 核心模块完成 | 标签到建筑/道路/水体/植被/体育/其他六类的集中映射，以及采集覆盖率安静哨兵 |
| 人工评审 | 🟡 核心模块完成 | 待定/保留/剔除三态评审、批量操作、暂停恢复与封账写回已实现；桌面步骤页仍是占位 |
| 生成与导出 | 🟡 核心模块完成 | Minecraft 方块模型生成、版本绑定用料表、Sponge `.schem` 写入及失败回滚已实现；桌面步骤页仍是占位 |
| 通知与教程 | 🟡 部分接入 | 错误模态弹窗、通知留底和跟练式教程状态已实现；普通 toast、铃铛角标及部分页面数据仍待接线 |

当前仓库包含：

- **20 个**已启用的 Cargo Workspace crate；
- **29 份**已接受的 ADR（架构与产品决策记录）；
- **6 份**专题研究报告；
- **42 项**中英双语领域术语；
- **394 项**由 `cargo test --workspace -- --list` 发现的自动化测试，当前全量测试通过。

## 核心能力

### 地图与 GIS 数据

- 使用高德 JS API + Wry/WebView2 在 Slint 桌面窗口中嵌入在线地图；
- 从 OSM 获取校区边界，完成 GCJ-02/WGS-84 坐标处理；
- 支持边界自动候选、顶点调整和人工绘制兜底；
- 将真实坐标转换为 Minecraft 平面坐标并保留方案级隔离。

### 数据处理流水线

- 以可插拔数据源采集原始对象，并将原始观测持久化到 SQLite；
- 通过集中标签映射把对象互斥归入六类；
- 检测重新采集时的新增、变化与未变化对象；
- 对空类别和“其他”占比异常执行安静审计；
- 以三态人工评审结果作为最终生成输入。

### Minecraft 生成与导出

- 按建筑、道路、水体、植被、体育和其他对象生成方块模型；
- 用料配置与目标 Minecraft 版本绑定，遇到不存在的方块时明确失败；
- 输出 Sponge `.schem`，支持覆盖写入、内容校验和失败回滚。

## 技术栈

| 层次 | 技术 |
|---|---|
| 桌面 UI | Rust、Slint |
| 地图嵌入 | Wry、WebView2、高德 JS API |
| 地理数据 | OSM、GCJ-02/WGS-84 坐标转换、`geo` |
| 本地数据 | SQLite |
| Minecraft 输出 | Sponge `.schem`、NBT、gzip |
| 工程组织 | Cargo Workspace、模块化单体、`xtask` |
| 质量门禁 | rustfmt、Clippy、cargo-machete、cargo-deny、GitHub Actions |

## 架构

项目采用模块化单体：最终交付一个桌面程序，但业务能力在内部以独立 crate 隔离。

```text
apps/desktop                    Slint 桌面呈现与用户操作转发
        │
        ├── 功能模块            设置、方案、采集、评审、审计、教程、导出
        │
        └── 基础模块            领域类型、SQLite、地图、几何、通知、生成、Sponge

xtask                          规模约束、架构依赖检查和开发自动化
```

主要约束：

- 功能模块之间禁止直接依赖；
- 基础模块不得反向依赖功能模块；
- 桌面壳不得直接访问受限制的 ETL/GIS 底层模块；
- UI 文案全部从语言资源注入，不在 Rust 或 Slint 中硬编码；
- 外部依赖版本在根 `Cargo.toml` 单点管理。

架构依据见 [ADR-0001](docs/adr/0001-modular-monolith-architecture.md)、[ADR-0017](docs/adr/0017-modular-architecture-and-crate-catalog.md) 和 [模块边界研究](docs/research/module-boundary-enforcement.md)。

## AI Agent 协作方式

本项目采用 ADR 驱动、Agent 辅助的开发流程：

```text
用户需求访谈
  → 技术与产品研究
  → ADR 确认
  → 工单与验收标准
  → Agent 实现
  → 自动化门禁与 Code Review
  → 用户可见行为验收
```

- [`AGENTS.md`](AGENTS.md)：规定人类产品负责人和 Agent 各自负责的决策；
- [`CONTEXT.md`](CONTEXT.md)：提供统一的中英双语领域语言，减少跨任务语义漂移；
- [`docs/adr/`](docs/adr/)：记录每项重要决策的背景、方案和后果；
- [`docs/module-decisions.md`](docs/module-decisions.md)：按模块汇总已确认需求；
- [`docs/research/`](docs/research/)：保存地图集成、桌面架构、数据流水线与边界执法等研究报告。

## 本地运行

### 环境要求

- Windows 10/11；
- WebView2 Runtime；
- Rust `1.96.0`（由 `rust-toolchain.toml` 固定）；
- 在线地图功能需要网络，以及有效的高德 JS API Key 和 `securityJsCode`。

### 启动开发版

```powershell
cargo run -p desktop-shell --bin campus-tool-dev
```

首次启动后可在设置页填写高德 Key。没有有效 Key 时，依赖在线地图的边界与朝向功能不可用，但不影响不依赖地图的本地模块测试。

## 验证与质量门禁

提交前按顺序运行：

```powershell
cargo machete
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check advisories bans licenses sources
cargo xtask ci
cargo xtask timings
```

GitHub Actions 运行 7 类并行检查：`rustfmt`、`clippy`、`test`、`xtask`、`timings`、`machete` 和 `dependencies`，最后由 `conclusion` 聚合为唯一 required check。

## 已知限制与下一步

- 校区搜索与结果选择的完整桌面流程仍需按 ADR-0008 接通；
- 采集、评审、导出三个桌面步骤页目前仍是占位，尚未形成完整端到端用户流程；
- 暗色主题切换、普通通知 toast、铃铛角标和部分回收站操作仍待接线；
- 开发版快捷方式、真实数据效果和安装包尚需端到端人工验收；
- v1.x 数据迁移策略尚未确定；
- v2.0.0 不包含单栋建筑精修。

## 目录结构

```text
.
├── AGENTS.md                 Agent 协作规则与本地门禁
├── CONTEXT.md                领域术语表
├── Cargo.toml                Workspace、依赖白名单与 lint 规则
├── apps/desktop/             Slint 桌面应用壳
├── core/                     功能模块与基础模块
├── docs/adr/                 产品与架构决策记录
├── docs/research/            专题技术研究
├── docs/module-decisions.md  按模块整理的决策索引
├── sqlite/schemas/           SQLite schema 草案
└── xtask/                    架构检查与开发自动化
```

架构规划目标为 30 个 crate；当前通过接口评审并加入 Workspace 的是 20 个。未加入 Workspace 的规划模块不视为已实现。

## ADR 索引

| 编号 | 决定 |
|---|---|
| [ADR-0001](docs/adr/0001-modular-monolith-architecture.md) | 模块化单体架构 |
| [ADR-0002](docs/adr/0002-sqlite-project-storage.md) | 项目数据采用 SQLite 存储 |
| [ADR-0003](docs/adr/0003-rust-slint-stack-reuse-core.md) | Rust + Slint 技术栈与 v1.x 核心复用 |
| [ADR-0004](docs/adr/0004-app-level-global-settings.md) | 应用级全局设置与首次知情告知 |
| [ADR-0005](docs/adr/0005-externalized-ui-text-i18n-ready.md) | UI 文案外置与多语言准备 |
| [ADR-0006](docs/adr/0006-landing-on-plan-list.md) | 老用户启动后进入方案列表 |
| [ADR-0007](docs/adr/0007-boundary-belongs-to-plan.md) | 边界属于方案级数据 |
| [ADR-0008](docs/adr/0008-campus-selected-via-gaode-search.md) | 通过高德搜索选择校区 |
| [ADR-0009](docs/adr/0009-no-escape-hatch-reserve-generalization.md) | 聚焦校园，同时预留通用化扩展点 |
| [ADR-0010](docs/adr/0010-lightweight-plan-creation.md) | 轻量创建方案 |
| [ADR-0011](docs/adr/0011-exclusive-category-tag-mapping.md) | 六类别互斥分类与标签映射 |
| [ADR-0012](docs/adr/0012-optional-collection-minimal-export.md) | 可选采集与最小导出路径 |
| [ADR-0013](docs/adr/0013-pluggable-data-sources-with-recommendation.md) | 可插拔数据源与推荐策略 |
| [ADR-0014](docs/adr/0014-preview-through-desktop-shortcut.md) | 开发版桌面快捷方式 |
| [ADR-0015](docs/adr/0015-version-naming-and-scope.md) | 版本命名及 v2.0.0 范围 |
| [ADR-0016](docs/adr/0016-review-process-design.md) | 评审流程与无卡顿交互 |
| [ADR-0017](docs/adr/0017-modular-architecture-and-crate-catalog.md) | 模块目录与模块化硬规则 |
| [ADR-0018](docs/adr/0018-plan-list-page-design.md) | 方案列表页设计 |
| [ADR-0019](docs/adr/0019-coverage-audit-quiet-sentinel.md) | 采集覆盖率安静哨兵 |
| [ADR-0020](docs/adr/0020-onboarding-follow-along-guidance.md) | 跟练式新手教程 |
| [ADR-0021](docs/adr/0021-notification-center-popup-rule.md) | 通知中心与错误弹窗规则 |
| [ADR-0022](docs/adr/0022-three-state-review-undo-deferred.md) | 三态评审与撤销暂缓 |
| [ADR-0023](docs/adr/0023-theming-color-cards-motion-table.md) | 主题色卡与动效表 |
| [ADR-0024](docs/adr/0024-export-initial-campus-generation-engine.md) | 初始校园生成与 `.schem` 导出 |
| [ADR-0025](docs/adr/0025-shell-can-depend-on-domain-types.md) | 桌面壳可依赖共享领域类型 |
| [ADR-0026](docs/adr/0026-material-table-in-b17.md) | 用料表与版本校验归属 |
| [ADR-0027](docs/adr/0027-stepper-wizard-navigation.md) | 五步向导导航 |
| [ADR-0028](docs/adr/0028-tutorial-bubble-list.md) | 教程气泡列表与触发点 |
| [ADR-0029](docs/adr/0029-boundary-from-osm-with-manual-adjustment.md) | OSM 自动边界与人工调整 |
