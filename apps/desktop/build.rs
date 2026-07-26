//! T19 构建脚本 - Slint UI 代码生成 + 快捷方式自动化
//!
//! # 功能
//!
//! ## Slint UI 代码生成
//! `build.rs` 调用 `slint-build` 编译 `ui/main.slint` 文件
//! 生成的 Rust 代码被写入 `$OUT_DIR/main_slint.rs` 并由 `lib.rs` include

#![allow(clippy::disallowed_methods, reason = "xtask 构建工具本身")]

use std::path::Path;

fn main() {
    // Slint UI 文件路径
    let ui_file = Path::new("ui/main.slint");

    if !ui_file.exists() {
        println!("cargo:warning=Slint UI file not found at {:?}, creating minimal template...", ui_file);
        create_minimal_slint_template();
    }

    // Slint 代码生成
    slint_build::compile(&ui_file).expect("Failed to compile Slint UI file");

    // 触发重新编译当 UI 文件变化
    println!("cargo:rerun-if-changed=ui/main.slint");

    // 自动更新开发版快捷方式（仅在 release 模式）
    let is_release = std::env::var("PROFILE").unwrap_or_default() == "release";
    if is_release {
        generate_shortcut();
    }
}

/// 创建最小化 Slint 模板文件（首次运行时）
fn create_minimal_slint_template() {
    let ui_dir = Path::new("ui");
    if !ui_dir.exists() {
        std::fs::create_dir_all(ui_dir).expect("Failed to create ui directory");
    }

    let template = r#"// Minimal Slint template for desktop shell
// TODO: Implement full UI design per PRD.md window contracts

import { MainWindow } from "components/mainwindow.slint";

export component App inherits MainWindow {
    width: 800px;
    height: 600px;
    
    title: "校园复刻工具 - 开发版";
    
    in-out property<string> app_title: "MCRebuild V2";
    
    callback navigate_to_plan_list();
    callback start_collection();
    callback start_review();
    callback export_diy();
    
    // TODO: 接入所有 ViewModel
    // F1: settings_view
    // F2: tutorial_bubble
    // F3: campus_list / plan_cards
    // F4: collection_progress
    // F5: workbench (三栏布局)
    // F7: audit_report_entry
    // F9: export_console
    
    Component root := MainWindow {
        title: root.app_title;
        
        // Top menu bar
        MenuBar {
            Menu { text: l10n.t("app.menu_file"); }
            Menu { text: l10n.t("app.menu_campus"); }
            Menu { text: l10n.t("app.menu_help"); }
        }
        
        // Main content area - to be populated by VMs
        ContentArea {
            // First-run setup → Campus search → Plan list → ...
        }
    }
}

component MainWindow {
    width: 800px;
    height: 600px;
    
    in-out property<string> title;
    
    Template {
        Rectangle {
            color: lightgray;
            width: parent.width;
            height: parent.height;
            
            // TODO: Add status bar, navigation panel, etc.
        }
    }
}

// Placeholder components - implement per module VMs
component ContentArea {
    width: parent.width;
    height: parent.height;
}"#;

    std::fs::write(ui_file, template).expect("Failed to write Slint template");
    println!("cargo:warning=Created minimal Slint template at {:?}", ui_file);
}

/// 生成/更新桌面快捷方式
fn generate_shortcut() {
    #[allow(
        clippy::disallowed_methods,
        reason = "xtask/build.rs 是构建工具本身，调用 powershell 属其本职"
    )]
    use std::process::Command;

    let target_exe = Path::new(&std::env::var("TARGET").unwrap_or_else(|_| "x86_64-pc-windows-msvc".to_string()));
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = Path::new(&manifest_dir).join("target").join("release");
    let exe_path = out_dir.join("campus-tool-dev.exe");

    // 检查可执行文件是否存在
    if !exe_path.exists() {
        println!("cargo:warning={} does not exist, skipping shortcut generation", exe_path.display());
        return;
    }

    // 获取 LOCALAPPDATA
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .expect("LOCALAPPDATA not set");

    let dev_dir = local_app_data.join("MCRebuildV2").join("dev");
    let current_exe = dev_dir.join("campus-tool-dev.exe");
    let backup_dir = dev_dir.join("previous");

    // 备份旧版本
    std::fs::create_dir_all(&backup_dir).ok();
    if current_exe.exists() {
        let backup_path = backup_dir.join("campus-tool-dev-old.exe");
        let _ = std::fs::copy(&current_exe, &backup_path);
        println!("cargo:warning=Backed up old version to {:?}", backup_path);
    }

    // 复制新版本到安装位
    std::fs::copy(&exe_path, &current_exe).expect("Failed to copy executable");

    // 生成 PowerShell 脚本创建快捷方式
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) '校园复刻工具 - 开发版.lnk')); \
         $lnk.TargetPath = '{}'; \
         $lnk.WorkingDirectory = '{}'; \
         $lnk.Save()",
        current_exe.display(),
        dev_dir.display()
    );

    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=✓ Desktop shortcut \"校园复刻工具 - 开发版.lnk\" updated");
        }
        Ok(_) => {
            println!("cargo:warning=⚠ Failed to create desktop shortcut (PowerShell error)");
        }
        Err(e) => {
            println!("cargo:warning=⚠ Cannot run PowerShell: {}", e);
        }
    }
}
