# 19 — 收窄 s1-05 tidy 行数豁免（backlog）

**来源：** s1-05 交付遗留（2026-08-01）：production/mod.rs、production/workspace_boundary.rs、ui/main.slint 带显式 ignore-tidy-filelength。
**Status:** backlog

**What to build：** 随工单 06/07/08/09 迁出采集/评审/导出与朝向交互后，逐文件收窄或摘除这三处行数豁免，回归 1000 行红线。

**验收标准：**
- 三处豁免在对应迁出后摘除，tidy 全绿且无豁免标记残留
- 摘除时机以对应工单为准；不得为凑行数做无意义拆分（ADR-0017 粒度适中）