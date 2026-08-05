//! S1 书面契约基线（工单 01）。
//!
//! 这些检查只固定已接受决定的可追溯性；不会把迁移中的内部函数或数据结构
//! 写进契约。S1 的用户可观察行为由 `docs/behavior-baselines/` 固定。

use desktop_shell::{landing_decision, AppWindow, LandingDecision};
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

fn read_desktop_sources() -> String {
    fn append_sources(path: &Path, output: &mut String) {
        let entries = fs::read_dir(path)
            .unwrap_or_else(|error| panic!("无法读取 S1 源目录 {}: {error}", path.display()));
        for entry in entries {
            let entry = entry.expect("读取 S1 源目录项");
            let path = entry.path();
            if path.is_dir() {
                append_sources(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push_str(&fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("无法读取 S1 源文件 {}: {error}", path.display())
                }));
            }
        }
    }

    let mut source = String::new();
    append_sources(&workspace_root().join("apps/desktop/src"), &mut source);
    source
}
fn find_flow_row<'a>(baseline: &'a str, flow: &str) -> &'a str {
    let row_prefix = format!("| **{flow}** |");
    baseline
        .lines()
        .find(|line| line.starts_with(&row_prefix))
        .unwrap_or_else(|| panic!("行为基线缺少流程行：{flow}"))
}

#[test]
fn latest_s1_and_map_decisions_are_traceable_from_all_indexes() {
    let readme = read_workspace_file("README.md");
    let glossary = read_workspace_file("CONTEXT.md");
    let module_decisions = read_workspace_file("docs/module-decisions.md");
    let adr_0017 = read_workspace_file("docs/adr/0017-modular-architecture-and-crate-catalog.md");
    let adr_0025 = read_workspace_file("docs/adr/0025-shell-can-depend-on-domain-types.md");

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
    assert!(
        adr_0025.contains("依赖授权由 ADR-0037 收紧")
            && adr_0025.contains("S1 不得直接依赖 B2")
            && adr_0025.contains("B17/B18（生成与导出实现） | ❌")
            && !adr_0025.contains("S1 → B2 → B1 模式正常")
            && !adr_0025.contains("B17（Manifest 生成器） | ✅"),
        "旧 ADR-0025 必须保留 B1 只读类型例外，但不再授权 S1 直接依赖持久化、生成或导出实现"
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
        let cells: Vec<_> = find_flow_row(&baseline, flow)
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
        baseline.contains("不切换到内存数据或假首开页")
            && baseline.contains("生成、版本核对或落盘失败时显示失败状态"),
        "基线必须如实固定启动读取失败与 M1 导出失败的可观察行为"
    );
}

#[test]
fn public_ui_seam_matches_startup_settings_and_m1_export() {
    let baseline =
        read_workspace_file("docs/behavior-baselines/s1-current-user-observable-behavior.md");
    let review_row = find_flow_row(&baseline, "评审");
    assert!(
        review_row.contains("六类标签页")
            && review_row.contains("封账")
            && review_row.contains("空态")
            && review_row.contains("不阻塞导出"),
        "评审基线应记录 M3 的真实可观察行为（六类分组、封账摘要、空态、不阻塞导出）"
    );
    let export_row = find_flow_row(&baseline, "导出");
    for observable in [
        "边界确认后",
        "后台完成后",
        "立即显示处理中",
        "失败不显示成功产物",
    ] {
        assert!(
            export_row.contains(observable),
            "M1 导出基线缺少可观察事实：{observable}"
        );
    }

    let collection_row = find_flow_row(&baseline, "采集");
    for observable in [
        "初始“待定”状态",
        "正在从地图平台拉数据……",
        "无疑点时静默通过",
        "合并为一扇",
    ] {
        assert!(
            collection_row.contains(observable),
            "采集基线必须记录 s1-07 的可观察行为：{observable}"
        );
    }

    let settings_row = find_flow_row(&baseline, "设置");
    assert!(
        settings_row.contains("默认导出位置")
            && settings_row.contains("清除全部密钥")
            && settings_row.contains("没有可观察的处理中状态"),
        "设置基线必须记录常规设置读写与清除密钥确认，且不声明处理中状态"
    );
    let campus_row = find_flow_row(&baseline, "校区与方案");
    assert!(
        campus_row.contains("搜索只在点击“搜索”或按回车时开始")
            && campus_row.contains("最近使用的校区（名称+地址）")
            && campus_row.contains("恢复、永久删除和清空成功后在回收站停留并短暂提示")
            && campus_row.contains("移除最近记录不弹确认"),
        "校区与方案基线必须记录校区搜索、最近记录与回收站接线现状"
    );

    let window = AppWindow::new().expect("创建公开 AppWindow");
    assert_eq!(window.get_active_screen(), 1, "窗口默认显示校区选择页");
    assert_eq!(landing_decision(None), LandingDecision::FirstRunSetup);
    window.set_active_screen(3);
    assert!(window.get_gaode_status_message().is_empty());
    assert!(!window.get_confirm_dialog_visible());

    window.set_active_screen(4);
    window.set_workspace_completed_steps(4);
    window.set_workspace_step_pending_notice("步骤待实现".into());
    for step in 2..=4 {
        window.set_workspace_active_step(step);
        assert_eq!(window.get_workspace_active_step(), step);
        assert_eq!(
            window.get_workspace_step_pending_notice().as_str(),
            "步骤待实现"
        );
        assert!(!window.get_error_dialog_visible());
        assert!(!window.get_confirm_dialog_visible());
    }
}

#[test]
fn export_s1_seam_submits_one_complete_f9_intent() {
    let source = read_workspace_file("apps/desktop/src/production/mod.rs");
    let seam = source
        .split("impl ExportProductionAdapter")
        .nth(1)
        .and_then(|tail| tail.split("struct NotificationLabels").next())
        .expect("导出适配器必须存在");

    assert!(seam.contains("ExportPresentationRequest::Start"));
    assert_eq!(
        seam.matches("self.flow.start()").count(),
        1,
        "S1 导出接缝只能提交一次 F9 完整开始意图"
    );
    for forbidden in [
        "collect_and_audit(",
        "database_mut(",
        "BoundaryExportRequest",
        "default_export_location",
        "minecraft_version",
        "boundary_gcj02",
        "orientation_angle",
        "session.plans",
        "PathBuf",
        "generate_flat_ground(",
        "write_schematic(",
        "ManifestGenerator",
        "GenerationEngine",
    ] {
        assert!(
            !seam.contains(forbidden),
            "S1 导出适配器不得协调底层步骤：{forbidden}"
        );
    }
    assert!(
        seam.contains("Presentation::processing")
            || seam.contains("ExportPresentationRequest::Poll"),
        "S1 必须呈现 F9 的后台处理中状态，而不是同步等待结果"
    );
}

#[test]
fn export_input_assembly_is_not_kept_in_runtime_or_workspace_shell_files() {
    for path in [
        "apps/desktop/src/runtime.rs",
        "apps/desktop/src/production/workspace_boundary.rs",
    ] {
        let source = read_workspace_file(path);
        for forbidden in [
            "BoundaryExportRequest",
            "BoundaryExportInput",
            "ExportInputSnapshot",
            "ExportInputStore",
            "BoundaryExportPort",
            "load_request(",
            "default_export_location",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} ??????? F9 ???????{forbidden}"
            );
        }
    }
}

#[test]
fn the_entire_s1_source_tree_has_no_formal_f9_input_assembly() {
    let source = read_desktop_sources();
    for forbidden in [
        "BoundaryExportRequest",
        "BoundaryExportInput",
        "ExportInputSnapshot",
        "ExportInputStore",
        "BoundaryExportPort",
        "load_request(",
        "BoundaryExportFlow {",
        "MockSealGate",
    ] {
        assert!(
            !source.contains(forbidden),
            "S1 全部源文件不得持有或组装 F9 正式输入：{forbidden}"
        );
    }
}

#[test]
fn export_failure_facts_have_a_localized_background_branch_and_diagnostic_seam() {
    let resources = read_workspace_file("core/localization/resources/zh-CN.json");
    assert!(
        resources.contains("\"export_background_failed\"")
            && resources.contains("\"failure_user_message\""),
        "????????????????????"
    );

    let source = read_workspace_file("apps/desktop/src/production/mod.rs");
    let failure = source
        .split("fn failure_presentation")
        .nth(1)
        .and_then(|tail| tail.split("impl PresentationAdapter").next())
        .expect("????????????");
    assert!(
        failure.contains("with_diagnostic_action"),
        "????????? B7 ????????"
    );
    assert!(
        !failure.contains("export.failure_detail")
            && !failure.contains("let diagnostic = error.to_string()"),
        "error.to_string() ?????????????"
    );
}

#[test]
fn workspace_shell_does_not_hold_formal_boundary_or_synthesize_collection_fallback() {
    let workspace = read_workspace_file("apps/desktop/src/production/workspace_boundary.rs");
    for forbidden in [
        "use shared_domain_types::{Boundary",
        "state.boundary",
        "export_flow.set_boundary(",
    ] {
        assert!(
            !workspace.contains(forbidden),
            "S1 工作区不得持有或镜像正式边界：{forbidden}"
        );
    }

    let production = read_workspace_file("apps/desktop/src/production/mod.rs");
    assert!(
        !production.contains("let delta = 0.0001")
            && !production.contains("anchor_lng - delta")
            && !production.contains("unwrap_or((116.397, 39.916))"),
        "候选采集不得用校区锚点合成静默矩形后备"
    );
}
