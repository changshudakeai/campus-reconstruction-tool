# T49 — 收口 desktop production 组合根

**Status:** ready-for-agent

**What to build:** 在各入口适配器与地图 seam 稳定后，按设置/方案/工作区/导航等
入口拆出 `production/mod.rs` 的构造期创建和回调绑定；保留一个小的组合根接口，
删除最终文件长度期限豁免，不把运行期业务编排重新藏进 helper。

**Blocked by:** T45, T46, T47.

## 验收与验证

- [ ] `production/mod.rs` 低于 1000 行并删除豁免；所有子模块均是有深度的入口绑定，
  不是逐函数转发的浅层包装。
- [ ] desktop-shell 定向入口/导航/确认测试 + crate 全部测试 + Clippy + fmt + tidy。
- [ ] 若只做同 crate 私有搬分不跑 timings；V2.1 最终豁免归零后跑完整门禁与 timings 一次。
