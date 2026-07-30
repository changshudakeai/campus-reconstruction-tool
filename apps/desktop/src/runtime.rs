//! S1 迁移期运行时：现有首开着陆组合 + Slint 主窗口装配。
//!
//! 当前行为仍按 ADR-0006 将首次运行、上次校区与校区选择组合成着陆去向，并由
//! 工单 01 的用户可观察行为基线固定。ADR-0037 的目标是由 F1 返回单一着陆结果，
//! S1 只负责呈现；本文件中的数据库打开、判定与模块装配是待迁出的现状，不构成
//! 新增业务协调的授权。
use std::cell::RefCell;
use std::rc::Rc;

use anyhow::Result;
use data_persistence::Database;
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{NotificationCenter, PresenterRegistry};
use slint::ComponentHandle;

use crate::injector::{ShellDatabases, ViewModelInjector};
use crate::presenter::ShellPresenter;
use crate::theme::apply_theme;
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
pub(crate) fn status_text(l10n: &Localization, decision: &LandingDecision) -> String {
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

    // 库不可用视同首次运行（原兜底语义）：无注入器，直接填首开文案
    let injector = match ShellDatabases::open(DEV_DB_FILE) {
        Ok(databases) => Some(ViewModelInjector::new(databases)?),
        Err(_) => None,
    };
    match injector {
        Some(injector) => {
            // 回调闭包共享注入器（Slint 单线程 UI），事件循环全程存活
            let injector = Rc::new(RefCell::new(injector));
            injector.borrow().inject(&window);
            ViewModelInjector::bind(&injector, &window);
            window.run()?;
        }
        None => {
            let l10n = Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?;
            window.set_app_title(l10n.t("app.welcome_title").into());
            window.set_status_text(status_text(&l10n, &landing_decision(None)).into());
            window.run()?;
        }
    }
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
            assert!(!text.starts_with("app."), "文本键未入库：{text}");
        }
    }
}
