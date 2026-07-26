# T19 — S1 薄壳实现状态（Phase1，CI 修复后）

> 本文件在 CI 修复提交时重写：原 Phase1 报告所述内容从未通过本地编译，
> 与仓库实际不符（详见修复提交信息）。以下为当前真实状态。

## ✅ Phase1 实际落地（全部过本地门禁 + 可运行）

- `apps/desktop` crate 立项，Cargo.toml 依赖段即白名单
  （slint / localization / global-settings / data-persistence / anyhow）
- `ui/main.slint`：最小根窗口，零硬编码文案，属性由 Rust 侧填充
- `build.rs`：slint-build 编译 UI 生成绑定（`slint::include_modules!`）
- `runtime::landing_decision`：首开着陆判定（首次向导 / 老用户直达 /
  校区选择），判定委托 F1 `SettingsManager`，附单元测试
- `run_dev()`：装配主窗口、经 B6 注入标题与状态文案、进入事件循环；
  `campus-tool-dev.exe` 实测可启动并显示窗口
- zh-CN.json 新增 `app.shell_status_*` 三个文本键
- deny.toml：slint 家族 Royalty-Free 许可豁免 + 传递依赖许可与
  停维护公告留痕豁免（见 deny.toml 内注释）

## 🚧 T19B 接线债务（未做，勿当已完成）

1. F1-F9 ViewModel 全量接线与页面级导航
2. F4→F7 采集报告入口、F2 教程钩子与"重新查看教程"按钮
3. 开发版快捷方式端到端验收（`cargo xtask dev-shortcut` 已有实现）
4. public-api 快照 + CODEOWNERS 守卫 .lnk 文件

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
