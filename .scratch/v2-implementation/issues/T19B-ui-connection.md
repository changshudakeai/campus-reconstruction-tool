# T19B — UI 细节接线（已拆分为 8 个子工单，本文件仅作索引）

> **解释规则（2026-08-01）**：本索引和旧子工单中的“注入/接线”仅允许表示构造期创建实现、注入能力和绑定 UI 回调；若文字要求 S1 在运行期依次调用多个功能入口，以 ADR-0037 为准并停止采用。

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

T19B 原始范围（把 T19 Phase1 最小壳填成完整应用 + 4 项接线债务）经评估远超单会话容量，
已于 2026-07-26 拆分为 8 个垂直切片工单，严格串行执行。

> ⚖️ **UI 决策法源（2026-07-26 访谈完工）**：导航骨架与全部子决策见 **ADR-0027**
> （五步步骤条 / 回跳自由改动确认前跳上锁 / 右上角四入口）；教程气泡清单见 **ADR-0028**
> （三泡：新建方案 · 步骤条总介绍 · 评审页介绍）。工单描述与 ADR 冲突时以 ADR 为准。

## 📋 子工单索引（串行，每单一个会话）

| # | 工单文件 | 阻塞于 | 一句话交付 | 状态 |
|---|----------|--------|-----------|------|
| 1 | T19B-1-shell-framework.md | 无 | Shell 基础框架与 VM 注入机制 | completed（2026-07-26） |
| 2 | T19B-2-first-run-setup.md | T19B-1 | 首次运行向导 + 设置页教程重置按钮【债务②】 | ✅ completed（2026-07-27, Push to main）| |
| 3 | T19B-3-campus-selection.md | T19B-2 | 老用户二次启动直达 + 校区选择页 | ✅ completed（2026-07-27）| |
| 4 | T19B-4-plan-list.md | T19B-3 | 方案列表页 CRUD + 首进列表钩子【债务③-1】 | ✅ completed（2026-07-27）| |
| 5 | **T19B-5A-debt-and-infra.md** | T19B-4 | **色卡基建 + 通用对话框 + ADR-0010/0018 修复** | ✅ completed（2026-07-27） |
| 5B | **T19B-5B-plan-workspace.md** | T19B-5A | **单击打开 + 五格步骤条 + 圈边界 + 定朝向 + stepper_intro** | 🔜 ready-for-agent |
| 9 | **T19B-9-toolbar-notice-trash.md** | T19B-5B | **右上角四入口统一工具栏 + 公告栏页 + 回收站页** | ✅ completed（2026-07-27） |
| 6 | T19B-6-collection-page.md | T19B-9 + s1-21 | 采集页 + A1 `collection-flow`（含覆盖体检）【债务①】 | blocked（等 B14） |
| 7 | T19B-7-review-workbench.md | T19B-6 | 评审工作台三态管理 + 首进评审页钩子【债务③-3】 | blocked（等 6） |
| 8 | T19B-8-export-and-closeout.md | T19B-7 | 导出 .schem + 气泡坐标定稿【债务④】+ 封账收尾 | blocked（等 7） |

> 注：顺序调整理由见第五节（负责人 2026-07-27 批准）

## 🗺️ 债务分布（原 4 项接线债务的最终归属）

- 债务① A1 完整采集入口内的覆盖体检与采集报告 → T19B-6（S1 不运行时串联 F4/F7）
- 债务② 设置页“重新查看教程”按钮 → T19B-2
- 债务③ F2 三个里程碑钩子（已按 ADR-0028 重排） →
  - T19B-4（首进方案列表） ✅ completed（2026-07-27）
  - **T19B-5B（步骤条首次亮相）** ← F2 枚举改造在 5B 实施；stepper_intro 钩子在 5B 埋设
  - **T19B-7（首进评审页）** ← review_intro 钩子在 7 埋设
  - ~~采集完成（T19B-6）/导出完成（T19B-8）两处钩子作废~~
- 债务④ 气泡坐标定稿 → T19B-8（三处一并定稿，负责人开发版审核）

### ADR 欠账修复记录

| 欠账法源 | 违规表现 | 修复工单 | 状态 |
|---------|---------|---------|------|
| ADR-0023 §一 | slint 十六进制颜色号散写 | T19B-5A 色卡基建 | ✅ completed（2026-07-27） |
| ADR-0010 | "新建方案"不是轻量对话框，无预填名 | T19B-5A | ✅ completed（2026-07-27） |
| ADR-0018 §三 | ···菜单收进、改名无确认、删除无二次确认、时间数字显示 | T19B-5A | ✅ completed（2026-07-27） |

> 暗色卡与切换开关延后声明（ADR-0023 §二）：本单亮色卡先行；暗色卡文件（`resources/themes/dark.json`）与设置页切换开关归后续工单。
> 机制已就绪：新增一张色卡 JSON + 在 Rust 侧 `apply_theme` 切换加载即可换肤，代码零改动。

## ⚠️ T19B-1 遗留事实（后续各单施工前必读）

- **🔴 T19B-4 已知缺陷（归 T19B-5 第 0 号前置任务）**：校区选择页两个示例按钮发送的是伪 ID（campus_001/002，非 UUID），运行时 CampusId::parse 必报错弹窗，主干路“选校区→方案列表”实际不通。修法：换成 F3 list_campuses 动态真列表 + “新建演示校区”按钮（create_campus 自动命名）
- **T19B-4 诚实占位待补（对话框基础设施落地后）**：改名对话框、删除前二次确认——建议随 T19B-5 的确认弹窗基础设施（朝向重算确认窗）一并落地
- **B7 Presenter 已注册（T19B-2 完成）**：弹窗界面可用，Error 级全窗遮罩
- **MockSealGate 假门占位（归 T19B-8）**：F9 封账门控现为内存模拟件，T19B-8 必须替换为真 SealGate
- **测试坑**：`ShellDatabases::open_in_memory()` 两条连接是相互独立的内存库；跨模块落库断言请用同一临时文件连接组或借道 F3 `database_mut()`
- **接线钥匙**：各单一律绑定一个完整功能入口；`ViewModelInjector` 只承担构造期注入和回调绑定，不得读取中间结果后编排多个功能模块；回调错误递 `report_callback_error`
- **F3 restore_plan 重名逻辑与既有决策不符**：v1.x 既有决策为“恢复时若名字已被占用，系统自动加‘（恢复 1）’后缀，零交互”；
  但 F3 现行代码（`core/project-management/src/view_models.rs` 的 `restore_plan`）遇到重名走“拒绝并报 RestoreNameConflict”逻辑。
  待回收站 API 接线单修正，不归 T19B-9。

## 📏 每单统一收工纪律（写进每份子工单，此处再强调）

1. 全套门禁：cargo test --workspace / xtask tidy / xtask arch / clippy -D warnings /
   fmt --check / machete / deny check advisories bans licenses sources
2. git push origin main 后 GitHub Actions conclusion 绿灯——唯一完成标准，汇报附链接
3. zh-CN.json 高冲突文件：文案改动单独 commit
4. 完成后勾选子工单勾选项、更新本索引"状态"列，并同步 apps/desktop/STATUS.md

## 🎯 全系列完成标准（负责人验收）

T19B-8 收工后，负责人双击桌面"校园复刻工具 - 开发版"，亲手走通四步剧本：
建方案 → 圈边界采一小块 → 评审一条候选 → 导出拿到真实 .schem 文件。
随后进入 T20 贯穿弹验收（端到端集成测试 + 四步剧本正式验收）。
