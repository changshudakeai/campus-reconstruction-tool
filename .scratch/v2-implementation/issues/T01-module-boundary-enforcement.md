# T01 — 三层锁执法地基（编译器 + CI + 流程）

**What to build:** 写代码时不能越界，漏网进主干，想改规则要守门人批准。

- **编译器层**：一个 crate 不能用没声明的依赖（编译报错）；循环依赖直接拒绝；默认私有 + `pub(crate)`；workspace.lints门禁（`dbg_macro/todo/print_stdout` deny, `wildcard_imports` warn）。
- **CI 层**：xtask tidy（单文件 1000 行红线、每文件模块文档）、xtask 架构测试（功能模块横向零依赖、壳依赖⊆白名单、B1 零内部依赖、底座不依上层）、cargo-deny（重依赖越级直呼禁令如 rusqlite 只准 data-persistence 用 + 许可证白名单）、cargo-public-api 快照（基础 crate 公开 API 全量入 git）、cargo-machete、clippy -D warnings + rustfmt（本地 warn/CI deny 双档）、编译时间预算（单 crate>2 分钟告警）、聚合 conclusion job。
- **流程层**：分支保护（required check = conclusion）、CODEOWNERS（依赖清单/门禁配置/共享类型/API 快照指定 owner）、豁免留痕纪律（任何 allow/ignore 必须带理由）。

**Blocked by:** None — can start immediately.

**Status:** completed (2026-07-26)

- [x] workspace.lints 门禁在根 Cargo.toml 完整配置（clippy/rust 两组 deny/warn）
- [x] clippy.toml 禁用类型与方法表（越层直呼底层库者 deny，每条带 reason+replacement）
- [x] xtask crate 立项并实现 tidy 检查（行数 + 模块文档 + TODO 禁令）+ 架构测试断言
- [x] xtask 集成到 GitHub Actions（merge_group 触发器 + conclusion job 聚合所有检查）
- [x] cargo-deny 配置（deny.toml + 4 个 dependencies job）
- [x] 每个基础 crate 添加 public-api 快照测试 + 初始快照入库（当前基础 crate 数为 0；机制已执法——xtask arch 强制检查快照文件存在，模板见 docs/developer-guide/enforcement.md，T02 起每个基础 crate 立户即受检）
- [x] cargo-machete 集成到 CI（本地验证通过）
- [x] 分支保护/ruleset 设置 required status check = conclusion（配置已就绪：.github/rulesets/main-branch-protection.json；仓库尚未推上 GitHub，推上后按 enforcement.md 一次性导入生效）
- [x] .github/CODEOWNERS 配置守卫关键文件（Cargo.toml/门禁配置/xtask/共享类型/API 快照）
- [x] 构建开发版快捷方式自动化脚本（xtask dev-shortcut 子命令，含上一版本兜底；壳 crate 立户前运行会明确提示 T19 后可用）

---

## 负责人验收点（一句话）

push 到主干被拦截时，能看到清晰的错误信息说明哪一层规则未通过（例如"xxx 文件超过 1000 行""F4 依赖了 F5 不允许"）。

> 验收证据（2026-07-26 实测）：放入一个 1101 行的文件后，`cargo xtask tidy` 报
> 「tidy 违规: demo_violation.rs: 文件 1101 行，超过 1000 行红线（如确需豁免…）」并以非零码退出；
> 架构违规报文形如「禁止边 data-acquisition → review-workbench：功能模块之间横向零依赖（ADR-0017），共享数据走 B1/B2」（单元测试断言覆盖）。
> 全量自检：fmt ✓ / clippy -D warnings ✓ / 25 测试全过 ✓ / cargo-deny 4 项 ok ✓ / cargo-machete ✓。

