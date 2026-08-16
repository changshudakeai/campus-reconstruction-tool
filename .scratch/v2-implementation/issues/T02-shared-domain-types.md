# T02 — B1 共享领域类型 crate 与公开 API 快照

**What to build:** 全工程唯一的名词定义代码化——校区、方案、候选、六类别、三态评审、封账等术语全部进入 core/domain crate，成为所有模块的共同语言；每个枚举/结构体加 `#[non_exhaustive]` 保护扩展。

- **共享类型**：CampusId, PlanId, CandidateCategory (Building, Road, Water, Vegetation, Sports, Other), ReviewState (Pending, Keep, Remove), Boundary, Orientation, CollectionJobStatus 等。
- **架构约束**：B1 零内部依赖（不依赖其他 crate）；public_api 快照最小化；trait ≤5 方法/模块（本阶段只定义数据，不设 trait）。
- **公开 API 快照**：tests/snapshots/public-api.txt 入 git，任何增删现形于 PR diff。

**Blocked by:** T01 （xtask tidy 需先存在以便验证 public-api 测试格式合规）

**Status:** completed

- [x] core/domain crate 立项并写入 B1 共同语言章的完整类型定义（按 PRD.md 章节）
- [x] 共享枚举添加 `#[non_exhaustive]` 属性（CandidateCategory, ReviewState 等）
- [x] workspace.lints 配置继承 + B1 无内部依赖断言
- [x] cargo-deny 白名单配置（B1 依赖极简）
- [x] public-api 快照测试实现 + 初始快照入库
- [x] B1 模块文档齐全（每文件第一行为//!模块文档）
- [x] B1 行数红线检查通过（<1000 行）

---

## 实施记录（2026-07-26）

### 初版实施（#4c95bca）

- crate 位置：`core/shared-domain-types/`，已入 workspace members。
- 类型与共同语言章对应：CampusId（校区）、PlanId（方案）、
  CandidateCategory（六类别：Building/Road/Water/Vegetation/Sports/Other）、
  ReviewState（三态：Pending/Keep/Remove）、Boundary（边界）、
  Orientation（朝向，无默认值，0~360°范围校验）、CollectionJobStatus（采集任务状态）。
- 三个共享枚举均带 `#[non_exhaustive]`；`CandidateCategory::priority()` 固化
  ADR-0011 冲突优先级（建筑>体育>水域>道路>植被>其他）并有测试断言。
- 验证结果：`cargo test` 全绿（含 serde 往返测试 8 条）；`cargo xtask tidy`、
  `cargo xtask arch`、`cargo clippy`（零警告）、`cargo deny check bans licenses` 全部通过。

### 依赖版本修正（#79b2407）

- public-api 版本从注释掉的 "0.46" 改为实际可用的 "0.52"
- dev-dependencies 配置在根 workspace 中定义并统一继承
- `cargo test -p shared-domain-types public_api` 通过

### 代码评审（初版 #4c95bca）

- **阻断问题**：无
- **建议修复**：Orientation `Deserialize` 添加范围校验 → 已完成并提交到同一 commit

---

## 负责人验收点（一句话）

cargo test public_api 通过且 tests/snapshots/public-api.txt 入 git，PR 中能看到快照 diff。

### 快照机器比对跟进（2026-07-31，T02-followup）

- 原验收缺口：tests/public_api.rs 只做手写类型存在性断言，不比对 tests/snapshots/public-api.txt；
  xtask arch 只检查两个文件存在，快照增删不产生测试失败。
- 已按 enforcement.md 模板接入 rustdoc-json + public-api crate + assert_eq_or_update 机器比对；
  原行为断言移入 tests/domain_behavior.rs（不重复枚举清单）。
- 版本配套（实测）：public-api 0.50.3 ↔ rustdoc-types 0.56 ↔ nightly-2025-08-02（format 55）。
  public-api 0.52.1 的 MINIMUM_NIGHTLY_RUST_VERSION（2025-08-02）与它要求的
  rustdoc-types 0.57.x（format 57）不配套，实测解析失败（上游发布缺陷），故降级到 0.50.3。
- rustdoc-json 0.9.10 不会自动安装 nightly；CI test job 已显式安装 nightly-2025-08-02。
- 快照已由工具重新生成（769 行，含 blanket impl/auto trait，为工具原始输出）。