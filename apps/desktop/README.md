# T19 — S1 主程序应用壳（薄壳 UI + 开发版快捷方式）

## 架构原则

### 依赖白名单 (ADR-0017/0025)

**允许依赖**:
- `slint` - UI 框架 (唯一外部依赖)
- F1-F9 功能模块 (`global-settings`, `onboarding-tutorial`, `project-management`, `data-acquisition`, `review-workbench`, `coverage-audit`, `export-console`)
- B1-B7, B9-B11, B17 基础模块 (`shared-domain-types`, `data-persistence`, `gaode-client`, `localization`, `foundation-mode`, `notification-center`, `theming`, `sponge-export`, `manifest-generator`)

**绝对禁止依赖**:
- B12-B16 (ETL/GIS 专属模块必须经功能模块中转)

**唯一例外**: 
- B1 `shared-domain-types` 只读访问，用于首次打开路由判断 (ADR-0025)

### 薄壳原则

1. **零业务逻辑**: Slint 声明层只做展示和事件分发
2. **ViewModel 绑定**: 向各功能模块索取 ViewModel 状态和操作回调
3. **横向隔离**: 所有功能模块间协作一律通过壳接线，互不直接调用
4. **文案外置**: 所有文本走 B6 国际化 (`l10n.t("xxx")`)

### 窗口契约 (缝 1: Shell ↔ F1-F9)

| 功能模块 | 提供的 ViewModel | 操作回调 |
|---------|----------------|---------|
| F1 全局设置 | 语言/MC 版本列表、当前设置 | 修改语言、修改 MC 版本 |
| F3 方案管理 | 校区列表、方案卡片列表、回收站条目 | 新建校区、新建方案、删除/恢复方案 |
| F4 数据采集 | 采集进度视图、刷新差异报告 | 启动采集、跳过采集、查看采集报告 |
| F5 评审台 | 三栏布局视图 (卡片列表/地图对象/信息面板) | 改变候选状态、批量操作、封账请求 |
| F7 覆盖率审计 | 疑点报告视图、无碍通过视图 | 关闭疑点提示 |
| F9 导出控制台 | 确认弹窗视图、进度条视图、完成视图 | 确认导出、取消导出、跳转目标 |
| F2 新手教程 | 气泡提示视图、设置页入口视图 | 关闭气泡、跳过全部、重新开始 |

## 目录结构

```
apps/desktop/
├── Cargo.toml                 # 仅允许 slint + 功能/基础模块
├── build.rs                   # Slint 代码生成 + 快捷方式自动化
├── ui/                        # Slint UI 文件
│   └── main.slint             # 根 UI 组件定义
└── src/
    ├── lib.rs                 # VM 集成层 (零业务逻辑)
    ├── main.rs                # 主进程入口
    └── service.rs             # 服务进程入口
```

## 首次打开流程

1. 检测 `app_settings.first_run_complete`
   - **未首次运行**: 显示 F1 设置向导 → 高德搜索校区 → F3 方案列表
   - **老用户二次启动**: 读取 `last_campus_id` → 直达该校区 F3 方案列表

2. 导航规则:
   - 如果上次校区已被删除 → 退回校区选择页
   - 如果校区存在但无方案 → 显示"新建方案"引导
   - 如果有方案 → 显示方案卡片列表

## 开发版快捷方式

构建时自动创建/更新桌面快捷方式：
- 名称："校园复刻工具 - 开发版.lnk"
- 目标：`$LOCALAPPDATA\MCRebuildV2\dev\campus-tool-dev.exe`
- 策略：旧版本备份到 `previous/` 目录兜底

## 接线债务清单 (T16/T17 移交)

1. **F4 采集页接入 F7 "采集报告"入口**
   - 使用 `audit.report_entry` 文本键
   - 点击后显示 `AuditReportView`
   
2. **设置页添加"重新查看教程"按钮**
   - 使用 F2 准备的文案与重置接口 (`OnboardingTutorial::restart`)
   
3. **F2 教程三个里程碑钩子接线**
   - 首进方案列表 → 检查是否首次且未看过教程
   - 采集完成 → 检查是否有疑点需提示
   - 导出完成 → 检查是否第一次完成
   
4. **教程气泡位置调整**
   - 从占位值 (`x=0, y=0, width=280`) 调到实际坐标
   - 最终定稿留待负责人开发版审核时拍板

---

**验收标准**: 负责人可双击桌面"校园复刻工具 - 开发版"快速预览最新构建，端到端流程四步打通。
