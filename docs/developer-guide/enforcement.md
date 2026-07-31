# 三层锁执法手册（T01 落地说明）

> 法源：ADR-0017 第四节红线表（2026-07-25 修订版）+
> `docs/research/module-boundary-enforcement.md` 第七节清单。
> 本文只讲"怎么用、怎么跑、怎么豁免"，不新增任何决策。

## 三层锁在哪里

| 层 | 载体 | 违规后果 |
|----|------|---------|
| 编译器层 | 各 crate `Cargo.toml`（依赖白名单）、根 `Cargo.toml` 的 `[workspace.lints]`、根 `clippy.toml`（禁用类型/方法表） | 编译失败 / clippy deny |
| CI 层 | `xtask`（tidy + 架构测试 + 编译时间预算）、`deny.toml`、cargo-machete、public-api 快照、`.github/workflows/ci.yml` | CI 红灯，进不了主干 |
| 流程层 | 分支保护 ruleset（required check = `conclusion`）、`.github/CODEOWNERS`、豁免留痕纪律 | 无守门人批准无法合并 |

## 本地怎么跑（提交前自查）

```powershell
cargo fmt --all              # 格式（CI 用 --check 阻断）
cargo clippy --workspace --all-targets   # 本地 warn 档；CI 加 -D warnings 提为 deny 档
cargo test --workspace       # 含执法测试：tidy 全量扫描 + 架构断言
cargo xtask tidy             # 单独跑规模红线/模块文档/半成品禁令
cargo xtask arch             # 单独跑架构测试（违规信息带"哪条边为什么不行"）
cargo xtask timings          # 编译时间预算（>2 分钟的编译单元发告警）
cargo xtask dev-shortcut     # 构建并更新桌面"校园复刻工具 - 开发版"（ADR-0014）
```

## 豁免留痕纪律（执法清单 3.3）

任何豁免必须显式、带理由、可被 grep、过守门人评审：

- **lint 豁免**：`#[allow(lint_name, reason = "为什么必须豁免")]`——
  不带 reason 直接被 `clippy::allow_attributes_without_reason`（deny）拒绝；
- **行数红线豁免**：文件内留 `// ignore-tidy-filelength: <理由>`——
  没有理由的标记本身就是 tidy 违规；
- **未用依赖误报**：根 Cargo.toml `[workspace.metadata.cargo-machete] ignored`，
  同时在旁边写注释说明原因；
- **依赖禁令例外**：改 `deny.toml` 的 `wrappers` 白名单——该文件有 CODEOWNERS
  守门人，例外必然经过评审。

## 公开 API 快照测试（执法清单 2.5）

---

### 工具链升级纪律

`rust-toolchain.toml`是工具链的单点控制（见根目录）：指定精确版本而非 `channel = "stable"`，防止 CI 拉"最新 stable"后引入新 clippy lint 导致本地全绿、CI 红灯的版本漂移。

**工具链升级必须走专门提交**，且升级提交中统一清理新版本引入的新 lint，不得与功能改动混在一起。升级步骤：

1. 改 `rust-toolchain.toml` 为新版号
2. 在 CI 环境运行完整门禁，清理所有新 lint 告警
3. 提 PR，标题格式：`chore: bump Rust toolchain to X.Y.Z`
4. 守门人验收时确认没有遗漏功能修改

每个基础 crate（B1-B18）立户时**必须**带公开 API 快照测试；xtask 架构测试
会检查 `tests/public_api.rs` 与 `tests/snapshots/public-api.txt` 的存在，
缺了 CI 红灯。模板（T02 第一次使用时把版本号提入 `[workspace.dependencies]`）：

```toml
# <crate>/Cargo.toml
[dev-dependencies]
public-api = { workspace = true }
rustdoc-json = { workspace = true }
```

```rust
// <crate>/tests/public_api.rs
#[test]
fn public_api() {
    // 构建 rustdoc JSON 需要 nightly 工具链（仅此测试用，主工具链仍是 stable）。
    let rustdoc_json = rustdoc_json::Builder::default()
        .toolchain(public_api::MINIMUM_NIGHTLY_RUST_VERSION)
        .build()
        .unwrap();
    let api = public_api::Builder::from_rustdoc_json(rustdoc_json)
        .build()
        .unwrap();
    // 快照不一致 = 测试失败；确认变更合理后用
    // `UPDATE_SNAPSHOTS=yes cargo test` 更新快照并把 diff 提交评审。
    api.assert_eq_or_update("tests/snapshots/public-api.txt");
}
```

> ⚠️ 实测修正（T02 跟进，2026-07-31）：`rustdoc-json` 0.9.x **不会自动安装** nightly 工具链；
> 运行快照测试前必须显式安装 `public_api::MINIMUM_NIGHTLY_RUST_VERSION` 对应的 nightly
> （本项目 CI 的 test job 已加 `rustup toolchain install nightly-2025-08-02`）。
> 版本配套：public-api 0.50.x ↔ rustdoc-types 0.56 ↔ nightly-2025-08-02（rustdoc JSON format 55）
> 已实测通过；public-api 0.52.x 实际要求 format 57 的新版 nightly，但其 0.52.1 发布时
> 的 `MINIMUM_NIGHTLY_RUST_VERSION` 常量仍指向 2025-08-02（上游发布缺陷，实测解析失败），
> 故 B1 当前使用 public-api 0.50.3，模板按 0.50.x 执行。
> 
> 换行符：快照文件由工具以 LF 生成；.gitattributes 已固定 **/tests/snapshots/public-api.txt text eol=lf，防止 Windows autocrlf 使 CI 比对失败（2026-07-31 实测）。

公开 API 的任何增删改由此**必然现形于 PR diff**；快照文件受 CODEOWNERS
守卫，更新快照需守门人批准（十戒第 5 条"接口最小化"的客观量尺）。

---

## xtask 测试架构（内嵌模式）

本项目的执法测试采用**内嵌模式**（`#[test]` 直接写在 source file 中），
而非独立的测试目录结构：

| 模块 | 测试位置 | 测试内容 |
|------|---------|----------|
| tidy | `xtask/src/tidy.rs` | 行数/模块文档/半成品禁令等纯函数测试 |
| arch | `xtask/src/arch.rs` | 架构断言的白名单/禁止边单元测试 |
| main | `xtask/src/main.rs` | 全量 workspace 执法扫描集成测试 |
| timings | `xtask/src/timings.rs` | cargo-timing HTML 解析器测试 |
| shortcut | `xtask/src/shortcut.rs` | PowerShell 脚本生成逻辑测试 |

这种设计的好处：
1. **单 crate 自闭环**：每个子命令的逻辑 + 测试在同一文件，阅读方便；
2. **减少样板代码**：无需额外的 `mod tests`、fixture 目录管理；
3. **便于增量修改**：改 tidy.rs 时可直接看到对应的单元测试，符合 TDD 流程。

注意：public-api 快照测试不在这里——每个基础 crate 有自己的 `tests/` 目录，
因为快照是对外公开面的客观度量，必须由 crate 自己的 Cargo.toml 管理依赖。

## 流程层：GitHub 仓库建立后的一次性设置

本地仓库已就绪，以下三步在仓库推上 GitHub 后执行（约十分钟）：

1. **导入分支保护 ruleset**：仓库 Settings → Rules → Rulesets →
   Import a ruleset → 选择 `.github/rulesets/main-branch-protection.json`。
   效果：main 只能走 PR；required status check = `conclusion`（CI 全绿才有
   合并按钮）；改到 CODEOWNERS 守卫的文件必须守门人批准。
2. **核对守门人账号**：确认 `.github/CODEOWNERS` 中的 `@changshudakeai`
   与实际 GitHub 账号一致（依据 git 提交身份推定，如有出入更正）。
3. **（可选）启用 merge queue**：CI 已带 `merge_group` 触发器，开启合并队列
   后合并瞬间的组合状态同样过 CI（rust-analyzer 模式）。

## 负责人验收点对照（工单 T01）

push 到主干被拦截时，错误信息说明哪层规则未通过，例如：

- `tidy 违规: xxx.rs: 文件 1042 行，超过 1000 行红线（如确需豁免，…）`
- `架构违规: 禁止边 data-acquisition → review-workbench：功能模块之间横向零依赖（ADR-0017），共享数据走 B1/B2`
- `error[E0432]: unresolved import`（编译器层：依赖没在 Cargo.toml 申报）
- cargo-deny：`banned crate rusqlite = 只准 data-persistence 依赖`
