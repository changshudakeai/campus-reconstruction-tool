# 评审抽屉隐藏展开详情卡

**Status:** ready-for-human（2026-08-17，独立工作树 `review-detail-card-hide`）

**What to build:** 隐藏评审抽屉内选中候选后出现的展开详情卡，把纵向空间还给候选
列表和三态审核操作。候选卡片标题、地图与卡片双向高亮、定位、三态、批量操作、
暂停/恢复和封账行为保持不变。

## 验收与验证

- [x] 抽屉不再渲染展开详情卡；候选列表在同一窗口高度下获得更多可见空间。
- [x] 点地图对象、点候选卡片和“定位到地图”仍高亮同一候选。
- [x] 三态、批量操作、分页和封账不受影响。
- [x] 定向 UI 布局契约与评审流程测试、desktop-shell Clippy、fmt、tidy、
  `git diff --check` 通过。
- [x] 纯 UI/测试/行为基线修改不触发 timings/machete/deny；版本收口再跑完整门禁。

## 证据

- 原生开发版在 `802 × 631` 的受限窗口中通过人工检查：详情卡消失，候选三态、
  分页、批量和底部评审动作均可见；视觉对照见根目录 `design-qa.md`。
- 红绿测试：新增 `s1_36_review_detail_card_layout`，修改前因仍存在详情卡失败，
  修改后通过。
- 评审流程定向测试：`s1_16_review_flow`、`s1_24_review_drawer_map_contract`、
  `s1_36_review_detail_card_layout` 共 3/3 通过。
- 滚动/大量候选契约：`s1_22_review_scroll_contract`、
  `s1_25_review_performance_contract` 共 2/2 通过。
- `cargo clippy -p desktop-shell --all-targets -- -D warnings`、
  `cargo fmt --all --check`、`cargo xtask tidy`、`git diff --check` 均通过。
- 未运行 workspace 全量、timings、machete 或 deny；按 T41 分层门禁留到版本收口。
