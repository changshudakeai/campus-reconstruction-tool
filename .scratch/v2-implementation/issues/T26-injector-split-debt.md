# T26 — injector.rs 拆分（架构债，另立）


**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

> 来源：T19B-5B Step B/C 代码审查遗留；负责人已确认归属"另立工单"，
> 不进高德接入批次（PRD-gaode-map-integration.md 附注 · 技术债归属）。
> **Status：backlog——暂不动工，待高德批次完成后排期。**

**What to build：**
injector.rs 在 T19B-5B 期间超过 1000 行 tidy 红线（当时用豁免注释放行），
并积累两个代码气味：Middle Man（4 个纯转发访问器）、Repeated Switches
（步骤条与卡片各自独立分支）。本单把 injector.rs 按绑定域拆成独立模块
（如计划列表绑定、工作区绑定、对话框绑定），消除豁免注释与两处气味。

**Blocked by：** 建议排在 T25 之后（高德批次还会持续改动 injector.rs，
提前拆分会反复冲突）。


## 验收标准（排期时细化）

- [ ] injector.rs 回归 1000 行 tidy 红线内，`ignore-tidy-filelength` 豁免删除
- [ ] Middle Man 访问器与 Repeated Switches 气味消除（审查复核）
- [ ] 拆分前后行为一致：既有手工验收路径全过
- [ ] 通用收工标准（门禁 / CI / 双扫 / 诚实声明）
