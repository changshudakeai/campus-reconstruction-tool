# T19B-1 — Shell 基础框架与 VM 注入机制

> **历史实现说明（2026-08-01）**：本单完成于 ADR-0037 收紧之前。“持有所有 F 模块”和旧依赖白名单仅是当时事实，不授权 `ViewModelInjector` 在运行期协调多个功能。后续迁移中组合根只保留构造期注入和回调绑定。

**What to build**: 薄壳应用壳的"连接桥接能力"——让 Shell 能访问所有 F 模块的 ViewModel，但不写任何业务逻辑本身。这是后续所有 UI 工单的基石。

- **窗口契约**: 缝 1 的基础设施层（S1 ↔ F1-F9）；Shell 通过 Rust 侧组装各模块 VM 实例并注入 Slint 属性
- **业务规则**: 薄壳原则 (ADR-0017)：Shell 自身不依赖 B12-B16（ETL/GIS 模块必须经功能模块中转）

**Blocked by**: None — can start immediately

**Status**: completed（2026-07-26，GitHub Actions conclusion success — https://github.com/changshudakeai/campus-reconstruction-tool/actions/runs/30209730920）

## 🎯 验收标准

### 核心交付物
- [x] `apps/desktop/src/lib.rs` 包含完整的 VM 注入器 `ViewModelInjector` 结构体（实体在 `src/injector.rs`，lib.rs 公开再导出）
- [x] `ViewModelInjector::new(db)` 能够创建并持有所有 F 模块的实例（F1-F9）
  - F1: `SettingsManager` (全局设置)
  - F2: `OnboardingTutorial` (新手教程)
  - F3: `ProjectManager` (方案管理)
  - F4: `AcquisitionPipeline` (数据采集)
  - F5: `ReviewWorkbench` (评审工作台)
  - F7: `QuietSentinel` (覆盖率审计)
  - F9: `ExportConsole` (导出控制台)
- [x] `ViewModelInjector::inject(&self, window: &AppWindow)` 方法将所有 VM 状态和回调绑定到 Slint 组件
  - 只设 in property，不修改 Slint 生成代码的行为逻辑
  - 所有回调函数返回 `Result<()>` 或 `Option<T>`（Phase1 最小窗口无回调；回调错误统一出口 `report_callback_error` 已就位）

### 架构断言（CI 门禁必过）
- [x] `cargo xtask arch` 通过：`desktop-shell` crate 的 member deps 完全落在白名单内 `[F1-F9, B2, B6, B7, slint]`
- [x] `cargo machete` 无未用依赖
- [x] `cargo deny check bans` 无违规跨层调用

### 测试要求
- [x] 单元测试：`test_injector_holds_all_vms()` 断言 Injector 成功持有所有 7 个 F 模块实例
- [x] 单元测试：`test_vm_state_injected_to_slint()` 断言每个 VM 的状态属性都成功设置到 AppWindow
- [x] 单元测试：`callback_error_reaches_b7_board()` 断言回调错误经 B7 公告栏留底

## 📋 实施提示

### 技术要点
1. **VM 注入器模式**：借鉴 Dependency Injection 思想，但由 Rust runtime 直接构造而非 IoC 容器
   ```rust
   pub struct ViewModelInjector {
       f1: SettingsManager,
       f2: OnboardingTutorial,
       f3: ProjectManager,
       // ... 等等
   }
   ```

2. **回调转换**：Slint 的 `on_CLICKED` 等信号需要适配成 Fn 闭包，闭包内部调用 VM 的方法并将 Result 转成错误分派到 B7

3. **零业务逻辑验证**：Clippy 扫描 `apps/desktop/src/` 下不得出现业务判断 if/else（除着陆判定外）

### 避免的错误
- ❌ 不要在 Shell 里写 any kind of business logic like "if user has no plans then show create button"
- ✅ VM 应该主动提供 "should_show_create_button()" 这样的视图状态供 Shell 绑定

### 相关文件
- `apps/desktop/Cargo.toml`: 确保依赖段已列出 F1-F9 crates
- `docs/research/module-boundary-enforcement.md`: 参考执法配置

## 💡 特别提示
这是 T19B 系列中最基础的一个工单，做完后后续的 9 个工单都能"拿到钥匙开门"。如果这里没做好，后面会反复遇到"怎么 access F3 的数据"这种问题。

✅ **收工自检清单**:
- [x] `cargo check` 全 workspace 无报错
- [x] `cargo test --workspace` 本工单新增的测试通过
- [x] `cargo xtask arch` 架构测试绿灯
- [x] `cargo run -p desktop-shell --bin campus-tool-dev` 能编译启动（哪怕只显示"占位文案"）

---

## 实施备注（2026-07-26）

- **`new(db)` 的 db 实形是 `ShellDatabases` 连接组**：B2 `Database` 有意不可
  `Clone`，而 F1 `SettingsManager` 与 F3 `ProjectManager` 都按值持有句柄，
  故壳对同一数据库文件开两条连接；F2/F5/F7 的落库统一借道
  `ProjectManager::database_mut()`（F3 公开 API 自带的接缝，未扩任何 F 模块 API）。
- **F5 以进台会话持有**：`ReviewWorkbench::load` 需要 plan_id（启动时尚无方案），
  注入器提供 `enter_review(plan_id)` 装载会话槽，测试已验证持有。
- **F9 门控暂用 `MockSealGate` 占位**：原计划“壳实现 `SealGate`、内部调 F5”已被 ADR-0037 废止；真门控由非 S1 业务适配器实现并在构造期注入 F9，归 T19B-8 替换。
- **F6/F8 不存在**：ADR-0017 目录中功能模块为 F1-F5/F7/F9 共 7 个，工单所列
  7 实例已全部持有。
- 未发现需要扩充的 F 模块公开 API；回调错误统一出口 `report_callback_error`
  （B7 error 模态，新增文本键 `app.source_tag`）随本单就位。

---

## 负责人验收点（一句话）

跑一次 `cargo run`，如果能在 `/src/` 里看到所有 F 模块的实例都被 Inject 到 Shell 且编译通过，就算完成。
