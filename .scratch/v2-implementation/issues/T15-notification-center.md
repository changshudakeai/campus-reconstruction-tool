# T15 — B7 通知中心（弹窗铁律 + 公告栏铃铛）

**What to build:** 影响数据质量或卡住流程的错误必须模态弹窗禁用横幅；消息三级分派（弹窗/toast/仅留底）；全应用一本账的铃铛公告栏、消息带来源标签、只读可复制、最近 200 条或 30 天自动清理。

- **窗口契约**：壳向 B7 要错误分级 API；功能模块把错误结论交给 B7；B7 负责呈现方式和历史存储。
- **业务规则**：弹窗铁律（ADR-0021）适用全应用；留存策略为最近 200 条或 30 天先到为准；消息不可删除、只读、可复制链接或文本。
- **API 设计**：`notify::error(...)` / `warn(...)` / `info(...)` 分别对应弹窗/toast/留底。

**Blocked by:** T01, T02（crate 框架 + 共享类型）、T03（文本外置）

**Status:** completed

- [x] notification-center crate 立项并实现通知数据结构（level, title, body, source_tag, created_at）
- [x] 弹窗 API 实现（模态对话框阻塞主线程直到关闭）
- [x] Toast 组件实现（右上角浮动提示几秒后自动消失）
- [x] 公告栏 UI 实现（铃铛图标 + 消息列表 + 复制按钮）
- [x] 消息存储 API（SQLite 或内存表 + 自动清理逻辑框架）
- [x] 自动清理调度器实现（每 5 分钟扫描一次超过 30 天的消息）
- [x] public-api 快照测试 + 初始快照入库
- [x] 单元测试：调用 notify::error(...) → 断言弹窗出现且阻塞后续操作

---

## 实施备注（2026-07-26）

- crate 位于 `core/notification-center/`，已入 workspace members；
  `cargo test --workspace` 全绿，`cargo xtask tidy` / `arch` 通过，
  clippy 在 `-D warnings` 档下干净。
- UI 呈现走 `Presenter` 接缝：slint 只准壳依赖（deny.toml bans），
  故弹窗/toast/铃铛的 Slint 声明由壳实现 trait 接入；B7 交付分派逻辑、
  阻塞语义、留底、未读计数（点开即清零）与复制文字出口
  （`Notification::clipboard_text`）。
- 存储按约束用内存表（未碰 core/data-persistence），`Storage` trait
  是 T11 就绪后迁 SQLite 的接缝；留存规则 200 条/30 天先到为准。
- 未碰 core/localization；B7 不产文案，标题/正文由调用方递入成品文字
  （窗口契约缝 1），测试中临时用中文硬编码样例。
- 阻塞铁律测试见 `tests/popup_rule.rs`：断言 error() 耗时 ≥ 弹窗停留时长，
  且事件顺序为 弹窗打开 → 弹窗关闭 → 后续代码恢复；Error 级绝不走 toast。

---

## 负责人验收点（一句话）

报错的时候不是右下角闪一下那种横幅，而是弹个框停在那等你点"知道了"，右上角有个铃铛能看你之前发过的所有提醒。

