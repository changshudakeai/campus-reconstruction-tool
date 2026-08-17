# T43 — 收束候选投影构造输入

**Status:** ready-for-agent

**What to build:** 按候选身份、来源身份、展示/形状与资格事实分组
`CandidateProjection` 构造输入，删除 `candidate_projections.rs` 的
`too_many_arguments` 期限豁免，同时保持 ADR-0040 的原始证据与候选资格语义。

**Blocked by:** T41.

## 验收与验证

- [ ] 豁免删除，构造接口更小且不隐藏必需业务事实；持久化字段和时间语义不变。
- [ ] `.\scripts\cargo-managed.ps1 -- test -p data-persistence` + 直接消费该接口的定向
  测试 + crate Clippy + fmt。
- [ ] 公共 API 变化使本票收口升级 workspace tests/public-api 快照；不改依赖图，不跑 timings。
- [ ] PR 最后一次代码改动后完整门禁一次。
