# T45 — 整理评审呈现 adapter 的私有地图实现

**Status:** completed（2026-08-17，产品负责人授权提交并入本地主线）

**What to build:** 保持现有深的 `ReviewProductionAdapter` 与外部 interface 不变，
不为轻量建议翻译建立浅 module；仅把 adapter 已有的地图回推、重放与创建实现整理
进既有私有 `review_map.rs`，删除 `production/review.rs` 的文件长度期限豁免。不得改变
评审三态、轻量建议筛选/一键应用/撤销、地图标注、回调契约或用户文案。

**Blocked by:** T41.

## 验收与验证

- [x] `review.rs` 保持完整评审请求到页面/通知的深 adapter；`review_map.rs` 只拥有
  评审地图可见集合、回推、重放与创建实现，外部 interface 不扩大。
- [x] deletion test 证明未新增建议翻译 module 或单 adapter 的假 seam。
- [x] desktop-shell 相关 review 定向测试 + crate Clippy + fmt + `xtask tidy`。
- [x] 同 crate 私有搬分不触发 timings；PR 收口完整门禁一次。

## 实施证据（2026-08-17）

- `review.rs` 由 1066 行降为 939 行并删除文件长度期限豁免；既有私有
  `review_map.rs` 由 125 行增至 251 行，未新增 module。
- `ReviewProductionAdapter` 对外仍只有 struct、`context` 与 `new` 三项 crate 内
  interface；`PresentationAdapter::present` 的请求、回调与返回类型未变。
- deletion test：若新增“建议翻译”module，删除后只会把少量转发放回单一 adapter，
  因此拒绝该浅 module；既有 `review_map.rs` 被删除时，可见集合、增量回推、重放和
  地图创建会重新散回 adapter 多处调用，故把三段地图实现收拢到该私有实现文件。
- 定向测试：`presentation_seams`、`s1_16`、`s1_17`、`s1_18`、`s1_24`、
  `s1_25`、`s1_32` 共 7 项全部通过，覆盖三态、批量/封账、建议筛选/应用/撤销、
  空态失败、地图同步/分页和边界变化后重新进入评审。
- desktop-shell `--all-targets` Clippy（`-D warnings`）、`fmt --all --check`、
  `xtask tidy` 与 `git diff --check` 通过。因总缓存超过自动回收水位且本任务未授权
  清理，Clippy/fmt/tidy 使用同一全局 Cargo 锁和当前工作树独立 target 运行，但未
  调用缓存维护。
- 按 T41 未运行 workspace 全量、timings、machete 或 deny；留待 PR/版本收口一次。
