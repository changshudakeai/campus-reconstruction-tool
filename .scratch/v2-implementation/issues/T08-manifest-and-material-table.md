# T08 — B17 Manifest 生成器与用料表配置

**What to build:** 导出时自动生成 foundation_manifest.json，如实记录本次包含/缺失哪些类别（建筑✓、水域✗、其他✓等）；用料表集中配置且与 MC 版本强绑定（只准用目标版本存在的方块）。

- **窗口契约**：缝 6（生成流水线 ↔ F9 → B18 → B4 + B17）。F9 把评审保留数据交给 B18 生成引擎后，B17 同时读取评审终态 + 方案信息生成 manifest。
- **业务规则**：manifest 格式符合 ADR-0012 要求；用料表按受支持 MC 版本各一张表或主表 + 降级替换规则。
- **数据结构**：manifest 包含版本号、校区名、方案名、各类别包含状态（包含的列表、缺失的列表）、导出时间戳。

**Blocked by:** T01, T02（共享类型 + crate 框架）、T03（文本外置用于错误提示）

**Status:** completed（2026-07-26 commit 175411e：B17 manifest-generator 已立户并实现）

- [ ] manifest-generator crate 立项并实现 manifest 数据结构定义（含 categories 字段）
- [ ] 用料表配置结构定义（BuildingBlocks { wall, roof, window ... }）按 MC 版本区分
- [ ] manifest 生成逻辑：读取评审终态（B2）+ 方案信息 → 输出 JSON 文件
- [ ] 用料表验证逻辑：请求的方块在目标版本是否存在检查
- [ ] public-api 快照测试 + 初始快照入库
- [ ] 默认用料表沿 v1.x Arnis 规则（需调研 v1.x arnis-core）

---

## 负责人验收点（一句话）

导出的 .schem 旁边有个 foundation_manifest.json，打开能看到"建筑：包含，水域：缺失，其他：包含"这样的清单。

