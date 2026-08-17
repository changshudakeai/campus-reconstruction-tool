# T42 — 收束导出请求与双文件发布接口

**Status:** completed（2026-08-17，本地 `main` 提交 `da99564`）

**What to build:** 在 `export-console` 内把方案信息、方案空间状态、输出目标和双文件
发布事实按语义分组，使边界直出与增强导出通过更小、更难误用的接口完成同一行为；
删除 `boundary_export.rs` 两处和 `enhanced.rs` 一处 `too_many_arguments` 豁免。

**Blocked by:** T41.

## 验收

- [x] 三处 `too_many_arguments` 期限豁免删除，无替代 lint 豁免。
- [x] 请求结构减少调用者需要同时记住的独立参数；内部发布 helper 保持私有 seam。
- [x] 边界直出/增强导出、失败回滚、manifest 内容与用户可见行为不变。
- [x] 公开构造契约测试与评审清单如实显示接口变化，不扩大无关公开面。

## 验证

- 证据范围：`HEAD 4b840c79dd6381e511cbadd2c13fc715a7a999eb`；
  `apps/export-flow + core/export-console` tracked diff fingerprint
  `a295839faf61bd010a806a90a0a5b39bb7ecf1b9`。
- 定向：`.\scripts\cargo-managed.ps1 -- test -p export-console --tests`（49 passed）；
  `.\scripts\cargo-managed.ps1 -- test -p export-flow`（6 passed）；两 crate 定向 Clippy
  与 `fmt --all --check` 均通过。
- 升级触发：公共 API 与跨 crate 调用变化，工单收口升级 workspace tests、公开构造
  契约测试与 API 评审清单。
- 升级证据：`.\scripts\cargo-managed.ps1 -- test --workspace` 退出码 0；公开构造契约
  已由 `public_api_types_exist` 直接编译调用，`snapshots/public-api.txt` 是人工评审清单，
  不是 rustdoc 自动生成快照。
- 不触发：不改 Cargo/依赖/crate 拓扑，因此本票开发循环与工单收口不跑 timings/machete/deny。
- 最终收口：PR 最后一次代码改动后完整门禁一次。
