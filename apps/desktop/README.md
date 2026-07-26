# T19 — S1 主程序应用壳（Phase1 最小壳）

## 架构原则

### 依赖白名单 (ADR-0017/0025)

**允许依赖**（壳的上限）: `slint`（唯一合法使用者）、F1-F9 功能模块、B1-B7/B9-B11/B17 基础模块。

**Phase1 实际依赖**（本 crate Cargo.toml 即白名单）:
- `slint` — UI 框架（官方默认 features：winit 后端 + femtovg/软件渲染器）
- `localization` (B6) — 全部文案经 `l10n.t()` 注入（ADR-0005 文本外置铁律）
- `global-settings` (F1) — 首次运行判定 + 老用户着陆校区
- `data-persistence` (B2) — 仅打开数据库交给 F1，不直接碰 SQL
- `anyhow` — 错误链路

**绝对禁止依赖**: B12-B16（ETL/GIS 专属模块必须经功能模块中转，xtask arch 强制）。

### 薄壳原则

1. **零业务逻辑**: Slint 声明层只做展示和事件分发
2. **ViewModel 绑定**: 向各功能模块索取 ViewModel 状态和操作回调（全量接线在 T19B）
3. **横向隔离**: 所有功能模块间协作一律通过壳接线，互不直接调用
4. **文案外置**: 所有文本走 B6 国际化（`l10n.t("xxx")` + zh-CN.json）

## 目录结构

```
apps/desktop/
├── Cargo.toml                 # 依赖段即白名单（受 CODEOWNERS 守卫）
├── build.rs                   # slint-build 编译 ui/main.slint
├── ui/
│   └── main.slint             # 根窗口（零硬编码文案，属性由 Rust 侧填充）
└── src/
    ├── lib.rs                 # include_modules! + 运行时导出
    ├── runtime.rs             # 首开着陆判定 + 主窗口装配
    └── main.rs                # campus-tool-dev 入口
```

## 首开着陆判定（Phase1 已实现，`runtime::landing_decision`）

1. 首次运行（或本地库尚不可用）→ 设置向导去向
2. 老用户且 F1 记有上次校区 → 直达该校区去向
3. 否则（含上次校区已被删，F1 兜底为 `None`）→ 校区选择去向

Phase1 窗口只把去向翻译成 l10n 状态文案展示；页面级导航与
ViewModel 全量接线是 T19B 的接线债务。

## 开发版快捷方式

由 `cargo xtask dev-shortcut` 承担（ADR-0014）：build.rs 运行于链接之前
拿不到 exe，不能在 build.rs 里做快捷方式。

## 接线债务清单 (T19B)

1. F1-F9 ViewModel 全量接线与页面级导航（设置向导 / 校区搜索 / 方案列表 / 评审台 / 导出）
2. F4 采集页接入 F7 "采集报告"入口（`audit.report_entry` + `AuditReportView`）
3. 设置页"重新查看教程"按钮（F2 重置接口）与教程三个里程碑钩子
4. public-api 快照 + CODEOWNERS 守卫 .lnk 相关文件

---

**验收标准**: 负责人可双击桌面"校园复刻工具 - 开发版"快速预览最新构建，端到端流程四步打通。
