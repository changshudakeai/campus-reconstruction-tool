# T10 — B18 初始校园生成引擎（Arnis 规则 + 用料表）

**What to build:** 评审保留数据 → 方块模型；承载全部生成规则（建筑规则：height 优先/层数×4+2 估高/屋顶规则；六类生成规则）。用料表与 MC 版本强绑定，查不到目标版本方块就报错而非替换。

- **窗口契约**：缝 6（生成流水线 ↔ F9 → B18 → B4）。F9 把评审保留数据交给 B18 生成引擎；B18 产出纯内存方块模型后传给 B4 落盘。
- **业务规则**：建筑高度按 data.source.height 标注优先、无则按层数×4+2 估算；屋顶形状规则沿用 Arnis/v1.x；其他类按标签家族生成（铁路→铁轨）。
- **复用来源**：v1.x arnis-core crate 迁入作地基（需调研 v1.x arnis-rule-lineage.md）。

**Blocked by:** T01, T02（crate 框架）, T08（用料表数据结构）

**Status:** completed

- [x] generation-engine crate 立项并从 v1.x 迁移 arnis-core 建筑规则逻辑
- [x] 高度计算规则实现（height field 优先、levels×4+2 fallback）
- [x] 屋顶形状规则实现（根据建筑类别选择不同屋顶形态）
- [x] 六类生成规则接口定义（BuildingGenerator, RoadGenerator ...）
- [x] 用料表查询 API：给定 MC 版本 + 类别 → 返回可用方块列表（不存在则 panic/error）
- [x] public-api 快照测试 + 初始快照入库
- [x] 单元测试：给定带 height=15 的建筑候选 → 断言生成的楼高为 15 层

---

## 负责人验收点（一句话）

B18 只负责算出一个大楼应该用哪些方块、排成什么样，不碰文件 IO；如果用料表里没有这个 MC 版本的墙，它直接报错而不是偷偷换一个。

