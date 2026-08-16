# review-map-usability — 评审工作台地图可用性（已完成）

> 来源：2026-08-14 真机验收发现真实环境缺陷，产品负责人曾决定暂缓记为债务。
> 2026-08-15 由独立实现修复并通过验收，已合入 `main` 并推送。
> **Status：completed（2026-08-15，commit `77abd12`，`origin/main` = `77abd12`）。**

## 原缺陷与根因

1. 六类选项卡显示不完整（“植被”等文字被截断）
   → 标准 Button 平台内边距挤压文案；改用自定义轻量 `ReviewCategoryButton`
   （固定 3×2、文字不省略不缩写）。
2. 评审地图候选虚线/实线画不出来
   → 根因：Rust 推数组 `setReviewCandidates([{...}])`，旧 JS 用
   `JSON.parse(candidatesJson)` 解析非字符串 → 解析失败、候选全空。
   修复：JS 直接接收数组 + `Array.isArray` 校验 + 安全错误回传
   （`review_map_draw_failed:<stage>`，不泄露坐标/ID）。
3. 定位到地图无反应
   → 候选未绘制时无 centroid 可跳；修复 `pendingLocateId` 队列 + 明确反馈
   （剔除=hidden、找不到=unavailable）+ 绘制后 `setFitView` 框住候选。

## 验证

- 定向：s1_24 / s1_25 / review_map_js_spy、gaode-client review_map 12 passed。
- `cargo test --workspace`：637 passed / 0 failed / 5 ignored。
- fmt / clippy 通过；`main` = `origin/main` = `77abd12`（已推送）。

## 相关位置

- 提交：`77abd12`（fix(review): restore review map usability）。
- 关键文件：`apps/desktop/ui/review.slint`、`core/gaode-client/src/review_map_page.rs`、`apps/desktop/src/production/review.rs`。
