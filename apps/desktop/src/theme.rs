//! T19B-5A — 色卡加载器（ADR-0023 §一 硬性架构约束）。
//!
//! 色卡 = 一张 JSON 文件（`resources/themes/<name>.json`），键为颜色角色名，
//! 值为十六进制颜色号。加载后设置 Slint `Theme` global 的属性，代码零改动即换肤。
//!
//! 查找顺序（参考 B6 localization 模式）：
//! 1. 可执行文件旁 `resources/themes/`
//! 2. 当前工作目录 `resources/themes/`
//! 3. 编译期内嵌副本兜底（保证从任意目录启动都不失败）
//!
//! 本单只落亮色卡（`light.json`）；暗色卡与设置页切换开关为后续债务。

use std::collections::HashMap;

use slint::{Color, ComponentHandle};

use crate::generated::Theme;
use crate::AppWindow;

/// 编译期内嵌的亮色卡副本（磁盘文件缺失时的兜底）
const EMBEDDED_LIGHT: &str = include_str!("../resources/themes/light.json");

/// 从色卡 JSON 文件加载颜色映射并设置到 Slint Theme global。
///
/// 磁盘文件优先于内嵌：改磁盘上的 JSON 后重启即可看到新配色。
pub(crate) fn apply_theme(window: &AppWindow) {
    let content = read_theme_file("light.json").unwrap_or_else(|| EMBEDDED_LIGHT.to_string());
    let colors: HashMap<String, String> = match serde_json::from_str(&content) {
        Ok(map) => map,
        Err(e) => {
            log::warn!("色卡解析失败，使用内嵌兜底: {e}");
            serde_json::from_str(EMBEDDED_LIGHT).expect("内嵌色卡必须合法")
        }
    };

    let theme = window.global::<Theme>();
    for (role, hex) in &colors {
        if let Some(color) = parse_hex_color(hex) {
            set_theme_color(&theme, role, color);
        }
    }
}

/// 按角色名设置 Theme global 属性（新增角色时在此补一行）
fn set_theme_color(theme: &Theme, role: &str, color: Color) {
    match role {
        "surface" => theme.set_surface(color),
        "overlay" => theme.set_overlay(color),
        "text-primary" => theme.set_text_primary(color),
        "text-secondary" => theme.set_text_secondary(color),
        "text-tertiary" => theme.set_text_tertiary(color),
        "text-quaternary" => theme.set_text_quaternary(color),
        "text-faint" => theme.set_text_faint(color),
        "separator" => theme.set_separator(color),
        "bubble-background" => theme.set_bubble_background(color),
        "bubble-border" => theme.set_bubble_border(color),
        "error" => theme.set_error(color),
        _ => log::warn!("色卡中出现未知角色名: {role}"),
    }
}

/// 依次尝试可执行文件旁与当前目录的 resources/themes/
fn read_theme_file(file_name: &str) -> Option<String> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("resources").join("themes").join(file_name));
        }
    }
    candidates.push(
        std::path::Path::new("resources")
            .join("themes")
            .join(file_name),
    );

    for path in candidates {
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => return Some(content),
                Err(e) => log::warn!("色卡读取失败 {:?}: {}", path, e),
            }
        }
    }
    None
}

/// 解析十六进制颜色：支持 #RGB / #RRGGBB / #RRGGBBAA
fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            (r, g, b, 255)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color::from_argb_u8(a, r, g, b))
}

/// 相对时间格式化（ADR-0018 §一第 3 条："相对表述（如'3 天前'）"）。
///
/// 规则：刚刚 / X 分钟前 / X 小时前 / X 天前；超 7 天显示日期。
/// 格式化逻辑放 Rust 侧（壳的展示层允许），文案键走 zh-CN.json。
pub(crate) fn format_relative_time(l10n: &localization::Localization, rfc3339: &str) -> String {
    use chrono::{DateTime, Utc};

    let Ok(dt) = DateTime::parse_from_rfc3339(rfc3339) else {
        // 解析失败原样返回（兜底）
        return rfc3339.to_string();
    };
    let now = Utc::now();
    let duration = now.signed_duration_since(dt.with_timezone(&Utc));

    let minutes = duration.num_minutes();
    if minutes < 1 {
        return l10n.t("time.just_now");
    }
    if minutes < 60 {
        return l10n.t_with_array("time.minutes_ago", &[&minutes.to_string()]);
    }
    let hours = duration.num_hours();
    if hours < 24 {
        return l10n.t_with_array("time.hours_ago", &[&hours.to_string()]);
    }
    let days = duration.num_days();
    if days <= 7 {
        return l10n.t_with_array("time.days_ago", &[&days.to_string()]);
    }
    // 超 7 天显示日期（YYYY-MM-DD）
    l10n.t_with_array("time.date_display", &[&dt.format("%Y-%m-%d").to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_color_variants() {
        let c = parse_hex_color("#ffffff").expect("6 位");
        assert_eq!((c.red(), c.green(), c.blue()), (255, 255, 255));

        let c = parse_hex_color("#000").expect("3 位");
        assert_eq!((c.red(), c.green(), c.blue()), (0, 0, 0));

        let c = parse_hex_color("#00000073").expect("8 位");
        assert!(c.alpha() < 128);

        assert!(parse_hex_color("xyz").is_none());
        assert!(parse_hex_color("#12345").is_none());
    }

    #[test]
    fn relative_time_just_now() {
        let l10n =
            localization::Localization::new(localization::Language::ZhCn).expect("加载 zh-CN");
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let result = format_relative_time(&l10n, &now);
        assert_eq!(result, "刚刚");
    }

    #[test]
    fn relative_time_minutes_hours_days() {
        let l10n =
            localization::Localization::new(localization::Language::ZhCn).expect("加载 zh-CN");

        let five_min_ago = (chrono::Utc::now() - chrono::Duration::minutes(5))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert_eq!(format_relative_time(&l10n, &five_min_ago), "5 分钟前");

        let three_hours_ago = (chrono::Utc::now() - chrono::Duration::hours(3))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert_eq!(format_relative_time(&l10n, &three_hours_ago), "3 小时前");

        let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert_eq!(format_relative_time(&l10n, &two_days_ago), "2 天前");
    }

    #[test]
    fn relative_time_over_seven_days_shows_date() {
        let l10n =
            localization::Localization::new(localization::Language::ZhCn).expect("加载 zh-CN");
        let ten_days_ago = (chrono::Utc::now() - chrono::Duration::days(10))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let result = format_relative_time(&l10n, &ten_days_ago);
        // 应为 YYYY-MM-DD 格式
        assert!(result.contains('-'), "超 7 天应显示日期: {result}");
        assert!(!result.contains("天前"), "超 7 天不应显示'天前': {result}");
    }
}
