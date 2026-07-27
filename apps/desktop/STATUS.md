# T19 — S1 薄壳实现状态（T19B-5A 后）

> 本文件随 T19B 系列子工单逐单更新，反映仓库真实状态。

## 📋 当前任务序列（负责人 2026-07-27 批准）

## ✅ 已落地（全部过本地门禁 + 可运行）

### Phase1（T19）

- `apps/desktop` crate 立项，Cargo.toml 依赖段即白名单
- `ui/main.slint`：最小根窗口，零硬编码文案，属性由 Rust 侧填充
- `build.rs`：slint-build 编译 UI 生成绑定（`slint::include_modules!`）
- `runtime::landing_decision`：首开着陆判定（首次向导 / 老用户直达 /
  校区选择），判定委托 F1 `SettingsManager`，附单元测试
- zh-CN.json `app.shell_status_*` 三个文本键
- deny.toml：slint 家族 Royalty-Free 许可豁免（见 deny.toml 内注释）

> ⚠️ **T19B-5 已拆分为 5A + 5B**：见 `.scratch/v2-implementation/issues/` 新工单；
本文件后续按新顺序补充文档（5A 还债与基建、5B 方案工作区）。

### T19B-5A（还债与基建：色卡 + 对话框 + ADR-0010/0018，2026-07-27）

- `ui/theme.slint`（新建）：Theme global 单例——11 个颜色角色名属性
  （surface/overlay/text-primary/text-secondary/text-tertiary/text-quaternary/
  text-faint/separator/bubble-background/bubble-border/error），
  代码永不写颜色号，只写角色名（ADR-0023 §一）
- `resources/themes/light.json`（新建）：亮色卡资源文件，键为角色名、值为 hex；
  切主题 = 换色卡 JSON，代码零改动
- `src/theme.rs`（新建）：色卡加载器（磁盘优先 + 内嵌兜底，参考 B6 模式）+
  相对时间格式化（ADR-0018 §一第 3 条：刚刚/X 分钟前/X 小时前/X 天前，超 7 天显示日期）
- `ui/dialogs.slint`（新建）：通用确认窗（ConfirmDialog）+ 通用输入窗（InputDialog），
  文案全部注入，后续所有危险操作与输入场景复用
- `ui/plan_list.slint`：三个平铺按钮收进 ··· PopupWindow 菜单（ADR-0018 §三）；
  全部硬编码颜色号替换为 Theme 角色名
- `ui/campus_select.slint`、`ui/main.slint`：硬编码颜色号全部替换为 Theme 角色名
- `ui/main.slint`：新增确认窗/输入窗属性组 + 回调声明 + ConfirmDialog/InputDialog 实例
- `src/injector.rs`：新建方案改为弹输入窗（ADR-0010 轻创建，预填默认名）；
  改名接通 F3 rename_plan；删除加二次确认；相对时间格式化
- zh-CN.json 新增 `dialog.confirm_button/cancel_button/delete_title/create_title/
  rename_title/name_label` + `time.just_now/minutes_ago/hours_ago/days_ago/date_display`
- 诚实妥协：ADR-0010"确认后进入方案工作区"这半条不在本单（工作区屏归 T19B-5B），
  本单建完仍留列表页
- 暗色卡与设置页切换开关延后（ADR-0023 §二），机制已就绪

### T19B-5B（方案工作区 UI 骨架占位，2026-07-27）

- `ui/main.slint`：新增屏 4（`if root.active-screen == 4` 分支）
  —— 最小化占位实现，仅显示五步流程文字与待完善提示
- zh-CN.json 新增 `workspace.placeholder_title` / `workspace.placeholder_subtitle`
  文本键（国际化铁律 ADR-0005）
- `src/injector.rs`：注入屏 4 文案属性（`workspace-placeholder-title` + 
  `workspace-placeholder-subtitle`）
- 预留接线：`stepper.slint` / `boundary_edit.slint` / `orientation.slint`
  三个组件文件已存在（语法需简化适配 Slint 基础版本）
- 诚实声明：步骤条完整交互逻辑（跳转规则/锁定/回跳确认窗）
  及圈边界/定朝向页面归后续完整实现
- 本单交付：编译绿灯 ✅ + 测试通过 ✅ + 手动可跳转屏 4 查看占位界面

> ⚠️ **已知技术债**：
> - StepperIntro 气泡钩子位置未埋设（原计划绑定到步骤条亮相时机）
> - B5 foundation-mode 的 BoundaryDrawer/OrientationCalculator 尚未接入
> - stepper/boundary_edit/orientation 三个 .slint 文件需简化语法后重新集成

### T19B-1（Shell 基础框架与 VM 注入，2026-07-26）

- `src/injector.rs`：`ViewModelInjector::new(ShellDatabases)` 构造并持有
  全部 7 个 F 模块实例（F1-F5/F7/F9）；`inject(&window)` 把视图状态
  注入 Slint in property；各模块访问器 + `enter_review` 进台会话作为
  后续工单的接线钥匙
- `src/dispatch.rs`：`report_callback_error` 回调错误统一出口（弹窗铁律
  ADR-0021，经 B7 error 模态 + 留底；新文本键 `app.source_tag`）
- `run_dev()`：启动即初始化 B7 全局一本账；数据库可用时经注入器装配
  窗口，不可用时回退首开文案（原兜底语义）
- 单元测试：`test_injector_holds_all_vms` / `test_vm_state_injected_to_slint`
  （真 AppWindow 属性断言）/ `callback_error_reaches_b7_board`
- 设计备注：B2 `Database` 不可 Clone → 壳对同库开两连接（F1/F3 各持一条）；
  F9 门控暂用 `MockSealGate` 占位，真门控归 T19B-8

### T19B-2（首次运行向导 + 债务② + 装喇叭，2026-07-27）

- `ui/settings_wizard.slint`（新建）：首跑向导组件——语言/MC 版本下拉框
  （选项来自 F1 `SUPPORTED_*` 常量，首版仅 zh-CN / 26.1.2）+ 知情告知
  勾选框；未勾选时“继续”按钮禁用（ADR-0004 强制，F1 侧另有二次校验）
- `ui/main.slint`：三屏路由（active-screen：0 首跑向导 / 1 着陆占位 /
  2 设置页）+ B7 错误模态遮罩层；显式指定 Microsoft YaHei 字体
  （femtovg 默认字体缺中文字形显方块）
- `src/injector.rs`：`inject` 注入向导/设置页/弹窗文案与选项 model；
  新增 `bind(&Rc<RefCell<Self>>, &window)` 接线两个回调——向导完成
  （F1 `complete_first_run` 落库 → 重判着陆跳下一屏）与重看教程
  （债务②：文案取 F2 `settings_entry().replay_label`，点击接 F2
  `restart(db)` 借道 F3 连接落库）；回调错误一律递 `report_callback_error`
- `src/presenter.rs`（新建）：`ShellPresenter` 实现 B7 `Presenter`，启动时
  经 `PresenterRegistry::set_presenter` 注册（装喇叭，无论库是否可用）。
  Error 级点亮全窗模态遮罩（TouchArea 吞全部输入，点“知道了”前界面
  不可操作）；非 UI 线程调用真阻塞到确认，UI 线程调用点亮即返回
  （Slint 公开 API 不支持嵌套事件循环，字面阻塞会死锁——见模块文档
  “诚实声明”节）；toast/铃铛呈现归公告栏工单
- `tests/ui_bindings.rs`（新建）：窗口类场景集中单进程串行（Slint 平台
  只能单线程初始化一次）：注入断言 / 向导双保险与落库跳屏 /
  重看教程清零落库 / 遮罩亮灭；跨模块断言用同一临时文件连接组
- zh-CN.json 新增 `settings.*` 六键（单独 commit）；B6 `ResourceBundle`
  增设 settings 类别
- 手动验收已过（删库→向导→勾选→下一屏；重启不再出向导；设置页
  重看教程可点；无方块字）

### T19B-3（校区选择页 + 五屏路由，2026-07-27）

- `ui/campus_select.slint`（新建）：校区选择页组件——标题 / 占位文案 /
  示例校区 A、B 按钮 / 设置按钮；全部文案由 Rust 侧 l10n 注入
- `ui/main.slint`：五屏路由（0 向导 / 1 校区选择 / 2 方案列表 / 3 设置）
- `src/injector.rs`：`bind_campus_select` 接线——点击校区 →
  `remember_campus` → 刷新方案列表 → 跳屏 2；点击设置 → 跳屏 3
- zh-CN.json 新增 `app.campus_select_*` 四键 + `app.settings_button`

### T19B-4（方案列表页 CRUD + 教程气泡钩子，2026-07-27）

- `ui/plan_list.slint`（新建）：方案列表页组件——标题 / 校区名 /
  新建按钮 / 卡片 ListView（PlanCardData struct）/ 返回按钮 /
  教程气泡浮层；全部文案由 Rust 侧 l10n 注入，零硬编码中文
- `ui/main.slint`：屏 2 从占位替换为 PlanList 组件，新增
  plan-list-* 属性组 + 7 个回调声明
- `src/injector.rs`：`bind_plan_list` 接线——新建方案（自动命名
  “新方案 1”“新方案 2”……）/ 返回校区选择 / 复制方案（F3 duplicate_plan）/
  删除到回收站（F3 delete_plan，保留 30 天）/ 改名占位（待对话框
  基础设施）/ 教程气泡钩子（F2 bubble_for PlanListIntro，坐标占位，
  定稿归 T19B-8）
- `src/injector.rs`：`refresh_plan_list` 辅助方法——刷新校区名 +
  卡片模型 + 教程气泡；`next_plan_name` 自动命名；
  `dismiss_tutorial_step` / `skip_all_tutorial` 解决借用冲突
- `src/lib.rs`：导出 `PlanCardData` 生成类型
- zh-CN.json 新增 `plan.empty_list` / `plan.delete_confirm`
- 手动验收已过（删库→向导→校区选择→点示例校区→方案列表页
  →新建方案→卡片出现→复制→删除→设置页可进）

## 🚧 剩余接线债务（归 T19B-5B..9，勿当已完成）

1. 五步步骤条导航骨架（ADR-0027）与各步骤页属性/回调绑定；
   校区选择与方案列表已于 T19B-3/4 落地，剩余步骤页归 T19B-5B..8
2. F4→F7 采集报告入口【债务①】与 F2 剩余两个里程碑钩子【债务③，
   ADR-0028 三泡】（首进方案列表已于 T19B-4 落地；步骤条总介绍归
   T19B-5B，首进评审页归 T19B-7）
3. 暗色卡文件（`resources/themes/dark.json`）与设置页主题切换开关（ADR-0023 §二）
4. F9 真封账门控（壳实现 SealGate 调 F5 seal）；B7 warn 级 toast 与
   铃铛角标的 Slint 呈现（随公告栏界面）
5. 开发版快捷方式端到端验收（`cargo xtask dev-shortcut` 已有实现）
6. public-api 快照 + CODEOWNERS 守卫 .lnk 文件

## 验证命令

```powershell
cargo test --workspace
cargo xtask tidy ; cargo xtask arch
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo machete
cargo deny check advisories bans licenses sources
cargo run -p desktop-shell --bin campus-tool-dev
```
