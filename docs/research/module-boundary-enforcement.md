# 调研报告：模块边界的执法机制——成熟 Rust 项目靠什么保证"写代码时不越界"

> **后续决策（2026-08-01）**：ADR-0037 已收紧 S1，ADR-0039 为跨功能操作增加 A1 应用流程模块。本文早期关于“Slint 壳无需专门边界执法”的结论只覆盖 UI DSL 不能直接 I/O，不覆盖 `apps/desktop/src` 中 Rust 代码的运行期业务编排。S1 仍必须通过架构测试和行为测试验证“一次用户操作只调用一个完整入口”；与此冲突处以 ADR-0037/0039 为准。

> 调研日期：2026-07-26（第四轮）
> 背景：MCRebuild V2（Rust + Slint，规划 31 crate 的模块化单体，ADR-0017/0024/0039）。前三轮回答了"模块怎么划"（见同目录 modular-desktop-architecture.md / desktop-module-catalog.md / data-pipeline-modules.md），本轮专攻"边界怎么执法"。
> 核心问题（产品负责人）：**成熟稳定的模块化应用，靠什么机制保证写代码时不越界、不耦合？**
> 方法：全部一手来源——官方文档原文、真实仓库源码与配置文件（rust-lang/rust、rust-analyzer、zed、bevy、vscode 等），每条结论附文件路径与 URL。

---

## 概要

1. **第一层执法是编译器，且是免费的。** Rust/Cargo 的设计使"依赖白名单"天然成立：一个 crate 只能 `use` 它在 `Cargo.toml [dependencies]` 里声明过的 crate（未声明 = 编译错误）；crate 内一切符号默认私有，`pub` 是显式豁免；循环依赖被 Cargo 解析器直接拒绝。**ADR-0017 的"依赖单向、横向零依赖"只要写对 30 份 Cargo.toml，就已经被编译器强制执行了**——这是 Rust 相比多数语言做模块化单体的独有优势。
2. **第二层是 CI 里的自动化检查，业界全部用"自写小工具 + 现成 cargo 插件"组合。** rust-lang/rust 有 tidy（含 3000 行文件红线和 rustc 依赖白名单）；rust-analyzer 有 `cargo xtask tidy`（模块文档强制、Cargo.toml 规范检查）和 CI 里一行 `cargo tree` 写成的架构测试（"proc-macro 服务器不得依赖 salsa"）；zed/bevy 用 cargo-deny、cargo-machete、clippy `-D warnings` 门禁。这一层的共同特征：**违规 = CI 红灯 = 代码进不了主干**。
3. **第三层是流程：分支保护 + required status checks + CODEOWNERS。** GitHub 原生支持"CI 不绿不能合并""改到某文件必须指定人审批"；vscode 甚至用 CODEOWNERS 守卫"lint 白名单文件"——想给自己开豁免，先过守门人评审。
4. **架构测试（"crate X 不得依赖 crate Y"写成测试）在 Rust 生态是成熟惯例但没有统一框架**：主流做法是 `cargo tree -i` / `cargo metadata` + 十几行自写断言（rust-analyzer、rustc 均如此），本项目可在 xtask 里用 <50 行代码把 ADR-0017 的整张依赖图变成每次 push 都跑的测试。
5. **接口最小化可以量化执法**：cargo-public-api 把 crate 的全部公开 API 生成快照文件放进 git，任何 `pub` 项的增删都会让测试失败、出现在 PR diff 里被评审；cargo-semver-checks 检查 API 兼容性破坏。
6. **Slint 薄壳没有专用执法工具，但不需要**：.slint 是声明式 DSL，写不了 IO/网络/SQL，业务逻辑物理上进不去；真正的执法点是壳 crate 的 `[dependencies]` 白名单（只列 ViewModel 接口所需 crate）+ 行数红线，两者都由第一、二层覆盖。

---

## 一、Cargo/Rust 语言层的边界执法（写代码当场报错）

### 1.1 `[dependencies]` 是强制白名单，不是备忘录

**结论**：Rust 编译器只把"通过 `--extern` 旗标提供的外部 crate"加入 extern prelude（即可被 `use` 的名字集合），而 Cargo 只为 `Cargo.toml` 中声明的依赖传递 `--extern`。因此 `features/review` 想 `use data_acquisition::...` 而不在自己的 Cargo.toml 里声明依赖，得到的是硬编译错误（E0432/E0433），**不是警告，不可绕过**。

**证据**（Rust Reference — Preludes §extern prelude 原文）：
> "External crates imported with `extern crate` in the root module or provided to the compiler (as with the `--extern` flag with rustc) are added to the extern prelude."

- 来源：https://doc.rust-lang.org/reference/names/preludes.html#extern-prelude
- 来源：The Cargo Book — Specifying Dependencies：https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html

**推论**：ADR-0017/0039 的依赖图，最直接的执法配置是规划模块各自的 `Cargo.toml`。评审一个 PR 是否越界，先看它有没有改动某个 `Cargo.toml` 的 `[dependencies]` 段——这也是后文 CODEOWNERS 要守卫的文件。

### 1.2 循环依赖被 Cargo 原生拒绝（修正 ADR-0017 的一处表述）

Cargo 解析器要求包依赖图无环，出现环时直接报错 `error: cyclic package dependency: package ... depends on itself`，构建无法进行；唯一的历史例外是 dev-dependencies 造成的环（rust-lang/cargo#4242 长期讨论的场景）。

- 来源：https://github.com/rust-lang/cargo/issues/4242（issue 标题即"Allow crates to be published with cyclic dev-dependencies"，反证常规依赖环被拒绝）

> ⚠️ **修正**：ADR-0017 第四节把"循环依赖"列为需要 `cargo-depgraph` 检测的红线——实际上 crate 级的环 Cargo 免费拒绝，cargo-depgraph 只是画图工具。真正需要额外执法的是**同层横向依赖**（如 F4→F5，合法 DAG 但违反分层规则），见第三节"架构测试"。

### 1.3 默认私有 + `pub(crate)`：信息隐藏是语言内建的

**证据**（Rust Reference — Visibility and Privacy 原文）：
> "By default, everything is private, with two exceptions: Associated items in a `pub` Trait are public by default; Enum variants in a `pub` enum are also public by default."
> "`pub(crate)` makes an item visible within the current crate."

- 来源：https://doc.rust-lang.org/reference/visibility-and-privacy.html（§vis.default、§vis.scoped）

配套的执法 lint：rustc 内建 `unreachable_pub`（标记"写了 pub 但实际上 crate 外根本够不到"的项，强迫改成 `pub(crate)`，防止公开面虚胖）。**rust-analyzer 在 CI 全局开启**：其 ci.yaml 第 19 行 `RUSTFLAGS: "-D warnings -W unreachable-pub"`，且根 Cargo.toml `[workspace.lints.rust]` 中 `unreachable_pub = "warn"`。

- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml；https://github.com/rust-lang/rust-analyzer/blob/master/Cargo.toml

### 1.4 `#[non_exhaustive]`：跨 crate 契约的"禁止绕过构造器"

**证据**（Rust Reference — Type system attributes 原文）：
> "Outside of the defining crate, types annotated with `non_exhaustive` have limitations that preserve backwards compatibility... Non-exhaustive types cannot be constructed outside of the defining crate... When pattern matching on a non-exhaustive enum, matching on a variant does not contribute towards the exhaustiveness of the arms"（下游 match 必须带 `_ =>` 通配臂）。

- 来源：https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute

**对本项目**：`core/shared-domain-types`（B1）的共享枚举（候选类别、审核状态等）加 `#[non_exhaustive]` 后，未来加变体不会破坏 7 个功能 crate；下游被编译器强制写兜底分支。

### 1.5 sealed trait：trait 接口只许官方实现

**证据**（Rust API Guidelines — C-SEALED 原文）：
> "Some traits are only meant to be implemented within the crate that defines them. In such cases, we can retain the ability to make changes to the trait in a non-breaking way by using the sealed trait pattern."
> 做法：`pub trait TheTrait: private::Sealed {...}`，其中 `mod private { pub trait Sealed {} }` 不导出——下游 crate 无法命名 `Sealed`，因此无法实现 `TheTrait`。

- 来源：https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed

**对本项目**：B12 数据源适配器的 `DataSource` trait 若只允许 Overpass/Overture/高德三个官方实现（防止功能 crate 私自实现假适配器绕过验证），用 sealed 模式；反之若 B12 就是要开放扩展（图生图插件），则**不要** seal——这是逐 trait 的设计决定。

### 1.6 `[workspace.lints]`：lint 门禁集中在根 Cargo.toml 一处声明

Cargo 支持在根清单 `[workspace.lints.rust]` / `[workspace.lints.clippy]` 统一配置 lint 等级，成员 crate 写 `[lints] workspace = true` 继承。三个考证项目全部使用（原文见第二节 2.4）。

- 来源：The Cargo Book — Workspaces §The lints table：https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table

---

## 二、真实大项目的自动化执法工具链（逐个考证）

### 2.1 rust-lang/rust 的 tidy：自写检查器 + 文件行数红线 + 依赖白名单

tidy 位于 `src/tools/tidy/`，随 `./x test tidy` 和 CI 运行。两份关键源码：

**（a）`src/tools/tidy/src/style.rs`——风格与规模红线**。文件头注释原文：
> "Tidy check to enforce various stylistic guidelines on the Rust codebase. Example checks are: No lines over 100 characters (in non-Rust files). No files with over 3000 lines (in non-Rust files). No tabs. No trailing whitespace. No CR characters. No `TODO` or `XXX` directives..."

具体实现（源码摘录）：
```rust
const COLS: usize = 100;
const LINES: usize = 3000;
...
if lines > LINES {
    ... "too many lines ({lines}) (add `// ignore-tidy-file-filelength`
         to the file to suppress this error)" ...
}
```
即：**行数上限是常量 3000，写死在检查器里；超限即报错；豁免必须在文件内留下显式标记 `// ignore-tidy-filelength`（可被 grep、可被评审质询）**。这正是 ADR-0017"单文件 1000 行红线"的上游原型——rustc 是 180 万行的老代码库才放宽到 3000，本项目新代码用 1000 合理。

- 来源：https://github.com/rust-lang/rust/blob/master/src/tools/tidy/src/style.rs

**（b）`src/tools/tidy/src/deps.rs`——依赖白名单（架构测试的鼻祖）**。源码摘录：
```rust
/// This list is here to provide a speed-bump to adding a new dependency to
/// rustc. Please check with the compiler team before adding an entry.
const PERMITTED_RUSTC_DEPENDENCIES: &[&str] = &[ "adler2", "aho-corasick", ... ];
```
tidy 用 `cargo_metadata` 读出真实依赖图，与 `PERMITTED_RUSTC_DEPENDENCIES` / `PERMITTED_STDLIB_DEPENDENCIES` 白名单比对，多一个少一个都报错。注释直说这是"减速带（speed-bump）"：加依赖必须先找编译器团队评审。

- 来源：https://github.com/rust-lang/rust/blob/master/src/tools/tidy/src/deps.rs

### 2.2 rust-analyzer 的 xtask tidy：模块文档、Cargo.toml 规范、许可证白名单

检查器在 `xtask/src/tidy.rs`（`cargo xtask tidy` 运行；文件末尾还有 `#[test] fn test()`，意味着普通 `cargo test` 也会执行它——**检查器本身就是测试**）。全文核实后的检查清单：

| 函数 | 检查内容 | 执法意义 |
|---|---|---|
| `check_lsp_extensions_docs` | `lsp/ext.rs` 的哈希必须与文档 `lsp-extensions.md` 中记录的哈希一致 | **改接口不改文档 = 构建失败**（代码-文档强制同步） |
| `TidyDocs::visit` | 每个 .rs 文件第一行必须是 `//!` 模块文档（tests/test_data 等目录豁免） | 每个模块必须自述职责 |
| `check_cargo_toml` | 内部 `dependencies` 必须写 `version`、`dev-dependencies` 禁止写 `version` | Cargo.toml 格式规范机器化 |
| `check_test_attrs` | 禁止 `#[should_panic]`（豁免文件列在 `need_panic` 白名单里） | 测试风格红线 |
| `check_licenses` | `cargo metadata` 提取全部依赖的 license，与 `EXPECTED` 白名单精确比对 | 新依赖带新许可证 = 构建失败 |
| `check_trailing_ws` / `TidyMarks` | 行尾空白；测试覆盖标记 hit/check 必须成对 | 卫生检查 |

- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/xtask/src/tidy.rs（经 GitHub API 定位路径并全文核读）

### 2.3 cargo-deny / deny.toml：能否禁止特定依赖关系？——能

cargo-deny 的 `[bans]` 配置支持精确到"谁可以依赖谁"。官方文档原文（bans 配置一章）：

> `deny = [{ crate = "crate-you-don't-want:<=0.7.0", wrappers = ["this-can-use-it"] }]`
> "This field allows **specific crates to have a direct dependency on the banned crate but denies all transitive dependencies on it**."

即：把 crate X 列入 `deny`，再用 `wrappers` 白名单放行唯一合法使用者——这就是"**只有 core/data 可以依赖 rusqlite，其他 crate 一律不准**"的现成实现。另有：
- `[bans.workspace-dependencies] duplicates = 'deny'`（默认）：成员 crate 不用 `workspace = true` 而自行声明版本 → 报错（强制版本单点管理）；`unused = 'deny'`：`[workspace.dependencies]` 里声明了但没人用 → 报错。
- `wildcards = "deny"`、`multiple-versions`、`[[bans.features]]`（禁止启用某依赖的某 feature）。

- 来源：https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html

**真实使用者考证**：
- **bevy 使用**：仓库根有 `deny.toml`（原文核实：`[bans] multiple-versions = "warn"`、`wildcards = "deny"`、`deny = [{ name = "ahash", deny-multiple-versions = true }, ...]`、`[[bans.features]] crate = "derive_more" deny = ["error"]`——连"这个依赖的这个 feature 不许开"都在执法；`[sources] unknown-registry = "deny"`）。CI 在 `.github/workflows/dependencies.yml` 里跑四个独立 job：`cargo deny check advisories / bans / licenses / sources`。
  - 来源：https://github.com/bevyengine/bevy/blob/main/deny.toml；https://github.com/bevyengine/bevy/blob/main/.github/workflows/dependencies.yml
- **zed 不使用 cargo-deny**（GitHub API 列举仓库根目录核实无 deny.toml）；zed 用 cargo-machete + GitHub dependency-review-action + 自写 `script/check-licenses` 覆盖同类需求。

### 2.4 clippy.toml 与 `#![deny(...)]` 门禁：与耦合相关的 lint

**zed 的 clippy.toml**（原文摘录）——`disallowed-methods` 把"禁用 API"写成配置，每条带理由和替代品：
```toml
disallowed-methods = [
  { path = "std::process::Command::spawn",
    reason = "Spawning `std::process::Command` can block the current thread...",
    replacement = "smol::process::Command::spawn" },
  ...
]
```
- 来源：https://github.com/zed-industries/zed/blob/main/clippy.toml

**zed 根 Cargo.toml `[workspace.lints.clippy]`**（原文摘录）：`dbg_macro = "deny"`、`todo = "deny"`、`redundant_clone = "deny"`、`disallowed_methods = "deny"`。执行入口 `script/clippy`（原文核实）：`cargo clippy --workspace --release --all-targets --all-features -- --deny warnings`。
- 来源：https://github.com/zed-industries/zed/blob/main/Cargo.toml；https://github.com/zed-industries/zed/blob/main/script/clippy

**rust-analyzer**：`[workspace.lints.rust]` 开 `unreachable_pub`、`unused_extern_crates`、`unused_lifetimes` 等；`[workspace.lints.clippy]` 平时 `dbg_macro/todo/print_stdout/print_stderr = "warn"`，注释写明 "CI raises these to deny"——CI 命令原文：`cargo clippy --all-targets -- -D clippy::disallowed_macros -D clippy::dbg_macro -D clippy::todo -D clippy::print_stdout -D clippy::print_stderr`（本地宽松、门禁从严的双档模式）。
- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/Cargo.toml；https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml（clippy job）

**bevy**：`[workspace.lints.rust] unsafe_code = "deny"`、`missing_docs = "warn"`、`unused_qualifications = "warn"`；clippy 侧 `undocumented_unsafe_blocks / allow_attributes_without_reason = "warn"`（连"写 allow 豁免必须给理由"都有 lint：`allow_attributes_without_reason`）。
- 来源：https://github.com/bevyengine/bevy/blob/main/Cargo.toml

**与耦合直接相关的 lint 清单**（供本项目选用）：`clippy::wildcard_imports`（禁 `use foo::*`，防隐式耦合；pedantic 组，https://rust-lang.github.io/rust-clippy/master/index.html#wildcard_imports）、`clippy::disallowed_methods` / `disallowed_types`（禁越层直呼底层 API）、rustc `unreachable_pub`（公开面虚胖）、rustc `unused_crate_dependencies`（白名单腐化，见 2.5）。

### 2.5 未用依赖清理：cargo-machete 实际使用者充分，cargo-udeps 小众

- **rust-analyzer CI** 直接跑 `cargo machete`（ci.yaml "Run cargo-machete" step，原文核实）。
- **zed CI** 的 `check_dependencies` job：安装 `cargo-machete@0.7.0` 并运行 + `cargo update --locked`（锁文件一致性）+ dependency-review-action；根 Cargo.toml 有 `[workspace.metadata.cargo-machete] ignored = [...]` 处理误报。
  - 来源：https://github.com/zed-industries/zed/blob/main/.github/workflows/run_tests.yml；https://github.com/zed-industries/zed/blob/main/Cargo.toml
- **cargo-udeps** 走编译分析路线但**必须 nightly 运行**（官方 README："it needs Rust nightly to actually run"，https://github.com/est31/cargo-udeps ），在考证的三个项目 CI 中均未出现。另有零成本替代：rustc 内建 lint `unused_crate_dependencies`（"detects crate dependencies that are never used"，https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html#unused-crate-dependencies ）。
- **结论**：选 cargo-machete（stable、快、有 workspace.metadata 豁免机制），未用依赖 = 依赖白名单在腐化，必须清。

### 2.6 CI 编排方式（三个仓库的 workflow 原文对比）

| 项目 | workflow 文件 | 边界相关 job | 编排要点 |
|---|---|---|---|
| rust-analyzer | `.github/workflows/ci.yaml` | rustfmt / clippy（-D 五连）/ rust（含 `cargo codegen --check`、cargo-machete）/ proc-macro-srv（含 salsa 架构测试）/ typo-check | 触发器 `on: pull_request, merge_group`（合并队列）；末尾 **conclusion job** 用 `jq --exit-status 'all(.result == "success" or .result == "skipped")'` 聚合所有 job——分支保护只需将 conclusion 一个 check 设为 required。文件头注释原文："Please make sure that the `needs` field for the `conclusion` job are updated when adding new jobs!" |
| zed | `.github/workflows/run_tests.yml` | check_style（fmt --check、check-todos、typos）/ clippy_{windows,linux,mac}（script/clippy = `--deny warnings`）/ check_dependencies（machete + locked + dependency-review）/ check_licenses / doctests | 文件头注释原文："**Generated from xtask::workflows::run_tests. Rebuild with `cargo xtask workflows`.**"——连 CI YAML 本身都是 xtask 生成的（工作流即代码）；末尾 tests_pass 聚合 job 同 conclusion 模式 |
| bevy | `.github/workflows/ci.yml` + `dependencies.yml` | `cargo run -p ci -- lints / test / compile / doc`（自写 CI 工具 crate `tools/ci`，注释原文 "See tools/ci/src/main.rs for the commands this runs"）；dependencies.yml 四个 cargo-deny job | 检查逻辑写成 workspace 内的 Rust crate（= xtask 模式），YAML 只负责调用 |

- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml；https://github.com/zed-industries/zed/blob/main/.github/workflows/run_tests.yml；https://github.com/bevyengine/bevy/blob/main/.github/workflows/ci.yml；https://github.com/bevyengine/bevy/blob/main/.github/workflows/dependencies.yml

**共同模式**：① 检查逻辑写成 Rust（tidy/xtask/tools-ci），YAML 只是启动器；② 一个聚合 job 作为唯一 required check；③ lint 本地 warn、CI deny。

---

## 三、架构测试：把"crate X 不得依赖 crate Y"写成测试

### 3.1 rust-analyzer 的一行架构测试（最直接的实例）

`.github/workflows/ci.yaml` 的 proc-macro-srv job 中有一个名为 **"Check salsa dependency"** 的 step，原文：

```yaml
- name: Check salsa dependency
  run: "! (cargo tree -p proc-macro-srv-cli -p proc-macro-srv -p proc-macro-api -i salsa)"
```

含义：`cargo tree -p <这几个crate> -i salsa` 会列出这些 crate 的依赖图中所有到 salsa 的反向路径；外层 `!` 取反——**一旦任何人让 proc-macro 服务器（须与 rustc 内嵌构建，必须保持轻依赖）传递依赖上 salsa（rust-analyzer 的增量计算核心），CI 立刻红灯**。这就是"crate X 不得依赖 crate Y"的最小可行执法：一行 shell，零框架。

- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml（第 74-75 行）

### 3.2 rustc tidy 的白名单式架构测试

2.1(b) 已述：`tidy/src/deps.rs` 用 `cargo_metadata::MetadataCommand` 读取真实依赖图与硬编码白名单比对——这是"允许的依赖集合"的正面清单执法，与 3.1 的"禁止边"负面清单互补。

### 3.3 配套的文档模式：Architecture Invariant

rust-analyzer 的 `docs/book/src/contributing/architecture.md` 为每个 crate 写明**架构不变量**，与测试互为表里。原文摘录：
> "Pay attention to the **Architecture Invariant** sections. They often talk about things which are **deliberately absent** in the source code."
> "**Architecture Invariant:** `syntax` crate is completely independent from the rest of rust-analyzer. It knows nothing about salsa or LSP."
> "**Architecture Invariant:** `base-db` doesn't know about file system and file paths."
> "**Architecture Invariant:** these crates (`hir-*`) are not, and will never be, an api boundary."

- 来源：https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md

### 3.4 生态现状与本项目做法

Rust 没有 ArchUnit（Java）那样的统一架构测试框架；社区讨论（如 r/rust "What tools exist for architectural testing in Rust"）与实践收敛到：**`cargo metadata`/`cargo tree` + 自写断言**是主流；小工具（如 pistonite/layered-crate，用 Layerfile.toml 管 crate 内部模块分层，https://github.com/pistonite/layered-crate ）存在但小众、维护风险高。

**给本项目的具体实现**（放 `xtask`，随 `cargo test`/CI 跑，约 40 行）：
1. `cargo metadata --format-version 1` 解析 workspace 成员依赖表；
2. 断言一：任意两个 `F*` 功能 crate 之间无依赖边（横向零依赖）；
3. 断言二：`desktop-shell` 的直接依赖服从 ADR-0037 收紧后的白名单；旧 `{F1..F9, B2-B7, B9-B11, B17, slint}` 不能继续作为授权；
4. 断言三：`shared-domain-types`（B1）的内部依赖数为 0；`sponge-export`/`foundation-mode` 等底座不依赖任何 `F*`（下不依上）；
5. 断言四（负面清单，学 3.1）：`! cargo tree -p desktop-shell -i data-source-adapters` 等关键禁止边。
6. 断言五：A1 `collection-flow` 只允许依赖 F4/F7/B1/B2/B14；S1 的采集回调只跨越 A1，F4/F7/B* 不得反向依赖 A1（ADR-0039）。

---

## 四、接口最小化的执法：cargo-public-api 与 cargo-semver-checks

### 4.1 cargo-public-api：公开 API 快照测试

> 命名说明：cargo-public-api 是 public-api crate 的 CLI 形态；本项目按 enforcement.md 采用其库模式（assert_eq_or_update）做快照测试。

官方 README（全文核实）给出的 CI 模式：把下面这个测试加进项目，公开 API 的**每一次增删改**都会让 `cargo test` 失败，必须显式运行 `UPDATE_SNAPSHOTS=yes cargo test` 更新快照文件并把 diff 提交进 git——**API 膨胀从"悄悄发生"变成"必须在 PR diff 里现形"**：

```rust
#[test]
fn public_api() {
    let rustdoc_json = rustdoc_json::Builder::default()
        .toolchain(public_api::MINIMUM_NIGHTLY_RUST_VERSION).build().unwrap();
    let public_api = public_api::Builder::from_rustdoc_json(rustdoc_json).build().unwrap();
    // Run with env var `UPDATE_SNAPSHOTS=yes` to update the snapshot.
    public_api.assert_eq_or_update("./tests/snapshots/public-api.txt");
}
```

README 原话："a regular `cargo test` will fail if your public API is accidentally or deliberately changed."（注意：构建 rustdoc JSON 需显式安装 nightly 工具链——rustdoc-json 0.9.x 不会自动安装；本项目在 CI test job 显式安装 nightly-2025-08-02，实测记录见 docs/developer-guide/enforcement.md。）

- 来源：https://github.com/cargo-public-api/cargo-public-api

**对本项目**：ADR-0017 十戒第 5 条"trait 方法 ≤5 个/模块"目前的检验方式是"代码审查"（人治）；给 17 个基础 crate 各加一个 public-api 快照测试后，快照文件行数就是公开面大小的客观量尺，评审只需看 diff。

### 4.2 cargo-semver-checks：API 兼容性破坏检测

定位（README 原文）："Lint your crate API changes for semver violations."——对比新旧版本的 rustdoc JSON，按 100+ 条 lint 规则找出破坏性变更；官方 GitHub Action `obi1kenobi/cargo-semver-checks-action@v2` 一行接入；设计目标零误报（"A design goal of cargo-semver-checks is to not have false positives. If they do occur, they are considered bugs."）。

- 来源：https://github.com/obi1kenobi/cargo-semver-checks

**对本项目的取舍**：semver-checks 主要服务于"发布到 crates.io 的库"；本项目规划的 31 个 crate 均为内部模块，破坏性变更由编译器直接暴露（下游编译失败）。**建议只采用 cargo-public-api 快照测试，不引入 semver-checks**——避免为不存在的外部消费者付维护成本。

---

## 五、Slint UI 薄壳的执法

### 5.1 语言本身就是第一道墙

.slint 是独立的声明式 DSL，由 `slint-build` 在 build.rs 编译。它能写的只有：组件声明、属性绑定、有限的表达式和回调转发——**打不开文件、连不了网、查不了 SQLite**。业务逻辑想"渗入 UI"只有一条路：写进壳 crate 的 Rust 侧（ViewModel/controller 代码）。因此执法对象收窄为壳 crate 的 Rust 代码，恰好被既有机制覆盖（见 5.3）。

### 5.2 官方推荐的解耦模式（原文）

Slint Best Practices（本轮重新核实原文）：
> "Separate Code, UI, and Assets... `src/main.rs <this is where your main business logic lives>`；`ui/app-window.slint <the entry point for your Slint UI>`"

即官方结构中业务逻辑在 Rust 侧、UI 声明在 .slint 侧，二者只通过 property/callback 接口相接。官方大型 demo energy-monitor 的 `src/controllers/` + `ui/pages|widgets/` 结构已在第一轮报告§三核实（每个功能域一个 controller 文件，UI 声明不含逻辑）。

- 来源：https://docs.slint.dev/latest/docs/slint/guide/development/best-practices/；https://github.com/slint-ui/slint/tree/master/demos/energy-monitor

### 5.3 Slint 没有、也不需要专用"防渗入"工具——用三件现成武器

考证结论：Slint 官方**没有**提供"防止业务逻辑写进壳 crate"的机制（这超出 UI 框架职责）。真实执法组合：

1. **依赖白名单（编译器层）**：`desktop-shell/Cargo.toml` 只保留 ADR-0037 允许的功能入口、显示类型和呈现能力。它能阻止越层依赖，但不能识别壳在一个回调中依次调用多个已获准功能入口。
2. **架构测试兜底（CI 层）**：§3.4 断言二/四防止有人往壳的 Cargo.toml 里加依赖蒙混过关——改白名单本身会被测试和 CODEOWNERS（§六）双重拦截。
3. **运行期编排检查（CI 层）**：除规模红线外，必须有架构/行为测试证明一个 UI 操作只调用一个完整入口；采集操作的入口固定为 A1。文件未超 1000 行不等于符合 ADR-0037/0039。

---

## 六、流程层执法：CODEOWNERS、required checks、分支保护

### 6.1 GitHub 官方机制（文档原文）

- **CODEOWNERS**："Code owners are automatically requested for review when someone opens a pull request that modifies code that they own." 且可在分支保护里升级为硬门禁："they also can optionally **require approval from a code owner** before the author can merge a pull request"。文件放 `.github/CODEOWNERS`，语法 `路径模式 @用户/@org/团队`，后匹配的行优先。
  - 来源：https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners
- **分支保护（branch protection / rulesets）**：可勾选 "Require pull request reviews before merging"、"Require status checks before merging"（"all required status checks must pass before collaborators can merge changes into the protected branch"）、"Dismiss stale pull request approvals when new commits are pushed"、"Require branches to be up to date"。
  - 来源：https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches

### 6.2 真实仓库实例：microsoft/vscode 的 CODEOWNERS

`.github/CODEOWNERS` 原文摘录（三个用法各具启发）：
```
# GitHub actions required reviewers
.github/workflows/pr.yml @lszomoru @alexdima @sandy081 @TylerLeonhardt @rzhao271 @Yoyokrazy

# VS Code API — Ensure the API team is aware of changes to the vscode-dts file
src/vscode-dts/vscode.d.ts @TylerLeonhardt @alexr00

# Allowlist for the `local/code-no-new-javascript-files` lint rule.
# Adding entries here lets a new .js/.cjs/.mjs file land in the repo;
# review is required to make sure TypeScript is not a better choice.
.eslint-allowed-javascript-files @alexr00 @alexdima @sbatten @TylerLeonhardt
```
三个模式：① **CI 配置本身有 owner**（改门禁先过门神）；② **公共 API 文件有 owner**（接口变更强制专人评审）；③ 最精彩的——**lint 豁免白名单文件有 owner**：想给自己开例外，必须先说服守门人。

- 来源：https://github.com/microsoft/vscode/blob/main/.github/CODEOWNERS

### 6.3 "唯一 required check"编排模式

rust-analyzer 的 conclusion job（§2.6）与 zed 的 tests_pass job 是同一个模式：所有检查 job 汇入一个聚合 job，分支保护里只把聚合 job 设为 required status check——新增检查只改 workflow 不动仓库设置；rust-analyzer 还启用 GitHub merge queue（workflow `on: merge_group`），保证合并瞬间的组合状态也过了 CI。

---

## 七、给本项目的执法工具链清单（可直接抄作业）

标注说明：✅ = ADR-0017 红线表（第四节）已覆盖；🆕 = 本轮建议新增；🔧 = ADR-0017 已列但实现方式需修正。

### 第 1 层：编译器层（写代码当场报错，零维护成本）

| # | 机制 | 要写的配置 | 检查内容 | 违规后果 | 状态 |
|---|---|---|---|---|---|
| 1.1 | `[dependencies]` 白名单 | 规划模块各自的 Cargo.toml（依赖严格按 ADR-0017/0039 依赖图声明） | 未声明的 crate 无法 `use` | 编译失败 | ✅（隐含；建议在 ARCHITECTURE.md 点明"Cargo.toml 即执法文件"） |
| 1.2 | `[workspace.dependencies]` 版本单点 | 根 Cargo.toml 集中声明 + 成员 `workspace = true` | 版本漂移 | 由 2.3 的 cargo-deny `workspace-dependencies.duplicates = 'deny'` 兜底 | 🆕 |
| 1.3 | 默认私有 + `pub(crate)` + `unreachable_pub` | 根 Cargo.toml：`[workspace.lints.rust] unreachable_pub = "warn"`（CI `-D`） | 写了 pub 但外部够不到 | 编译警告→CI 失败 | 🆕（落实十戒第 4 条"pub 符号表最小化"） |
| 1.4 | `#[non_exhaustive]` | `core/shared-domain-types` 的共享枚举/配置结构体 | 下游绕过构造器、match 不写兜底 | 编译失败 | 🆕 |
| 1.5 | sealed trait | 需要封闭实现的 trait（逐个评审决定，如 B4 导出器内部 trait） | 下游私自实现内部 trait | 编译失败 | 🆕 |
| 1.6 | `[workspace.lints]` 门禁 | 根 Cargo.toml：clippy `dbg_macro/todo/print_stdout/print_stderr = "deny"`、`wildcard_imports = "warn"`、rust `unsafe_code = "deny"` | 调试残留、通配导入、unsafe | CI 失败 | 🆕（抄 zed/ra/bevy 三家并集） |
| 1.7 | clippy.toml `disallowed-types/methods` | 根 clippy.toml：如禁 `rusqlite::Connection`（replacement 指向 `core/data` API）、禁 `std::process::Command`（本项目 ADR 有安静哨兵纪律） | 越过底座 crate 直呼底层库 | clippy deny → CI 失败 | 🆕（zed 模式，每条带 reason） |

### 第 2 层：CI 脚本层（xtask + GitHub Actions，push/PR 阻断）

| # | 工具 | 要写的配置/代码 | 检查内容 | 违规后果 | 状态 |
|---|---|---|---|---|---|
| 2.1 | xtask tidy（自写） | `xtask/src/tidy.rs`：行数红线（1000 行 + 显式豁免标记 `// ignore-tidy-filelength` 需评审）；每文件 `//!` 模块文档；禁 TODO 入主干 | 巨型文件、无文档模块 | CI 失败 | ✅（1000 行已定；豁免标记机制 🆕，抄 rustc；tidy 写成 `#[test]` 🆕，抄 ra） |
| 2.2 | xtask 架构测试 + S1 契约测试 | `cargo metadata` 断言 F* 横向零依赖、S1 依赖服从 ADR-0037、A1 只依赖 F4/F7/B1/B2/B14、B1 零内部依赖、底座不反依 A1/F*；另断言采集 UI 操作只调用 A1 | 同层依赖、越层依赖、下依上、S1 运行期业务编排 | CI 失败 | 🔧（现有依赖图检查尚未识别 A1，ADR-0037/0039 门禁需随代码立户补齐） |
| 2.3 | cargo-deny | 根 deny.toml：`[licenses]` 白名单、`[bans] wildcards="deny"` + `deny = [{ crate = "rusqlite", wrappers = ["data-persistence"] }]` 等关系约束、`[bans.workspace-dependencies] duplicates="deny", unused="deny"`、`[sources] unknown-registry="deny"`、`[advisories]` | 依赖关系、许可证、漏洞、来源 | CI 失败 | 🆕（bevy dependencies.yml 四 job 直接抄） |
| 2.4 | cargo-machete | 根 Cargo.toml `[workspace.metadata.cargo-machete]` 豁免表；CI 一步 `cargo machete` | 声明了但未用的依赖（白名单腐化） | CI 失败 | 🆕（zed/ra 均用；不选 cargo-udeps——需 nightly） |
| 2.5 | cargo-public-api 快照测试 | 17 个基础 crate 各加 `tests/public_api.rs` + `tests/snapshots/public-api.txt` 入 git | 公开 API 任何增删改必须现形于 PR diff | cargo test 失败 | 🔧（替代 ADR-0017"cargo doc + 静态分析"的模糊描述；"trait ≤5 方法"由快照行数客观化；不引入 cargo-semver-checks——无外部消费者） |
| 2.6 | clippy + rustfmt 门禁 | CI：`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`（本地 warn、CI deny 双档） | 全量 lint | CI 失败 | ✅（十戒第 10 条提及 clippy；双档模式 🆕） |
| 2.7 | 编译时间预算 | xtask 解析 `cargo build --timings` | 单 crate >2 分钟 | CI 告警 | ✅ |
| 2.8 | 聚合 conclusion job | workflow 末尾一个 job `needs:` 全部检查，jq 断言全绿 | —— | 作为唯一 required check | 🆕（ra/zed 模式） |

### 第 3 层：流程层（GitHub 仓库设置 + 一个文件）

| # | 机制 | 要写的配置 | 检查内容 | 违规后果 | 状态 |
|---|---|---|---|---|---|
| 3.1 | 分支保护/ruleset | 仓库设置：main 必须走 PR；required status check = conclusion job；require branches up to date | CI 不绿、分支落后 | 无法合并（GitHub 拒绝按钮） | 🆕 |
| 3.2 | CODEOWNERS | `.github/CODEOWNERS`：`**/Cargo.toml`、`deny.toml`、`clippy.toml`、`xtask/**`、`core/shared-domain-types/**`、快照文件 `**/public-api.txt` 指定 owner | 改依赖声明/门禁配置/共享类型/API 快照必须专人批 | 无 owner 批准无法合并 | 🆕（vscode 三模式：门禁有门神、API 有门神、**豁免白名单有门神**） |
| 3.3 | 豁免留痕纪律 | 约定：任何 `#[allow(...)]`/`ignore-tidy-*`/machete-ignored 必须带 reason（bevy 有 lint `allow_attributes_without_reason` 可机器化） | 无理由豁免 | clippy warn/评审拒绝 | 🆕 |

**落地顺序建议**：1.1/1.6/1.7 + 2.6（一天内可完成，纯配置）→ 2.1/2.2（xtask 两个检查，两三天）→ 2.3/2.4/2.5 + 2.8（CI 完整化）→ 3.1/3.2（仓库设置，十分钟，但依赖 2.8 先存在）。

---

## 八、给产品负责人的一段话："三层锁"为什么可信

成熟项目防止代码越界，靠的不是工程师的自觉，而是**三层上锁**。第一层锁在编译器里：Rust 语言规定，一个模块能用哪些其他模块，必须白纸黑字写在自己的"依赖清单"（Cargo.toml）里——没写的，代码根本编译不过，就像门禁卡没授权连门都刷不开，这层锁没人能绕过。第二层锁在流水线里：每次提交代码，机器人自动跑一遍全部规矩——文件超过 1000 行？两个功能模块私下勾连？偷偷用了没申报的第三方库？公开接口悄悄变胖？任何一条不过，代码就进不了主干（我们考证过：rustc 编译器本身、rust-analyzer、Zed 编辑器、Bevy 引擎这些千万级用户的项目，用的就是这套完全相同的做法，连"文件行数上限写死在检查器里"这种细节都一致——rustc 定的是 3000 行，我们定 1000 行）。第三层锁在流程里：GitHub 平台设置保证"机器人不点头就没有合并按钮"，而依赖清单、门禁配置这些关键文件被指定了守门人——**想给自己开例外，必须先说服守门人**（VS Code 团队连"lint 豁免名单"这个文件本身都派了守门人）。三层锁的分工是：第一层让越界代码**写不出来**，第二层让漏网的**进不了主干**，第三层让想改规则的**过不了人这一关**。剩下的唯一风险是"有权限的人故意同时拆三层锁"——那已经不是工程问题，任何机制都防不住，但机制保证了这种事**必然留下痕迹、必然是显式决定**，不可能"不小心"发生。

---

## 附：来源清单

| # | 来源 | URL |
|---|---|---|
| 1 | Rust Reference — Visibility and Privacy（默认私有、pub(crate)） | https://doc.rust-lang.org/reference/visibility-and-privacy.html |
| 2 | Rust Reference — Preludes §extern prelude（--extern 白名单机制） | https://doc.rust-lang.org/reference/names/preludes.html#extern-prelude |
| 3 | Rust Reference — non_exhaustive | https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute |
| 4 | Rust API Guidelines — C-SEALED | https://rust-lang.github.io/api-guidelines/future-proofing.html |
| 5 | Cargo Book — Specifying Dependencies / Workspaces（lints table） | https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html ; https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table |
| 6 | rust-lang/cargo#4242（依赖环拒绝的例外讨论） | https://github.com/rust-lang/cargo/issues/4242 |
| 7 | rust-lang/rust — src/tools/tidy/src/style.rs（LINES=3000） | https://github.com/rust-lang/rust/blob/master/src/tools/tidy/src/style.rs |
| 8 | rust-lang/rust — src/tools/tidy/src/deps.rs（依赖白名单） | https://github.com/rust-lang/rust/blob/master/src/tools/tidy/src/deps.rs |
| 9 | rust-analyzer — xtask/src/tidy.rs（全文核读） | https://github.com/rust-lang/rust-analyzer/blob/master/xtask/src/tidy.rs |
| 10 | rust-analyzer — .github/workflows/ci.yaml（salsa 架构测试、conclusion job） | https://github.com/rust-lang/rust-analyzer/blob/master/.github/workflows/ci.yaml |
| 11 | rust-analyzer — 根 Cargo.toml（workspace.lints） | https://github.com/rust-lang/rust-analyzer/blob/master/Cargo.toml |
| 12 | rust-analyzer — architecture.md（Architecture Invariant 模式） | https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md |
| 13 | cargo-deny 官方文档 — bans 配置（deny/wrappers/workspace-dependencies） | https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html |
| 14 | bevy — deny.toml / dependencies.yml / ci.yml / 根 Cargo.toml | https://github.com/bevyengine/bevy/blob/main/deny.toml ; https://github.com/bevyengine/bevy/blob/main/.github/workflows/dependencies.yml ; https://github.com/bevyengine/bevy/blob/main/.github/workflows/ci.yml ; https://github.com/bevyengine/bevy/blob/main/Cargo.toml |
| 15 | zed — clippy.toml / 根 Cargo.toml / script/clippy / run_tests.yml | https://github.com/zed-industries/zed/blob/main/clippy.toml ; https://github.com/zed-industries/zed/blob/main/Cargo.toml ; https://github.com/zed-industries/zed/blob/main/script/clippy ; https://github.com/zed-industries/zed/blob/main/.github/workflows/run_tests.yml |
| 16 | rustc lint — unused_crate_dependencies | https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html#unused-crate-dependencies |
| 17 | clippy — wildcard_imports | https://rust-lang.github.io/rust-clippy/master/index.html#wildcard_imports |
| 18 | est31/cargo-udeps（nightly 限制） | https://github.com/est31/cargo-udeps |
| 19 | cargo-public-api（快照测试模式） | https://github.com/cargo-public-api/cargo-public-api |
| 20 | obi1kenobi/cargo-semver-checks | https://github.com/obi1kenobi/cargo-semver-checks |
| 21 | Slint Docs — Best Practices（代码/UI/资源分离） | https://docs.slint.dev/latest/docs/slint/guide/development/best-practices/ |
| 22 | slint-ui/slint — energy-monitor demo（controllers 模式，第一轮已核实） | https://github.com/slint-ui/slint/tree/master/demos/energy-monitor |
| 23 | GitHub Docs — About code owners | https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners |
| 24 | GitHub Docs — About protected branches | https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches |
| 25 | microsoft/vscode — .github/CODEOWNERS（原文） | https://github.com/microsoft/vscode/blob/main/.github/CODEOWNERS |
| 26 | pistonite/layered-crate（生态小工具参照） | https://github.com/pistonite/layered-crate |
