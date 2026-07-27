//! T19B-1 —— VM 注入器：壳与 F 模块之间唯一的连接桥（缝 1 基础设施）。
//!
//! 薄壳原则（ADR-0017）：壳零业务逻辑，本模块只做三件事——
//! 1. [`ViewModelInjector::new`] 构造并持有全部 F 模块实例；
//! 2. [`ViewModelInjector::inject`] 把各 VM 的视图状态绑定到 Slint
//!    窗口的 in property（只设属性，不改生成代码的行为逻辑）；
//! 3. 为后续 T19B 工单预留接线钥匙（各模块访问器 + 评审进台会话）。
//!
//! 主窗口导航骨架由 ADR-0027 决策，页面级绑定归 T19B-2..8；
//! 本模块不发明任何页面或导航 UI。回调错误的统一出口见
//! [`crate::report_callback_error`]（弹窗铁律 ADR-0021，经 B7 分派）。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use coverage_audit::QuietSentinel;
use data_acquisition::AcquisitionPipeline;
use data_persistence::Database;
use export_console::{ExportConsole, MockSealGate};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use onboarding_tutorial::{OnboardingTutorial, TutorialStep};
use project_management::ProjectManager;
use review_workbench::ReviewWorkbench;
use shared_domain_types::{CampusId, PlanId};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::dispatch::report_callback_error;
use crate::runtime::{decide, status_text, LandingDecision};
use crate::theme::format_relative_time;
use crate::{generated::CampusData, AppWindow};

/// 壳持有的 B2 连接组。
///
/// B2 `Database` 有意不可 `Clone`（一个持有者一条连接）；F1 与 F3 都
/// 按值持有句柄，因此壳对同一数据库文件开两条连接。F2/F5/F7 的落库
/// 操作统一借道 F3 [`ProjectManager::database_mut`]，不再额外开连接。
pub struct ShellDatabases {
    /// F1 全局设置的专属连接
    settings: Database,
    /// F3 方案管理的专属连接（`database_mut` 供 F2/F5/F7 借用）
    projects: Database,
}

impl ShellDatabases {
    /// 对同一数据库文件打开两条连接（开发版入口用）
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            settings: Database::open(path.as_ref())?,
            projects: Database::open(path.as_ref())?,
        })
    }

    /// 内存库连接组（测试用；注意两条连接是相互独立的内存库）
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            settings: Database::open_in_memory()?,
            projects: Database::open_in_memory()?,
        })
    }
}

/// VM 注入器：构造并持有全部 F 模块实例，把视图状态注入 Slint 窗口。
///
/// 借鉴依赖注入思想，但由 Rust 运行时直接构造而非 IoC 容器；
/// 壳内其余代码一律经本类型访问 F 模块，不各自另起炉灶。
pub struct ViewModelInjector {
    /// B6 文案解析（文本外置铁律 ADR-0005）
    l10n: Localization,
    /// F1 全局设置（自持一条 B2 连接）
    settings: SettingsManager,
    /// F2 新手教程（引导进度经 B2 app_settings 持久化）
    tutorial: OnboardingTutorial,
    /// F3 方案管理（自持一条 B2 连接）
    projects: ProjectManager,
    /// F4 数据采集流水线（内含 B13 归类引擎）
    acquisition: AcquisitionPipeline,
    /// F5 评审工作台：按方案进台的会话（[`Self::enter_review`] 装载）
    review: Option<ReviewWorkbench>,
    /// F7 覆盖率审计安静哨兵
    sentinel: QuietSentinel,
    /// F9 导出控制台。门控暂用 `MockSealGate` 占位——真门控（壳实现
    /// `SealGate`、内部调 F5 `seal`）随 T19B-8 导出接线落地。
    export: ExportConsole<MockSealGate>,
}

impl ViewModelInjector {
    /// 构造并持有全部 F 模块实例（F1-F9，B8/F6/F8 未立户不在册）。
    pub fn new(db: ShellDatabases) -> Result<Self> {
        let l10n = Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?;
        // F2 引导进度与 F3 同库装载（生产环境两连接指向同一文件）
        let tutorial = OnboardingTutorial::load(&db.projects)?;
        Ok(Self {
            l10n,
            settings: SettingsManager::new(db.settings),
            tutorial,
            projects: ProjectManager::new(db.projects),
            acquisition: AcquisitionPipeline::new()?,
            review: None,
            sentinel: QuietSentinel::new(),
            export: ExportConsole::new(MockSealGate::new()),
        })
    }

    /// 把 VM 视图状态注入 Slint 窗口（只设 in property）。
    ///
    /// T19B-2 起覆盖：首开文案 + 首跑向导（屏 0）+ 设置页文案（屏 3）+
    /// B7 弹窗静态文案；T19B-3 校区选择页文案；T19B-4 方案列表页文案。
    pub fn inject(&self, window: &AppWindow) {
        let l10n = &self.l10n;
        window.set_app_title(l10n.t("app.welcome_title").into());
        window.set_status_text(status_text(l10n, &self.landing()).into());

        // 首跑向导（ADR-0004：选项与默认值全部来自 F1，壳零业务逻辑）
        window.set_active_screen(match self.landing() {
            LandingDecision::FirstRunSetup => 0,
            _ => 1,
        });
        window.set_wizard_title(l10n.t("settings.wizard_title").into());
        window.set_wizard_language_label(l10n.t("settings.language_label").into());
        window.set_wizard_version_label(l10n.t("settings.minecraft_version_label").into());
        window.set_wizard_notice_text(l10n.t("settings.notice_checkbox").into());
        window.set_wizard_continue_label(l10n.t("settings.continue_button").into());
        window.set_wizard_language_options(string_model(global_settings::SUPPORTED_LANGUAGES));
        window.set_wizard_version_options(string_model(
            global_settings::SUPPORTED_MINECRAFT_VERSIONS,
        ));
        let settings = self.settings.settings().unwrap_or_default();
        window.set_wizard_language(settings.language.into());
        window.set_wizard_version(settings.minecraft_version.into());
        window.set_wizard_acknowledged(false);

        // 设置页（债务②：按钮文案取自 F2 settings_entry，规矩④）
        window.set_settings_title(l10n.t("app.settings_title").into());
        window.set_settings_back_label(l10n.t("app.back_button").into());
        window.set_tutorial_replay_label(self.tutorial.settings_entry(l10n).replay_label.into());

        // 屏 4：方案工作区占位文案（T19B-5B）
        window.set_workspace_placeholder_title(l10n.t("workspace.placeholder_title").into());
        window.set_workspace_placeholder_subtitle(l10n.t("workspace.placeholder_subtitle").into());

        // 步骤条文案
        window.set_workspace_stepper_title_label(l10n.t("collection.title").into());
        window.set_workspace_stepper_boundary_label(l10n.t("collection.boundary_step").into());
        window
            .set_workspace_stepper_orientation_label(l10n.t("collection.orientation_step").into());
        window.set_workspace_stepper_collection_label(l10n.t("collection.collect_button").into());
        window.set_workspace_stepper_review_label(l10n.t("review.workbench_title").into());
        window.set_workspace_stepper_export_label(l10n.t("export.confirm_title").into());

        // B7 错误弹窗的静态文案（动态内容由 ShellPresenter 每次填入）
        window.set_error_dialog_ok_label(l10n.t("dialog.ok_button").into());

        // 校区选择页（T19B-3/T19B-5）
        window.set_campus_select_title(l10n.t("app.campus_select_title").into());
        window.set_campus_select_new_demo_campus_button_text(l10n.t("app.new_demo_button").into());
        window.set_campus_select_settings_button_text(l10n.t("app.settings_button").into());

        // 方案列表页（T19B-4）
        window.set_plan_list_title(l10n.t("plan.list_header").into());
        window.set_plan_list_create_button_text(l10n.t("plan.create").into());
        window.set_plan_list_back_button_text(l10n.t("app.switch_campus").into());
        window.set_plan_list_empty_text(l10n.t("plan.empty_list").into());
        window.set_plan_list_rename_label(l10n.t("plan.rename").into());
        window.set_plan_list_duplicate_label(l10n.t("plan.duplicate").into());
        window.set_plan_list_delete_label(l10n.t("plan.delete").into());
        window.set_plan_list_tutorial_visible(false);
        window.set_plan_list_tutorial_text(SharedString::new());
        window.set_plan_list_tutorial_dismiss_label(l10n.t("tutorial.dismiss_button").into());
        window.set_plan_list_tutorial_skip_all_label(SharedString::new());

        // 通用对话框文案（T19B-5A 对话框基建）
        window.set_confirm_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_confirm_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_input_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_label(l10n.t("dialog.name_label").into());

        // ── T19B-9: 右上角工具栏 + 公告栏页 + 回收站页文案 ────────────────
        // 工具栏可见性由 .slint 侧按 active-screen 派生（屏 2/4/5/6 显示）
        window.set_toolbar_title(l10n.t("app.welcome_title").into());

        // 公告栏页（Screen 5）文案
        window.set_notice_board_title(l10n.t("notice.page_title").into());
        window.set_notice_board_empty_list_text(l10n.t("notice.empty_list").into());
        window.set_notice_board_archive_button_text(l10n.t("notice.archive_button").into());
        window.set_notice_board_date_today(l10n.t("notice.date_today").into());
        window.set_notice_board_date_yesterday(l10n.t("notice.date_yesterday").into());
        window.set_notice_board_importance_high_label(l10n.t("notice.importance_high").into());
        window.set_notice_board_unread_marker(l10n.t("notice.unread_marker").into());

        // 回收站页（Screen 6）文案
        window.set_trash_page_title(l10n.t("trash.page_title").into());
        window.set_trash_page_empty_list_text(l10n.t("trash.empty_list").into());
        window.set_trash_page_restore_button_text(l10n.t("trash.restore_button").into());
        window.set_trash_page_purge_button_text(l10n.t("trash.purge_button").into());
        window.set_trash_page_retention_notice_text(l10n.t("trash.retention_notice").into());
        window.set_trash_page_campus_prefix((l10n.t("domain.campus").to_string() + "：").into());
        window.set_trash_page_date_today(l10n.t("notice.date_today").into());
    }

    /// 把页面回调绑到 VM（T19B-2：向导完成 + 重看教程；T19B-3：校区选择；
    /// T19B-4：方案列表 CRUD + 教程气泡钩子）。
    ///
    /// 回调闭包持 `Rc<RefCell<Self>>` 共享可变访问（Slint 单线程 UI）；
    /// 回调错误一律递 [`report_callback_error`]（弹窗铁律 ADR-0021）。
    pub fn bind(injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        // 完成设置：读窗口选项 → F1 complete_first_run → 重判着陆跳下一屏
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_wizard_continue_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let setup = FirstRunSetup {
                language: window.get_wizard_language().to_string(),
                minecraft_version: window.get_wizard_version().to_string(),
                acknowledged: window.get_wizard_acknowledged(),
            };
            let mut injector = shared.borrow_mut();
            match injector.settings_mut().complete_first_run(&setup) {
                Ok(()) => {
                    // 向导完成 → 重新判定着陆（无上次校区 → 校区选择占位；
                    // 有 → 该校区方案列表占位），判定仍全权委托 F1
                    window
                        .set_status_text(status_text(injector.l10n(), &injector.landing()).into());
                    window.set_active_screen(1);
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        // 设置页“重新查看教程”：F2 规矩④，进度清零落库
        let shared = Rc::clone(injector);
        window.on_replay_tutorial_clicked(move || {
            let mut injector = shared.borrow_mut();
            if let Err(error) = injector.restart_tutorial() {
                report_callback_error(injector.l10n(), &error);
            }
        });

        // ── T19B-3/T19B-5：校区选择页回调 ────────────────────────
        Self::bind_campus_select(injector, window);
        // ── T19B-4：方案列表页回调 ────────────────────────────
        Self::bind_plan_list(injector, window);
        // ── T19B-9: 右上角工具栏回调 ─────────────────────
        Self::bind_toolbar(injector, window);
        // ── T19B-5B: 方案工作区回调绑定（Phase 1）───────────────────
        Self::bind_workspace(injector, window);
    }

    /// T19B-3/T19B-5：校区选择页回调绑定。
    ///
    /// 新建演示校区 → create_campus 刷新列表；点列表项 → remember_campus →
    /// 刷新方案列表 → 跳屏 2；点击设置 → 跳屏 3。
    fn bind_campus_select(injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_campus_select_new_demo_campus_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let campus_name = injector.l10n().t("campus.demo_name").to_string();

            match injector.projects_mut().create_campus(&campus_name) {
                Ok(campus) => {
                    // 创建成功后立即选中该校区
                    if let Ok(campus_id) = CampusId::parse(&campus.id) {
                        // 先记住校区，再刷新列表
                        let _ = injector.projects_mut().remember_campus(&campus_id);
                        injector.refresh_campus_list(&window);
                        // 自动进入方案列表
                        window.set_active_screen(2);
                    }
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        let weak = window.as_weak();
        window.on_campus_select_settings_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_active_screen(3);
        });

        // 单击已有校区行（T19B-5B 补接）：remember_campus → 刷新方案列表 → 跳屏 2
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_campus_select_campus_clicked(move |campus_id_str| {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let campus_id = match CampusId::parse(&campus_id_str) {
                Ok(id) => id,
                Err(error) => {
                    report_callback_error(injector.l10n(), &error);
                    return;
                }
            };
            if let Err(error) = injector.projects_mut().remember_campus(&campus_id) {
                report_callback_error(injector.l10n(), &error);
                return;
            }
            injector.refresh_plan_list(&window, &campus_id);
            window.set_active_screen(2);
        });
    }

    /// T19B-4/T19B-5A：方案列表页回调绑定。
    ///
    /// 新建方案（ADR-0010 轻创建对话框）/ 返回校区选择 / ···菜单操作
    /// （改名/复制/删除，ADR-0018 §三）/ 教程气泡钩子（F2 规矩①②）。
    fn bind_plan_list(injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        // 新建方案（ADR-0010）：弹输入窗，预填可修改的默认名
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_create_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let injector = shared.borrow();
            let Some(campus_id) = injector.current_campus_id() else {
                report_callback_error(injector.l10n(), &"cannot create plan: no campus selected");
                return;
            };
            let base_name = injector.l10n().t("plan.default_name");
            let default_name = injector.next_plan_name(&campus_id, &base_name);
            // 设置输入窗为“新建方案”模式（mode=0）
            window.set_input_dialog_mode(0);
            window.set_input_dialog_title(injector.l10n().t("dialog.create_title").into());
            window.set_input_dialog_text(default_name.into());
            window.set_input_dialog_visible(true);
        });

        // 返回校区选择页
        let weak = window.as_weak();
        window.on_plan_list_back_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_active_screen(1);
        });

        // 单击方案卡片（ADR-0027 第 6 轮：单击即开，无概览层）→ 跳屏 4 工作区
        let weak = window.as_weak();
        window.on_plan_list_card_clicked(move |plan_id| {
            let Some(window) = weak.upgrade() else { return };
            // 记录当前方案 ID 供后续接线单使用；Phase 1 从第①步开始
            window.set_active_plan_id(plan_id);
            window.set_workspace_active_step(0);
            window.set_active_screen(4);
        });

        // 改名（ADR-0018 §三）：···菜单 → 输入窗（预填现名）→ F3 rename_plan
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_rename_clicked(move |plan_id| {
            let Some(window) = weak.upgrade() else { return };
            let injector = shared.borrow();
            // 查找当前方案名作为预填值
            let current_name = if let Some(campus_id) = injector.current_campus_id() {
                injector
                    .projects()
                    .list_plan_cards(&campus_id)
                    .unwrap_or_default()
                    .iter()
                    .find(|c| c.plan_id.as_str() == plan_id.as_str())
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            // 设置输入窗为“改名”模式（mode=1）
            window.set_input_dialog_mode(1);
            window.set_input_dialog_title(injector.l10n().t("dialog.rename_title").into());
            window.set_input_dialog_text(current_name.into());
            window.set_active_plan_id(plan_id);
            window.set_input_dialog_visible(true);
        });

        // 复制方案：调 F3 duplicate_plan，后缀取 l10n “副本”
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_duplicate_clicked(move |plan_id_str| {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let plan_id = match PlanId::parse(&plan_id_str) {
                Ok(id) => id,
                Err(e) => {
                    report_callback_error(injector.l10n(), &e);
                    return;
                }
            };
            let suffix = injector.l10n().t("plan.duplicate_suffix");
            match injector.projects_mut().duplicate_plan(&plan_id, &suffix) {
                Ok(_) => {
                    if let Some(campus_id) = injector.current_campus_id() {
                        injector.refresh_plan_list(&window, &campus_id);
                    }
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        // 删除（ADR-0018 §三）：先弹确认窗，确认后进回收站
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_delete_clicked(move |plan_id_str| {
            let Some(window) = weak.upgrade() else { return };
            let injector = shared.borrow();
            // 设置确认窗文案
            window.set_confirm_dialog_title(injector.l10n().t("dialog.delete_title").into());
            window.set_confirm_dialog_body(injector.l10n().t("plan.delete_confirm").into());
            window.set_active_plan_id(plan_id_str);
            window.set_confirm_dialog_visible(true);
        });

        // ── 确认窗回调（T19B-5A）───────────────────────────
        // 确认删除：调 F3 delete_plan（保留 30 天）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_confirm_dialog_confirmed(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            let mut injector = shared.borrow_mut();
            let plan_id_str = window.get_active_plan_id().to_string();
            let plan_id = match PlanId::parse(&plan_id_str) {
                Ok(id) => id,
                Err(e) => {
                    report_callback_error(injector.l10n(), &e);
                    return;
                }
            };
            let Some(campus_id) = injector.current_campus_id() else {
                report_callback_error(injector.l10n(), &"cannot delete plan: no campus selected");
                return;
            };
            match injector.projects_mut().delete_plan(&campus_id, &plan_id) {
                Ok(_) => injector.refresh_plan_list(&window, &campus_id),
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        // 取消删除
        let weak = window.as_weak();
        window.on_confirm_dialog_cancelled(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
        });

        // ── 输入窗回调（T19B-5A）───────────────────────────
        // 确认输入：根据 mode 分派新建/改名
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_input_dialog_confirmed(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let mode = window.get_input_dialog_mode();
            let name = window.get_input_dialog_text().to_string();
            let name = name.trim().to_string();
            if name.is_empty() {
                return; // 空名不提交，窗保持打开
            }
            let Some(campus_id) = injector.current_campus_id() else {
                report_callback_error(injector.l10n(), &"no campus selected");
                return;
            };
            match mode {
                0 => {
                    // 新建方案（ADR-0010）
                    match injector.projects_mut().create_plan(&campus_id, &name) {
                        Ok(_) => {
                            window.set_input_dialog_visible(false);
                            injector.refresh_plan_list(&window, &campus_id);
                        }
                        Err(error) => report_callback_error(injector.l10n(), &error),
                    }
                }
                _ => {
                    // 改名（ADR-0018 §三）
                    let plan_id_str = window.get_active_plan_id().to_string();
                    let plan_id = match PlanId::parse(&plan_id_str) {
                        Ok(id) => id,
                        Err(e) => {
                            report_callback_error(injector.l10n(), &e);
                            return;
                        }
                    };
                    match injector.projects_mut().rename_plan(&plan_id, &name) {
                        Ok(_) => {
                            window.set_input_dialog_visible(false);
                            injector.refresh_plan_list(&window, &campus_id);
                        }
                        Err(error) => report_callback_error(injector.l10n(), &error),
                    }
                }
            }
        });

        // 取消输入
        let weak = window.as_weak();
        window.on_input_dialog_cancelled(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_input_dialog_visible(false);
        });

        // 教程气泡“知道了”（F2 规矩①）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_tutorial_dismiss_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            if let Err(error) = injector.dismiss_tutorial_step(TutorialStep::PlanListIntro) {
                report_callback_error(injector.l10n(), &error);
            }
            window.set_plan_list_tutorial_visible(false);
        });

        // 教程气泡“跳过全部引导”（F2 规矩②）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_tutorial_skip_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            if let Err(error) = injector.skip_all_tutorial() {
                report_callback_error(injector.l10n(), &error);
            }
            window.set_plan_list_tutorial_visible(false);
        });
    }

    // ── T19B-9: 右上角工具栏回调绑定 ────────────────────────
    fn bind_toolbar(_injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak = window.as_weak();

        // 公告栏入口：跳屏 5
        let weak_clone = weak.clone();
        window.on_notice_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                window.set_active_screen(5);
            }
        });

        // 切换校区入口：跳屏 1
        let weak_clone = weak.clone();
        window.on_switch_campus_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                window.set_active_screen(1);
            }
        });

        // 回收站入口：跳屏 6
        let weak_clone = weak.clone();
        window.on_trash_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                window.set_active_screen(6);
            }
        });

        // 设置入口：跳屏 3
        let weak_clone = weak.clone();
        window.on_settings_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                window.set_active_screen(3);
            }
        });
    }

    // ── T19B-5B: 方案工作区回调绑定（Phase 1）───────────────────────
    fn bind_workspace(_injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        // 步骤点击：上锁步骤不可点击（ADR-0027：前跳上锁，回跳自由，第①格永远解锁）
        let weak = window.as_weak();
        window.on_workspace_step_clicked(move |step_index| {
            let Some(window) = weak.upgrade() else { return };
            let completed = window.get_workspace_completed_steps();
            if step_index > completed {
                return; // 锁定步骤忽略点击
            }
            window.set_workspace_active_step(step_index);
        });
    }

    /// 首开着陆判定（委托 F1，壳只消费结果）
    pub fn landing(&self) -> LandingDecision {
        decide(&self.settings)
    }

    /// 评审进台：从 B2 一次性读入该方案的候选集（缝 4）。
    ///
    /// 再次调用即重新装载——导出失败回滚后"丢弃已封账实例、重新
    /// 进台"的落点（见 F9 `SealGate` 文档）。
    pub fn enter_review(&mut self, plan_id: &PlanId) -> Result<()> {
        self.review = Some(ReviewWorkbench::load(
            self.projects.database_mut(),
            plan_id,
        )?);
        Ok(())
    }

    // ── 接线钥匙：后续 T19B 工单经这些访问器绑定回调 ─────────────

    /// B6 文案解析器
    pub fn l10n(&self) -> &Localization {
        &self.l10n
    }

    /// F1 全局设置
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// F1 全局设置（可变：设置页写入）
    pub fn settings_mut(&mut self) -> &mut SettingsManager {
        &mut self.settings
    }

    /// F2 新手教程
    pub fn tutorial(&self) -> &OnboardingTutorial {
        &self.tutorial
    }

    /// F2 新手教程（可变：气泡 dismiss/skip_all 落库）
    pub fn tutorial_mut(&mut self) -> &mut OnboardingTutorial {
        &mut self.tutorial
    }

    /// 债务②：设置页“重新查看教程”→ F2 进度清零（规矩④）。
    ///
    /// F2 落库借道 F3 连接（与装载时同库），壳只做转接不碰业务。
    pub fn restart_tutorial(&mut self) -> Result<()> {
        let Self {
            tutorial, projects, ..
        } = self;
        tutorial.restart(projects.database_mut())?;
        Ok(())
    }

    /// F2 规矩①：气泡“知道了”→ 该提示点记为已见并落库。
    pub fn dismiss_tutorial_step(&mut self, step: TutorialStep) -> Result<()> {
        let Self {
            tutorial, projects, ..
        } = self;
        tutorial.dismiss(projects.database_mut(), step)?;
        Ok(())
    }

    /// F2 规矩②：“跳过全部引导”→ 直接转 Completed，经 B7 留底。
    pub fn skip_all_tutorial(&mut self) -> Result<()> {
        let Self {
            tutorial,
            projects,
            l10n,
            ..
        } = self;
        tutorial.skip_all(projects.database_mut(), l10n)?;
        Ok(())
    }

    /// F3 方案管理（只读）
    pub fn projects(&self) -> &ProjectManager {
        &self.projects
    }

    /// F3 方案管理（可变：CRUD 与借出 B2 句柄给 F2/F5/F7 落库）
    pub fn projects_mut(&mut self) -> &mut ProjectManager {
        &mut self.projects
    }

    /// F4 采集流水线
    pub fn acquisition(&self) -> &AcquisitionPipeline {
        &self.acquisition
    }

    /// F5 当前评审会话（未进台时为 `None`）
    pub fn review(&self) -> Option<&ReviewWorkbench> {
        self.review.as_ref()
    }

    /// F7 安静哨兵
    pub fn sentinel(&self) -> &QuietSentinel {
        &self.sentinel
    }

    /// F9 导出控制台
    pub fn export(&self) -> &ExportConsole<MockSealGate> {
        &self.export
    }

    // ── T19B-4/T19B-5 辅助方法：校区列表与方案列表数据流转 ────────────────

    /// 当前校区 ID（读 B2 app_settings 中的“上次使用的校区”）
    fn current_campus_id(&self) -> Option<CampusId> {
        self.projects
            .landing_campus()
            .ok()
            .flatten()
            .and_then(|c| CampusId::parse(&c.id).ok())
    }

    /// 刷新校区选择页：动态列表 + 空占位文案
    fn refresh_campus_list(&mut self, window: &AppWindow) {
        let l10n = &self.l10n;

        // 动态列表（F3 list_campuses）
        let campuses = self.projects.list_campuses().unwrap_or_default();
        let model: Vec<CampusData> = campuses
            .iter()
            .map(|c| CampusData {
                id: c.id.clone().into(),
                name: c.name.clone().into(),
            })
            .collect();
        window.set_campus_select_model(ModelRc::new(VecModel::from(model.clone())));

        // 空列表占位
        if model.is_empty() {
            window.set_campus_select_empty_list_text(l10n.t("app.campus_select_no_campus").into());
        }
    }

    /// 刷新方案列表页数据：校区名 + 卡片模型 + 教程气泡钩子。
    fn refresh_plan_list(&mut self, window: &AppWindow, campus_id: &CampusId) {
        let l10n = &self.l10n;

        // 校区名显示
        let campus_name = self
            .projects
            .landing_campus()
            .ok()
            .flatten()
            .map(|c| l10n.t_with_array("app.shell_status_last_campus", &[&c.name]))
            .unwrap_or_default();
        window.set_plan_list_campus_name(campus_name.into());

        // 卡片模型（F3 返回已按修改时间倒序）
        let cards = self.projects.list_plan_cards(campus_id).unwrap_or_default();
        let model: Vec<crate::PlanCardData> = cards
            .iter()
            .map(|card| crate::PlanCardData {
                plan_id: card.plan_id.clone().into(),
                name: card.name.clone().into(),
                progress_desc: l10n.t(card.progress.text_key()).into(),
                // ADR-0018 §一第 3 条：相对表述（“刚刚/X 分钟前/X 小时前/X 天前”）
                last_modified: format_relative_time(l10n, &card.last_modified_at).into(),
            })
            .collect();
        window.set_plan_list_model(ModelRc::new(VecModel::from(model)));

        // 教程气泡钩子（F2 规矩③“只教一次”：已见过则返回 None）
        if let Some(bubble) = self.tutorial.bubble_for(TutorialStep::PlanListIntro, l10n) {
            window.set_plan_list_tutorial_visible(true);
            window.set_plan_list_tutorial_text(bubble.message.into());
            window.set_plan_list_tutorial_dismiss_label(bubble.dismiss_label.into());
            window.set_plan_list_tutorial_skip_all_label(
                bubble.skip_all_label.unwrap_or_default().into(),
            );
        } else {
            window.set_plan_list_tutorial_visible(false);
        }
    }

    /// 生成不冲突的方案名：“新方案 1”“新方案 2”……
    fn next_plan_name(&self, campus_id: &CampusId, base_name: &str) -> String {
        let existing = self.projects.list_plan_cards(campus_id).unwrap_or_default();
        let names: Vec<&str> = existing.iter().map(|c| c.name.as_str()).collect();
        for n in 1..1000 {
            let candidate = format!("{base_name} {n}");
            if !names.contains(&candidate.as_str()) {
                return candidate;
            }
        }
        format!("{base_name} {}", names.len() + 1)
    }
}

/// 把 F1 的静态选项表转成 Slint 下拉菜单 model（纯数据搬运）。
fn string_model(items: &[&str]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|item| SharedString::from(*item))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use onboarding_tutorial::TutorialStatus;
    use shared_domain_types::CampusId;

    use super::*;

    // 注意：凡需要创建 AppWindow 的测试一律放 tests/ui_bindings.rs（独立
    // 进程内单个 #[test] 串行）——Slint 平台只能在一个线程初始化一次，
    // 单元测试并行多线程创窗口必炸。

    fn injector() -> ViewModelInjector {
        let db = ShellDatabases::open_in_memory().expect("内存库连接组");
        ViewModelInjector::new(db).expect("构造注入器")
    }

    #[test]
    fn test_injector_holds_all_vms() {
        let mut injector = injector();

        // F1：全新库 → 首次运行
        assert!(injector.settings().is_first_run().expect("读首次运行标记"));
        // F2：引导尚未开始
        assert_eq!(injector.tutorial().status(), TutorialStatus::NotStarted);
        // F3：可建校区与方案
        let campus = injector
            .projects_mut()
            .create_campus("演示大学")
            .expect("建校区");
        let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "第一个方案")
            .expect("建方案");
        // F4：归类引擎就绪
        let _engine = injector.acquisition().engine();
        // F5：进台会话装载成功（零候选照样成台）
        injector.enter_review(&plan_id).expect("评审进台");
        let workbench = injector.review().expect("评审会话已持有");
        assert_eq!(workbench.candidate_count(), 0);
        // F7 / F9：实例可达
        let _sentinel = injector.sentinel();
        let _export = injector.export();
    }

    #[test]
    fn restart_tutorial_resets_progress_in_db() {
        // F2 借道 F3 连接落库；内存库即可（同一条 projects 连接）
        let mut injector = injector();
        injector.restart_tutorial().expect("重看教程");
        assert_eq!(injector.tutorial().status(), TutorialStatus::NotStarted);
        let reloaded = OnboardingTutorial::load(injector.projects_mut().database_mut())
            .expect("重新装载引导进度");
        assert_eq!(reloaded.status(), TutorialStatus::NotStarted);
    }
}
