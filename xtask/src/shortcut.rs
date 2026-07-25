//! xtask dev-shortcut —— 开发版快捷方式自动化（ADR-0014）。
//!
//! 剧本：构建壳 crate（release）→ 把上一份可用版本挪到 `previous\` 兜底 →
//! 复制新 exe 到固定安装位 → 在桌面创建/更新快捷方式
//! "校园复刻工具 - 开发版"。快捷方式仅本地预览用，不做系统级注册。
//!
//! 壳 crate（desktop-shell，T19）尚未立户时给出明确提示而非静默失败。

use std::path::{Path, PathBuf};

/// 桌面快捷方式名（ADR-0014 命名规范；正式版另名并存，互不覆盖）。
pub(crate) const SHORTCUT_NAME: &str = "校园复刻工具 - 开发版.lnk";

/// 壳 crate 名（ADR-0017 S1）。
const SHELL_CRATE: &str = "desktop-shell";

/// 计算安装位：`<local_app_data>\MCRebuildV2\dev\` 下的当前版与兜底版路径。
pub(crate) fn install_paths(local_app_data: &Path) -> (PathBuf, PathBuf) {
    let dev_dir = local_app_data.join("MCRebuildV2").join("dev");
    let current = dev_dir.join("campus-rebuild-dev.exe");
    let previous = dev_dir.join("previous").join("campus-rebuild-dev.exe");
    (current, previous)
}

/// 生成创建快捷方式的 PowerShell 脚本（纯函数便于测试）。
pub(crate) fn shortcut_script(target_exe: &Path) -> String {
    format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $lnk = $ws.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Desktop')) '{SHORTCUT_NAME}')); \
         $lnk.TargetPath = '{}'; \
         $lnk.WorkingDirectory = '{}'; \
         $lnk.Save()",
        target_exe.display(),
        target_exe.parent().unwrap_or(Path::new(".")).display()
    )
}

/// `cargo xtask dev-shortcut` 入口。
pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    let metadata = crate::workspace_metadata(root)?;
    let shell_exists = metadata
        .workspace_packages()
        .iter()
        .any(|package| package.name == SHELL_CRATE);
    anyhow::ensure!(
        shell_exists,
        "壳 crate `{SHELL_CRATE}` 尚未立户（T19）——开发版快捷方式在壳可构建后自动可用，\
         本子命令与流程已就绪，无需届时改动"
    );

    #[allow(
        clippy::disallowed_methods,
        reason = "xtask 是构建自动化工具本身，调用 cargo/powershell 属其本职（clippy.toml 禁令针对业务模块）"
    )]
    let build = std::process::Command::new("cargo")
        .args(["build", "--release", "-p", SHELL_CRATE])
        .current_dir(root)
        .status()?;
    anyhow::ensure!(
        build.success(),
        "壳 crate 构建失败，未更新开发版（保留上一可用版本）"
    );

    let built_exe = metadata
        .target_directory
        .join("release")
        .join(format!("{SHELL_CRATE}.exe"));
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("未找到 %LOCALAPPDATA%"))?;
    let (current, previous) = install_paths(&local_app_data);

    // 版本更替策略（ADR-0014 第 3 条）：先把当前版挪去兜底，再落新版。
    std::fs::create_dir_all(previous.parent().expect("previous 必有父目录"))?;
    if current.exists() {
        std::fs::copy(&current, &previous)?;
    }
    std::fs::copy(built_exe.as_std_path(), &current)?;

    #[allow(
        clippy::disallowed_methods,
        reason = "xtask 是构建自动化工具本身，调用 cargo/powershell 属其本职（clippy.toml 禁令针对业务模块）"
    )]
    let link = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &shortcut_script(&current)])
        .status()?;
    anyhow::ensure!(link.success(), "快捷方式创建失败");

    println!(
        "dev-shortcut: 桌面快捷方式\"校园复刻工具 - 开发版\"已更新，上一版本保留在 previous\\"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_keep_previous_version_for_fallback() {
        let (current, previous) = install_paths(Path::new(r"C:\Users\u\AppData\Local"));
        assert!(current.ends_with(r"MCRebuildV2\dev\campus-rebuild-dev.exe"));
        assert!(previous.ends_with(r"MCRebuildV2\dev\previous\campus-rebuild-dev.exe"));
    }

    #[test]
    fn shortcut_script_points_at_fixed_desktop_name() {
        let script = shortcut_script(Path::new(r"C:\x\app.exe"));
        assert!(script.contains(SHORTCUT_NAME));
        assert!(script.contains(r"C:\x\app.exe"));
        assert!(script.contains("GetFolderPath('Desktop')"));
    }
}
