//! S1 运行时：首开着陆判定 + Slint 主窗口装配。
//!
//! 着陆规则（ADR-0006）：首次运行（或数据库尚不可用）→ 设置向导；
//! 老用户且记有上次校区 → 直达该校区；否则 → 校区选择页。
//! 判定逻辑委托 F1 `SettingsManager`（校区被删返回 `None` 的兜底在 F1 内），
//! 壳只把结果翻译成 l10n 文案填进窗口——零业务逻辑。

use anyhow::Result;
use data_persistence::Database;
use global_settings::SettingsManager;
use localization::{Language, Localization};
use slint::ComponentHandle;

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
    let settings = SettingsManager::new(db);
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
fn status_text(l10n: &Localization, decision: &LandingDecision) -> String {
    match decision {
        LandingDecision::FirstRunSetup => l10n.t("app.shell_status_first_run"),
        LandingDecision::CampusSelect => l10n.t("app.shell_status_campus_select"),
        LandingDecision::LastUsedCampus { name } => {
            l10n.t_with_array("app.shell_status_last_campus", &[name])
        }
    }
}

/// 开发版桌面应用入口：装配主窗口并进入事件循环。
pub fn run_dev() -> Result<()> {
    let l10n = Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?;
    let decision = landing_decision(Database::open(DEV_DB_FILE).ok());

    let window = AppWindow::new()?;
    window.set_app_title(l10n.t("app.welcome_title").into());
    window.set_status_text(status_text(&l10n, &decision).into());
    window.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
            assert!(!text.starts_with("app."), "文本键未入库: {text}");
        }
    }
}
