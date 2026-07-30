//! S1 书面契约基线（工单 01）。
//!
//! 这些检查只固定已接受决定的可追溯性；不会把迁移中的内部函数或数据结构
//! 写进契约。S1 的用户可观察行为由 `docs/behavior-baselines/` 固定。

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("desktop-shell 必须位于 workspace/apps/desktop")
        .to_path_buf()
}

fn read_workspace_file(relative: &str) -> String {
    let path = workspace_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("无法读取书面契约 {}: {error}", path.display());
    })
}

#[test]
fn latest_s1_and_map_decisions_are_traceable_from_all_indexes() {
    let readme = read_workspace_file("README.md");
    let glossary = read_workspace_file("CONTEXT.md");
    let module_decisions = read_workspace_file("docs/module-decisions.md");
    let adr_0017 = read_workspace_file("docs/adr/0017-modular-architecture-and-crate-catalog.md");

    assert!(
        readme.contains("[ADR-0037](docs/adr/0037-s1-presentation-only-shell.md)")
            && readme.contains("[ADR-0038](docs/adr/0038-no-amap-offline-map-in-v2.md)"),
        "README 的 ADR 索引必须包含 ADR-0037 与 ADR-0038"
    );
    assert!(
        glossary.contains("ADR-0037")
            && glossary.contains("只负责呈现")
            && glossary.contains("不承担业务协调"),
        "“薄壳”术语必须引用 ADR-0037 的最新定义"
    );
    assert!(
        module_decisions.contains("S1 只负责呈现")
            && module_decisions.contains("不得直接依赖")
            && module_decisions.contains("B17/B18 生成与导出实现")
            && module_decisions.contains("不提供高德离线地图")
            && module_decisions.contains("不抓取或缓存高德地图"),
        "模块决策索引必须同时追溯 S1 薄壳与高德离线地图范围"
    );
    assert!(
        adr_0017.contains("由 ADR-0037 收紧")
            && adr_0017.contains("不得直接协调持久化、生成或导出"),
        "旧 ADR-0017 必须明确不再授权 S1 协调业务实现"
    );
}

#[test]
fn dependency_rule_separates_presentation_from_business_implementation() {
    let adr_0037 = read_workspace_file("docs/adr/0037-s1-presentation-only-shell.md");

    for allowed in ["页面状态", "进度", "导航结果", "通知"] {
        assert!(
            adr_0037.contains(allowed),
            "ADR-0037 必须列出 S1 可使用的呈现能力：{allowed}"
        );
    }
    for forbidden in ["持久化", "采集转换", "几何规则", "生成", "导出实现"] {
        assert!(
            adr_0037.contains(forbidden),
            "ADR-0037 必须列出 S1 禁止接触的业务实现：{forbidden}"
        );
    }
}

#[test]
fn behavior_baseline_covers_every_flow_and_outcome_at_the_ui_seam() {
    let baseline =
        read_workspace_file("docs/behavior-baselines/s1-current-user-observable-behavior.md");

    for flow in [
        "启动",
        "设置",
        "校区与方案",
        "五步流程",
        "采集",
        "评审",
        "导出",
    ] {
        let row_prefix = format!("| **{flow}** |");
        let row = baseline
            .lines()
            .find(|line| line.starts_with(&row_prefix))
            .unwrap_or_else(|| panic!("行为基线缺少流程行：{flow}"));
        let cells: Vec<_> = row
            .split('|')
            .skip(1)
            .take_while(|cell| !cell.is_empty())
            .map(str::trim)
            .collect();
        assert!(
            cells.len() == 7 && cells.iter().all(|cell| !cell.is_empty()),
            "流程 {flow} 必须分别记录页面、成功、失败、处理中、确认、导航与通知"
        );
    }
    assert!(
        baseline
            .contains("| 流程 | 页面与初始状态 | 成功 | 失败 | 处理中 | 需要确认 | 导航与通知 |"),
        "流程矩阵必须把四类结果及导航与通知分别列出"
    );
    for observable in ["页面", "状态", "通知", "导航"] {
        assert!(
            baseline.contains(observable),
            "行为基线缺少公开观察通道：{observable}"
        );
    }
    assert!(
        baseline.contains("不固定内部函数或数据结构"),
        "行为基线必须明确排除内部实现细节"
    );
    assert!(
        baseline.contains("当前实现静默显示首次设置")
            && baseline.contains("当前实现静默显示校区选择")
            && baseline.contains("失败显示错误通知后导航回评审步骤"),
        "基线必须如实固定启动读取失败与导出失败的现有可观察行为"
    );

    let ui_contract = read_workspace_file("apps/desktop/ui/main.slint");
    for public_observation in [
        "active-screen",
        "status-text",
        "error-dialog-visible",
        "confirm-dialog-visible",
    ] {
        assert!(
            ui_contract.contains(public_observation),
            "AppWindow 缺少基线所需的公开观察通道：{public_observation}"
        );
    }
}
