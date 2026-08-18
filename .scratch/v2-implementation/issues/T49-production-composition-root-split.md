# T49 — 全局导航转移收口

**Status:** completed（2026-08-18，产品负责人验收后本地合并，提交 `a4867d6`；
未推送 GitHub）

**What to build:** 只收口全局导航的呈现层内部结构：

1. 在 `apps/desktop/src/production/` 建立导航策略模块 `navigation.rs`，集中拥有：
   - 历史栈与 stackable 规则（从哪儿进、从哪儿出；首启向导不入栈）；
   - 返回/离开/确认/取消路由：输入“当前屏幕 + 返回/离开意图 + 工作区入口的离开
     安全判定”，输出结构化转移结果（显示目标页 / 需要确认 / 停留）；
   - 离开工作区时标记放弃交付上下文（ADR-0042 §6：旧 worker 结果不得回写）；
   - 栈空回退当前校区方案列表与工具栏返回可见性。
2. 离开是否安全仍由 `workspace_leave.rs`（工作区入口侧）判定，语义不变，
   不迁移判定逻辑；`map_session` 接口与地图行为不变。
3. 组合根 `production/mod.rs` 保留构造创建与全部 UI 回调绑定；机械接线
   （回调绑定、弹窗调度、轮询定时器）可在组合根模块内另落 `bindings.rs`。
4. `production/mod.rs` 降到 ≤1000 行并删除第 4 行的 v2.1.0 / 2026-12-31
   期限豁免标记。

**Not blocked by:** T45/T46/T47/T48 已完成或并入 T46；本票经 2026-08-17 重新
评审收窄（产品行为范围以当前主线计划与 ADR-0044/0037/0039 为准），不做旧票的
按设置/方案/工作区/导航等入口大范围拆文件。

## 产品行为范围（产品负责人已确认，不得改变）

1. 逐层返回：从哪儿进、从哪儿出；返回工作区时恢复同一方案、步骤与未保存边界点。
2. 边界未保存或采集/导出进行中，点返回/离开先弹确认，说明会放弃未保存修改或
   中断中的操作。
3. 确认弹窗点“取消”：留在原步骤，进行中的采集/导出继续。
4. 工作区从零进入（无上一页）时返回按钮仍显示，点击回当前校区方案列表。
5. 本次只整理内部结构，用户看到的页面、按钮、流程与现在完全一样。

## 验收与验证

- [x] 既有导航/离开确认契约测试全过（真实回调到最终 Screen，不只属性断言）：
  `s1_13_leave_workspace_confirmation`、`s1_31_toolbar_navigation_back_stack`、
  `s1_21_notification_tutorial_contract`。
- [x] `navigation.rs` 单测覆盖：压栈/弹栈、stackable、栈空回退、from_back 与
  正向跳转区分、确认/取消路由、放弃交付上下文标记。
- [x] `workspace_leave.rs` 无逻辑改动；`map_session` 无改动。
- [x] `production/mod.rs` ≤1000 行，v2.1.0 / 2026-12-31 期限标记删除；
  `xtask tidy` 通过。
- [x] 不按入口拆分；构造与回调绑定仍在组合根（`bindings.rs` 属组合根模块内部）。

## 定向验证（T41 分级）

- 定向测试：`.\scripts\cargo-managed.ps1 -- test -p desktop-shell`
- 代码规范：`.\scripts\cargo-managed.ps1 -- clippy -p desktop-shell --all-targets`
- 格式：`.\scripts\cargo-managed.ps1 -- fmt --all --check`
- 规模/卫生：`.\scripts\cargo-managed.ps1 -- xtask tidy`
- 升级门禁触发项：无（desktop-shell 同 crate 私有重构，不动 Cargo/依赖/公共 API/
  schema/架构规则），machete/deny/workspace 全量/timings 由 V2.1 收口统一跑。

## 实施证据（2026-08-17，未提交交付验收）

工作树：`worktrees/t49-navigation-transfer`（`refactor/t49-navigation-transfer`，
HEAD `00f378a` = 本地 main）；已快进合并入本地 main（提交 `a4867d6`，未推送）。
改动文件：

- `apps/desktop/src/production/navigation.rs`（新增，导航策略 + 7 个单测）
- `apps/desktop/src/production/bindings.rs`（新增，机械接线：回调绑定/弹窗调度/
  轮询定时器）
- `apps/desktop/src/production/mod.rs`（983 行，删除 v2.1.0 / 2026-12-31 期限标记）
- `.scratch/v2-implementation/issues/T49-production-composition-root-split.md`
- `.scratch/v2-implementation/v0.1-end-to-end-mainline-plan.md`
- `workspace_leave.rs` 与 `map_session` 零改动（git diff 无输出）

定向门禁（全部经 `scripts/cargo-managed.ps1`，`SLINT_BACKEND=software`、
`CARGO_BUILD_JOBS=2`）：

```text
cargo check -p desktop-shell              通过（22s）
cargo test -p desktop-shell               除 s1_30_fetch_stability 外全部通过；
                                          该测试在洁净 main 基线同样失败
                                          （断言“超过 15 秒必须显示阶段与已耗时”
                                          为空 + 0xc000041d），属既有环境问题，
                                          与本次重构无关
cargo test -p desktop-shell -- --skip fetch_stability   全部通过（含 s1_13 2/2、
                                          s1_31 2/2、s1_21 1/1、navigation 7/7）
cargo clippy -p desktop-shell --all-targets  通过（30s，0 warning）
cargo fmt --all --check                   通过
cargo xtask tidy                          通过（行数红线 / 模块文档 / 半成品禁令）
git diff --check                          通过（无空白错误）
```

升级门禁触发项：无（同 crate 私有重构）。machete/deny/workspace 全量/timings 由
V2.1 收口在最后一次代码改动后统一运行。

## 最终收口证据（V2.1 收口时补）

验收通过并授权合并后，在此绑定 HEAD + diff fingerprint 补记完整门禁与 timings
证据；本票工作树保持未提交，不以未授权合入冒充完成。
