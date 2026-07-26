# T19 — S1 主程序应用壳（薄壳 UI + 开发版快捷方式）

**What to build:** 零业务逻辑的 Slint 声明 + ViewModel 绑定；老用户着陆方案列表页；固定桌面快捷方式"校园复刻工具 - 开发版"每次构建后自动更新，保留上一可用版本兜底。

- **窗口契约**：壳向 F1-F9/B2-B11/B17 要 ViewModel 状态和可执行操作回调；壳自身不依赖 B12-B16（ETL/GIS 模块必须经功能模块中转）。
- **业务规则**：薄壳原则（ADR-0017）：Slint 文件里不能写业务逻辑；依赖白名单严格限制在 [F1-F9, B2-B7, B9-B11, B17, slint]。
- **开发版快捷方式**：build.rs 或 xtask 子命令在构建后自动创建/更新快捷方式；旧版本备份到 backup/ 目录。

**Blocked by:** T01/T02/T03（三层锁 + 共享类型 + 文本外置先行完成）、T04-T18（功能模块全部就绪）

**Status**: phase1-complete（CI 修复后重新核实，见 apps/desktop/STATUS.md；原 Phase1 提交 f2b7b47 从未过编译，已重写为可运行最小壳）
**Phase2-pending**: [待办任务清单](#待办接线债务-t19b)

- [x] desktop-shell crate 立项并定义根 .slint 入口文件 ✅ Phase1
- [ ] Slint 项目集成所有功能模块的 ViewModel ⏳ T19B（Phase1 仅接 F1 着陆判定）
- [x] 首次打开流程逻辑：着陆判定（首次向导 / 老用户直达 / 校区选择）✅ Phase1
- [x] 老用户二次启动逻辑：经 F1 landing_campus 判定去向 ✅ Phase1（页面导航待 T19B）
- [ ] 开发版快捷方式端到端验收 ⏳ T19B（build.rs 方案不可行——链接前拿不到 exe，改用 cargo xtask dev-shortcut）
- [ ] CODEOWNERS 配置守卫 .desktop/.lnk 相关文件 ⏳ T19B
- [ ] public-api 快照测试 + 初始快照入库 ⏳ T19B
- [x] 架构测试断言验证：desktop-shell 不依赖 B12-B16 ✅ xtask arch 已覆盖并通过

---

## 🚧 待办接线债务 (T19B)

以下 4 项是 Phase1 遗留的 UI 细节接线任务，将在后续会话中完成：

| # | 债务项 | 具体内容 | 依赖模块 |
|---|--------|----------|----------|
| 1️⃣ | F4→F7 接线 | 采集页底部按钮点击后打开 F7 报告视图（`audit.report_entry` + `AuditReportView`） | T16 |
| 2️⃣ | 设置页→教程重置 | 设置页面"重新查看教程"按钮可见且可点击（调用 F2 重置接口） | T17 |
| 3️⃣ | F2 三个钩子 | 首进方案列表 / 采集完成 / 导出完成的触发逻辑接入 UI | T17 |
| 4️⃣ | CODEOWNERS + API 快照 | `.github/CODEOWNERS`守卫 .desktop/.lnk 文件；public-api 初始快照入库 | T01/T02 |

💡 **处理方式**：建议创建新工单 `T19B-ui-connection.md` 专门处理这些 UI 细节，完成后直接开 T20。

---

## 🎯 负责人验收点（一句话）

双击桌面上的"校园复刻工具 - 开发版"图标就能打开软件，下次构建这个快捷方式会自动更新成新版本。

