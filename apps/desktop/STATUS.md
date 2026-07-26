# T19 — 薄壳 UI 实现状态报告

## ✅ 已完成的工作

### 1. 架构基础设施
- ✓ 创建 `apps/desktop` crate (T19 S1 主程序应用壳)
- ✓ 配置 Cargo.toml 依赖白名单 (仅 slint + F1-F9 + B1-B7/B9-B11/B17)
- ✓ deny.toml 已配置 slint 白名单 (唯一使用者: desktop-shell)
- ✓ workspace dependencies 添加 slint + slint-build

### 2. 薄壳核心框架
- ✓ `src/lib.rs`: ViewModel 集成层框架 (零业务逻辑声明)
  - 导出所有功能模块的 ViewModel trait
  - 集成 B6 国际化 (`Localization`)
  - 集成 B2 数据持久化 (`Database`, `AppSettingsApi`)
  
- ✓ `src/runtime.rs`: 运行时管理
  - `AppShell`: 应用状态管理 (首次打开流程判断)
  - `CurrentView`: 视图枚举 (导航状态机)
  - `run_dev()`: 开发版桌面应用入口
  - `run_service()`: 后台服务进程入口

- ✓ `src/main.rs`: 命令行入口
  - 支持 `--service` 参数切换到后台模式
  - 默认启动开发版桌面应用

### 3. Slint UI 模板
- ✓ `ui/main.slint`: 最小化 UI 模板文件
  - MainWindow 组件框架
  - MenuBar 占位符
  - Callbacks 定义 (`navigate_to_plan_list`, `start_collection` 等)
  - TODO 注释标明需接入的 VMs

### 4. 构建自动化
- ✓ `build.rs`: Slint 代码生成 + 快捷方式自动化
  - Slint UI 编译为 Rust 代码
  - Release 模式下自动创建/更新桌面快捷方式 "校园复刻工具 - 开发版.lnk"
  - 旧版本备份到 `previous/` 目录兜底

---

## 🚧 待完成工作（后续完整 UI 实现）

### 接线债务清单 (T16/T17 移交)

#### 1. F4 采集页 → F7 采集报告入口
- 在 F4 页面底部添加"查看采集报告"按钮
- 使用 `audit.report_entry` 文本键
- 点击后显示 `AuditReportView` 弹窗/新页面

#### 2. 设置页 → 重新查看教程按钮
- 在设置页添加"重新查看教程"按钮
- 接线到 `OnboardingTutorial::restart` 接口
- 按钮文案来自 `tutorial.replay_button`

#### 3. F2 教程三个里程碑钩子
- **首进方案列表**: 检查 `tutorial.status == NotStarted` → 显示第一泡
- **采集完成**: 触发覆盖率审计疑点检查 → 有疑点才提示
- **导出完成**: 第一次完成时显示引导气泡

#### 4. 教程气泡位置调整
- 从占位值 `x=0, y=0, width=280` 调到实际坐标
- 最终定稿留待负责人开发版审核时拍板

---

## 🏃 如何运行当前版本

### 本地开发构建

```powershell
# 进入项目根目录
cd c:\Users\chang\Desktop\MCRebuild_Renovation\New-branch-v2

# 验证依赖完整性
cargo check -p desktop-shell

# 编译并运行开发版
cargo run -p desktop-shell --bin campus-tool-dev
```

**预期输出**:
```
🚀 校园复刻工具 - 开发版启动...
🎓 检测到首次使用，启动设置向导
⚙️  UI 薄壳层初始化完成
✅ ViewModel 集成就绪 (F1-F9)
🔌 接口接线：SealGate(F5→F9)、采集报告 (F4→F7)、教程钩子 (F2)

📝 运行环境：Windows + Rust + Slint
💡 按 Ctrl+C 退出
```

### Service 模式

```powershell
# 后台服务模式（无界面，用于自动化/CI）
cargo run -p desktop-shell --bin campus-tool-service -- --service
```

---

## 🔨 下一步计划

1. **完善 Slint UI 实现** (完整三栏布局 + 各页面)
   - 首次运行设置向导
   - 高德校区搜索页
   - 方案卡片列表页
   - 边界绘制与朝向设定
   - F4 采集页面
   - F5 评审台 (左卡片/中地图/右信息)
   - F7 覆盖率审计报告
   - F9 导出控制台

2. **ViewModel 全接线**
   - 每个页面的数据绑定
   - 用户操作回调函数路由
   - SealGate 封账确认流程

3. **接线债务处理**
   - F4→F7 报告入口
   - F2 教程钩子与重置按钮
   - 气泡位置优化

4. **测试与门禁**
   - `cargo test --workspace` 全绿
   - `cargo fmt --all --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo xtask tidy && cargo xtask arch`

---

## 📁 文件结构

```
apps/desktop/
├── Cargo.toml                 # 依赖白名单配置
├── build.rs                   # Slint 生成 + 快捷方式自动化
├── README.md                  # 此文档
├── ui/
│   └── main.slint             # Slint UI 模板 (最小化)
└── src/
    ├── lib.rs                 # ViewModel 集成层 (零业务逻辑)
    ├── runtime.rs             # 运行时管理与流程判断
    ├── main.rs                # 开发版入口
    └── service.rs             # 后台服务入口
```

---

## 💡 架构原则重申

### 薄壳铁律
1. **零业务逻辑**: Slint 声明层不做任何计算/判断
2. **ViewModel 分离**: 所有业务逻辑留在各自 F 模块内
3. **横向隔离**: F 模块间协作必须通过壳接线，不得直接调用
4. **文案外置**: 所有可见文字走 B6 (`l10n.t("xxx")`)

### 依赖白名单 (ADR-0017/0025)
- ✅ 允许：slint + F1-F9 + B1-B7 + B9-B11 + B17
- ❌ 禁止：B12-B16 (ETL/GIS 专属，必须经 F 模块中转)
- ⚠️ 例外：B1 只读访问 (首次打开路由判断，ADR-0025)

---

## 📋 验收标准

负责人可以：
- ✅ 运行 `cargo run -p desktop-shell --bin campus-tool-dev` 看到开发版启动
- ✅ 理解薄壳架构原理 (VM 集成、事件分发、零业务逻辑)
- ⏳ 后续完整 UI 上线后可双击桌面快捷方式预览最新构建

---

**T19 工单进度**: 基础架构完成，待完整 UI 实现与接线债务处理。
