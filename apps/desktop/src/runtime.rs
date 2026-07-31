//! S1 迁移期运行时：Slint 主窗口装配与正式入口组合根。
//!
//! 启动与设置流程已经由呈现入口一次取得完整结果（工单 03），S1 只呈现返回的
//! 页面、状态、导航与通知。landing_decision 等旧判定助手只保留给行为基线
//! 测试与迁移期占位路径，生产启动不再自行组合着陆条件。
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use data_persistence::Database;
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{Notification, NotificationCenter, PresenterRegistry};
use slint::ComponentHandle;

use crate::injector::{ShellDatabases, ViewModelInjector};
use crate::presenter::ShellPresenter;

use crate::production::ProductionEntries;
use crate::AppWindow;

/// 开发版数据库文件名（工作目录下，与 F1/F3 约定一致）。
const DEV_DB_FILE: &str = "campus-rebuild.db";

/// 首开着陆去向（壳据此决定第一屏）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandingDecision {
    /// 首次运行（或本地库尚不可用）→ F1 设置向导
    FirstRunSetup,
    /// 老用户但无可用的上次校区 → 校区选择页
    CampusSelect,
    /// 老用户直达上次使用的校区
    LastUsedCampus { name: String },
}

/// 由 F1 设置接口判定着陆去向；`db` 为 `None` 视同首次运行。
pub fn landing_decision(db: Option<Database>) -> LandingDecision {
    let Some(db) = db else {
        return LandingDecision::FirstRunSetup;
    };
    decide(&SettingsManager::new(db))
}

/// 着陆判定本体（独立入口与 VM 注入器共用，逻辑全部委托 F1）。
pub(crate) fn decide(settings: &SettingsManager) -> LandingDecision {
    match settings.is_first_run() {
        Ok(false) => match settings.landing_campus() {
            Ok(Some(campus)) => LandingDecision::LastUsedCampus { name: campus.name },
            // 未设置过或校区已被删（F1 兜底为 None）→ 回校区选择页
            Ok(None) | Err(_) => LandingDecision::CampusSelect,
        },
        // 首次运行；读取失败也按首次运行兜底，向导会重建设置
        Ok(true) | Err(_) => LandingDecision::FirstRunSetup,
    }
}

/// 着陆去向 → 状态栏文案（文本键见 zh-CN.json `app.*`）。
#[cfg(test)]
pub(crate) fn status_text(l10n: &Localization, decision: &LandingDecision) -> String {
    match decision {
        LandingDecision::FirstRunSetup => l10n.t("app.shell_status_first_run"),
        LandingDecision::CampusSelect => l10n.t("app.shell_status_campus_select"),
        LandingDecision::LastUsedCampus { name } => {
            l10n.t_with_array("app.shell_status_last_campus", &[name])
        }
    }
}

/// 正式窗口装配的生命周期句柄；持有旧接线与八类呈现入口直到窗口退出。
pub struct ApplicationRuntime {
    _injector: Rc<RefCell<ViewModelInjector>>,
    _presentation: Rc<RefCell<ProductionEntries>>,
}

/// 使用与开发版主程序相同的组合根装配窗口，供正式入口和集成测试共用。
pub fn assemble_application(
    window: &AppWindow,
    injector: ViewModelInjector,
    center: Arc<NotificationCenter>,
) -> ApplicationRuntime {
    let injector = Rc::new(RefCell::new(injector));
    injector.borrow().inject(window);

    let presentation = Rc::new(RefCell::new(ProductionEntries::new(
        Rc::clone(&injector),
        window,
        center,
    )));
    presentation.borrow_mut().show_startup(window);
    ViewModelInjector::bind(&injector, &presentation, window);
    ProductionEntries::bind_actions(&presentation, window);

    ApplicationRuntime {
        _injector: injector,
        _presentation: presentation,
    }
}

/// 开发版桌面应用入口：装配主窗口并进入事件循环。
pub fn run_dev() -> Result<()> {
    // B7 一本账先于任何回调可用（弹窗铁律 ADR-0021）。
    let center = NotificationCenter::init(PresenterRegistry::new());

    let window = AppWindow::new()?;
    // T19B-5A 色卡机制（ADR-0023）：启动时加载亮色卡设置 Theme global
    apply_theme(&window);
    // T19B-2 装喇叭：Slint 弹窗 Presenter 就位——无论数据库是否可用
    // 都注册（要紧错误从此真正可见，不再只留底）。
    center
        .registry()
        .set_presenter(ShellPresenter::install(&window));

    // 库不可用时不进入假首开页：由下方失败分支明确提示（ADR-0037）
    let injector = match ShellDatabases::open(DEV_DB_FILE) {
        Ok(databases) => Some(ViewModelInjector::new(databases)?),
        Err(_) => None,
    };
    match injector {
        Some(injector) => {
            let _runtime = assemble_application(&window, injector, Arc::clone(&center));
            window.run()?;
        }
        None => {
            // 正式数据不可用：明确失败并提示重试，不切换到内存数据或假首开页（ADR-0037）
            let l10n = Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?;
            window.set_app_title(l10n.t("app.welcome_title").into());
            window.set_status_text(l10n.t("app.shell_status_load_failed").into());
            window.set_operation_state(crate::OperationPresentationState::Failed);
            center.publish(Notification::error(
                l10n.t("app.source_tag"),
                l10n.t("dialog.error_title"),
                l10n.t("app.startup_failure_body"),
            ));
            window.run()?;
        }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// 色卡加载器与相对时间（原 theme.rs；S1-04 为生产适配器腾出模块文件配额并入）
// ADR-0023 §一 硬性架构约束：色卡 = JSON 文件，键为颜色角色名，值为 hex。
// ────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;

use slint::Color;

use crate::generated::Theme;

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
mod theme_tests {
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
        assert!(result.contains('-'), "超 7 天应显示日期: {result}");
        assert!(!result.contains("天前"), "超 7 天不应显示'天前': {result}");
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn assembly_calls_only_startup_and_retains_the_coverage_port() {
        crate::production::reset_entry_calls();
        let directory = tempfile::tempdir().expect("建立临时目录");
        let databases = ShellDatabases::open(directory.path().join("assembly.db"))
            .expect("建立正式数据库连接组");
        let injector = ViewModelInjector::new(databases).expect("建立正式注入器");
        let window = AppWindow::new().expect("建立正式窗口");
        let center = Arc::new(NotificationCenter::new(PresenterRegistry::new()));

        let runtime = assemble_application(&window, injector, center);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 0, 0, 0, 0, 0, 0]
        );

        // 真实 AppWindow 步骤回调：采集/评审/导出分别只经过各自入口
        window.set_workspace_completed_steps(4);
        window.invoke_workspace_step_clicked(2);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 0, 0, 0, 0, 0],
            "点击采集步骤只能经过采集入口"
        );
        window.invoke_workspace_step_clicked(3);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 0, 0, 0],
            "点击评审步骤只能经过评审入口"
        );
        window.invoke_workspace_step_clicked(4);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 1, 0, 0],
            "点击导出步骤只能经过导出入口"
        );
        window.invoke_workspace_step_clicked(1);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 1, 0, 0],
            "边界与朝向旧路径不得触发新入口"
        );

        runtime
            ._presentation
            .borrow_mut()
            .show_coverage_for_test(&window);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 1, 1, 0, 0]
        );
    }
    use super::*;

    #[test]
    fn missing_database_means_first_run() {
        assert_eq!(landing_decision(None), LandingDecision::FirstRunSetup);
    }

    #[test]
    fn fresh_database_means_first_run() {
        let db = Database::open_in_memory().expect("内存库");
        assert_eq!(landing_decision(Some(db)), LandingDecision::FirstRunSetup);
    }

    #[test]
    fn status_text_is_localized_not_hardcoded() {
        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
        for decision in [
            LandingDecision::FirstRunSetup,
            LandingDecision::CampusSelect,
            LandingDecision::LastUsedCampus {
                name: "示例大学".to_owned(),
            },
        ] {
            let text = status_text(&l10n, &decision);
            // 文本键缺失时 l10n 会原样返回键名——断言键都已入 zh-CN.json
            assert!(!text.starts_with("app."), "文本键未入库：{text}");
        }
    }
}
