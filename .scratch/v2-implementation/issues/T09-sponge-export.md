# T09 — B4 Sponge 导出引擎（.schem 落盘）

**What to build:** 复用 v1.x Sponge 导出引擎，输入评审保留数据 + 用料表 → 输出 .schem 文件（Sponge V3 格式）。

- **窗口契约**：缝 6（生成流水线 ↔ F9 → B18 → B4）。F9 把生成的方块模型交给 B4 落成 .schem；B4 不依赖任何 UI/业务逻辑，只负责文件格式。
- **业务规则**：仅支持 .schem 格式（MC 存档世界已否决）；校园铺在同一水平面（平地 + 高度扩展位）；真实起伏不入 v2.0.0。
- **复用策略**：v1.x `campus-state`中的 Sponge 导出逻辑迁入；保持 Arnis 式完整初始校园的语义（自动起楼、有墙有窗有屋顶）。

**Blocked by:** T01, T02（crate 框架）、T08（用料表数据结构）

**Status:** completed

- [x] sponge-export crate 立项并从 v1.x 迁移 Sponge V3 导出逻辑
- [x] .schem 文件格式解析器与构建器实现（NBT 编码）
- [x] 方块建模接口：由本 crate 自行定义 VoxelModel（方块坐标 + 方块 ID 列表）；类别→方块生成规则属 B18 generation-engine（ADR-0024），F9 将来负责适配
- [x] 平地地形生成逻辑（VoxelModel::flat_ground：边界内的所有格子高度一致）
- [x] 高度字段预留结构（height 维度为一等公民字段，将来起伏无须翻修）
- [x] public-api 快照测试 + 初始快照入库
- [x] 单元测试：给定小范围方块列表 → 断言生成合法 .schem（含往返测试：写出后重新解析与输入一致）

---

## 负责人验收点（一句话）

导出的 .schem 文件能用 MCEdit 或 Sponge 工具打开，能看到里面确实有建筑的方块排列。

