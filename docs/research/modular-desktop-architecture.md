# 调研报告：Rust + Slint 桌面应用的模块化单体架构

> 调研日期：2026-07-25
> 背景：MCRebuild V2 重写（Windows 单机桌面应用，Rust + Slint，模块化单体）。
> v1.x 教训：少数巨型 crate（单文件最大 5900 行）导致任何修改触发 20-30 分钟全量重编译。
> 本报告全部结论基于一手来源（官方文档、源码仓库、权威作者文章），每条事实性结论附来源 URL。

---

## 概要

1. **crate 是 Rust 的编译单元**，增量编译与并行编译都以 crate 依赖图（DAG）为粒度。要最小化增量编译时间，关键不是"多拆 crate"，而是把依赖图拆"宽"：一个小的公共词汇 crate + 多个互不依赖的功能 crate + 一个把所有东西拼起来的叶子 crate（rust-analyzer 作者 matklad 的结论，与 rust-analyzer/typst/zed 的实际结构一致）。
2. **业界共识是两级架构**：宏观层面按业务能力（bounded context）垂直切分模块；微观层面（模块内部）再按 feature 组织。"分层 vs 垂直"不是二选一——模块边界（crate）按业务垂直切，基础设施（持久化、地图、导出引擎）作为被依赖的水平底座。模块间通信在单进程桌面应用中优先用最简单的手段：直接函数/trait 调用，由叶子 crate（主程序）做编排；事件总线只在确有多对多广播需求时引入。
3. **Slint 官方推荐** `src/`（业务逻辑）与 `ui/`（.slint 文件）分离；.slint 语言自带模块系统（import/export），官方大型 demo（energy-monitor）用 `ui/pages / widgets / components` + `src/controllers` 的结构，Rust 侧 controller 与 UI 声明解耦，等价于轻量 MVVM。
4. **真实项目参照**：rust-analyzer 约 32 个 crate（20 万行）、typst 17 个 crate、zed 240 个 crate、lapce 4 个 crate。crate 数量与代码规模成正比，本项目起步 8-10 个 crate 是合理区间。
5. **对产品负责人五模块直觉的评估**：方向正确（按业务能力垂直切分），四个业务模块（教程/项目方案/数据采集/候选审核）可直接映射为 crate；但"UI 一个模块"需要修正——UI 应是**薄投影层**（只做界面声明和绑定），业务逻辑必须留在各功能 crate 里，否则 UI crate 会重演 v1.x 的巨型 crate。另外需补充产品负责人视角看不到的底座 crate：共享领域类型、SQLite 持久化、地图集成、导出引擎。

---

## 一、Rust 模块化单体最佳实践：怎样拆 crate 才能编译快

### 结论

1. **crate 是增量编译和并行编译的基本单位。** Cargo workspace 内所有成员共享一个 `Cargo.lock` 和一个 `target` 输出目录；修改一个 crate 只重编译它和依赖它的下游 crate。
   - 来源：[The Cargo Book — Workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)（"All packages share a common output directory"）
   - 来源：[The Cargo Book — Profiles: incremental](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental)（`incremental = true` 让 rustc 把增量信息存入 target 目录以加速重编译；dev profile 默认开启，release 默认关闭）
2. **依赖图的形状比 crate 数量更重要。** matklad（rust-analyzer 主要作者）在 *Fast Rust Builds* 中明确指出：链式依赖 `A→B→C→D→E` 拆了也白拆，必须拆成"宽"图——"a common vocabulary crate, a number of independent features, and a leaf crate to tie everything together"（一个公共词汇 crate、若干互不依赖的功能 crate、一个收拢一切的叶子 crate）。宽图同时带来并行编译和增量收益："改 B 不需要重编 C 和 D"。他还强调："**一个 crate 最重要的属性是它（传递地）不依赖什么**"。
   - 来源：[matklad — Fast Rust Builds §Compilation Model: Crates](https://matklad.github.io/2021/09/04/fast-rust-builds.html)
3. **workspace 布局采用扁平结构（flat layout）+ 虚拟清单（virtual manifest）。** matklad 在 *Large Rust Workspaces* 中总结：1 万-100 万行的项目用扁平的 `crates/*` 目录最合理（rust-analyzer 即如此，约 32 个子目录）；不要把主 crate 放 workspace 根（会污染根目录且破坏一致性）；目录名与 crate 名保持一致；内部 crate 用 `version = "0.0.0"`；自动化脚本用 `cargo xtask` 模式写成 Rust。
   - 来源：[matklad — Large Rust Workspaces](https://matklad.github.io/2021/08/22/large-rust-workspaces.html)
4. **重代价依赖（proc-macro / serde / syn）只放在叶子 crate。** proc macro 无法参与 Cargo 的流水线编译（pipelined compilation），会阻塞下游；serde derive 生成大量代码。matklad："把序列化留在系统边界的叶子 crate；如果放在底座，所有中间 crate 都要付它的编译成本。" 另外注意 feature 统一化陷阱：下游 crate 开启 `serde/derive` 会让上游所有用到 serde 的 crate 一起等 syn 编译。
   - 来源：[matklad — Fast Rust Builds §Compilation Model: Macros And Pipelining](https://matklad.github.io/2021/09/04/fast-rust-builds.html)
5. **泛型不要放在 crate 边界上。** 单态化（monomorphization）按 crate 进行，跨 crate 的泛型接口会导致重复实例化，"generics in Rust can lead to accidentally-quadratic compilation times across many crates"。大系统应设计为"每个组件做具体的事、暴露非泛型接口"；需要泛型人体工学时用薄泛型壳委托给非泛型实现（`fs::read` 的 inner 模式），闭包参数优先 `&dyn Fn()` 而非 `impl Fn()`。
   - 来源：[matklad — Fast Rust Builds §Compilation Model: Monomorphization / Keeping Instantiations In Check](https://matklad.github.io/2021/09/04/fast-rust-builds.html)
6. **持续度量，防止劣化。** 编译时间会"悄悄爬升"，等到不可忍受时已无廉价优化手段（帕累托法则对编译时间不成立——每一行代码都贡献编译时间）。工具：`cargo build --timings`（查看各 crate 编译时长与调度）、`cargo llvm-lines`（查看单态化膨胀）。
   - 来源：[matklad — Fast Rust Builds §The Silver Bullet / Profile Before Optimize](https://matklad.github.io/2021/09/04/fast-rust-builds.html)
7. **workspace 依赖统一声明。** 用根清单的 `[workspace.dependencies]` 集中管理版本，成员用 `dep.workspace = true` 继承，避免版本漂移（lapce、zed 均如此使用）。
   - 来源：[The Cargo Book — Workspaces: The dependencies table](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-dependencies-table)

### 对本项目的直接含义

v1.x 的 20-30 分钟重编译正是"巨型 crate + 链式依赖"的后果：任何一行改动都落在同一个编译单元里。V2 只要保证（a）功能 crate 互不依赖、（b）公共底座 crate 小且稳定、（c）serde/slint 代码生成只出现在叶子，日常"改一个功能 → 重编译该 crate + 主程序"即可控制在秒级到一两分钟。

---

## 二、分层 vs 按功能垂直切分（vertical slice）

### 结论

1. **两级架构，不是二选一。** 模块化单体的业界共识（以 Milan Jovanović 的系列文章为代表）是区分：
   - **宏观架构（macro）**：如何把系统分解为模块——模块边界、通信方式、数据隔离、公共 API。模块按**业务能力（bounded context）垂直切**，"Getting module boundaries wrong is expensive to fix"（模块边界切错的代价最高）。
   - **微观架构（micro）**：单个模块内部怎么组织代码——这是每个模块可独立决定的局部决策，可以用 vertical slice（按 feature 一个文件夹/文件），也可以用轻量分层，"模块边界已经保护了系统其余部分，模块内部不需要层来保护"。
   - 来源：[Milan Jovanović — Where Vertical Slices Fit Inside the Modular Monolith Architecture](https://milanjovanovic.tech/blog/where-vertical-slices-fit-inside-the-modular-monolith-architecture)
2. **vertical slice ≠ 模块。** 同文指出常见误区："A module is a bounded context... A vertical slice is a feature implementation pattern"。模块定义系统边界，slice 组织边界内的代码。对应到 Rust：**crate = 模块边界**（宏观），**crate 内的 mod / 文件 = slice**（微观）。
3. **纯技术分层（横切 UI 层/业务层/数据层各一个大 crate）是反模式**——它让每个功能的修改都跨越多个 crate，且所有功能挤在同一层 crate 里，等于重演 v1.x。但**基础设施做成被依赖的水平 crate 是正确的分层**：持久化、地图客户端、导出引擎被多个功能共享，单独成 crate 正好形成第一节要求的"宽"依赖图。matklad 描述的理想结构（公共词汇 crate + 独立功能 crate + 叶子 crate）本质上就是"垂直功能 + 少量水平底座"的混合。
   - 来源：[matklad — Fast Rust Builds §Compilation Model: Crates](https://matklad.github.io/2021/09/04/fast-rust-builds.html)
4. **模块间通信方式（单进程桌面应用）按成本从低到高选择：**
   - **直接函数调用 / 具体类型 API**（默认首选）：crate 的 `pub` API 就是模块的公共接口，Cargo 依赖方向天然强制单向调用。matklad 建议接口保持非泛型（见第一节第 5 条）。
   - **trait 接口（依赖倒置）**：仅当需要打破依赖方向（如底座回调上层）或需要测试替身时使用；用 `&dyn Trait` 而非泛型参数以避免单态化开销。
   - **共享状态/数据库中转**：功能 A 写入 SQLite，功能 B 读取——两个功能 crate 无需互相依赖，天然解耦（适合"数据采集 → 候选审核"这类流水线关系）。
   - **消息/事件总线**：在多对多广播、插件系统场景才划算（zed 的 gpui 内置 entity/event 机制服务于协作编辑器这种高动态场景）。对单人离线工具，事件总线增加间接层却无对应收益，业界模块化单体文章也将"模块间通信模式"列为宏观决策而非默认必需品。
   - 来源：[Milan Jovanović（同上，宏观架构包含 communication patterns）](https://milanjovanovic.tech/blog/where-vertical-slices-fit-inside-the-modular-monolith-architecture)；[matklad — Fast Rust Builds](https://matklad.github.io/2021/09/04/fast-rust-builds.html)

### 对本项目的直接含义

产品负责人的直觉（按用户能感知的功能切模块）恰好符合宏观层面"按业务能力垂直切分"的业界结论。需要工程侧补上的是：功能 crate 之间**禁止互相依赖**，跨功能协作通过（a）领域类型 + SQLite 状态、（b）主程序编排，二选一完成。

---

## 三、Slint 桌面应用的模块化经验

### 结论

1. **官方推荐目录：代码、UI、资源分离。** Slint 官方 Best Practices 文档给出的基础结构：

   ```
   my-project
   ├── src/            # main.rs——"this is where your main business logic lives"
   ├── ui/
   │   ├── app-window.slint   # Slint UI 入口
   │   ├── <additional .slint files>
   │   └── images/
   ```

   并建议：用户可见字符串一开始就用 `@tr("...")` 标记为可翻译；用 `{}` 占位符插值而非 `+` 拼接（与本项目 ADR-0005 完全一致）。
   - 来源：[Slint Docs — Best Practices](https://docs.slint.dev/latest/docs/slint/guide/development/best-practices/)
2. **.slint 语言自带模块系统，支持按文件拆分 UI。** 每个 .slint 文件默认私有，`export` 才对外可见；跨文件用 `import { Button } from "./button.slint"` 组合；支持重命名导出、re-export、以及跨项目共享的 component library 语法（`@mylibrary/switch.slint`，在 build.rs 里用 `slint_build::CompilerConfiguration::with_library_paths` 映射路径）。官方明确说明："Splitting your code base into separate module files promotes re-use and improves encapsulation by allowing you to hide helper components."
   - 来源：[Slint Docs — The .slint File §Modules / Component Libraries](https://docs.slint.dev/latest/docs/slint/guide/language/coding/file/)
3. **官方大型 demo（energy-monitor）的实际组织**（slint-ui/slint 仓库 `demos/energy-monitor`）：

   ```
   src/
     main.rs, lib.rs
     controllers/          # Rust 侧控制器（header.rs, weather.rs）
   ui/
     desktop_window.slint  # 窗口入口（另有 mcu/mobile 变体）
     theme.slint           # 主题集中定义
     pages/                # 每个页面一个 .slint（dashboard, usage, weather, settings...）
     widgets/              # 可复用控件（22 个文件，每控件一个文件）
     components/           # 更底层的组合件
     blocks/               # 页面级布局块（header 等）
   ```

   要点：**Rust 侧按 controller（每个功能域一个文件）与 UI 声明解耦**；UI 侧按 pages/widgets/components 三层粒度拆文件，每个组件一个文件。这就是 Slint 生态下事实上的 MVVM/ViewModel 形态：.slint 声明视图与可绑定属性/回调（View + ViewModel 接口），Rust controller 实现逻辑并回填数据（Model + ViewModel 实现）。
   - 来源：[slint-ui/slint — demos/energy-monitor 目录树](https://github.com/slint-ui/slint/tree/master/demos/energy-monitor)（结构经 GitHub API `git/trees` 逐文件核实）
4. **对编译的影响**：.slint 文件由 `slint_build` 在**一个 crate 的 build.rs** 里统一编译进该 crate（见 component library 文档中的 build.rs 示例）。这意味着 UI 声明天然聚合在一个叶子 crate（主程序）——改 .slint 只重编主程序 crate；反过来也要求**业务逻辑绝不能写进这个 crate**，否则该 crate 会同时承担 UI 代码生成 + 业务逻辑，重编译成本叠加，重演 v1.x。
   - 来源：[Slint Docs — The .slint File §Component Libraries（build.rs 用法）](https://docs.slint.dev/latest/docs/slint/guide/language/coding/file/)

---

## 四、真实 Rust 桌面/工具软件的 crate 划分实例

以下数据均直接取自各仓库源码（GitHub API 逐目录核实，2026-07 时点）。

### 4.1 rust-analyzer（语言服务器，约 20 万行，~32 个 crate）

- **布局**：虚拟清单 + 扁平 `crates/*`，32 个子目录：`base_db, hir, hir_def, hir_ty, ide, ide_assists, ide_completion, ide_db, parser, syntax, vfs, ...` + 叶子 crate `rust-analyzer`。
- **划分逻辑**：按编译管线阶段（syntax → hir → ide）+ 基础设施（vfs, paths, stdx）切分；`ide_*` 系列是互不依赖的功能 crate，全部汇入叶子 `rust-analyzer`。
- **依赖方向**：底层（syntax/base_db）→ 中层（hir*）→ 功能层（ide_*）→ 叶子（rust-analyzer），功能层横向不依赖。
- 来源：[matklad — Large Rust Workspaces（含完整 crate 列表）](https://matklad.github.io/2021/08/22/large-rust-workspaces.html)；[rust-analyzer 仓库 crates/](https://github.com/rust-lang/rust-analyzer/tree/master/crates)

### 4.2 typst（排版编译器，17 个 crate）

- **crate 清单**（`crates/` 下 17 个目录）：`typst, typst-cli, typst-syntax, typst-eval, typst-realize, typst-layout, typst-library, typst-html, typst-pdf, typst-svg, typst-render, typst-ide, typst-kit, typst-macros, typst-timing, typst-utils, typst-bundle`。
- **划分逻辑**：官方架构文档明确按编译四阶段（parsing → evaluation → layout → export）切 crate；**每种导出器单独一个 crate**（"Exporters live in separate crates"）；`typst-cli` 是"compiler 与 exporters 之上的一个相对小的层"（薄叶子）。
- **依赖方向**：`typst-syntax`（无依赖底座，"parsing is a pure function"）→ eval/realize/layout → 导出器们（互不依赖）→ cli。
- 来源：[typst/typst — docs/dev/architecture.md](https://github.com/typst/typst/blob/main/docs/dev/architecture.md)；[typst/typst — crates/ 目录](https://github.com/typst/typst/tree/main/crates)

### 4.3 zed（协作代码编辑器，240 个 crate）

- **规模**：`crates/` 下 240 个目录（GitHub API 计数）。虚拟清单 + 扁平布局，与 matklad 建议一致。
- **划分逻辑**：细粒度垂直切分——每个面板/功能一个 crate（`project_panel, outline_panel, file_finder, search, vim, journal, ...`），每个 LLM 供应商一个 crate（`anthropic, ollama, open_ai, ...`），UI 框架自研为独立 crate 族（`gpui, gpui_windows, gpui_macros, ...`），基础类型下沉（`rope, sum_tree, text, fs, db, settings, theme, util`），最后由叶子 crate `zed` 拼装。
- **依赖方向规则**：util/collections/gpui 等底座 → 领域核心（language, project, editor, workspace）→ 功能面板 crate（互不依赖，都插到 workspace 上）→ 叶子 `zed`。
- 来源：[zed-industries/zed — crates/ 目录](https://github.com/zed-industries/zed/tree/main/crates)（240 个子目录经 GitHub API 计数并逐名列出）

### 4.4 lapce（代码编辑器，4 个 workspace crate）——反向参照

- **crate 清单**：`lapce-app`（UI + 业务，巨型）、`lapce-proxy`（后端进程）、`lapce-rpc`（两者间协议）、`lapce-core`（公共类型）。
- **参考价值**：说明 crate 拆分粒度是一个谱系；lapce 用进程边界（app/proxy）代替了细粒度 crate 边界，其 `lapce-app` 单 crate 承载全部 UI 与编辑逻辑——这正是本项目要避免的形态。值得借鉴的是它的 `[profile.fastdev]`：依赖以 release 编译、自有代码以 dev 编译，"After the initial build subsequent ones are as fast as dev mode builds"。
- 来源：[lapce/lapce — Cargo.toml（workspace members 与 fastdev profile 注释）](https://github.com/lapce/lapce/blob/master/Cargo.toml)

### 4.5 横向对比

| 项目 | 代码量级 | crate 数 | 划分主轴 | 叶子 crate |
|---|---|---|---|---|
| rust-analyzer | ~200k 行 | ~32 | 管线阶段 + 独立 ide 功能 | `rust-analyzer` |
| typst | 中型 | 17 | 管线阶段 + 每导出器一 crate | `typst-cli` |
| zed | 巨型 | 240 | 细粒度垂直功能 + 底座族 | `zed` |
| lapce | 中型 | 4 | 进程边界 | `lapce` |

规律：**所有健康的大项目都收敛到"底座 → 领域 → 互不依赖的功能 → 单一叶子"的宽 DAG**；crate 数量随规模自然增长，起步时不必贪多，但功能之间的横向隔离从第一天就要成立。

---

## 五、给本项目的具体建议

### 5.1 对产品负责人五模块直觉的评估

| 直觉模块 | 评估 | 说明 |
|---|---|---|
| UI 一个模块 | ⚠️ 需修正 | UI 应是**薄投影层**：只含 .slint 声明 + ViewModel 绑定/编排，不含业务逻辑（Slint 官方结构与 energy-monitor demo 均如此；typst-cli"相对小的层"同理）。若把"所有界面相关代码"堆进一个 UI crate，它将成为新的巨型 crate——v1.x 复发。 |
| 新手教程 | ✅ 合理 | 教程的流程状态机（当前步骤、完成条件、跳过规则）独立成 crate；教程的**画面**属于 UI 投影层。预计是最小的功能 crate。 |
| 项目方案管理 | ✅ 合理 | 典型 bounded context（项目的创建/列表/恢复等行为规则），直接映射为功能 crate。 |
| 数据采集 | ✅ 合理 | 独立功能 crate；其依赖的地图/网络客户端下沉为底座 crate（v1.x 迁移件）。 |
| 候选审核批准 | ✅ 合理 | 独立功能 crate；与数据采集之间通过 SQLite 中的候选数据衔接，两个 crate 无需互相依赖。 |
| （缺失） | ➕ 需补充 | 产品视角看不到的底座：共享领域类型、SQLite 持久化、高德地图集成、Sponge 导出引擎、地基模式引擎（后三者按 ADR-0003 自 v1.x 迁移，已在现有 Cargo.toml 规划中）。 |

**总评**：五模块直觉在宏观层面正确（按业务能力垂直切分，符合第二节业界结论），采纳为功能 crate 骨架；工程侧补充底座层并把"UI 模块"重新定义为薄投影层。

### 5.2 推荐 crate 划分清单（12 个，扁平布局）

结合现有 `Cargo.toml` 规划（apps/desktop、core/foundation、core/sponge-export、core/data、core/maps）扩展：

| crate | 职责（一句话） |
|---|---|
| `apps/desktop` | 唯一叶子：.slint UI 声明 + ViewModel 绑定，把各功能 crate 拼成一个主程序，不含业务逻辑。 |
| `features/tutorial` | 新手教程的流程状态机（步骤、完成条件、跳过/重看规则）。 |
| `features/project` | 项目方案管理的用例逻辑（创建、列表、打开、删除/恢复规则）。 |
| `features/acquisition` | 数据采集编排（调用地图底座取数、写入候选数据）。 |
| `features/review` | 候选审核批准的用例逻辑（五类目队列、批准/驳回状态流转）。 |
| `core/foundation` | 地基模式引擎（校园边界、地块生成）。 |
| `core/sponge-export` | Sponge V3 导出引擎（自 v1.x 迁移，ADR-0003）。 |
| `core/maps` | 高德地图集成客户端（自 v1.x 迁移，ADR-0003）。 |
| `core/data` | SQLite 持久化：schema、迁移、各功能的存取接口（ADR-0002）。 |
| `core/domain` | 公共词汇 crate：跨功能共享的领域类型（项目、地块、候选物、审核状态），近零依赖。 |
| `core/i18n` | 语言资源加载与文本键查找（ADR-0005 的运行时支撑）。 |
| `xtask` | 全部构建/打包/校验自动化，替代零散脚本（matklad 的 cargo xtask 模式）。 |

> 说明：`features/*` 与 `core/*` 的目录前缀仅为可读性分组；workspace 成员列表仍是扁平清单，目录名 = crate 名（matklad 扁平布局建议）。教程 crate 起步很小没关系——matklad："即使 crate 现在只有一个文件，以后也会长大"。

### 5.3 依赖方向图

```
                        apps/desktop  （唯一叶子：Slint 代码生成 + serde 等重依赖只在此层）
        ┌────────────┬───────┴────────┬─────────────┐
 features/tutorial  features/project  features/acquisition  features/review
        │            │                │        │             │
        │            │                │   core/maps          │      ← 功能层互不依赖（宽 DAG）
        └──────┬─────┴───────┬────────┴────────┬─────────────┘
          core/data    core/foundation   core/sponge-export
               └─────────────┴───────┬──────────┘
                              core/domain（+ core/i18n）      ← 底座：小、稳定、近零依赖
```

规则：箭头只能向下（上层依赖下层）；同层之间零依赖；`core/domain` 不依赖任何同 workspace crate。跨功能协作（采集 → 审核）通过 `core/data` 中的持久化状态衔接，或由 `apps/desktop` 编排，**禁止** `features/review` 直接 import `features/acquisition`。

### 5.4 防止 v1.x 巨型 crate 复发的硬性规则

1. **文件行数上限**：单个 .rs 文件超过 500 行必须说明理由，超过 1000 行 CI 拒绝（v1.x 峰值 5900 行；rust-analyzer/typst 的常态是数百行/文件）。
2. **功能 crate 横向零依赖**：`features/*` 之间禁止互相出现在 `[dependencies]`；CI 用 `cargo metadata` 检查依赖图（Cargo 本身已禁止循环依赖，此规则额外禁止同层横向依赖）。
3. **叶子独占重依赖**：`slint`/`slint-build`、`serde` 的 `derive` feature 只允许出现在 `apps/desktop`（及确需序列化边界的 `core/data`）；`core/domain` 禁止任何 proc-macro 重依赖（依据：proc macro 阻塞流水线编译，序列化应留在系统边界叶子）。
4. **crate 边界非泛型**：跨 crate 的 `pub` API 禁止泛型参数（薄泛型壳 + 非泛型 inner 除外）；回调参数用 `&dyn Fn()` 而非 `impl Fn()`（依据：单态化跨 crate 二次方膨胀）。
5. **编译时间预算入 CI**：每次 CI 记录 `cargo build --timings` 摘要；工作区全量 dev 构建超过预算（建议初始 5 分钟）即视为回归，当周处理（依据：matklad"编译时间会悄悄爬升，必须在成为问题前治理"）。
6. **新 crate 准入 = 接口评审**（延续现有纪律）：加入 `members` 前先评审其 `pub` API 与依赖方向；`apps/desktop` 中任何超过"绑定/编排"范畴的逻辑必须下沉到对应 `features/*`。
7. **开发体验兜底**：参照 lapce 增加 `[profile.fastdev]`（依赖 release、自有代码 dev），保证首编之后的日常迭代速度。

### 5.5 落地顺序建议

先建 `core/domain` + `core/data` + `apps/desktop` 三件套跑通"空壳主程序 + 持久化"，随后按 ADR-0003 迁入 `core/maps` / `core/sponge-export` / `core/foundation`，最后按产品优先级逐个开 `features/*`——每开一个即验证一次"只改该 crate 时的重编译时间"。

---

## 附：来源清单

| # | 来源 | URL |
|---|---|---|
| 1 | The Cargo Book — Workspaces | https://doc.rust-lang.org/cargo/reference/workspaces.html |
| 2 | The Cargo Book — Profiles（incremental / codegen-units / lto） | https://doc.rust-lang.org/cargo/reference/profiles.html |
| 3 | matklad — Large Rust Workspaces | https://matklad.github.io/2021/08/22/large-rust-workspaces.html |
| 4 | matklad — Fast Rust Builds | https://matklad.github.io/2021/09/04/fast-rust-builds.html |
| 5 | Milan Jovanović — Where Vertical Slices Fit Inside the Modular Monolith Architecture | https://milanjovanovic.tech/blog/where-vertical-slices-fit-inside-the-modular-monolith-architecture |
| 6 | Slint Docs — Best Practices（Separate Code, UI, and Assets；Translations） | https://docs.slint.dev/latest/docs/slint/guide/development/best-practices/ |
| 7 | Slint Docs — The .slint File（Modules / Component Libraries） | https://docs.slint.dev/latest/docs/slint/guide/language/coding/file/ |
| 8 | slint-ui/slint — energy-monitor demo 源码目录 | https://github.com/slint-ui/slint/tree/master/demos/energy-monitor |
| 9 | rust-lang/rust-analyzer — crates/ 目录 | https://github.com/rust-lang/rust-analyzer/tree/master/crates |
| 10 | typst/typst — 官方架构文档 docs/dev/architecture.md | https://github.com/typst/typst/blob/main/docs/dev/architecture.md |
| 11 | typst/typst — crates/ 目录（17 crate 清单） | https://github.com/typst/typst/tree/main/crates |
| 12 | zed-industries/zed — crates/ 目录（240 crate，GitHub API 计数） | https://github.com/zed-industries/zed/tree/main/crates |
| 13 | lapce/lapce — 根 Cargo.toml（4 成员 workspace 与 fastdev profile） | https://github.com/lapce/lapce/blob/master/Cargo.toml |
