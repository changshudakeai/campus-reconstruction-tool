# T43 — 候选投影生命周期 module 深化

**Status:** completed（2026-08-17，产品负责人验收后本地合并）

**What to build:** 把 B2 的候选投影深化为小 interface 后的生命周期 module，
集中拥有从来源/验证事实创建合法投影、Reviewable/Isolated 资格转换、边界变化与
重新采集后的状态演进、旧评审决定作废/回待定、完整批次构建与原子发布不变量。
删除 `candidate_projections.rs` 的 `too_many_arguments` 期限豁免，但不得以“把长参数
列表换成几个字段容器”冒充完成。collection/revalidation 只提交事实；review/export
只读取 module 产出的合法当前投影。

**Blocked by:** T41.

## 验收与验证

- [x] `CandidateProjection` 不能由调用方通过公开字段或松散构造器拼出矛盾的
  validation / eligibility / isolation reason；正式数据库字段、历史追溯、时间语义、
  原始观测永久保留规则与稳定候选身份不变。
- [x] 某次采集或边界变化后候选消失/隔离，后来重新出现或恢复 Reviewable 时，沿用
  同一稳定候选身份；旧“保留/剔除”只保留为历史，当前决定为待定；用户再次保留前
  不进入增强导出。
- [x] 完整批次的投影、当前批次指针、资格演进和相关决定作废/回待定原子发布；失败时
  上一完整批次仍是当前批次，原始观测仍永久保留。
- [x] 评审封账携带打开页面时的候选批次 revision；采集/边界变化后旧页面和旧暂停文件
  不得把旧“保留/剔除”重新写回或恢复到新批次。
- [x] 删除测试成立：假设删除生命周期 module，合法组合、隔离/恢复、决定作废和批次
  原子性会重新扩散到 collection、revalidation、persistence、review/export 多处；若
  只是搬参数或透传调用，不得验收。
- [x] 采用红灯回归测试 → 最小实现 → 绿灯；至少覆盖“候选消失后重新出现”全链：旧
  决定不恢复、当前待定、未再次保留不进入增强导出。
- [x] `.\scripts\cargo-managed.ps1 -- test -p data-persistence` + 直接消费该接口的定向
  测试 + crate Clippy + fmt。
- [x] 公共 interface 变化使本票收口升级 workspace tests/public-api 快照；若未改 Cargo/
  依赖/拓扑/构建配置，不因本票单独跑 timings；最终版本收口按主线计划统一运行一次。
- [x] 产品负责人验收后、本地合并前完整门禁一次；本票未创建或推送 PR。
