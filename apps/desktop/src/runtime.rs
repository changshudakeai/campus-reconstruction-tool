//! S1 迁移期运行时：Slint 主窗口装配与正式入口组合根。
//!
//! 启动与设置流程已经由呈现入口一次取得完整结果（工单 03），S1 只呈现返回的
//! 页面、状态、导航与通知。landing_decision 等旧判定助手只保留给行为基线
//! 测试与迁移期占位路径，生产启动不再自行组合着陆条件。
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use coverage_audit::QuietSentinel;
use data_acquisition::AcquisitionPipeline;
use data_persistence::Database;
use export_console::{ExportConsole, MockSealGate};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{Notification, NotificationCenter, PresenterRegistry};
use onboarding_tutorial::{OnboardingTutorial, TutorialStep};
use project_management::ProjectManager;
use review_workbench::ReviewWorkbench;
use shared_domain_types::PlanId;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::presenter::report_callback_error;
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

// T19B-1 —— VM 注入器（S1-05 收窄后）。
//
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

/// VM 注入器：构造并持有全部 F 模块实例，把静态视图文案注入 Slint 窗口。
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
    /// F9 导出控制台。门控暂用 `MockSealGate` 占位——真门控随后续导出接线落地。
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

    /// 把静态视图文案注入 Slint 窗口（只设 in property；动态页面状态由
    /// 呈现入口每次完整渲染）。
    pub(crate) fn inject(&self, window: &AppWindow) {
        let l10n = &self.l10n;

        // 屏 4：方案工作区占位文案（T19B-5B）
        window.set_workspace_placeholder_title(l10n.t("workspace.placeholder_title").into());
        window.set_workspace_placeholder_subtitle(l10n.t("workspace.placeholder_subtitle").into());
        window.set_workspace_step_pending_notice(l10n.t("workspace.step_pending_notice").into());

        // 步骤条文案
        window.set_workspace_stepper_title_label(l10n.t("collection.title").into());
        window.set_workspace_stepper_boundary_label(l10n.t("collection.boundary_step").into());
        window
            .set_workspace_stepper_orientation_label(l10n.t("collection.orientation_step").into());
        window.set_workspace_stepper_collection_label(l10n.t("collection.collect_button").into());
        window.set_workspace_stepper_review_label(l10n.t("review.workbench_title").into());
        window.set_workspace_stepper_export_label(l10n.t("export.confirm_title").into());

        // 屏 4：步骤条教程气泡（F2 钩子，ADR-0028）——初始隐藏，进屏 4 时索泡
        window.set_workspace_tutorial_visible(false);
        window.set_workspace_tutorial_text(SharedString::new());
        window.set_workspace_tutorial_dismiss_label(l10n.t("tutorial.dismiss_button").into());
        window.set_workspace_tutorial_skip_all_label(SharedString::new());

        // 屏 4 步骤①：圈边界编辑器静态文案（动态状态由工作区入口渲染）
        window.set_workspace_boundary_points(ModelRc::new(VecModel::default()));
        window.set_workspace_boundary_path_commands(SharedString::new());
        window.set_workspace_boundary_title(l10n.t("boundary.step_title").into());
        window.set_workspace_boundary_hint(l10n.t("boundary.hint").into());
        window.set_workspace_boundary_undo_label(l10n.t("boundary.undo_button").into());
        window.set_workspace_boundary_confirm_label(l10n.t("boundary.confirm_button").into());
        window.set_workspace_boundary_reset_label(l10n.t("boundary.reset_button").into());
        window.set_workspace_boundary_status(l10n.t("boundary.status_idle").into());
        window.set_workspace_boundary_map_placeholder(l10n.t("boundary.map_placeholder").into());
        window.set_workspace_boundary_is_determined(false);
        window.set_workspace_boundary_point_count(0);

        // 屏 4 步骤②：定朝向交互页静态文案（动态状态由工作区入口渲染）
        window.set_workspace_orientation_points(ModelRc::new(VecModel::default()));
        window.set_workspace_orientation_path_commands(SharedString::new());
        window.set_workspace_orientation_arrow_commands(SharedString::new());
        window.set_workspace_orientation_mode("two-points".into());
        window.set_workspace_orientation_angle(-1.0);
        window.set_workspace_orientation_is_determined(false);
        window.set_workspace_orientation_title(l10n.t("orientation.step_title").into());
        window.set_workspace_orientation_two_points_hint(
            l10n.t("orientation.two_points_hint").into(),
        );
        window.set_workspace_orientation_bearing_angle_hint(
            l10n.t("orientation.bearing_angle_hint").into(),
        );
        window.set_workspace_orientation_angle_input_placeholder(
            l10n.t("orientation.angle_input_placeholder").into(),
        );
        window.set_workspace_orientation_angle_display(SharedString::new());
        window.set_workspace_orientation_input_text("".into());
        window.set_workspace_orientation_submit_label(l10n.t("orientation.submit_button").into());
        window.set_workspace_orientation_reset_label(l10n.t("orientation.reset_button").into());
        window.set_workspace_orientation_status(l10n.t("orientation.status_idle").into());
        window.set_workspace_orientation_mode_two_points_label(
            l10n.t("orientation.mode_two_points").into(),
        );
        window.set_workspace_orientation_mode_bearing_angle_label(
            l10n.t("orientation.mode_bearing_angle").into(),
        );

        // B7 错误弹窗的静态文案（动态内容由 ShellPresenter 每次填入）
        window.set_error_dialog_ok_label(l10n.t("dialog.ok_button").into());

        // 通用对话框文案（T19B-5A 对话框基建）
        window.set_confirm_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_confirm_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_input_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_label(l10n.t("dialog.name_label").into());

        // ── T19B-9: 右上角工具栏 + 公告栏页 + 回收站页文案 ────────────────
        window.set_toolbar_title(l10n.t("app.welcome_title").into());

        // 公告栏页（Screen 5）文案
        window.set_notice_board_title(l10n.t("notice.page_title").into());
        window.set_notice_board_empty_list_text(l10n.t("notice.empty_list").into());
        window.set_notice_board_archive_button_text(l10n.t("notice.archive_button").into());
        window.set_notice_board_date_today(l10n.t("notice.date_today").into());
        window.set_notice_board_date_yesterday(l10n.t("notice.date_yesterday").into());
        window.set_notice_board_importance_high_label(l10n.t("notice.importance_high").into());
        window.set_notice_board_unread_marker(l10n.t("notice.unread_marker").into());
        window.set_trash_page_campus_prefix((l10n.t("domain.campus").to_string() + ":").into());
    }

    /// 把剩余页面回调绑到 VM（方案列表教程气泡 + 通用确认窗）。
    ///
    /// 工作区/步骤/边界/工具栏回调已全部迁到 ProductionEntries::bind_actions
    /// （S1-05），本函数不再包含业务接线。
    pub(crate) fn bind(
        injector: &Rc<RefCell<Self>>,
        presentation: &Rc<RefCell<ProductionEntries>>,
        window: &AppWindow,
    ) {
        // ── 方案列表遗留：教程气泡（F2，S1-04 后仅剩此绑定）──
        Self::bind_plan_list(injector, window);
        // ── 通用确认窗：确认/取消统一交给呈现入口消费 ─────
        Self::bind_confirm_dialog(presentation, window);
    }

    /// 方案列表教程气泡绑定（S1-04 后仅保留：卡片单击打开工作区已迁到
    /// ProductionEntries::bind_actions）。
    fn bind_plan_list(injector: &Rc<RefCell<Self>>, window: &AppWindow) {
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

    /// 通用确认窗回调：确认/取消统一交给呈现入口（S1-05 起不再有工作区遗留分支）。
    fn bind_confirm_dialog(presentation: &Rc<RefCell<ProductionEntries>>, window: &AppWindow) {
        let weak = window.as_weak();
        let presentation_confirmed = Rc::clone(presentation);
        window.on_confirm_dialog_confirmed(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            presentation_confirmed
                .borrow_mut()
                .confirm_pending_action(&window);
        });

        let weak = window.as_weak();
        let presentation_cancel = Rc::clone(presentation);
        window.on_confirm_dialog_cancelled(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            presentation_cancel
                .borrow_mut()
                .cancel_pending_action(&window);
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

    // ── F 模块访问器 ─────────────────────────────────────

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
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );

        // S1-05：步骤点击先经过工作区功能入口（索引 9）取得导航决定；
        // 允许进入的占位步骤再由对应步骤入口呈现页面
        window.set_workspace_completed_steps(4);
        window.invoke_workspace_step_clicked(2);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 0, 0, 0, 0, 0, 1],
            "点击采集步骤必须经过工作区入口与采集入口"
        );
        window.invoke_workspace_step_clicked(3);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 0, 0, 0, 2],
            "点击评审步骤必须经过工作区入口与评审入口"
        );
        window.invoke_workspace_step_clicked(4);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 1, 0, 0, 3],
            "点击导出步骤必须经过工作区入口与导出入口"
        );
        // 步骤①/②（边界/朝向）现在由工作区入口统一裁决（不再走旧路径）
        window.invoke_workspace_step_clicked(1);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 0, 1, 0, 0, 4],
            "点击朝向步骤只能经过工作区入口"
        );

        runtime
            ._presentation
            .borrow_mut()
            .show_coverage_for_test(&window);
        assert_eq!(
            crate::production::entry_calls(),
            [1, 0, 0, 1, 1, 1, 1, 0, 0, 4]
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
    // ── 注入器（S1-05 并入运行时组合根）测试 ──────────────────────────

    use onboarding_tutorial::TutorialStatus;
    use shared_domain_types::CampusId;

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
