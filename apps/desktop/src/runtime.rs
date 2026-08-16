//! S1 运行时：Slint 主窗口装配与正式入口组合根。
//!
//! 启动与设置流程已经由呈现入口一次取得完整结果（工单 03），S1 只呈现返回的
//! 页面、状态、导航与通知。landing_decision 等判定助手供行为基线测试与
//! 组合根复用，生产启动不再自行组合着陆条件。
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{mpsc, Arc};

use anyhow::Result;
use collection_flow::CollectionFlow;
use data_acquisition::overpass::OverpassClient;
use data_persistence::Database;
use export_flow::{BoundaryExportFlow, ExportFileSystem, StdExportFileSystem};
use global_settings::SettingsManager;
use localization::{Language, Localization};
use notification_center::{Notification, NotificationCenter, PresenterRegistry};
use onboarding_tutorial::{OnboardingTutorial, TutorialStep};
use project_management::{PlanBoundarySession, ProjectManager};
use review_workbench::ReviewWorkbench;
use shared_domain_types::PlanId;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::boundary_source::{
    boundary_session_source, production_boundary_source, BoundaryFetchSource,
};
use crate::presenter::report_callback_error;
use crate::presenter::ShellPresenter;

use crate::production::campus_search::CampusSearchTransport;
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

/// 正式窗口装配的生命周期句柄；持有注入器与全部呈现入口直到窗口退出。
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
    // T36：MCREBUILD_LOG_FILE 环境变量 → 把 map_webview 生命周期/IPC/销毁
    // 日志写入文件作为真机走查证据；未设置时不安装 logger（log 宏自动空转）。
    crate::diagnostic_log::init();

    // B7 一本账先于任何回调可用（弹窗铁律 ADR-0021）。
    let center = NotificationCenter::init(PresenterRegistry::new());

    let window = AppWindow::new()?;
    // T19B-5A 色卡机制（ADR-0023）：启动时加载亮色卡设置 Theme global
    apply_theme(&window);
    // T38 退出安全：任何关闭请求（用户 X / Alt+F4 / 系统 WM_CLOSE）先同步
    // 释放地图 WebView（事件循环仍存活、COM 健康），避免进程退出时 TLS
    // 析构 drop InnerWebView → Close() → combase.dll 0xc0000005 崩溃。
    {
        window.window().on_close_requested(|| {
            log::info!("runtime: 收到窗口关闭请求，先同步释放地图 WebView");
            crate::map_webview::shutdown();
            slint::CloseRequestResponse::HideWindow
        });
    }
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
            // T38 崩溃取证：记录事件循环退出时机与原因（进程退出前 TLS 析构
            // 会 drop 仍持有的 wry InnerWebView，Close() 在 COM 拆除阶段触发
            // combase 崩溃；先确认 run() 何时/为何返回）。
            log::info!("runtime: 进入事件循环 window.run()");
            let run_result = window.run();
            log::info!("runtime: window.run() 已返回（进程即将退出）: {run_result:?}");
            run_result?;
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
/// 按值持有句柄，因此壳对同一数据库文件开两条连接。F2/F5/F7 与 A1
/// 采集的落库操作统一借道 F3 [`ProjectManager::database`]，不再额外开连接。
pub struct ShellDatabases {
    /// F1 全局设置的专属连接
    settings: Database,
    /// F3 方案管理的专属连接（`database` 供 F2/F5/F7/A1 借用）
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
    l10n: Arc<Localization>,
    /// F1 全局设置（自持一条 B2 连接）
    settings: SettingsManager,
    /// F2 新手教程（引导进度经 B2 app_settings 持久化）
    tutorial: OnboardingTutorial,
    /// F3 方案管理（自持一条 B2 连接）
    projects: ProjectManager,
    /// F5 评审工作台：按方案进台的会话（[`Self::enter_review`] 装载）
    review: Option<ReviewWorkbench>,
    /// F9 完整导出入口的稳定输入能力端口状态。
    export_flow: Arc<BoundaryExportFlow>,
    /// A1 候选采集完整用例（F4 → B2 → B14 → F7 在入口后协调，ADR-0039）。
    collection_flow: Arc<CollectionFlow>,
    /// F3 方案边界完整会话入口；S1 不持有缓存键、完成结果或后台 receiver。
    boundary_session: PlanBoundarySession,
    /// 校区在线搜索 WebView 桥的响应通道（D-3，S1 只做传输转发）。
    campus_search_ipc: mpsc::Sender<String>,
    /// 校区在线搜索传输（生产走 WebView，测试注入罐头）。
    campus_search_transport: Arc<CampusSearchTransport>,
}

impl ViewModelInjector {
    /// 构造并持有全部 F 模块实例（F1-F9，B8/F6/F8 未立户不在册）。
    pub fn new(db: ShellDatabases) -> Result<Self> {
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_collection_source_and_overpass(
            db,
            Arc::new(StdExportFileSystem),
            collection_source::production_collection_source(Arc::clone(&overpass)),
        )
    }

    /// 方案边界数据源注入点（生产为 OSM，测试使用 fake 计数器）。
    pub fn new_with_boundary_source(
        db: ShellDatabases,
        boundary_source: BoundaryFetchSource,
    ) -> Result<Self> {
        let (campus_search_ipc, campus_search_transport) =
            crate::production::campus_search::campus_search_production_transport();
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_sources(
            db,
            Arc::new(StdExportFileSystem),
            campus_search_ipc,
            campus_search_transport,
            collection_source::production_collection_source(Arc::clone(&overpass)),
            boundary_source,
            overpass,
        )
    }

    /// 边界源与候选采集源双注入点（D 工单壳层验收：离线计数数据源 +
    /// 罐头边界源，重验证期间网络请求数断言为 0）。
    pub fn new_with_boundary_and_collection_source(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
        boundary_source: BoundaryFetchSource,
        collection_source: Arc<dyn data_acquisition::DataSource + Send + Sync>,
    ) -> Result<Self> {
        let (campus_search_ipc, campus_search_transport) =
            crate::production::campus_search::campus_search_production_transport();
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_sources(
            db,
            file_system,
            campus_search_ipc,
            campus_search_transport,
            collection_source,
            boundary_source,
            overpass,
        )
    }

    pub fn new_with_export_file_system(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
    ) -> Result<Self> {
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_collection_source_and_overpass(
            db,
            file_system,
            collection_source::production_collection_source(Arc::clone(&overpass)),
        )
    }

    /// T31：候选采集数据源注入点（生产 = OverpassDataSource 直连；
    /// 测试注入罐头 Overpass 响应，离线可测）。
    pub fn new_with_collection_source(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
        collection_source: Arc<dyn data_acquisition::DataSource + Send + Sync>,
    ) -> Result<Self> {
        Self::new_with_collection_source_and_overpass(db, file_system, collection_source)
    }

    /// T31 注入点 + 共享 Overpass 客户端（生产边界源与候选源共用同一客户端）。
    fn new_with_collection_source_and_overpass(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
        collection_source: Arc<dyn data_acquisition::DataSource + Send + Sync>,
    ) -> Result<Self> {
        let (campus_search_ipc, campus_search_transport) =
            crate::production::campus_search::campus_search_production_transport();
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_sources(
            db,
            file_system,
            campus_search_ipc,
            campus_search_transport,
            collection_source,
            production_boundary_source(Arc::clone(&overpass)),
            overpass,
        )
    }

    /// 测试/替代校区搜索传输注入点（罐头响应不经 WebView；响应通道照常存在，
    /// 保证 IPC 转发链路在生产与测试形态一致）。
    pub fn new_with_campus_search_transport(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
        transport: CampusSearchTransport,
    ) -> Result<Self> {
        let (campus_search_ipc, _) =
            crate::production::campus_search::campus_search_production_transport();
        let overpass = Arc::new(OverpassClient::production());
        Self::new_with_sources(
            db,
            file_system,
            campus_search_ipc,
            transport,
            collection_source::production_collection_source(Arc::clone(&overpass)),
            production_boundary_source(Arc::clone(&overpass)),
            overpass,
        )
    }

    fn new_with_sources(
        db: ShellDatabases,
        file_system: Arc<dyn ExportFileSystem>,
        campus_search_ipc: mpsc::Sender<String>,
        campus_search_transport: CampusSearchTransport,
        collection_source: Arc<dyn data_acquisition::DataSource + Send + Sync>,
        boundary_source: BoundaryFetchSource,
        _overpass: Arc<OverpassClient>,
    ) -> Result<Self> {
        let l10n = Arc::new(Localization::new(Language::ZhCn).map_err(anyhow::Error::msg)?);
        let tutorial = OnboardingTutorial::load(&db.projects)?;
        let projects = ProjectManager::new(db.projects);
        let export_flow = Arc::new(BoundaryExportFlow::new_with_candidate_store(
            file_system,
            projects.shared_database(),
        ));
        let collection_flow = Arc::new(CollectionFlow::new(
            projects.shared_database(),
            collection_source,
            Arc::clone(&l10n),
        ));
        Ok(Self {
            l10n,
            settings: SettingsManager::new(db.settings),
            tutorial,
            projects,
            review: None,
            export_flow,
            collection_flow,
            boundary_session: PlanBoundarySession::new(boundary_session_source(boundary_source)),
            campus_search_ipc,
            campus_search_transport: Arc::new(campus_search_transport),
        })
    }

    pub(crate) fn boundary_session_mut(&mut self) -> &mut PlanBoundarySession {
        &mut self.boundary_session
    }

    /// 把静态视图文案注入 Slint 窗口（只设 in property；动态页面状态由
    /// 呈现入口每次完整渲染）。
    pub(crate) fn inject(&self, window: &AppWindow) {
        let l10n = &self.l10n;

        // 屏 4：方案工作区静态文案（动态状态由工作区入口渲染）
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
        window.set_workspace_back_to_plan_list_label(l10n.t("workspace.back_to_plan_list").into());

        // 屏 4：步骤条教程气泡（F2，ADR-0028）——初始隐藏，进屏 4 时由入口索泡
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
        window.set_workspace_boundary_refresh_label(l10n.t("boundary.refresh_button").into());
        window.set_workspace_boundary_status(l10n.t("boundary.status_idle").into());
        window.set_workspace_boundary_map_placeholder(l10n.t("boundary.map_placeholder").into());
        window.set_workspace_boundary_is_determined(false);
        window.set_workspace_boundary_point_count(0);

        // B7 错误弹窗的静态文案（动态内容由 ShellPresenter 每次填入）
        window.set_error_dialog_ok_label(l10n.t("dialog.ok_button").into());

        // 通用对话框文案（T19B-5A 对话框基建）
        window.set_confirm_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_confirm_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_confirm_label(l10n.t("dialog.confirm_button").into());
        window.set_input_dialog_cancel_label(l10n.t("dialog.cancel_button").into());
        window.set_input_dialog_label(l10n.t("dialog.name_label").into());

        // ── 右上角工具栏 + 公告栏页 + 回收站页文案 ────────────────────────
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
        // ── 方案列表教程气泡（F2）──
        Self::bind_plan_list(injector, window);
        // ── 通用确认窗：确认/取消统一交给呈现入口消费 ─────
        Self::bind_confirm_dialog(presentation, window);
    }

    /// 方案列表教程气泡绑定（卡片单击打开工作区在 ProductionEntries::bind_actions）。
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
            // T34：弹窗遮挡统一机制——确认弹窗关闭后按当前步骤模式恢复地图
            crate::map_webview::restore_after_modal(window.as_weak());
            let needs_campus_search_polling = presentation_confirmed
                .borrow_mut()
                .confirm_pending_action(&window);
            if needs_campus_search_polling {
                // 校区搜索失败弹窗点"重试"：确认后重新搜索并拉起轮询
                crate::production::ProductionEntries::start_campus_search_polling(
                    &presentation_confirmed,
                    &window,
                );
            }
        });

        let weak = window.as_weak();
        let presentation_cancel = Rc::clone(presentation);
        window.on_confirm_dialog_cancelled(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            // T34：弹窗遮挡统一机制——确认弹窗取消后按当前步骤模式恢复地图
            crate::map_webview::restore_after_modal(window.as_weak());
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
        let database = self.projects.database();
        self.review = Some(ReviewWorkbench::load(&database, plan_id)?);
        Ok(())
    }

    // ── F 模块访问器 ─────────────────────────────────────

    /// B6 文案解析器
    pub fn l10n(&self) -> &Localization {
        self.l10n.as_ref()
    }

    /// F1 全局设置
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// F1 全局设置（可变：设置页写入）
    pub fn settings_mut(&mut self) -> &mut SettingsManager {
        &mut self.settings
    }

    pub(crate) fn export_flow(&self) -> Arc<BoundaryExportFlow> {
        self.export_flow.clone()
    }

    /// A1 候选采集完整用例（S1 只转发意图并呈现返回状态）。
    pub(crate) fn collection_flow(&self) -> Arc<CollectionFlow> {
        self.collection_flow.clone()
    }

    /// 校区在线搜索 WebView 桥的响应通道（壳把原始 IPC 原样转交）。
    pub(crate) fn campus_search_ipc_sender(&self) -> mpsc::Sender<String> {
        self.campus_search_ipc.clone()
    }

    /// 校区在线搜索传输（生产走 WebView，测试注入罐头）。
    pub(crate) fn campus_search_transport(&self) -> Arc<CampusSearchTransport> {
        Arc::clone(&self.campus_search_transport)
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
        let mut database = projects.database();
        let database: &mut Database = &mut database;
        tutorial.restart(database)?;
        Ok(())
    }

    /// F2 规矩①：气泡“知道了”→ 该提示点记为已见并落库。
    pub fn dismiss_tutorial_step(&mut self, step: TutorialStep) -> Result<()> {
        let Self {
            tutorial, projects, ..
        } = self;
        let mut database = projects.database();
        let database: &mut Database = &mut database;
        tutorial.dismiss(database, step)?;
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
        let mut database = projects.database();
        let database: &mut Database = &mut database;
        tutorial.skip_all(database, l10n)?;
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

    /// F5 当前评审会话（未进台时为 `None`）
    pub fn review(&self) -> Option<&ReviewWorkbench> {
        self.review.as_ref()
    }

    /// F5 评审工作台的可变访问（逐项判定/批量确认/暂停恢复）。
    pub fn review_mut(&mut self) -> Option<&mut ReviewWorkbench> {
        self.review.as_mut()
    }

    /// 封账：一次性取得评审工作台与 F3 共享数据库连接，终态批量写回 B2。
    ///
    /// 写回失败返回 `Err` 且封账不生效（评审状态保持可改）；无评审会话时返回 `None`。
    pub fn seal_review(
        &mut self,
    ) -> Option<review_workbench::Result<review_workbench::ExportSummary>> {
        let workbench = self.review.as_mut()?;
        let mut database = self.projects.database();
        Some(workbench.seal(&mut database))
    }
}

mod collection_source;
mod workspace_state;

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
        "highlight" => theme.set_highlight(color),
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
    fn assembly_calls_only_startup_and_workspace_routes_steps() {
        crate::production::reset_entry_calls();
        let directory = tempfile::tempdir().expect("建立临时目录");
        let databases = ShellDatabases::open(directory.path().join("assembly.db"))
            .expect("建立正式数据库连接组");
        let injector = ViewModelInjector::new(databases).expect("建立正式注入器");
        let window = AppWindow::new().expect("建立正式窗口");
        let center = Arc::new(NotificationCenter::new(PresenterRegistry::new()));

        let _runtime = assemble_application(&window, injector, center);
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

    use data_persistence::CampusCrudApi;
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
        // 校区只经高德 POI 建立（T30 删除 create_campus 业务入口）；夹具
        // 直接用 projects 连接上的 B2 原语（内存库两连接相互独立）。
        let campus = injector
            .projects_mut()
            .database()
            .create_campus("夹具大学")
            .expect("建校区");
        let campus_id = CampusId::parse(&campus.id).expect("解析校区 ID");
        let plan_id = injector
            .projects_mut()
            .create_plan(&campus_id, "第一个方案")
            .expect("建方案");
        // A1：候选采集完整用例就绪（F4/F7 在入口后协调，壳不直接持有）
        let _flow = injector.collection_flow();
        // F5：进台会话装载成功（零候选照样成台）
        injector.enter_review(&plan_id).expect("评审进台");
        let workbench = injector.review().expect("评审会话已持有");
        assert_eq!(workbench.candidate_count(), 0);
        // F9：实例可达
        let _export = injector.export_flow();
    }

    #[test]
    fn restart_tutorial_resets_progress_in_db() {
        // F2 借道 F3 连接落库；内存库即可（同一条 projects 连接）
        let mut injector = injector();
        injector.restart_tutorial().expect("重看教程");
        assert_eq!(injector.tutorial().status(), TutorialStatus::NotStarted);
        let database = injector.projects().database();
        let database: &Database = &database;
        let reloaded = OnboardingTutorial::load(database).expect("重新装载引导进度");
        assert_eq!(reloaded.status(), TutorialStatus::NotStarted);
    }
}
