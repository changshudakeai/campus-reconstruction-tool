# MCRebuild V2 —— 校园复刻工具（重写版）

V2 是对 v1.x 的彻底重写，目标：**轻便、高效、好维护**。

当前阶段：**决策讨论期**。产品与架构决策通过访谈逐条确认并记录为
ADR（决策记录），代码实施在决策定案后才开始。

## 已确认的决策（docs/adr/）

| 编号 | 决定 |
|------|------|
| [ADR-0001](docs/adr/0001-modular-monolith-architecture.md) | 模块化单体：一个安装包、一个主程序，内部按业务能力分独立模块 |
| [ADR-0002](docs/adr/0002-sqlite-project-storage.md) | 项目数据存储采用 SQLite（事务安全、可查询、保存快） |
| [ADR-0003](docs/adr/0003-rust-slint-stack-reuse-core.md) | 沿用 Rust + Slint；复用 v1.x 导出引擎/Arnis 规则/地图逻辑；动画与主题系统为正式需求 |
| [ADR-0004](docs/adr/0004-app-level-global-settings.md) | 语言与 Minecraft 版本为应用级全局设置；首次设置页兼任知情告知 |
| [ADR-0005](docs/adr/0005-externalized-ui-text-i18n-ready.md) | 界面文本一律外置资源文件，禁止硬编码，多语言就绪 |
| [ADR-0006](docs/adr/0006-landing-on-plan-list.md) | 老用户二次启动着陆于方案列表页 |
| [ADR-0007](docs/adr/0007-boundary-belongs-to-plan.md) | 校区边界属于方案级数据 |
| [ADR-0008](docs/adr/0008-campus-selected-via-gaode-search.md) | 通过高德搜索选定校区，而非手动创建 |
| [ADR-0009](docs/adr/0009-no-escape-hatch-reserve-generalization.md) | 无逃生通道；但为“复刻任意真实世界区域”预留扩展点 |
| [ADR-0010](docs/adr/0010-lightweight-plan-creation.md) | 轻创建：先有方案再补充信息 |
| [ADR-0011](docs/adr/0011-exclusive-category-tag-mapping.md) | 六类数据互斥分类——物理形态铁律 + 标签映射实现 |
| [ADR-0012](docs/adr/0012-optional-collection-minimal-export.md) | 采集非强制——最小路径为“边界→朝向→导出地基” |
| [ADR-0013](docs/adr/0013-pluggable-data-sources-with-recommendation.md) | 数据源可插拔——以采集推荐优先，非强制采集 |
| [ADR-0014](docs/adr/0014-preview-through-desktop-shortcut.md) | 日常预览通过桌面快捷方式——固定入口“校园复刻工具 - 开发版” |
| [ADR-0015](docs/adr/0015-version-naming-and-scope.md) | 版本命名与范围——v2.0.0 不包含单栋精修 |
| [ADR-0016](docs/adr/0016-review-process-design.md) | 评审交互设计——无卡顿模式 + A3 混合批量操作 |
| [ADR-0017](docs/adr/0017-modular-architecture-and-crate-catalog.md) | 模块化单体架构与 crate 目录清单（29 个模块）+ 模块化十戒硬规则 |
| [ADR-0018](docs/adr/0018-plan-list-page-design.md) | 方案列表页——卡片三件套（名称/进度/时间）、最近优先、三操作菜单、无搜索 |
| [ADR-0019](docs/adr/0019-coverage-audit-quiet-sentinel.md) | 采集覆盖体检——安静哨兵，纯数据依据，两条疑点规则 |
| [ADR-0020](docs/adr/0020-onboarding-follow-along-guidance.md) | 新手教程——跟练式气泡引导，四条规矩，实施排在界面定稿之后 |
| [ADR-0021](docs/adr/0021-notification-center-popup-rule.md) | 通知中心——轻公告栏全应用一本账 + 弹窗铁律（要紧错误禁用横幅） |
| [ADR-0022](docs/adr/0022-three-state-review-undo-deferred.md) | 三态评审（待定/保留/剔除）+ 撤销暂缓（保留席位与接口）+ 导出封账弹窗 |
| [ADR-0023](docs/adr/0023-theming-color-cards-motion-table.md) | 主题与动画——亮暗双色卡 + 动效表 + Codex 手感基准 + 减少动画开关 |
| [ADR-0024](docs/adr/0024-export-initial-campus-generation-engine.md) | 导出成品——Arnis 式完整初始校园（.schem）+ 新立 B18 生成引擎 + 用料表版本绑定
| [ADR-0025](docs/adr/0025-shell-can-depend-on-domain-types.md) | Shell 层可依赖 domain types
| [ADR-0026](docs/adr/0026-material-table-in-b17.md) | 用料配置表与版本校验器同居 B17
| [ADR-0027](docs/adr/0027-stepper-wizard-navigation.md) | 步骤条向导导航设计
| [ADR-0028](docs/adr/0028-tutorial-bubble-list.md) | 新手教程气泡列表机制
| [ADR-0029](docs/adr/0029-boundary-from-osm-with-manual-adjustment.md) | 边界从 OSM 自动获取 + 人工调整
| [ADR-0037](docs/adr/0037-s1-presentation-only-shell.md) | S1 只承担呈现，不承担业务协调
| [ADR-0038](docs/adr/0038-no-amap-offline-map-in-v2.md) | V2 不提供高德离线地图，不抓取或缓存高德地图内容

## 已确认的产品流程（持续补充中）

全部已定决策的**按模块归类视图**：[docs/module-decisions.md](docs/module-decisions.md)（出工单时以此为索引）。

首次打开：基本设置（语言、游戏版本）→ 选择/创建校区 → 校区下的方案列表 → 进入方案。

层级关系：

```
应用（全局设置：语言、游戏版本）
 └─ 校区（共享知识：建筑名称等）
     ├─ 方案 A
     └─ 方案 B
```

## 目录说明

```
New-branch-v2/
├── AGENTS.md            Agent 协作规则（开工前必读顺序在此）
├── CONTEXT.md           领域术语表（42 个中英对照术语，引用术语以它为准）
├── docs/adr/            决策记录 ADR-0001~0038（本项目的"为什么"）
├── docs/agents/         Agent 技能配置（issue-tracker / triage-labels / domain）
├── docs/research/       深度技术研究（地图集成、模块边界执法等）
├── docs/module-decisions.md  ADR 按模块归类索引
├── .scratch/            工单与 handoff 文档（本地 markdown tracker）
├── sqlite/schemas/      数据库设计草案（随决策扩充）
├── Cargo.toml           Rust workspace 根清单（依赖白名单 + lint 门禁）
└── apps/ core/ xtask/   30 个 crate（1 薄壳 + 7 功能 + 17 基础 + 构建自动化）
```

## 尚未决定（禁止抢跑实施）

- 校区级共享知识与方案级数据的完整清单
- v1.x 项目数据的迁移策略与排期
- F1 全局设置扩展点、F4 增量刷新交互（P2，见 docs/module-decisions.md 待访谈队列）
- 界面定稿后的开发版审核项：教程气泡位置、动画舞步、快捷键清单、用料效果
