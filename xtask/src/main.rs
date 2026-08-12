//! xtask —— S2 构建与自动化（ADR-0017），三层锁执法的 CI 层入口。
//!
//! 子命令：
//! - `tidy`：规模红线（1000 行）/ 模块文档 / 半成品禁令；
//! - `arch`：架构测试（依赖 DAG 断言 + public-api 快照存在性）；
//! - `ci`：tidy + arch 一次跑完（CI 的 xtask job 用）；
//! - `timings`：编译时间预算（单编译单元 >2 分钟告警）；
//! - `cache-report`：只读报告 Cargo 产物体积、类型与重复哈希代际；
//! - `dev-shortcut`：构建并更新桌面"校园复刻工具 - 开发版"快捷方式（ADR-0014）。
//!
//! 检查逻辑同时以 `#[test]` 形式存在（`cargo test -p xtask` 即执法）。

#![allow(
    clippy::print_stdout,
    reason = "xtask 是命令行工具，stdout 即其用户界面（业务模块的输出纪律不适用）"
)]

mod arch;
mod cache_report;
mod shortcut;
mod tidy;
mod timings;

use std::path::{Path, PathBuf};

/// workspace 根目录（xtask 自身位于 `<root>/xtask`）。
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask 必在 workspace 根之下")
        .to_path_buf()
}

/// 读取 workspace 元数据（成员清单 + 各成员的直接依赖声明）。
pub(crate) fn workspace_metadata(root: &Path) -> anyhow::Result<cargo_metadata::Metadata> {
    Ok(cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .no_deps()
        .exec()?)
}

const USAGE: &str = "\
用法: cargo xtask <子命令>

  tidy          规模红线 / 模块文档 / 半成品禁令
  arch          架构测试（ADR-0017 依赖 DAG + public-api 快照存在性）
  ci            tidy + arch（CI 聚合入口）
  timings       编译时间预算（单编译单元 >2 分钟发 CI 告警）
  cache-report  只读报告 Cargo target 体积、产物类型与重复哈希代际
  dev-shortcut  构建并更新桌面\"校园复刻工具 - 开发版\"快捷方式（ADR-0014）";

fn main() -> anyhow::Result<()> {
    let root = workspace_root();
    let command = std::env::args().nth(1).unwrap_or_default();
    match command.as_str() {
        "tidy" => tidy::run(&root),
        "arch" => arch::run(&root),
        "ci" => {
            tidy::run(&root)?;
            arch::run(&root)
        }
        "timings" => timings::run(&root),
        "cache-report" => cache_report::run(&root),
        "dev-shortcut" => shortcut::run(&root),
        _ => {
            println!("{USAGE}");
            anyhow::bail!("未知子命令: {command:?}");
        }
    }
}

#[cfg(test)]
mod enforcement_tests {
    //! 执法即测试：对真实 workspace 跑全量 tidy 与架构断言。

    use super::*;

    #[test]
    fn workspace_is_tidy() {
        let violations = tidy::workspace_violations(&workspace_root()).expect("tidy 扫描");
        assert!(
            violations.is_empty(),
            "tidy 违规:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn workspace_architecture_holds() {
        let violations = arch::workspace_violations(&workspace_root()).expect("架构扫描");
        assert!(
            violations.is_empty(),
            "架构违规:\n{}",
            violations.join("\n")
        );
    }
}
