# T19 — S1 薄壳实现状态（T19B-5A 后）

> 本文件随 T19B 系列子工单逐单更新，反映仓库真实状态。
> 本文件中的 `ViewModelInjector` 持有关系和旧“壳接线”是历史实现事实，不构成当前架构授权；按 ADR-0037，组合根只可构造期注入，一个 UI 操作不得由 S1 运行期编排多个功能入口。

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

### T19B-5B（方案工作区 Phase 1 完成，2026-07-27）

**Phase 1（三步法落地）：屏 4 步骤条 + 单击即开 + stepper_intro 气泡**

- `ui/stepper.slint`（新建）：五格步骤条组件（ADR-0027）——StepButton
  子组件 + Stepper 导出组件，绝对定位布局（绕开 HorizontalLayout
  max-width binding loop）；当前高亮 / 已完成打勾 / 锁定置灰半透明；
  文案全部经属性注入，颜色全部 Theme 角色名
- `ui/main.slint`：屏 4 = Stepper + 占位内容区（平铺 if-block，组件只做
  展示不做计算）；新增 workspace-* 属性组（当前步/已完成步数/六段文案/
  气泡四属性）与三个回调；**Step B 成功采用条件渲染（if-block），
  未启用 visible 兑底模式**
- 单击即开（ADR-0027 第 6 轮）：`plan_list.slint` 卡片新增 TouchArea
  热区 + `card-clicked` 回调 → 跳屏 4；`campus_select.slint` 校区行
  补接单击（老用户进已有校区的入口，此前只有新建演示校区一条路）
- 工具栏可见性改为 .slint 侧按 `active-screen` 派生（屏 2/4/5/6 显示，
  0/1/3 隐藏）——Slint Rust API 无属性变化监听回调，且覆盖 .slint
  内部返回跳转（设置页返回按钮）
- 步骤点击守卫（Rust 侧）：锁定步骤忽略点击（前跳上锁，第①格永远解锁）；
  回跳转屏逻辑归 Phase 2/3
- Step C：`injector.rs` 进屏 4 时向 F2 索泡 `bubble_for(StepperIntro)`
  ——首次弹出右上角气泡（步骤条下方），dismiss 记已见落库（只教一次），
  skip_all 转 Completed 经 B7 留底；气泡 UI 复用 Theme.bubble-* 色卡
- zh-CN.json 零改动（`tutorial.step_stepper_intro` 已于 b8b4294 在库）
- 三步 commit：Step A `14a16a9`（占位）→ Step B `6362720`（步骤条+路由）
  → Step C `a703cef`（气泡钩子），每步独立全门禁绿

> ⚠️ **已知技术债（归 Phase 2/3 与后续接线单）**：
> - 圈边界/定朝向步骤页未实现（占位文字区，归 Phase 2/3）
> - B5 foundation-mode 的 BoundaryDrawer/OrientationCalculator 尚未接入
> - 回跳“改动需确认窗”未实现（Phase 1 无数据可改，直接自由回跳）
> - 气泡位置为占位坐标（右上角，定稿归 T19 界面审核）
> - 步骤条为固定像素布局（x 坐标绝对定位），窗口拉宽时不自适应

### S1-03（启动与设置流程迁移到呈现入口，2026-07-31）

- 启动入口（StartupRequest::Show / CompleteFirstRun）一次返回首次设置、校区
  搜索或方案页的完整着陆结果；首次设置完成后重新取得着陆结果进入校区
  搜索与最近记录页（ADR-0037）
- 设置入口（SettingsRequest）覆盖常规设置读写（语言、Minecraft 版本、
  默认导出位置）与高德密钥的保存、测试、清除（确认后一次清除）及
  教程重播；保存/测试/清除按已确认规则返回成功或失败通知事实，由 B7
  决定呈现方式（ADR-0004 / ADR-0021）
- F1 公开接口最小化扩展：default_export_location / set_default_export_location /
  clear_gaode_keys；B2 新增 AppSettingKey::DefaultExportLocation 键（
  公开展开接口，手工快照同步）
- 正式数据读取失败改为明确失败状态 + 错误通知，不再回退默认值/假首开页；
  数据库无法打开时同样明确失败（基线文档同步更新）
- 旧接线删除：injector 中的首跑完成、教程重播、高德保存/测试、设置页
  返回与设置/向导文案注入全部移除；新绑定集中在 ProductionEntries
- 测试：	ests/s1_03_startup_settings_flow.rs（新）、presentation_seams、
  ui_bindings、s1_contract_baseline 与 F1 public_api 快照同步更新

### S1-04（校区搜索、最近记录、方案列表与回收站迁移到方案管理入口，2026-07-31）

- 校区选择页按 ADR-0006 改为搜索框 + 最近使用的校区（名称+地址，最近进入排最前）；
  搜索只在点击"搜索"或按回车时开始，输入期间不自动搜索；无记录时直接显示搜索框
- 选择重复校区直接进入原校区方案页，并返回"该校区已添加，已为你切换"通知事实；
  最近记录右侧小叉立即移除且不弹确认（只清快捷记录，不删校区）
- 创建、改名（输入窗）、复制、删除（确认窗）与恢复均由方案管理入口完成；
  恢复重名自动加"（恢复 N）"后缀（ADR-0018 §五，替换旧的 RestoreNameConflict 拒绝）
- 回收站恢复/永久删除/清空均经入口：先确认（永久删除与清空），成功后停留回收站
  并产生"方案已恢复/已永久删除/回收站已清空"提示事实（ADR-0018）
- F1 公开接口扩展：RecentCampus / recent_campuses / remove_recent_campus /
  remember_campus 维护最近列表、select_campus_with_anchor 增加地址参数；
  F3 公开接口扩展：search_campuses / suggest_plan_name / restore_plan 模板后缀 /
  purge_all_trash_confirmed / TrashItemView 增加名称、校区与剩余天数；
  B2 扩展：campuses.address 列（迁移 4）、AppSettingKey::RecentCampuses、
  TrashApi::purge_all_in_campus_trash；B6 ResourceBundle 启用 campus 文本段
- 旧接线删除：injector 中校区选择、方案 CRUD、输入/确认窗、回收站原始跳转全部移除；
  呈现入口 bind_actions 统一接线；tests/s1_04_campus_plan_trash_flow.rs（新）
- 行为基线同步更新：校区与方案流程新增搜索/最近记录/回收站可观察结果（冲突点单列，
  见交付说明）；F1/F3/B2 public_api 与手工快照同步
### S1-05（方案工作区、步骤导航与边界流程迁移到功能入口，2026-08-01）

- 方案工作区经工作区功能入口一次返回完整页面状态与导航决定：步骤点击/“下一步”统一返回允许进入/条件不足/需要确认（`WorkspaceRequest::Navigate`），S1 不再自行判断步骤门控
- 五个步骤顶部始终同时显示校区名与方案名（ADR-0027）；步骤锁定/完成状态由功能入口注入（Stepper 不再按 completed-steps 自行推导）
- 边界闭合、有效性、重置与保存全部经工作区功能入口调用 B5（BoundaryDrawer/validate_polygon_closure/CoordinateConverter）并返回完整结果；地图通道（map_webview/B3 页面）只显示地图并转交原始 IPC 消息
- 地图加载立即呈现处理中状态且不冻结；高德地图故障只暂停地图相关操作（公告留底），设置、方案列表与已有正式数据仍可访问
- 离开边界页由功能入口判定可以离开/需要确认（未保存绘制）/必须停留（地图加载中），删除旧的壳内直接跳转
- 朝向门控（提交/重置/重算确认）随工作区入口迁移，交互细节归工单 06
- F3 公开接口最小化扩展：`plan_context`（方案名/校区名/锚点一次返回）；B6 ResourceBundle 启用既有 boundary/orientation/map 文本段（此前被 serde 忽略，键名直接显示）
- 旧接线删除：injector 中步骤/边界/朝向/工具栏/方案卡片打开工作区全部移除（injector 并入 runtime 组合根）；测试：tests/s1_05_workspace_boundary_flow.rs（新）、presentation_seams、runtime 入口计数、s1_contract_baseline 同步

### S1-06（朝向流程迁移到功能入口，2026-08-01）

- 朝向流程完整迁移到工作区功能入口（workspace_boundary.rs）：地图两点参考线（orientation_points/confirm_orientation IPC）经通用地图通道转交，F5 OrientationCalculator 计算角度并回填路径/箭头/角度/状态；方位角输入经 F5 normalize_angle 归一化（NaN/∞ 拒绝，越界值按 F5 语义折回 0~360），S1 不重复计算
- 覆盖已有朝向统一走“影响说明 + 确认”决策：F5 check_orientation_change_impact 报告按类别列出重算影响（orientation.impact_item_line，B6 本地化），确认后应用、取消不落库
- 确认/取消/重置/保存全部由功能入口返回完整 OrientationViewState + 导航决策，S1 不再手工拼装；地图“清除重来”（orientation_clear）只清草稿，不清已保存朝向
- 朝向保存失败（无活动方案等）返回 Failed + 明确错误通知，正式状态（has_orientation/orientation_angle）保持不变可重试；地图故障只暂停地图操作（公告留底），已保存边界/朝向与方案列表不丢失
- 旧接线删除：runtime 中朝向页静态文案注入全部移除（由 OrientationViewState 渲染）；tests/s1_06_orientation_flow.rs（新）覆盖全流程验收
- 行为基线同步：两点模式中间态（计算后未确认）与“清除重来不清正式状态”已补入 docs/behavior-baselines（冲突点单列，见 PR 交付说明）


### T19B-9（右上角四入口工具栏 + 公告栏页 + 回收站页，2026-07-27）

- `ui/notice_board.slint`（新建）：公告栏页组件（屏 5）——ListView 公告列表 +
  重要性标识（high/normal 两级）+ 已读/未读状态 + empty-state + 归档按钮；
  全部文案由 Rust 侧 l10n 注入，零硬编码中文
- `ui/trash.slint`（新建）：回收站页组件（屏 6）——ListView 回收项列表 +
  方案名/原校区/删除时间/过期时间 + 恢复/永久删除双按钮 + empty-state；
  全部文案由 Rust 侧 l10n 注入，零硬编码中文
- `ui/main.slint`：右上角四入口工具栏（公告栏📢 / 切换校区🔄 / 回收站🗑️ / 设置⚙️）
  固定右上角，图标 + 文字标签（ADR-0027 §决定）；屏 5/6 路由分支
- `ui/theme.slint`：新增 `text-inverse` 角色（白色，用于深色背景上的文字）
- `src/injector.rs`：`bind_toolbar` 接线四个工具栏回调（屏切换）；
  `inject` 注入 notice.*/trash.* 全部文案属性
- zh-CN.json 新增 `notice.*`（8 键）+ `trash.*`（13 键）
- 诚实声明：
  - 工具栏默认可见性 = false，待 T19B-5B 装配步骤条容器后激活
  - 公告栏数据源待 F3 通知中心 API；回收站恢复/永久删除回调待 F3 接线
  - toast/铃铛 Presenter 空壳延后（ADR-0028 后果节，优先级低）
  - F3 restore_plan 重名逻辑与既有决策不符（应自动加“（恢复 1）”后缀，
    现为拒绝并报 RestoreNameConflict），待回收站 API 接线单修正

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
2. A1 `collection-flow` 完整入口内的覆盖体检与采集报告【债务①；禁止 S1 运行时 F4→F7】及 F2 剩余两个里程碑钩子【债务③，
   ADR-0028 三泡】（首进方案列表已于 T19B-4 落地；步骤条总介绍归
   T19B-5B，首进评审页归 T19B-7）
3. 暗色卡文件（`resources/themes/dark.json`）与设置页主题切换开关（ADR-0023 §二）
4. F9 真封账门控（非 S1 适配器实现 SealGate，构造期注入 F9；壳不得调用 F5 seal）；B7 warn 级 toast 与
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
