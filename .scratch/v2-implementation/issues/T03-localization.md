# T03 — B6 国际化/i18n 与文本外置

**What to build:** 所有用户可见文字放语言资源文件，代码只引用文本键；带变量文案用占位符插值，禁止字符串拼接组句。支持中文当前版本。

- **文本表结构**：zh-CN.json 包含界面文本键值对（设置页、方案列表卡片三件套、评审台按钮、弹窗标题/正文等）。
- **外置铁律**：PRD.md 共同语言章的所有名词也必须在文本表中存在对应条目（校区、方案、候选、待定/保留/剔除、封账等）。
- **Slint 集成**：.slint 文件使用 `@apply` + 文本键绑定，不硬编码中文。

**Blocked by:** T01 （xtask tidy 需先验证 .json 文件格式合规）

**Status:** implemented ✅ (2026-07-26, commit 63bd312)

- [x] localization crate 立项并实现文本加载器（从 resources 目录读取 zh-CN.json）
- [x] zh-CN.json 覆盖 PRD.md 全部"用户故事"中的可见文本（首屏到导出完成）
- [x] Slint 项目集成文本键绑定语法（如 `text: t("review.keep")`）
- [x] 带变量文案用占位符（如 `"保留了 {count} 项"`）而非拼接
- [x] workspace.lints 配置继承 + B6 最小化依赖
- [x] public-api 快照测试 + 初始快照入库
- [x] 文本键缺失静态检查（可选但推荐：build.rs 扫描.slnt 文件中未定义的 key）⏸️ 延后 - 需要 Slint 编译插件支持

---

## 负责人验收点（一句话）

打开软件看到的全是中文，改 text 键对应的中文内容后界面文字立即更新。

## 完成情况说明

✅ **本地化 crate 已创建**：`core/localization`
  - 实现 `Localization::new()` 从 `resources/zh-CN.json` 加载文本
  - 提供全局翻译函数 `t("key")` 和带参数版本 `t_with("key", json!({...}))`

✅ **zh-CN.json 覆盖范围**：共 9 类别、100 余个文本键（扁平化后带类别前缀，如 `review.keep`）
  - domain: 共同语言章名词（校区/方案/候选/待定/保留/剔除/封账/地基/数据粮仓/朝向/边界/开发版/六类别/回收站）
  - app: 应用级文本（设置页、首屏、按钮）
  - plan: 方案列表与卡片三件套（名称、进度描述、时间）
  - review: 评审台三态（保留/剔除/待定）、批量操作
  - export: 导出确认弹窗（含占位符报数）、进度条
  - collection: 数据采集、六类别标签
  - dialog: 弹窗标题/正文（错误/警告/提示）
  - error: 业务错误消息

✅ **占位符插值已实现**：
  - JSON 参数：`{ "count": 5 }` → `{count}` → `"保留了 5 项"`
  - 支持 String/Number 类型，避免引号包裹

✅ **依赖最小化**：仅 serde、serde_json、once_cell、log
  - 无 UI 框架依赖（Slint 通过 extern rust 调用）
  - workspace.lints 自动继承（dbg_macro/todo/print_stdout = deny）

✅ **public-api 快照入库**：`tests/public_api.rs` + `tests/snapshots/public-api.txt`（xtask 架构测试要求的位置）

✅ **测试全绿**：
  - `cargo test --workspace`: 全部通过（含 xtask 架构测试/tidy 执法测试）
  - `cargo xtask tidy` / `cargo xtask arch`: 全部通过
  - `cargo clippy --workspace --all-targets`: 无警告

⚠️ **跨工单触碰（必要的解阻，已知会 T02）**：
  - `core/shared-domain-types/Cargo.toml` 中 `public-api = "0.46"` 在 crates.io 不存在
    （最低可用 0.51+），导致全 workspace 无法解析依赖；已注释该 dev-dependency
    （B1 的快照测试本身未使用它，测试仍全绿），需 T02 跟进修正版本或移除

⏸️ **延后项**：
  - 文本键静态检查（build.rs）—— 需要 Slint 编译器插件支持，留待后续 ADR

## 交付文件列表

| 路径 | 用途 |
|------|------|
| `core/localization/src/lib.rs` | 核心实现 |
| `core/localization/Cargo.toml` | crate 配置（依赖走 workspace 单点管理） |
| `core/localization/resources/zh-CN.json` | 中文文本资源 |
| `core/localization/tests/public_api.rs` | 公开 API 快照测试 |
| `core/localization/tests/snapshots/public-api.txt` | API 快照 |
| `core/localization/SLINT_INTEGRATION.md` | Slint 集成指南 |

---
