# MCRebuild —— 校园复刻工具（V2）

MCRebuild V2 是一款 Windows 桌面工具，帮你把自己学校的真实校园“复刻”进 Minecraft：
搜索并选中真实校区 → 圈定方案边界 →（可选）采集并评审校园里的建筑、道路、水体、
植被 → 在应用里预览整校园方块模型 → 导出为 Minecraft 可直接导入的 `.schem` 文件。

> [!IMPORTANT]
> 当前版本 **V2.1.0（2026-08-20）**。产品行为基线见
> [`docs/product-baseline.md`](docs/product-baseline.md)；施工与发布状态见
> [`.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md`](.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md)；
> V2.1 收口证据见
> [`docs/developer-guide/v2.1-release-closeout-evidence.md`](docs/developer-guide/v2.1-release-closeout-evidence.md)。

## 目录

- [它能做什么](#它能做什么)
- [快速开始（5 分钟上手）](#快速开始5-分钟上手)
- [首次设置：高德 Key 与 Minecraft 版本](#首次设置高德-key-与-minecraft-版本)
- [五步工作区使用教程](#五步工作区使用教程)
- [常见问题](#常见问题)
- [给开发者](#给开发者)
- [版本历史](#版本历史)

## 它能做什么

- 高德地图上搜索并选择真实校区，以该校为“校区”管理复刻方案；
- 从 OSM 自动获取校区边界，支持顶点拖拽微调与人工圈画兜底；
- 按六类（建筑/道路/水体/植被/体育/其他）采集校园对象，自动命名、检测重复与重叠；
- 评审台按“置信度”分档（高/中/低）帮你快速决定每个候选：保留、剔除或待定，
  支持一键应用建议与批量操作；
- 导出前先看**整校园 3D 方块预览**（与最终导出的 `.schem` 完全一致），可旋转/缩放；
- 一键导出 Minecraft 可导入的 Sponge `.schem` 文件（附用料清单 manifest）。

## 快速开始（5 分钟上手）

1. 运行应用，完成首次设置：选择语言、目标 Minecraft 版本，并填写高德 Key（见下节）；
2. 在“校区”页搜索你的学校，点搜索结果进入该校的方案列表；
3. 新建一个方案（输入方案名即可）；
4. 进入方案后，第一步“圈边界”：等待 OSM 自动获取边界，必要时拖动顶点微调，
   点“确认边界”；
5. 顶部切到第五步“导出”，点“开始导出”——一份 `.schem` 就生成在默认导出目录了。

边界是**唯一必填项**；朝向、采集、评审都可以跳过，不会阻塞导出。

## 首次设置：高德 Key 与 Minecraft 版本

- **语言**：界面语言（当前提供中文）。
- **Minecraft 版本**：生成方块用料与目标版本绑定，请选择你将要导入的世界版本。
- **高德 Key**：校区搜索、边界、朝向和评审都依赖在线高德地图，需要：
  1. 到高德开放平台（lbs.amap.com）创建“Web端（JS API）”应用；
  2. 复制 **Key** 与**安全密钥（securityJsCode）**；
  3. 在设置页粘贴并点“测试连通性”。

没有有效 Key 时，依赖地图的功能不可用；已保存的边界、评审与导出不受影响。

## 五步工作区使用教程

方案打开后，顶部是“边界、朝向、采集、评审、导出”五个步骤。它们不是五个强制关卡，
只有第一步边界是必填。

### ① 圈边界

边界是唯一必填项，确认后导出入口立即可用。

- 进入本步会自动从 OSM 获取校区边界（左侧抽屉显示获取进度）；
- 边界出现后可直接拖动顶点微调，也可用“撤销 / 清空 / 重新获取”；
- 获取失败或数据缺失时，可切到“人工圈画”：点击地图逐点圈出边界；
- 确认前请检查边界确实圈住了目标校园——导出的范围以它为准。

### ② 定朝向（可选）

- 不设置时默认“地图正北”；
- 想旋转整个校园（比如让正门朝南），用“两点画线”或直接输入角度；
- 修改已确认的朝向会提示影响范围，确认后生效。

### ③ 候选采集（可选）

- 点“开始采集”，应用会在边界内按六类寻找校园对象（建筑/道路/水体/植被/体育/其他）；
- 采集过程显示进度，可取消；重新采集会显示“新增 / 有变化 / 本次未找到 / 未变化”
  摘要；
- 采集失败只影响本次候选，不影响已确认的边界与导出资格。

### ④ 候选评审（可选）

评审台帮助你在导出前决定哪些候选进入校园内容：

- 候选按**置信度**分档：高（名称清晰、形状完整，建议保留）、中（存在不确定信号，
  需人工确认）、低（未命名 / 重复嫌疑 / 重叠等，需关注）；
- 用顶部“置信度筛选”（全部 / 高 / 中 / 低）过滤，候选与地图按 高 → 中 → 低 排序；
- **一键应用建议**：把全部高置信候选改为“保留”（不会剔除任何候选），确认弹窗会
  显示将变更数量与理由分布；
- 每条候选三态：**保留**（唯一进入导出）/ **剔除**（排除）/ **待定**（暂不决定）；
  批量改状态时，全选只勾当前页，批量剔除会先确认一次；
- 点“封账完成评审”后决定不可再改，导出即按封账结果生成。

### ⑤ 导出与 3D 预览

- **3D 预览**：进入本步不会自动生成；点“生成 3D 预览”后，整校园以真实方块呈现
  （建筑 / 道路 / 水面 / 植被），拖动旋转、滚轮缩放、可复位视角，还能把已保留候选
  “定位到 3D 预览”。预览数据与最终导出的 `.schem` 完全一致；
- **开始导出**：确认导出（会明示封账后果与待定项数量）后进入后台导出，进度可跟踪；
- 完成后得到 `.schem` 与用料清单（manifest），同一区域继续显示 3D 预览与导出结果；
- 预览失败不影响导出：会有明确错误提示，导出照常进行。

## 常见问题

- **为什么导出的文件只有地基？** 未采集候选时，导出生成边界覆盖范围内的平整场地；
  采集并保留候选后，才生成包含建筑等内容的初始校园。
- **导出的 .schem 怎么用？** 这是 Sponge 格式的 Minecraft 结构文件，可用
  WorldEdit / FAWE 等工具在存档中粘贴（`//schem load` + `//paste`）。
- **高德 Key 填了但地图还是不可用？** 确认 Key 与安全密钥都来自“Web端（JS API）”
  应用，且已通过“测试连通性”；地图功能需要联网。
- **删错了方案？** 方案删除后进回收站，30 天内可恢复；同名恢复会自动加“（恢复 N）”。
- **评审封账后想改怎么办？** 封账后决定不可再改；想调整只能重新采集、重新评审。

## 给开发者

### 环境要求

- Windows 10/11；
- WebView2 Runtime；
- Rust `1.96.0`（由 `rust-toolchain.toml` 固定）；
- 在线地图功能需要网络，以及有效的高德 JS API Key 与 `securityJsCode`。

### 启动开发版

```powershell
cargo run -p desktop-shell --bin campus-tool-dev
```

首次启动后可在设置页填写高德 Key。没有有效 Key 时，依赖在线地图的边界与朝向功能
不可用，但不影响不依赖地图的本地模块测试。

### 技术栈

| 层次 | 技术 |
|---|---|
| 桌面 UI | Rust、Slint |
| 地图嵌入 | Wry、WebView2、高德 JS API |
| 3D 预览 | WebView + Three.js（隐藏面剔除、分块 Worker） |
| 地理数据 | OSM、GCJ-02/WGS-84 坐标转换、`geo` |
| 本地数据 | SQLite |
| Minecraft 输出 | Sponge `.schem`、NBT、gzip |
| 工程组织 | Cargo Workspace、模块化单体、`xtask` |
| 质量门禁 | rustfmt、Clippy、cargo-machete、cargo-deny、GitHub Actions |

### 架构

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

- 功能模块之间默认不直接依赖；确需直连时必须有决策记录并命中架构白名单；
- 基础模块不得反向依赖功能模块；
- 桌面壳不得直接访问受限制的 ETL/GIS 底层模块；
- UI 文案全部从语言资源注入，不在 Rust 或 Slint 中硬编码；
- 外部依赖版本在根 `Cargo.toml` 单点管理。

架构依据见 [ADR-0001](docs/adr/0001-modular-monolith-architecture.md)、
[ADR-0017](docs/adr/0017-modular-architecture-and-crate-catalog.md) 和
[模块边界研究](docs/research/module-boundary-enforcement.md)。

### AI Agent 协作方式

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
- [`docs/product-baseline.md`](docs/product-baseline.md)：当前用户可见产品行为的唯一基线；
- [`CONTEXT.md`](CONTEXT.md)：提供统一的中英双语领域语言，减少跨任务语义漂移；
- [`docs/adr/`](docs/adr/)：记录每项重要决策的背景、方案和后果；
- [`docs/module-decisions.md`](docs/module-decisions.md)：按模块汇总已确认需求；
- [`docs/research/`](docs/research/)：保存地图集成、桌面架构、数据流水线与边界执法等研究报告。

### 验证与质量门禁

Windows 本地验证按“开发循环定向验证 → 工单按风险扩圈 → PR/版本一次完整兜底”分级
执行；不得在每个小改动后机械重复全套。`cargo xtask timings` 只在依赖图、crate 拓扑、
构建配置或明确的编译性能风险变化时本地运行，版本候选再统一运行。详细触发矩阵见
[`AGENTS.md`](AGENTS.md) 和
[`docs/developer-guide/enforcement.md`](docs/developer-guide/enforcement.md)。

需要完整收口时，通过自动缓存治理入口按顺序运行（会隔离各 worktree 的 target 并自动
回收旧缓存，见 [`docs/developer-guide/cargo-cache-discipline.md`](docs/developer-guide/cargo-cache-discipline.md)）：

```powershell
.\scripts\cargo-managed.ps1 -- machete
.\scripts\cargo-managed.ps1 -- test --workspace
.\scripts\cargo-managed.ps1 -- fmt --all --check
.\scripts\cargo-managed.ps1 -- clippy --workspace --all-targets -- -D warnings
.\scripts\cargo-managed.ps1 -- deny check advisories bans licenses sources
.\scripts\cargo-managed.ps1 -- xtask ci
.\scripts\cargo-managed.ps1 -- xtask timings
```

GitHub Actions 运行 7 类并行检查：`rustfmt`、`clippy`、`test`、`xtask`、`timings`、
`machete` 和 `dependencies`，最后由 `conclusion` 聚合为唯一 required check。

### 已知限制与下一步

- 生成规则精细效果（真实候选的精细化生成）未实现，另行决策；
- Overture 离线补充包未实现（在线 OSM/Overpass 为主）；
- “提升导出 3D 精度”（更高清纹理 / 更精细逐块渲染）已列为后续升级项；
- 朝向可选、默认地图正北（ADR-0041）；边界是唯一必填项。

### 目录结构

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

架构规划与已立户 crate 清单以 `Cargo.toml` members 与 `docs/module-decisions.md` 为准；
未立户的规划模块不视为已实现（ADR-0017）。

### ADR 索引

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
| [ADR-0015](docs/adr/0015-version-naming-and-scope.md) | 版本命名及范围 |
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
| [ADR-0030](docs/adr/0030-incremental-refresh-changes-first.md) | 重新采集结果变化优先、全量可查 |
| [ADR-0031](docs/adr/0031-notification-diagnostics-seam.md) | 通知中心与故障诊断分工 |
| [ADR-0032](docs/adr/0032-isolate-invalid-candidate-geometry.md) | 隔离无效候选几何，不阻断整次采集 |
| [ADR-0033](docs/adr/0033-warn-on-overlapping-building-outlines.md) | 建筑轮廓重叠时提醒用户 |
| [ADR-0034](docs/adr/0034-recognize-explicit-building-part-containment.md) | 明确的建筑组成关系按正常结构处理 |
| [ADR-0035](docs/adr/0035-warn-only-on-clear-cross-category-collisions.md) | 仅对明确的跨类别位置冲突发出提醒 |
| [ADR-0036](docs/adr/0036-no-road-connectivity-warnings.md) | V2 不检查道路连通性 |
| [ADR-0037](docs/adr/0037-s1-presentation-only-shell.md) | S1 只承担呈现，不承担业务协调 |
| [ADR-0038](docs/adr/0038-no-amap-offline-map-in-v2.md) | V2 不提供高德离线地图 |
| [ADR-0039](docs/adr/0039-non-shell-application-flow-modules.md) | 跨功能完整操作使用非 S1 应用流程模块 |
| [ADR-0040](docs/adr/0040-review-candidate-projection-and-eligibility.md) | 原始观测与评审候选分层，以候选资格控制评审和导出 |
| [ADR-0041](docs/adr/0041-boundary-only-export-and-default-north.md) | 方案边界是唯一导出前置，未设置朝向时默认地图正北 |
| [ADR-0042](docs/adr/0042-export-flow-boundary-only-application-flow.md) | 边界直出由独立应用流程完整负责 |
| [ADR-0043](docs/adr/0043-enhanced-export-application-flow.md) | 增强导出由独立应用流程完整负责 |
| [ADR-0044](docs/adr/0044-global-navigation-and-top-toolbar.md) | 全局返回历史栈与顶部工具栏布局 |
| [ADR-0045](docs/adr/0045-one-continuous-map-session.md) | 一个方案只使用一段连续地图会话 |

## 版本历史

- **V2.1.0（2026-08-20）**：评审台置信度分档（高/中/低）与一键应用建议；第五步整校园
  3D 方块预览；采集步地图误导提示修复；V2.1 版本收口（完整门禁与证据归档见
  `docs/developer-guide/v2.1-release-closeout-evidence.md`）。
- **V2.0.0（2026-08-17）**：正式版——五步工作区、OSM 边界、六类候选采集、三态评审、
  增强导出与 `.schem` 交付。
- **V1.x**：v1.0.0 / v1.0.1（历史版本，GitHub Release 可下载）。
