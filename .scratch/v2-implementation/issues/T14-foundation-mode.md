# T14 — B5 地基模式引擎（边界绘制 + 朝向两点参考线）

**What to build:** 在地图上圈画方案边界、画两点参考线设定朝向；朝向为必选步骤无默认值；修改朝向触发重算并明确告知影响范围。

- **窗口契约**：缝 7（地图服务 ↔ F3/F4 ↔ B3/B5）。F3 向 B5 要朝向计算；B5 提供坐标系转换子模块（内部实现，不独立成 crate）。
- **业务规则**：朝向=高德地图上画两点参考线；既定交互不变；修改朝向需提示哪些已生成数据会重算。
- **坐标系转换**：投影变换（高德经纬度 → 平面坐标）+ Minecraft 世界坐标映射为 B5 内部子模块。

**Blocked by:** T02（共享类型定义）、T03（文本外置用于错误提示）

**Status:** completed

- [x] foundation-mode crate 立项并实现边界绘制 UI 组件（Slint 多点触控或鼠标拖拽）——事件驱动状态机 `BoundaryDrawer`，壳层把 Slint 指针/触控事件转成 `BoundaryUiEvent` 送入（缝 1：壳零业务逻辑，Slint 渲染层待壳 crate 立项后绑定）
- [x] 朝向两点参考线绘制 UI 组件——`OrientationLine`（两点重合拒收）+ `OrientationCalculator`
- [x] 坐标系转换算法实现（高德 Mercator 投影 → 平面米单位 → MC 块坐标）——`coordinate` 内部子模块，含纬度畸变纠正与比例尺
- [x] 朝向角度计算逻辑（两点连线与正北夹角）——产出 B1 `Orientation`（T02 类型复用，0~360 范围校验由 B1 把关）
- [x] 边界有效性验证（多边形闭合检查）——B5 内部实现（≥3 点 + 自动闭环 + 面积阈值）。注：原文"调用 B14 空间索引子模块"与 ADR-0017 依赖 DAG 冲突（基础层横向零依赖，B5→B14 被 xtask arch 拒收），且 B14 尚未立项；自相交等拓扑检查待 B14 立项后由上层功能模块编排
- [x] 朝向修改影响的警告弹窗逻辑——`check_orientation_change_impact` 产出影响报告（类别沿用 B1 `CandidateCategory`，建筑/体育需二次确认）
- [x] public-api 快照测试 + 初始快照入库（tests/public_api.rs + tests/snapshots/public-api.txt）
- [x] 单元测试：给定四点边界 → 断言面积计算正确（正方形/梯形/校园尺度；另含 -90°/400° 范围校验测试）

验收记录（2026-07-26）：cargo test 全绿（foundation-mode 42 个测试；workspace 除在途的 notification-center 外全绿）；cargo xtask tidy / arch 通过；clippy 零警告。文案暂为中文硬编码（基础层禁依 B6，由壳层经 B6 解析文本键，已在代码注释标注）。

---

## 负责人验收点（一句话）

在地图上能用手画一个框当边界，再点两个点定朝向，改朝向的时候弹窗告诉你"这会重算你之前画的 XX 栋楼"。

