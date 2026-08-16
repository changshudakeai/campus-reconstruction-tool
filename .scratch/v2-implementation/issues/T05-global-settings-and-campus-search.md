# T05 — F1 应用全局设置与 B3 高德地图客户端（校区搜索）

**What to build:** 首次打开时弹出设置页选择语言和 Minecraft 版本（默认中文/26.1.2），勾选知情告知；再次打开直接进入上次校区的方案列表页，顶部显示校区名 + "切换校区"按钮。通过高德地图搜索选定校区而非手动创建。

- **窗口契约**：缝 7（地图服务 ↔ B3/B5）。F3/F4 向 B3 要高德 API 封装、坐标拾取、边界绘制。
- **业务规则**：语言与游戏版本为应用级全局设置；上次校区被删则退回校区选择页；校区必须显式确认而非自动进入。
- **高德集成**：使用官方 CDN，地图最小高度 300px；搜索 POI 返回候选列表；点选后显示详情确认。

**Blocked by:** T02, T03（共享类型 + 文本外置）, T11（B2 持久化——"上次使用的校区"读写）

**Status:** completed

- [x] global-settings crate 立项并实现语言/MC 版本的读写 + 首次设置页逻辑（`SettingsManager` + `FirstRunSetup`，知情告知未勾选则拒绝完成）
- [x] gaode-client crate 立项并实现高德 POI 搜索 API 调用（官方 CDN v1.2；搜索在地图页 JS 侧执行，结果经桥接回传由 `parse_place_search_response` 解析 + 教育类目筛选 + 去重）
- [x] 高德地图 UI 组件集成（B3 交付 `build_map_page_html` WebView 地图页，最小高度 300px；壳层 WebView 嵌入属 T19 desktop-shell 职责——deny.toml 规定 slint 只准壳直接依赖）
- [x] 搜索候选列表展示 + 详情确认流程（`CampusSearchFlow`：候选 → 详情 → 显式确认唯一出口，跳过详情直接确认被拒）
- [x] "上次使用的校区"回读逻辑（`landing_campus`/`remember_campus`，被删/未设置 → None 回退校区选择页，集成测试覆盖）
- [x] 校区 POI 数据持久化（`CampusPoiRecord`：POI identity + coordinate lineage（GCJ-02/gaode）serde 载荷；落库由 F3 经 B2 完成，T05 不动 data-persistence）
- [x] public-api 快照测试 + 初始快照入库（两个 crate 各带 tests/public_api.rs + tests/snapshots/public-api.txt）

---

## 负责人验收点（一句话）

第一次打开先让你选语言和 MC 版本，勾个框说"我知道用途了"；第二次打开直接进上次校区的方案列表，顶上有校区名字和个"换个校区"的按钮。

