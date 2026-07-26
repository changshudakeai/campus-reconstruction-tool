//! campus-tool-dev —— 开发版桌面应用入口（ADR-0014 开发版形态）。
//!
//! 薄壳原则：入口只调 `run_dev`，全部装配逻辑在 `desktop_shell::runtime`。

use anyhow::Result;

fn main() -> Result<()> {
    desktop_shell::run_dev()
}
