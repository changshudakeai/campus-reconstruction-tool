//! xtask arch —— 架构测试（执法清单 2.2，仿 rust-analyzer / rustc tidy deps）。
//!
//! 用 `cargo metadata` 读出真实依赖图（workspace 成员间的 normal/build 边，
//! dev-dependencies 不计），断言 ADR-0017 第三节依赖 DAG 的整套规则：
//! 1. 功能模块（F*）横向零依赖；
//! 2. 壳 `desktop-shell` 的成员依赖 ⊆ 白名单（F1-F9、B1-B7、B9-B11、B17；
//!    绝对禁止 B12-B15 ETL/GIS 层）；
//! 3. B1 `shared-domain-types` 内部依赖数为 0；
//! 4. 基础模块不依赖功能层/壳（下不依上）；基础层横向零依赖，唯二例外：
//!    人人可依 B1；B13→B14→B15 单向链；
//! 5. `xtask` 与业务 crate 互不依赖；未在 ADR-0017 目录立户的 crate 拒收；
//! 6. 每个已立户的基础 crate 必须有 public-api 快照测试与入库快照
//!    （执法清单 2.5，模板见 docs/developer-guide/enforcement.md）。
//!
//! 循环依赖无需在此检测：crate 级的环被 Cargo 原生拒绝（调研报告 §1.2）。

use std::path::Path;

use cargo_metadata::DependencyKind;

/// F1-F9 功能模块 crate 名（ADR-0017 第二节 A 表）。
const FEATURE_CRATES: &[&str] = &[
    "global-settings",     // F1
    "onboarding-tutorial", // F2
    "project-management",  // F3
    "data-acquisition",    // F4
    "review-workbench",    // F5
    "coverage-audit",      // F7
    "export-console",      // F9
];

/// B1-B18 基础模块 crate 名（ADR-0017 第二节 B 表，B16 已并入 B14）。
const BASE_CRATES: &[&str] = &[
    "shared-domain-types",  // B1
    "data-persistence",     // B2
    "gaode-client",         // B3
    "sponge-export",        // B4
    "foundation-mode",      // B5
    "localization",         // B6
    "notification-center",  // B7
    "undo-redo",            // B8（暂缓实施，保留席位）
    "global-shortcuts",     // B9
    "theming",              // B10
    "diagnostics",          // B11
    "data-source-adapters", // B12
    "data-transformers",    // B13
    "geometry-validator",   // B14
    "topology-rules",       // B15
    "manifest-generator",   // B17
    "generation-engine",    // B18
];

/// S1 薄壳。
const SHELL_CRATE: &str = "desktop-shell";

/// S2 工程工具（仅构建期，不与业务 crate 相互依赖）。
const TOOLING_CRATE: &str = "xtask";

/// 壳允许依赖的成员 crate 白名单（ADR-0017 DAG：F1-F9 + B1-B7 + B9-B11 + B17）。
const SHELL_ALLOWED_MEMBER_DEPS: &[&str] = &[
    // 功能层
    "global-settings",
    "onboarding-tutorial",
    "project-management",
    "data-acquisition",
    "review-workbench",
    "coverage-audit",
    "export-console",
    // 允许的基础层（B1 人人可依；B2-B7、B9-B11、B17 按 DAG）
    "shared-domain-types",
    "data-persistence",
    "gaode-client",
    "sponge-export",
    "foundation-mode",
    "localization",
    "notification-center",
    "global-shortcuts",
    "theming",
    "diagnostics",
    "manifest-generator",
];

/// 基础层内部允许的横向边：人人可依 B1；B13→B14→B15 单向链。
const BASE_ALLOWED_EDGES: &[(&str, &str)] = &[
    ("data-transformers", "geometry-validator"),
    ("geometry-validator", "topology-rules"),
];

fn is_feature(name: &str) -> bool {
    FEATURE_CRATES.contains(&name)
}

fn is_base(name: &str) -> bool {
    BASE_CRATES.contains(&name)
}

fn is_known(name: &str) -> bool {
    is_feature(name) || is_base(name) || name == SHELL_CRATE || name == TOOLING_CRATE
}

/// 对成员依赖边集合执行全部架构断言，返回违规描述（空 = 通过）。
///
/// `edges` 中每条 `(from, to)` 是一个 workspace 成员对另一个成员的
/// 直接依赖（normal/build 边）。
pub(crate) fn check_edges(members: &[String], edges: &[(String, String)]) -> Vec<String> {
    let mut violations = Vec::new();

    // 断言 5（前半）：所有成员必须已在 ADR-0017 目录立户。
    for member in members {
        if !is_known(member) {
            violations.push(format!(
                "crate `{member}` 未在 ADR-0017 模块目录中立户——先补 ADR/接口评审再入 workspace"
            ));
        }
    }

    for (from, to) in edges {
        let (from, to) = (from.as_str(), to.as_str());

        // 断言 5（后半）：xtask 与业务 crate 互不依赖。
        if from == TOOLING_CRATE || to == TOOLING_CRATE {
            violations.push(format!(
                "禁止边 {from} → {to}：xtask 是构建期工具，不得与业务 crate 相互依赖"
            ));
            continue;
        }

        // 断言 3：B1 共同语言零内部依赖。
        if from == "shared-domain-types" {
            violations.push(format!(
                "禁止边 {from} → {to}：B1 共享领域类型必须零内部依赖（全工程词汇的最底座）"
            ));
            continue;
        }

        // 断言 1：功能模块横向零依赖。
        if is_feature(from) && is_feature(to) {
            violations.push(format!(
                "禁止边 {from} → {to}：功能模块之间横向零依赖（ADR-0017），共享数据走 B1/B2"
            ));
            continue;
        }

        // 功能层不得反依壳。
        if is_feature(from) && to == SHELL_CRATE {
            violations.push(format!(
                "禁止边 {from} → {to}：下层不得依赖壳（依赖单向向下）"
            ));
            continue;
        }

        // 断言 2：壳依赖 ⊆ 白名单（尤其绝对禁止 B12-B15）。
        if from == SHELL_CRATE && !SHELL_ALLOWED_MEMBER_DEPS.contains(&to) {
            violations.push(format!(
                "禁止边 {from} → {to}：壳只准依赖 F1-F9、B1-B7、B9-B11、B17；\
                 ETL/GIS 层（B12-B15）必须经功能模块中转"
            ));
            continue;
        }

        // 断言 4：基础层不依上层；横向只许 B*→B1 与 B13→B14→B15。
        if is_base(from) {
            if is_feature(to) || to == SHELL_CRATE {
                violations.push(format!(
                    "禁止边 {from} → {to}：基础模块不得依赖功能层/壳（下不依上）"
                ));
            } else if is_base(to)
                && to != "shared-domain-types"
                && !BASE_ALLOWED_EDGES.contains(&(from, to))
            {
                violations.push(format!(
                    "禁止边 {from} → {to}：基础层横向零依赖（例外仅 B*→B1、B13→B14→B15）"
                ));
            }
        }
    }
    violations
}

/// 从 cargo metadata 提取成员名单与成员间依赖边（normal/build，不含 dev）。
pub(crate) fn member_edges(
    metadata: &cargo_metadata::Metadata,
) -> (Vec<String>, Vec<(String, String)>) {
    let members: Vec<String> = metadata
        .workspace_packages()
        .iter()
        .map(|package| package.name.clone())
        .collect();
    let mut edges = Vec::new();
    for package in metadata.workspace_packages() {
        for dep in &package.dependencies {
            let counts = matches!(dep.kind, DependencyKind::Normal | DependencyKind::Build);
            if counts && members.contains(&dep.name) {
                edges.push((package.name.clone(), dep.name.clone()));
            }
        }
    }
    (members, edges)
}

/// 断言 6：已立户的基础 crate 必须有 public-api 快照测试 + 入库快照。
pub(crate) fn snapshot_violations(metadata: &cargo_metadata::Metadata) -> Vec<String> {
    let mut violations = Vec::new();
    for package in metadata.workspace_packages() {
        if !is_base(&package.name) {
            continue;
        }
        let crate_dir = package.manifest_path.parent().expect("crate 必有父目录");
        for required in ["tests/public_api.rs", "tests/snapshots/public-api.txt"] {
            if !crate_dir.join(required).exists() {
                violations.push(format!(
                    "基础 crate `{}` 缺少 {required}——公开 API 必须以快照入库\
                     （模板见 docs/developer-guide/enforcement.md）",
                    package.name
                ));
            }
        }
    }
    violations
}

/// 对真实 workspace 执行全部架构断言。
pub(crate) fn workspace_violations(root: &Path) -> anyhow::Result<Vec<String>> {
    let metadata = crate::workspace_metadata(root)?;
    let (members, edges) = member_edges(&metadata);
    let mut violations = check_edges(&members, &edges);
    violations.extend(snapshot_violations(&metadata));
    Ok(violations)
}

/// `cargo xtask arch` 入口：打印违规并以非零码退出（CI 阻断）。
pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    let violations = workspace_violations(root)?;
    if violations.is_empty() {
        println!("arch: 依赖图符合 ADR-0017（横向零依赖 / 壳白名单 / B1 零依赖 / 下不依上）");
        return Ok(());
    }
    for violation in &violations {
        println!("架构违规: {violation}");
    }
    anyhow::bail!("架构测试失败：{} 处违规", violations.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: &str, to: &str) -> (String, String) {
        (from.to_owned(), to.to_owned())
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn legal_dag_passes() {
        let members = names(&[
            "desktop-shell",
            "project-management",
            "data-acquisition",
            "shared-domain-types",
            "data-persistence",
            "data-transformers",
            "geometry-validator",
            "topology-rules",
            "xtask",
        ]);
        let edges = vec![
            edge("desktop-shell", "project-management"),
            edge("project-management", "data-persistence"),
            edge("data-persistence", "shared-domain-types"),
            edge("data-acquisition", "data-transformers"),
            edge("data-transformers", "geometry-validator"),
            edge("geometry-validator", "topology-rules"),
        ];
        assert_eq!(check_edges(&members, &edges), Vec::<String>::new());
    }

    #[test]
    fn feature_to_feature_edge_is_rejected() {
        let violations = check_edges(&[], &[edge("data-acquisition", "review-workbench")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("横向零依赖"));
    }

    #[test]
    fn shell_reaching_etl_layer_is_rejected() {
        let violations = check_edges(&[], &[edge("desktop-shell", "data-source-adapters")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("B12-B15"));
    }

    #[test]
    fn shared_domain_types_must_have_zero_deps() {
        let violations = check_edges(&[], &[edge("shared-domain-types", "data-persistence")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("零内部依赖"));
    }

    #[test]
    fn base_depending_on_feature_is_rejected() {
        let violations = check_edges(&[], &[edge("sponge-export", "export-console")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("下不依上"));
    }

    #[test]
    fn base_lateral_edge_outside_exceptions_is_rejected() {
        let violations = check_edges(&[], &[edge("notification-center", "data-persistence")]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("基础层横向零依赖"));
        // 例外：人人可依 B1；B13→B14→B15。
        assert!(check_edges(&[], &[edge("notification-center", "shared-domain-types")]).is_empty());
        assert!(check_edges(&[], &[edge("data-transformers", "geometry-validator")]).is_empty());
    }

    #[test]
    fn unknown_crate_is_rejected() {
        let violations = check_edges(&names(&["mystery-crate"]), &[]);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("未在 ADR-0017"));
    }

    #[test]
    fn xtask_must_stay_isolated() {
        let violations = check_edges(
            &[],
            &[
                edge("xtask", "shared-domain-types"),
                edge("project-management", "xtask"),
            ],
        );
        assert_eq!(violations.len(), 2);
    }
}
