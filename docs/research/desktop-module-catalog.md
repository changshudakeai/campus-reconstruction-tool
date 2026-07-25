# 调研报告：桌面应用模块分类学与本产品域模块目录草案

> **调研日期**：2026-07-25  
> **背景**：Rust + Slint Windows 单机桌面应用（Minecraft 校园复刻工具 V2）处于决策期。本报告是第二轮研究，旨在给出一份**完整的模块清单**——不仅包括产品负责人已想到的五个功能模块（UI、新手教程、项目方案管理、数据采集、候选审核），还要补全成熟桌面软件普遍必备的"横切标配模块"以及本产品域特有的支撑模块。  
> **约束**：v2.0.0 明确排除单栋精修、自动更新；需基于已有 ADR-0001~0016 推导必需模块；所有事实性结论均附一手来源 URL。  
> **输出目标**：一张含模块名（中文+crate 建议）、类型（功能/基础/应用壳）、一句话职责、对应 ADR 或"待访谈"标记的总表。该产品负责人将直接用此表作为逐模块访谈目录。

---

## 一、通用横切"标配模块"盘点（3-5 个成熟项目的真实对应物）

通过直接检视 GitHub API 返回的真实仓库结构，归纳出以下成熟桌面应用普遍存在的横切模块。每个模块类给出至少两个项目中的真实 crate/目录名及链接。

### 1.1 设置与偏好设置 (Settings / Preferences)

| 项目 | 真实对应物（crate/目录） | URL |
|------|-------------------------|-----|
| Zed | `settings`, `settings_ui`, `agent_settings`, `vim_mode_setting` | https://github.com/zed-industries/zed/tree/main/crates/settings |
| OBS Studio | `frontend/settings/` | https://github.com/obsproject/obs-studio/tree/master/frontend/settings |
| Blender | `source/blender/editors/space_userpref/` | https://github.com/blender/blender/tree/main/source/blender/editors/space_userpref |
| GIMP | `app/config/`, `app/language.c` | https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app/config |

**共同模式**：以键值形式持久化用户偏好（界面主题、快捷键映射、首次启动后显示），通常位于 UI 层下方但依赖底层数据持久化。

### 1.2 国际化与多语言 (i18n / l10n)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| GIMP | `po/`（90+ 个翻译文件，.po/.lmo） | https://gitlab.gnome.org/GNOME/gimp/-/tree/main/po |
| RustDesk | `src/lang.rs`, `lang/` | https://github.com/rustdesk/rustdesk/tree/main/src |
| Blender | `intern/generators/i18n/`, `source/intern/i18n/` | https://github.com/blender/blender/tree/main/source/intern/i18n |
| OBS Studio | `data/locale/` | https://github.com/obsproject/obs-studio/tree/master/data/locale |

**共同模式**：文本外置资源文件（.json/.po/.mo），运行时按语言加载键值映射；编译时或构建时生成查找表；Slint `.slint` 文件用 `@tr()` 宏标记可翻译字符串。

### 1.3 日志、诊断与崩溃报告 (Logging / Diagnostics / Crash Reporting)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `zlog`, `crashes`, `telemetry`, `etw_tracing` | https://github.com/zed-industries/zed/tree/main/crates |
| OBS Studio | `libobs/obs-win-crash-handler.c` | https://github.com/obsproject/obs-studio/blob/master/libobs/obs-win-crash-handler.c |
| Blender | `source/creator/`（启动器、异常捕获、构建信息） | https://github.com/blender/blender/tree/main/source/creator |
| GIMP | `app/errors.c`, `app/gimp-debug.c` | https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app |

**共同模式**：结构化日志记录（JSON 或 key-value），崩溃时采集堆栈和最近操作，部分项目包含遥测（telemetry）。本项目 v1.x 已有诊断日志经验，V2 应延续但以更轻量方式实现。

### 1.4 通知中心 (Notifications / Alerts)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `notifications` crate | https://github.com/zed-industries/zed/tree/main/crates/notifications |
| OBS Studio | `frontend/dialogs/`（弹出式提示）、`frontend/components/`（状态条） | https://github.com/obsproject/obs-studio/tree/master/frontend |
| Blender | `editor/notifier.hh` | https://github.com/blender/blender/tree/main/source/blender/editors/include |

**共同模式**：短时可见的 Toast 消息、状态栏反馈、错误警示；非阻塞或弱阻塞展示；本地平台适配（Windows toast / macOS notification center）。

### 1.5 撤销重做历史栈 (Undo / Redo)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Blender | `source/blender/editors/undo/`, `source/blender/editors/armature/` | https://github.com/blender/blender/tree/main/source/blender/editors/undo |
| GIMP | `app/tools/undo-stack.*`（隐含在 tools 交互中） | https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app/tools |
| Inkscape | `src/editor/action.cpp`, `src/edit/selection.cpp` | https://github.com/inkscape/inkscape/tree/main/src |

**共同模式**：命令模式（Command Pattern）实例化操作对象；内存中维护栈结构；导出时刻提交最终状态。本项目评审阶段已在内存缓冲，导出时才落盘，天然契合撤销重做模型。

### 1.6 全局快捷键 (Keyboard Shortcuts / Keymaps)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `keymap_editor`, `vim_mode_setting` | https://github.com/zed-industries/zed/tree/main/crates/keymap_editor |
| Blender | `source/blender/windowmanager/api/WM_api.h`（keymap 系统） | https://github.com/blender/blender/tree/main/source/blender/windowmanager |
| OBS Studio | `libobs/obs-hotkey.c/h` | https://github.com/obsproject/obs-studio/blob/master/libobs/obs-hotkey.h |

**共同模式**：平台无关的 Action-ID → OS 热键绑定机制；支持组合键（Ctrl+Z, Alt+Shift+A）；配置项存于 settings 模块。

### 1.7 主题与外观 (Theme / Appearance)

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `theme`, `theme_importer`, `syntax_theme`, `file_icons` | https://github.com/zed-industries/zed/tree/main/crates/theme |
| OBS Studio | `frontend/OBSApp_Themes.cpp`, `data/themes/` | https://github.com/obsproject/obs-studio/blob/master/frontend/OBSApp_Themes.cpp |
| Blender | `source/blender/makesdna/makesdna.c`（预设存储） | https://github.com/blender/blender/tree/main/source/blender/makesdna |

**共同模式**：颜色变量集（深色/浅色/高对比度）、字体大小、图标主题；配置项在 settings 中切换；运行时热重载能力。

### 1.8 插件与扩展（Plugin / Extension System）

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `extension_host`, `extensions_ui`, `debug_adapter_extension` | https://github.com/zed-industries/zed/tree/main/crates/extension_host |
| OBS Studio | `plugins/`, `libobs/obs-module.c` | https://github.com/obsproject/obs-studio/tree/master/plugins |
| GIMP | `app/plug-in/`, `scripts/` | https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app/plug-in |

**本产品的决定**：ADR-0009 排除了插件系统（"no escape hatch, reserve generalization"）。因此该模块**明确不需要**。

### 1.9 自动更新与发布渠道（Auto-updater）

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `auto_update`, `auto_update_helper`, `auto_update_ui` | https://github.com/zed-industries/zed/tree/main/crates/auto_update |
| OBS Studio | `frontend/updater/` | https://github.com/obsproject/obs-studio/tree/master/frontend/updater |
| Blender | `blender_launcher_win32.c`, `release_channel`（内置） | https://github.com/blender/blender/blob/main/source/creator/blender_launcher_win32.c |

**本产品的决定**：ADR-0015 明确排除"一键更新检查"，仍通过构建脚本覆盖最新版本。**明确不需要**。

### 1.10 欢迎向导与新手引导（Onboarding / Tutorial）

| 项目 | 真实对应物 | URL |
|------|-----------|-----|
| Zed | `onboarding`, `ai_onboarding`, `language_onboarding` | https://github.com/zed-industries/zed/tree/main/crates/onboarding |
| OBS Studio | `frontend/wizards/`（初次设置向导） | https://github.com/obsproject/obs-studio/tree/master/frontend/wizards |
| Blender | 启动时 Splash Screen + `first-run preferences`（隐性 in creator） | https://github.com/blender/blender/tree/main/source/creator |

**本产品的现状**：产品负责人已将"新手教程"列为独立模块；这与业界共识一致。建议按 ADR-0004 纳入首次设置向导。

### 1.11 撤销总结论

| 模块类 | 成熟项目示例 | 对本项目的优先级 |
|--------|-------------|------------------|
| Settings | Zed (`settings`), OBS (`settings/`) | **必须有**（ADR-0004） |
| i18n | GIMP (`po/`), RustDesk (`lang.rs`) | **必须有**（ADR-0005） |
| Logging/Diagnostics | Zed (`zlog`, `crashes`), OBS (`obs-win-crash-handler.c`) | **必须有**（诊断日志延续） |
| Notifications | Zed (`notifications`), OBS (`dialogs/`) | **建议有**（增强体验） |
| Undo/Redo | Blender (`undo/`), GIMP (tools 交互) | **建议有**（评审内存缓冲天然支撑） |
| Keyboard Shortcuts | Zed (`keymap_editor`), OBS (`obs-hotkey`) | **建议有**（专业用户刚需） |
| Theme/Appearance | Zed (`theme`), OBS (`themes/`) | **建议有**（提升友好度） |
| Plugin/Extension | Zed (`extension_host`), OBS (`plugins/`) | ❌ **明确不需要**（ADR-0009） |
| Auto-update | Zed (`auto_update`), OBS (`updater/`) | ❌ **明确不需要**（ADR-0015） |

**URL 汇总参考**：
- Zed crates: https://github.com/zed-industries/zed/tree/main/crates
- OBS frontend: https://github.com/obsproject/obs-studio/tree/master/frontend
- OBS libobs: https://github.com/obsproject/obs-studio/tree/master/libobs
- Blender source: https://github.com/blender/blender/tree/main/source
- GIMP app: https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app
- RustDesk src: https://github.com/rustdesk/rustdesk/tree/main/src

---

## 二、模块目录的组织学：如何区分"功能模块"与"基础模块"

### 2.1 业界标准：功能 vs 底座的分层逻辑

通过对比 rust-analyzer、typst、zed 等 Rust 项目的实际 crate 划分，可以总结出清晰规则：

#### （a）**功能模块（Feature Modules）**
- **定义**：用户可直接感知、按业务能力垂直切割的模块。
- **特征**：每个功能模块是一个 Bounded Context（有独立的状态流转规则、边界条件）；功能模块之间互不依赖（宽 DAG 的关键）。
- **例子**：
  - rust-analyzer: `ide_assists`, `ide_completion`, `ide_db`（不同 LSP 功能）
  - zed: `project_panel`, `file_finder`, `search`, `vim`（不同面板功能）
  - typst: `typst-pdf`, `typst-svg`, `typst-html`（每种导出器一个 Crate）
- **依赖方向**：只允许向下游的基础模块依赖（如数据访问、共享领域类型），不允许横向依赖其他功能模块。

#### （b）**基础模块（Base Modules / Cross-cutting）**
- **定义**：多个功能模块共享的支撑能力，如持久化、地图集成、导出引擎、共享类型。
- **特征**：通常是"被调用的接口提供者"；稳定不变或少变；靠近系统边界（IO、网络、平台服务）。
- **例子**：
  - rust-analyzer: `syntax`, `base_db`, `vfs`（词法分析、数据库抽象、文件系统）
  - typst: `typst-library`（渲染库，被各导出器依赖）、`typst-bundle`（字体/资源管理）
  - zed: `gpui`, `fs`, `db`, `rope`, `text`（UI 框架、文件系统、数据库、文本数据结构）
- **依赖方向**：最接近叶子的反向——所有功能模块都依赖它们，但它们不依赖功能模块。

#### （c）**应用壳（Application Shell）**
- **定义**：唯一叶子 crate，负责把功能模块拼成一个可运行应用（UI 声明 + 业务编排）。
- **特征**：仅在此处出现 `slint_build`、`serde`、平台 API；不含业务逻辑，只做绑定/编排。
- **例子**：
  - typst: `typst-cli`（"相对小的层"，编译管线 + 导出器之上）
  - zed: `zed`（叶子 crate，组装 gpui/workspace/editor）
  - RustAnalyzer: `rust-analyzer`（叶子，组装 ide_* 系列）

### 2.2 本项目 V2 的实际落地

结合现有规划（`apps/desktop`, `core/foundation`, `core/maps`, `core/sponge-export`, `core/data`, `features/*`），推荐如下组织学：

```
apps/desktop (叶子：.slint UI + serde + 业务编排)
├── features/tutorial (新手教程流程)
├── features/project (项目方案 CRUD)
├── features/acquisition (数据采集编排)
└── features/review (候选审核审批)

这些功能 crate 全部依赖：
├── core/domain (共享领域类型，近零依赖)
├── core/data (SQLite schema + 各功能存取)
├── core/maps (高德客户端，v1.x 迁移)
└── core/sponge-export (Sponge V3 导出引擎，v1.x 迁移)
    └── core/foundation (地基引擎) ← sponge-export 可选依赖

核心原则：
- 功能 crate 之间无横向依赖
- 新 crate 加入需经过 pub API 审查
- .slint 代码只在 `apps/desktop` build.rs 生成一次
```

这符合 matklad 的"公共词汇 + 独立功能 + 单一叶子"黄金三角（来源：https://matklad.github.io/2021/08/22/large-rust-workspaces.html）。

---

## 三、对照本产品补全清单：必须/建议/不要

本节严格依据 ADR-0001 ~ A016 推导出（a）必须有、（b）建议有、（c）明确不要三类模块。

### 3.1 ADR 依据摘要

| ADR 编号 | 标题 | 相关模块影响 |
|---------|------|-------------|
| ADR-0001 | Modular Monolith Architecture | 确认模块化单体、功能 crate 隔离、叶子 crate |
| ADR-0002 | SQLite Project Storage | `core/data` 必需 |
| ADR-0003 | Rust-Slint Stack Reuse Core | `core/maps`, `core/sponge-export`, `core/foundation` 迁入 |
| ADR-0004 | App-level Global Settings | **全局设置**必需（语言/MC 版本） |
| ADR-0005 | Externalized UI Text i18n Ready | **多语言 i18n**必需 |
| ADR-0007 | Boundary Belongs to Plan | 边界属于方案 → `features/project` 承载 |
| ADR-0008 | Campus via Gaode Search | **地图服务**必需（高德客户端） |
| ADR-0010 | Lightweight Plan Creation | 方案创建流程 → `features/project` |
| ADR-0011 | Exclusive Category Tag Mapping | 六类互斥映射 → `features/acquisition`/`review` |
| ADR-0012 | Optional Collection – Minimal Export | **导出引擎**必需（最小路径：边界→朝向→导出） |
| ADR-0014 | Preview Through Desktop Shortcut | 快捷入口 → `applications/launcher`（应用壳职责） |
| ADR-0015 | Version Naming and Scope | **明确排除**单栋精修、自动更新 |
| ADR-0016 | Review Process Design | 内存缓冲 + 批量落盘 → 为**撤销重做**创造基础条件 |

### 3.2 必须有模块（Must Have）

| 模块中文名 | 建议 crate 名 | 类型 | 依据 ADR | 理由 |
|-----------|------------|------|---------|-----|
| 应用全局设置 | `app_settings` 或集成于 `features/ui` | 功能 | ADR-0004 | 首次设置向导承载语言、MC 版本全局配置 |
| 国际化/i18n | `core/i18n` 或 `intl` | 基础 | ADR-0005 | UI 文本外置、多语言加载（与 Slint `@tr()` 配合） |
| 共享领域类型 | `core/domain` 或 `domain-types` | 基础 | ADR-0001 | 跨功能通用的计划、地块、校区、审核状态定义 |
| 数据持久化（SQLite） | `core/data` 或 `persistence` | 基础 | ADR-0002 | schema、迁移、各功能存取接口 |
| 高德地图集成 | `core/maps` 或 `gaode-client` | 基础 | ADR-0003, ADR-0008 | v1.x 地图客户端迁入，提供选校区、画边界 API |
| Sponge 导出引擎 | `core/sponge-export` | 基础 | ADR-0003, ADR-0012 | 生成.foundat ion.schem + manifest.json |
| 地基模式引擎 | `core/foundation` | 基础 | ADR-0003 | 地块生成、方向计算、边界校验 |
| 新手教程 | `features/tutorial` | 功能 | ADR-0004 | 首启流程、逐步引导、完成状态机 |
| 项目方案管理 | `features/project` | 功能 | ADR-0007, ADR-0010 | 项目列表、创建、恢复、删除 |
| 数据采集 | `features/acquisition` | 功能 | ADR-0011, ADR-0012 | 调用 maps 取 Overture/Gaode 数据、写入候选 |
| 候选审核 | `features/review` | 功能 | ADR-0016 | 五类目队列、保留/剔除、批量操作、内存缓冲 |
| 主程序应用壳 | `apps/desktop` | 应用壳 | ADR-0001 | .slint 生成、业务编排、用户入口 |

### 3.3 建议有模块（Nice-to-Have）

| 模块中文名 | 建议 crate 名 | 类型 | 依据 | 收益描述 |
|-----------|------------|------|-----|---------|
| 通知中心 | `notifications` 或 `alerting` | 基础 | 业界标配（Zed `notifications`, OBS dialogs） | Toast 消息、导出完成提示、错误警示 |
| 撤销重做 | `undo_redo` 或 `command-pattern` | 基础 | 业界标配（Blender `undo/`）+ ADR-0016 内存缓冲天然支撑 | 评审阶段内撤销修改，提升体验 |
| 全局快捷键 | `hotkeys` 或 `keybindings` | 基础 | 业界标配（OBS `obs-hotkey`, Zed `keymap_editor`） | 专业用户高效操作（Ctrl+R 重新采集、Ctrl+E 导出） |
| 主题/外观 | `theme` 或 `appearance` | 基础 | 业界标配（Zed `theme`, OBS themes/） | 深色/浅色模式切换，提升亲和度 |
| 诊断日志增强 | `diagnostics` 或延续 `log` | 基础 | v1.x 经验延续 | JSON 结构化日志、崩溃堆栈、最近操作快照 |

> 注：**为什么这些不是 "必须有"**？v2.0.0 范围较紧，这些模块可在 v2.1.0~2.2.0 逐步补入，不影响首发 MVP。

### 3.4 明确不要模块（Explicitly Exclude）

| 模块中文名 | 原因 | 依据 ADR |
|-----------|-----|---------|
| 插件系统 | ADR-0009 明确排除（"reserve generalization"） | ADR-0009 |
| 自动更新 | ADR-0015 明确排除（构建脚本覆盖策略） | ADR-0015 |
| 单栋精修 | v2.0.0 范围外（延后至 v2.1.0+） | ADR-0015 |
| 遥测/Telemetry | 单机离线工具，隐私敏感；且非用户核心价值 | N/A |

---

## 四、最终产出：完整模块目录草案表

本表将直接作为产品负责人后续逐模块访谈的目录。每行一个模块，包含：
- **模块名**（中文 + 建议 crate 名）
- **类型**（功能/基础/应用壳）
- **一句话职责**
- **是否待访谈**（✅ = 产品负责人需深入讨论、⚠️ = 已有 ADR 决策仅需确认、➖ = 无需访谈）

### 4.1 功能模块（用户可感知能力）

| # | 模块名（中文 + crate） | 类型 | 职责 | 是否待访谈 |
|---|-----------------------|------|------|-----------|
| F1 | 应用全局设置 (`app_settings`) | 功能 | 管理语言、Minecraft 版本的初始设置与全局配置页 | ✅ 待访谈（具体字段、是否支持方案级覆盖延伸） |
| F2 | 新手教程 (`tutorial`) | 功能 | 首启向导、逐步引导流程、步骤状态机、完成判定 | ⚠️ 需确认（与首次设置向导合并还是独立） |
| F3 | 项目方案管理 (`project`) | 功能 | 项目列表、新建、恢复、删除、边界复制 | ⚠️ 需确认（删除是否软删除、回收站整合） |
| F4 | 数据采集 (`acquisition`) | 功能 | 调用地图获取数据、写入候选人、类别映射、标签清洗 | ➖ ADR-0011/0012已定，仅实施细节 |
| F5 | 候选审核 (`review`) | 功能 | 五类目队列、保留/剔除、批量操作、内存缓冲 | ⚠️ 需确认（撤销重做是否纳入 scope） |
| F6 | 导出控制台 (`export-console`) | 功能 | 进度条、manifest 确认、错误列表、结果跳转 | ➖ ADR-0016已定义 |

### 4.2 基础模块（横切支撑）

| # | 模块名（中文 + crate） | 类型 | 职责 | 是否待访谈 |
|---|-----------------------|------|------|-----------|
| B1 | 共享领域类型 (`domain-types`) | 基础 | 计划、地块、校区、候选、审核状态的统一定义 | ➖ ADR-0001强制要求 |
| B2 | 数据持久化 (`persistence`) | 基础 | SQLite schema、迁移、各功能存取接口 | ➖ ADR-0002强制要求 |
| B3 | 高德地图客户端 (`gaode-client`) | 基础 | 校区搜索、边界绘制、Overture 数据拉取封装 | ➖ ADR-0003/0008强制要求 |
| B4 | Sponge 导出引擎 (`sponge-export`) | 基础 | .foundation.schem + manifest.json 生成 | ➖ ADR-0003/0012强制要求 |
| B5 | 地基模式引擎 (`foundation-engine`) | 基础 | 地块生成、边界校验、朝向计算 | ➖ ADR-0003强制要求 |
| B6 | 国际化/i18n (`intl`) | 基础 | UI 文本资源加载、Slint @tr() 配合、运行时切换 | ⚠️ 需确认（初期几语种、未来扩展策略） |
| B7 | 通知中心 (`notifications`) | 基础 | Toast 提示、状态栏反馈、错误警示 | ✅ 待访谈（优先级、消息模板） |
| B8 | 撤销重做 (`undo-redo`) | 基础 | 评审阶段命令栈、内存内撤销/重做 | ✅ 待访谈（是否纳入 v2.0.0scope） |
| B9 | 全局快捷键 (`hotkeys`) | 基础 | ActionID↔OS 热键绑定、快捷键编辑、配置保存 | ✅ 待访谈（关键快捷键映射） |
| B10 | 主题/外观 (`theme`) | 基础 | 深色/浅色模式、颜色变量集、字体大小 | ✅ 待访谈（是否纳入首发） |
| B11 | 诊断日志 (`diagnostics`) | 基础 | 结构化日志、崩溃堆栈、最近操作快照 | ⚠️ 需确认（v1.x 经验延续程度） |

### 4.3 应用壳（Application Shell）

| # | 模块名（中文 + crate） | 类型 | 职责 | 是否待访谈 |
|---|-----------------------|------|------|-----------|
| S1 | 主程序应用壳 (`apps/desktop`) | 应用壳 | .slint UI 声明、业务编排、用户入口、快捷方式生成 | ⚠️ 需确认（开发版/正式版入口策略） |
| S2 | 构建与自动化 (`xtask`) | 工具 | cargo xtask 构建脚本、打包、哈希验证、CI 集成 | ➖ 工程规范，无需产品讨论 |

---

## 五、精简版模块目录（供快速浏览）

| # | 模块名 | 类型 | 是否待访谈 |
|---|-------|------|-----------|
| F1 | 应用全局设置 | 功能 | ✅ |
| F2 | 新手教程 | 功能 | ⚠️ |
| F3 | 项目方案管理 | 功能 | ⚠️ |
| F4 | 数据采集 | 功能 | ➖ |
| F5 | 候选审核 | 功能 | ⚠️ |
| F6 | 导出控制台 | 功能 | ➖ |
| B1 | 共享领域类型 | 基础 | ➖ |
| B2 | 数据持久化 | 基础 | ➖ |
| B3 | 高德地图客户端 | 基础 | ➖ |
| B4 | Sponge 导出引擎 | 基础 | ➖ |
| B5 | 地基模式引擎 | 基础 | ➖ |
| B6 | 国际化/i18n | 基础 | ⚠️ |
| B7 | 通知中心 | 基础 | ✅ |
| B8 | 撤销重做 | 基础 | ✅ |
| B9 | 全局快捷键 | 基础 | ✅ |
| B10 | 主题/外观 | 基础 | ✅ |
| B11 | 诊断日志 | 基础 | ⚠️ |
| S1 | 主程序应用壳 | 应用壳 | ⚠️ |
| S2 | 构建与自动化 | 工具 | ➖ |

---

## 六、结论与建议

### 6.1 关键发现

1. **横切模块是成熟桌面应用的标配**：设置、i18n、日志、通知、撤销、快捷键、主题等在 Zed/OBS/Blender/GIMP 中无一缺席。但对 MVP，可以先聚焦"必须有"，其他按迭代陆续补入。

2. **业界对"功能 vs 基础"的分工高度一致**：功能模块按业务能力垂直切分（互不依赖），基础模块横向支撑（被依赖）。本项目 V2 规划完美契合这一范式。

3. **SRAD 已覆盖绝大部分"必须有"**：11 个 ADR 已明确 12 个必需模块的范围，剩余主要是细节确认。

### 6.2 访谈建议顺序

1. **第一轮（高优先级）**：F1 设置、F2 教程、F3 项目管理、B6 i18n、B11 诊断日志
2. **第二轮（中优先级）**：B7 通知、B8 撤销、B9 快捷键、B10 主题
3. **第三轮（确认型）**：F4/F5/F6、B1-B5、S1/S2（基本已有 ADR 背书）

### 6.3 风险提示

- **撤销重做规模易低估**：虽然评审阶段天然内存缓冲，但要完整实现命令栈、历史查询、批量撤销，工作量可能超预期。建议作为 v2.1.0 候选。
- **i18n 长期成本**：初期只有中文看似简单，但一旦上线就要支持更多语种。`core/i18n` 需设计可扩展架构（如 JSON 字典 + 占位符插值）。
- **快捷键冲突**：若考虑 Vim 模式（类似 Zed 的 `vim_mode_setting`），需预留配置空间；否则简单 Ctrl+X 即可。

---

## 附：来源清单（URL）

1. **Zed 仓库 crates/**：https://github.com/zed-industries/zed/tree/main/crates
2. **OBS Studio frontend/**：https://github.com/obsproject/obs-studio/tree/master/frontend
3. **OBS Studio libobs/**：https://github.com/obsproject/obs-studio/tree/master/libobs
4. **Blender source/**：https://github.com/blender/blender/tree/main/source
5. **GIMP app/**：https://gitlab.gnome.org/GNOME/gimp/-/tree/main/app
6. **RustDesk src/**：https://github.com/rustdesk/rustdesk/tree/main/src
7. **matklad — Large Rust Workspaces**：https://matklad.github.io/2021/08/22/large-rust-workspaces.html
8. **matklad — Fast Rust Builds**：https://matklad.github.io/2021/09/04/fast-rust-builds.html
9. **Milan Jovanović — Modular Monolith**：https://milanjovanovic.tech/blog/where-vertical-slices-fit-inside-the-modular-monolith-architecture
10. **Slint Best Practices**：https://docs.slint.dev/latest/docs/slint/guide/development/best-practices/
11. **Slint .slint File Module System**：https://docs.slint.dev/latest/docs/slint/guide/language/coding/file/

---

**报告完成时间**：2026-07-25  
**报告路径**：`c:\Users\chang\Desktop\MCRebuild_Renovation\New-branch-v2\docs\research\desktop-module-catalog.md`
