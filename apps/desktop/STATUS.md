# T19 — S1 薄壳实现状态（T19B-1 后）

> 本文件随 T19B 系列子工单逐单更新，反映仓库真实状态。

## ✅ 已落地（全部过本地门禁 + 可运行）

### Phase1（T19）

- `apps/desktop` crate 立项，Cargo.toml 依赖段即白名单
- `ui/main.slint`：最小根窗口，零硬编码文案，属性由 Rust 侧填充
- `build.rs`：slint-build 编译 UI 生成绑定（`slint::include_modules!`）
- `runtime::landing_decision`：首开着陆判定（首次向导 / 老用户直达 /
  校区选择），判定委托 F1 `SettingsManager`，附单元测试
- zh-CN.json `app.shell_status_*` 三个文本键
- deny.toml：slint 家族 Royalty-Free 许可豁免（见 deny.toml 内注释）

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

## 🚧 剩余接线债务（归 T19B-2..8，勿当已完成）

1. 页面级导航骨架（步骤条向导式，ADR-0027）与各页属性/回调绑定
2. F4→F7 采集报告入口、F2 教程钩子与"重新查看教程"按钮
3. F9 真封账门控（壳实现 SealGate 调 F5 seal）+ B7 Slint Presenter 注册
4. 开发版快捷方式端到端验收（`cargo xtask dev-shortcut` 已有实现）
5. public-api 快照 + CODEOWNERS 守卫 .lnk 文件

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
