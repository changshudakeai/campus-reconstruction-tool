// 主进程入口 - 开发版桌面应用
//!
//! 功能：
//! - 显示主界面框架（Slint 渲染）
//! - 集成所有 ViewModel
//! - 开发版快捷方式自动更新

use anyhow::Result;
use desktop_shell::{run_dev, run_service};

fn main() -> Result<()> {
    // 默认启动开发版模式
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "--service" {
        // 服务进程模式（后台运行）
        println!("Starting service mode...");
        run_service()
    } else {
        // 开发版桌面应用
        println!("Starting campus-rebuild tool (development)...");
        run_dev()
    }
}
