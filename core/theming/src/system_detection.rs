//! MCRebuild V2 —— 系统主题检测模块
//!
//! 检测当前系统的亮暗色偏好 (Windows/MacOS/Linux),返回 SystemColorScheme.

/// 系统色温方案枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemColorScheme {
    /// 系统使用亮色模式
    Light,
    /// 系统使用暗色模式
    Dark,
}

/// 检测当前系统的亮暗色偏好
///
/// - Windows:从注册表HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize
///   读取 AppsUseLightTheme(1=亮色，0=暗色)
/// - Non-Windows:默认返回 Light
#[cfg(target_os = "windows")]
pub fn detect_system_color_scheme() -> SystemColorScheme {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_CURRENT_USER);
    match hklm.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize") {
        Ok(key) => {
            match key.get_value::<u32, _>("AppsUseLightTheme") {
                Ok(value) => {
                    if value == 1 { SystemColorScheme::Light } else { SystemColorScheme::Dark }
                },
                Err(_) => {
                    log::warn!("无法读取 AppsUseLightTheme 注册表项，默认使用亮色");
                    SystemColorScheme::Light
                }
            }
        },
        Err(_) => {
            log::warn!("无法打开系统主题注册表路径，默认使用亮色");
            SystemColorScheme::Light
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn detect_system_color_scheme() -> SystemColorScheme {
    // MacOS 和 Linux 未实现，暂时返回 Light
    log::info!("非 Windows 系统，系统主题检测暂不支持，默认使用亮色");
    SystemColorScheme::Light
}
