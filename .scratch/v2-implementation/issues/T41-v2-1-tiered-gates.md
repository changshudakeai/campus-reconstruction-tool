# T41 — V2.1 分级门禁治理与豁免盘点

**Status:** completed（2026-08-17，本地 `main` 提交 `cfc5d57`；版本最终完整门禁
仍统一归 V2.1 收口执行）

**What to build:** 把本地验证改为“开发循环定向验证 → 工单按风险扩圈 → PR/版本
一次完整兜底”，并给最慢的 `xtask timings` 单独规定触发条件。盘点全部带
v2.1.0 / 2026-12-31 期限的豁免，形成 T42–T49 主线。

**Blocked by:** None.

## 验收

- [x] `AGENTS.md` 给出逐门禁触发矩阵，并禁止同一
  `HEAD + 该门禁受检范围的 tracked/untracked diff fingerprint` 重复验证；完整门禁
  才绑定整个 diff。
- [x] `README.md` 与执法手册指向同一分级策略。
- [x] timings 仅由依赖图、crate 拓扑、构建配置/生成或明确性能风险触发；版本候选再跑。
- [x] 主线计划列出全部有期限豁免的消除顺序。

## 验证

- 定向：`git diff --check`；人工核对 CI 与 `xtask/src/timings.rs` 的真实行为。
- 升级触发：若修改 CI/xtask 实现，追加对应 xtask 测试；本票只改治理文档，不触发 Cargo 全量。
- 最终收口：随首个 V2.1 代码票的 PR 完整门禁一次。
