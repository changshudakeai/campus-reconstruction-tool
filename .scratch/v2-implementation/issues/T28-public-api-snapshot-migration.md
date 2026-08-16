# T28 — 其余 17 个基础 crate 的 public-api 快照机器比对迁移（backlog）

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

> 来源：2026-07-31 文档审计（B1 已按 enforcement.md 模板接入机器比对，其余 crate 仍是手写存在性测试）。
> **Status:** backlog

**What to build:** 把 core/ 下其余基础/功能 crate 的 tests/public_api.rs 从手写存在性断言迁移为 rustdoc-json + public-api assert_eq_or_update 模板（同 B1），重新生成各自 tests/snapshots/public-api.txt，并对照 PRD/ADR 核对公开面。

**验收标准：**
- 每个 crate 的 public_api 测试在 API 增删时失败，UPDATE_SNAPSHOTS=yes 可更新快照
- 快照 diff 进入 PR 评审；不新增/删除/改名任何公开 API（仅记录发现）
- cargo test --workspace / tidy / arch / clippy -D warnings / machete / deny 全绿
- CI test job 已有 nightly-2025-08-02 与 SLINT_BACKEND=software，无需再改

**注意：** 依赖版本以 B1 实测配套为准（public-api 0.50.x + rustdoc-json 0.9.10 + nightly-2025-08-02）。
