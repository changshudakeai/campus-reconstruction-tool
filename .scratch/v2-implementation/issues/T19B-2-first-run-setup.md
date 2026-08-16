# T19B-2 — 首次运行向导（F1 设置页）

**What to build**: 首次用户打开软件时看到的"设置向导"——引导选择界面语言和 Minecraft 版本，完成后自动跳转到校区选择或直接进入方案列表页。

- **窗口契约**: 缝 1（Shell ↔ F1），壳只负责展示 F1 提供的 `SettingsViewModel`，零业务逻辑
- **业务规则**: 文本外置铁律 (ADR-0005)：所有可见文案从 zh-CN.json 读取；全局设置是应用级配置 (ADR-0004)
- **UI 决策法源**: ADR-0027（五步步骤条向导骨架 / 跳转规则 / 右上角四入口）+ ADR-0028（教程三泡清单，本单无教程钩子责任）；与本单描述冲突时以 ADR 为准

**Blocked by**: T19B-1 (VM 注入机制已就位)

**Status**: completed（2026-07-27，Actions 绿灯后定案）

## 🎯 验收标准

### 核心交付物
- [x] Slint UI 组件 `settings_wizard.slint`（新建文件）
  - 语言选择器：下拉菜单含当前支持的选项（首版仅 zh-CN，为未来扩语种留位置）
  - Minecraft 版本选择器：下拉菜单含当前支持的选项（首版仅 26.1.2，为未来扩版本留位置）
  - 知情告知勾选框："请确认你的 Minecraft 游戏版本与此一致，否则导入可能失败"
    - ⚠️ ADR-0004 强制要求：**未勾选时'完成设置'按钮禁用，不得放行**（已实现：enabled: acknowledged，F1 侧 NoticeNotAcknowledged 二次校验兜底）
  - "完成设置"按钮：点击后调用 F1 `complete_first_run(&FirstRunSetup { language, minecraft_version, acknowledged })`
- [x] F1 `SettingsManager::complete_first_run()` 成功持久化到 B2 `app_settings` 表（tests/ui_bindings.rs 同文件连接组断言 + 手动验收重启不再出向导）
- [x] 设置完成后自动关闭向导窗口并跳转到：
  - 首次 + 无 last_campus_id → 校区选择页（占位，真页面归 T19B-3）
  - 首次 + 有 last_campus_id → 直达该校区方案列表页（占位，真页面归 T19B-4）
  - 实现说明：跳转复用 `runtime::decide`（完成后自然落入 CampusSelect / LastUsedCampus 两支），未加 GuidanceCompleted 新变体——decide 现有两变体已完整表达落点，加变体反而要引入"来源"状态
- [x] 【T17 移交债务②】常规设置页（主窗口可打开的设置入口）摆入"重新查看教程"按钮：按钮文案取自 F2 `settings_entry(l10n).replay_label`，点击接线到 F2 `OnboardingTutorial::restart(db)`（借道 F3 连接落库）
- [x] 【装喇叭·B7 Presenter】壳实现 B7 `Presenter` trait 的 Slint 弹窗界面并在启动时经 `PresenterRegistry::set_presenter()` 注册（无论库是否可用）
  - ⚠️ 实现备注（诚实声明）：Error 级点亮全窗模态遮罩，TouchArea 吞全部输入，点"知道了"前界面不可操作（禁横幅/toast ✓）。"阻塞到用户确认"在非 UI 线程调用时字面成立（channel 等待）；UI 线程调用时点亮即返回——Slint 公开 API 不支持嵌套事件循环（官方 Modal Windows 特性尚在路线图），字面阻塞会死锁；用户层面"点掉才能继续"由遮罩承担
- [x] 【钩子清理】本单无教程钩子责任；债务③全量归 T19B-4/5/7（本单未碰任何 bubble_for 接线）

### 文案与国际化
- [x] zh-CN.json 新增以下文本键（单独 commit 5a127af；B6 ResourceBundle 同步增设 settings 类别）：
  ```json
  {
    "settings.wizard_title": "欢迎使用校园复刻工具",
    "settings.language_label": "界面语言",
    "settings.minecraft_version_label": "Minecraft 版本",
    "settings.notice_checkbox": "请确认你的 Minecraft 游戏版本与此一致，否则导入可能失败",
    "settings.continue_button": "继续",
    "settings.save_success": "设置已保存"
  }
  ```
- [x] .slint 文件中不得出现硬编码中文（另：显式指定 Microsoft YaHei 字体修复 femtovg 中文方块字）

### 架构断言（CI 门禁必过）
- [x] `cargo xtask arch` 通过：shell 不依赖 B12-B16
- [x] `cargo test` F1 的首次设置持久化测试通过（实际测试名 `acknowledged_first_run_persists_choices`，工单原提 test_save_settings_persists_to_db 为旧名）

### 用户体验
- [x] 首次运行（删除 `campus-rebuild.db` 后）能看到向导（ComputerUse 截图验收已过）
- [x] 选择语言/MC 版本后勾选知情告知框，"完成设置"按钮才变可用
- [x] 向导关闭后能看到下一屏（校区选择占位；重启不再出向导）

## 📋 实施提示

### 实施顺序建议
1. 先在 `resources/zh-CN.json` 补充文本键（避免后续反复改）
2. 编写 `settings_wizard.slint` 最小可行界面（两个下拉框 + 勾选框 + 一个按钮）
3. 在 Rust runtime 中加载 slint 组件并将 F1 的 `get_settings()` 返回值注入属性
4. 按钮的 `on_CLICKED` 回调检查勾选框状态，然后调用 F1 `complete_first_run()`
5. 成功后返回 `LandingDecision::NextScreen` 给 shell 做路由判断

### 技术难点
- **Slint 下拉菜单**：用 `ComboBox` 组件，但需要确保 `ListModel<String>` 能正确绑定
- **勾选框联动**：未勾选前按钮禁用（`.enabled: !notice_checked`）
- **状态保持**：向导关闭后 shell 需要知道跳转到哪里 —— 建议在 `LandingDecision` 枚举加新 variant `GuidanceCompleted`
- **接线钥匙（T19B-1 已备好）**：经 `ViewModelInjector` 的 `settings_mut()` / `tutorial()` / `l10n()` 访问器绑定，回调错误一律递 `report_callback_error`
- **测试坑提醒**：`ShellDatabases::open_in_memory()` 的两条连接是相互独立的内存库；跨模块落库断言（如 F1 写、F2 读）请用指向同一临时文件的连接组，或统一借道 F3 `database_mut()`

### 避免的错误
- ❌ 不要在 Shell 里写 if language == "中文" then show 中文按钮 logic
- ✅ 应该让 F1 返回纯数据，Shell 只展示

## 💡 特别提示
这是贯穿弹剧本的第一步——如果向导都走不通，后面的"建方案→采数据→评审→导出"就无从谈起。

✅ **收工自检清单**:
- [x] `cargo check` 全 workspace 无报错
- [x] 手动测试：删除数据库 → 启动 → 能看到向导 → 选择配置 → 勾选确认 → 能看到下一屏
- [x] `cargo clippy --workspace -- -D warnings` 零警告
- [x] `cargo fmt --all --check` 格式化通过

---

## 负责人验收点（一句话）

删库重启后能看到一个"欢迎使用校园复刻工具"的向导窗，勾选确认后顺利看到下一个页面。
