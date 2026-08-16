# T11 — B2 数据持久化核心（原始观测 + 候选投影 + 评审终态）

> **ADR-0040 hardening（2026-08-01）**：原始观测不再兼任评审候选。既有三表交付保持完成，但 M3 还必须给 B2 增加可重建候选投影、批次发布和“只列可评审候选”的契约；该扩展完成前不得用本单旧的 completed 状态证明 M3 数据链完整。

**What to build:** SQLite 存储项目数据的完整能力——原始观测表（采集时写入、永不删除）、评审终态表（封账时批量写入）、回收站功能。

- **窗口契约**：缝 2/4（方案管理 ↔ 持久化；评审 ↔ 存储）。F3/F4/F5向 B2 要 CRUD API；B2 负责所有表结构、事务控制、迁移脚本。
- **业务规则**：数据粮仓（采集爬回的原始信息永久存库不删）；评审期间零写库、封账一次性批量写回；回收站保留 30 天逻辑框架。
- **数据结构扩展**：从既有 schema 草案扩充原始观测表（raw_observations）、评审终态表（review_decisions）、回收站表（trash）。

**Blocked by:** T01, T02（B2 依赖 B1 的共享类型定义）

**Status:** historical（2026-08-17 v2.0.0 发布收口；不具独立开工权）

### ADR-0040 增补验收

- [ ] 原始观测继续永久保存，候选资格变化不得删除或改写来源证据
- [ ] 候选投影记录稳定候选标识、采集批次、来源观测引用、数据源、类别、规范化几何、验证结果和可评审/已隔离资格
- [ ] 完整候选批次原子发布；半截批次不解锁评审，也不覆盖上一份完整投影
- [ ] 提供“只列可评审候选”与按稳定标识读取生成输入的最小接口；不要求调用方遍历原始观测自行过滤
- [ ] 重新采集“本次未找到”保留上一份已验证投影并重置为待定；本次形状变为不可用时改为已隔离，旧保留决定不得继续导出
- [ ] 多来源和多几何分片不会因“类别 + 实体 ID”碰撞而丢失来源关系

- [x] data-persistence crate 扩展 schema_migrations 表（迁移版本号记录）
- [x] 原始观测表设计并实现（包含建筑/道路/水域/植被/体育/其他六类原始字段 + data_source 标签）
- [x] 评审终态表设计并实现（plan_id, candidate_id, review_state, updated_at）
- [x] 回收站表设计并实现（campus_id, plan_id, entity_type, entity_id, deleted_at, restored_at, permanently_deleted_at）
- [x] 批量写回 API：给定评审终态列表 → 原子写入 review_decisions
- [x] 回收站功能实现（进站/恢复查询/到期自动清理框架/确认后永久删除）
- [x] public-api 快照测试 + 初始快照入库
- [x] 集成测试：模拟一次完整采集 → 断言 raw_observations 表中有记录

---

## 负责人验收点（一句话）

数据库里能看到三张表：一张存采集回来的原始数据（永远不删）、一张存评审决定（只有导出封账时才写进去）、一张存删除的方案（保留 30 天）。

---

## 实施纪要

### 关键决策

1. **UPSERT 语义**: 
   - `raw_observations`: `(plan_id, entity_type, entity_id)` 唯一约束，按 digest 增量刷新，created_at 保持原值，符合"永不删除"铁律
   - `review_decisions`: 同主键 UPSERT，支持重审后状态更新
   - 避免使用 `ON CONFLICT DO NOTHING` —— 我们总是希望内容变化时有所反馈

2. **事务边界**: 
   - 每条 API 都是单事务：`write_raw_observations` / `batch_update_review_decisions`
   - F4/F5/封账调用者无感知原子性，rusqlite 事务 Drop-rollback 兜底

3. **弱引用策略**:
   - `review_decisions.plan_id` 不设 FK CASCADE：与 `raw_observations` 一致，方案删除的清理由后续 F3 计划统一处理（审计轨迹保留）

4. **回收站留痕**:
   - `trash.plan_id` 也去掉 CASCADE，永久删除后仍作为审计记录存在
   - 30 天过期清理只提供框架 (`purge_expired`)，调用时机（启动时/定时）由 F3 决定

5. **类别名编解码**:
   - `CandidateCategory` 非 exhaustive，用 match+fallthrough 保护
   - JSON `source_data` 承载一切，本层不做业务校验

6. **时间戳 RFC3339**:
   - 全工程统一 RFC3339 UTC（带 Z 或 +00:00），SQLite 的 `datetime()` 可解析，chrono 的 `to_rfc3339_opts` 写库
   - 不用 `DateTime::from_timestamp`, 全部 text 往返

7. **Never Delete Discipline**:
   - `data_persistence::lib.rs` 文档明确声明永不删除原始观测的 API 不存在
   - 未来如需"清理粮仓"只能做物理归档而非逻辑删除

### 验证结果

- ✅ 编译检查通过 (`cargo check -p data-persistence`)
- ✅ 单元/集成测试全绿 (15 测试)
- ✅ 架构测试通过 (`cargo xtask arch`): 横向零依赖，仅依 B1
- ✅ Tidy 检查通过 (行数/模块文档/半成品禁令/文件数上限)
- ✅ Clippy 检查通过 (rustfmt 格式化 3 处警告)
- ✅ Public-API 快照测试 + 初始快照入库 `tests/snapshots/public-api.txt`

### 待办事项（留给 T12/T13 等）

- 🚫 不在这里做任何 UI 展示
- 🚫 不在这里做调度器/定时任务
- 🚫 不在这里做版本迁移工具 (v1.x → V2)
- 🟢 方案删除时的级联清理由后续 F3 计划执行 (可以调用本层接口但不由本层触发)
- 🟢 回收站定期清理可由调用方启动时触发 `purge_expired()`
