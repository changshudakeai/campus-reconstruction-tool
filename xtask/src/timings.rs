//! xtask timings —— 编译时间预算（执法清单 2.7，ADR-0017 红线表）。
//!
//! 运行 `cargo build --workspace --all-targets --timings` 后解析
//! `target/cargo-timings/cargo-timing.html` 内嵌的 `UNIT_DATA` JSON，
//! 任何单个编译单元耗时超过 2 分钟即发出 CI 告警（`::warning::` 注解，
//! 不阻断——ADR-0017 规定该项为"告警"级）。
//!
//! v1.x 的 20-30 分钟全量编译是重写的首要动因，此预算是防复发哨兵。

use std::path::Path;

/// 单编译单元预算：2 分钟（ADR-0017 第四节）。
pub(crate) const BUDGET_SECONDS: f64 = 120.0;

/// 从 cargo-timing.html 中提取各编译单元的 `(名称, 耗时秒)`。
pub(crate) fn parse_unit_data(html: &str) -> anyhow::Result<Vec<(String, f64)>> {
    let marker = "const UNIT_DATA = ";
    let start = html.find(marker).ok_or_else(|| {
        anyhow::anyhow!("cargo-timing.html 中未找到 UNIT_DATA（cargo 版本变更？）")
    })? + marker.len();
    // serde_json 流式解析：读取第一个完整 JSON 值即停，无须自己找结束分号。
    let mut stream =
        serde_json::Deserializer::from_str(&html[start..]).into_iter::<serde_json::Value>();
    let units = stream
        .next()
        .ok_or_else(|| anyhow::anyhow!("UNIT_DATA 后未跟随 JSON 数组"))??;
    let units = units
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("UNIT_DATA 不是数组"))?;

    let mut timings = Vec::new();
    for unit in units {
        let name = unit
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("<unknown>");
        let mode = unit
            .get("mode")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let duration = unit
            .get("duration")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        timings.push((format!("{name}({mode})"), duration));
    }
    Ok(timings)
}

/// 返回超预算单元的告警文案（空 = 全部在预算内）。
pub(crate) fn over_budget(timings: &[(String, f64)], budget_seconds: f64) -> Vec<String> {
    timings
        .iter()
        .filter(|(_, duration)| *duration > budget_seconds)
        .map(|(name, duration)| {
            format!(
                "编译单元 {name} 耗时 {duration:.1}s，超过 {budget_seconds:.0}s 预算——\
                 检查是否该拆分 crate 或裁剪依赖（ADR-0017）"
            )
        })
        .collect()
}

/// `cargo xtask timings` 入口：全量构建、解析报告、发 CI 告警（不阻断）。
pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    #[allow(
        clippy::disallowed_methods,
        reason = "xtask 是构建自动化工具本身，调用 cargo 属其本职（clippy.toml 禁令针对业务模块）"
    )]
    let status = std::process::Command::new("cargo")
        // 排除 xtask 自身：Windows 下无法重建正在运行的 xtask.exe（文件锁，
        // os error 5）；预算哨兵针对业务 crate，xtask 本体不在预算范围内。
        .args([
            "build",
            "--workspace",
            "--exclude",
            "xtask",
            "--all-targets",
            "--timings",
        ])
        .current_dir(root)
        .status()?;
    anyhow::ensure!(status.success(), "cargo build --timings 失败");

    // cargo 把 --timings 报告写到 CARGO_TARGET_DIR/cargo-timings；xtask 不能
    // 假设目标目录固定在 root/target（多个工作树共享 target 时 os error 3）。
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let report = target_dir.join("cargo-timings/cargo-timing.html");
    let html = std::fs::read_to_string(&report)?;
    let warnings = over_budget(&parse_unit_data(&html)?, BUDGET_SECONDS);
    if warnings.is_empty() {
        println!("timings: 全部编译单元在 {BUDGET_SECONDS:.0}s 预算内");
    } else {
        for warning in &warnings {
            // GitHub Actions 注解格式；本地运行时同样可读。
            println!("::warning::{warning}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_HTML: &str = r#"<html><script>
const UNIT_DATA = [
  {"i":0,"name":"anyhow","version":"1.0.0","mode":"build","target":"","duration":1.5},
  {"i":1,"name":"review-workbench","version":"2.0.0-dev","mode":"build","target":"","duration":150.2}
];
const CONCURRENCY_DATA = [];
</script></html>"#;

    #[test]
    fn parses_unit_names_and_durations() {
        let timings = parse_unit_data(FAKE_HTML).expect("解析假报告");
        assert_eq!(timings.len(), 2);
        assert_eq!(timings[0].0, "anyhow(build)");
        assert!((timings[1].1 - 150.2).abs() < f64::EPSILON);
    }

    #[test]
    fn units_over_two_minutes_are_flagged() {
        let timings = parse_unit_data(FAKE_HTML).expect("解析假报告");
        let warnings = over_budget(&timings, BUDGET_SECONDS);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("review-workbench"));
        assert!(warnings[0].contains("120s 预算"));
    }

    #[test]
    fn missing_marker_is_a_clear_error() {
        let error = parse_unit_data("<html></html>").unwrap_err();
        assert!(error.to_string().contains("UNIT_DATA"));
    }
}
