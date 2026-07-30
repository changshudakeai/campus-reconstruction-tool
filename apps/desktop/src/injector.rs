//! T19B-1 —— 迁移期 VM 注入器。
//!
//! 当前实现持有多个功能与基础模块并承担跨模块接线；工单 01 仅将其作为现有用户
//! 可观察行为的来源保留。按照 ADR-0037，目标态的一次用户操作只转发给一个功能
//! 模块，S1 只绑定页面状态、进度、导航结果与通知。本文件的协调职责是后续工单
//! 待迁出的遗留，不构成继续扩展 S1 业务逻辑的授权。
// ignore-tidy-filelength: 核心注入器，承载全部 F 模块接线与 B5 边界编辑器状态同步，拆分会增加跨模块耦合

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use coverage_audit::QuietSentinel;
use data_acquisition::AcquisitionPipeline;
use data_persistence::Database;
use export_console::{ExportConsole, MockSealGate};
use foundation_mode::{
    validate_polygon_closure, BoundaryDrawer, BoundaryState, BoundaryUiEvent, CoordinateConverter,
    EventResult, MercatorCoord, OrientationCalculator, OrientationLine, Point2D, Vertex,
};
use gaode_client::{BoundarySorter, IpcMessage};
use global_settings::{FirstRunSetup, SettingsManager};
use localization::{Language, Localization};
use onboarding_tutorial::{OnboardingTutorial, TutorialStep};
use project_management::ProjectManager;
use review_workbench::ReviewWorkbench;
use shared_domain_types::{CampusId, PlanId};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::dispatch::report_callback_error;
use crate::production::ProductionEntries;
use crate::runtime::{decide, status_text, LandingDecision};
use crate::{AppWindow, BoundaryPointData, OrientationPointData};

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
    // T19B-5B: B5 地基模式 (边界绘制状态机)
    boundary_drawer: BoundaryDrawer,
    // T19B-5B Step B/C: 按方案隔离的进度状态内存模型（不落 DB）
    plan_progress_states: std::collections::HashMap<String, PlanProgressState>,
    // 当前激活方案的 plan_id（用于多方案切换时查进度）
    active_plan_id: Option<String>,
    // T19B-5B Step B: 朝向交互状态（UI 局部状态）
    orientation_points: Vec<(f64, f64)>,
    orientation_angle: Option<f32>,
    // P0-3: 等待确认的朝向值（用于 recalc 确认窗）
    pending_orientation_angle: Option<f32>,
    // T22: 空 Gaode Key 引导至设置页（confirm dialog 模式）
    pending_gaode_redirect: bool,
}

/// 单方案进度状态（内存模型，Step C 新增；T25 扩展 per-plan 朝向与边界坐标）
#[derive(Debug, Clone, Default)]
struct PlanProgressState {
    has_boundary: bool,
    has_orientation: bool,
    // T25: 已确认朝向角度（per-plan，切方案时恢复）
    orientation_angle: Option<f32>,
    // T25: 已确认边界 GCJ-02 坐标（per-plan，朝向模式半透明参照）
    boundary_gcj02: Option<Vec<[f64; 2]>>,
}

impl PlanProgressState {
    /// 派生 completed_steps = has_boundary as u8 + has_orientation as u8
    fn completed_steps(&self) -> u8 {
        self.has_boundary as u8 + self.has_orientation as u8
    }
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
            boundary_drawer: BoundaryDrawer::new(),
            plan_progress_states: std::collections::HashMap::new(),
            active_plan_id: None,
            orientation_points: Vec::new(),
            orientation_angle: None,
            pending_orientation_angle: None,
            pending_gaode_redirect: false,
        })
    }

    /// 把 VM 视图状态注入 Slint 窗口（只设 in property）。
    ///
    /// T19B-2 起覆盖：首开文案 + 首跑向导（屏 0）+ 设置页文案（屏 3）+
    /// B7 弹窗静态文案；T19B-3 校区选择页文案；T19B-4 方案列表页文案。
    pub(crate) fn inject(&self, window: &AppWindow) {
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

        // 屏 4 步骤①：圈边界编辑器文案（T19B-5B Phase 2 Step A）
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

        // 屏 4 步骤②：定朝向交互页文案（T19B-5B Phase 2 Step B）
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
        // P1-4: 模式切换按钮文案
        window.set_workspace_orientation_mode_two_points_label(
            l10n.t("orientation.mode_two_points").into(),
        );
        window.set_workspace_orientation_mode_bearing_angle_label(
            l10n.t("orientation.mode_bearing_angle").into(),
        );

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

        // ── T19B-5B Step C: 方案工作区初始值（内存进度） ────────────────
        // completed-steps 由 active_plan 的派生状态决定，此处占位
        let current_progress = self.current_plan_completed_steps();
        window.set_workspace_completed_steps(current_progress.into());

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
        window.set_trash_page_campus_prefix((l10n.t("domain.campus").to_string() + ":").into());
        window.set_trash_page_date_today(l10n.t("notice.date_today").into());

        // ── T22: 高德地图密钥设置──────────────────────
        window.set_gaode_group_title(l10n.t("settings.gaode_group_title").into());
        window.set_gaode_api_key_label(l10n.t("settings.gaode_api_key_label").into());
        window.set_gaode_api_key_placeholder(l10n.t("settings.gaode_api_key_placeholder").into());
        window.set_gaode_security_key_label(l10n.t("settings.gaode_security_key_label").into());
        window.set_gaode_security_key_placeholder(
            l10n.t("settings.gaode_security_key_placeholder").into(),
        );
        window.set_gaode_save_button_label(l10n.t("settings.gaode_save_button").into());
        window.set_gaode_test_button_label(l10n.t("settings.gaode_test_button").into());
        window.set_gaode_status_message(SharedString::new());

        let api_key = self
            .settings
            .gaode_api_key()
            .unwrap_or(None)
            .unwrap_or_default();
        let security_key = self
            .settings
            .gaode_security_key()
            .unwrap_or(None)
            .unwrap_or_default();
        window.set_gaode_api_key(api_key.into());
        window.set_gaode_security_key(security_key.into());

        // T21: 高德地图嵌入探针初始化 (直接读文件)
        let _t21_probe_info = "T21 REDO: gaode-demo-keys.txt 路径：%LOCALAPPDATA%\\MCRebuildV2\\dev\\gaode-demo-keys.txt";
    }

    /// 同步边界绘制器状态到 Slint 显示模型
    fn sync_boundary_display(&self, window: &AppWindow) {
        let vertices = self.boundary_drawer.vertices();
        let is_closed = matches!(self.boundary_drawer.state(), BoundaryState::Determined);

        // 计算点显示数据（每个点偏移 -5 以居中渲染 10px 圆点）
        let points: Vec<BoundaryPointData> = vertices
            .iter()
            .map(|v| BoundaryPointData {
                x: v.x as f32 - 5.0,
                y: v.y as f32 - 5.0,
            })
            .collect();

        // 构建 SVG path 命令字符串（连接连续顶点，闭合时加 Z 闭合）
        let path_commands = Self::build_path_commands(vertices, is_closed);

        window.set_workspace_boundary_points(ModelRc::new(VecModel::from(points)));
        window.set_workspace_boundary_path_commands(path_commands.into());
        window.set_workspace_boundary_point_count(vertices.len() as i32);
        window.set_workspace_boundary_is_determined(is_closed);

        // 更新状态文案
        let status = match self.boundary_drawer.state() {
            BoundaryState::Idle => self.l10n.t("boundary.status_idle").into(),
            BoundaryState::Drawing => {
                let count_str = vertices.len().to_string();
                self.l10n
                    .t_with_array("boundary.status_drawing", &[&count_str])
                    .into()
            }
            BoundaryState::Determined => self.l10n.t("boundary.status_determined").into(),
            BoundaryState::Editing { .. } => self.l10n.t("boundary.status_editing").into(),
        };
        window.set_workspace_boundary_status(status);
    }

    /// 构建 SVG path 命令字符串（用于 Slint Path 元素渲染连线）
    fn build_path_commands(vertices: &[Vertex], is_closed: bool) -> String {
        if vertices.is_empty() {
            return String::new();
        }
        let mut commands = format!("M {} {}", vertices[0].x, vertices[0].y);
        for v in &vertices[1..] {
            commands.push_str(&format!(" L {} {}", v.x, v.y));
        }
        if is_closed && vertices.len() >= 3 {
            commands.push_str(" Z");
        }
        commands
    }

    /// P1-5: 构建方向箭头 Path commands (三角形，按角度旋转)
    ///
    /// 罗盘中心 (50, 50)，箭头从中心向外指，角度 0°=正北(上)，顺时针增加。
    fn build_arrow_commands(angle: f32) -> String {
        let rad = angle.to_radians();
        let cx = 50.0_f32;
        let cy = 50.0_f32;
        let r = 40.0_f32; // 箭头长度
        let base_r = 25.0_f32; // 箭头基底距离
        let half_w = 8.0_f32; // 箭头基底半宽

        // 箭头尖端
        let tip_x = cx + r * rad.sin();
        let tip_y = cy - r * rad.cos();

        // 箭头基底左右两点 (垂直于箭头方向)
        let base_left_x = cx + base_r * rad.sin() + half_w * rad.cos();
        let base_left_y = cy - base_r * rad.cos() + half_w * rad.sin();
        let base_right_x = cx + base_r * rad.sin() - half_w * rad.cos();
        let base_right_y = cy - base_r * rad.cos() - half_w * rad.sin();

        format!("M {tip_x:.1} {tip_y:.1} L {base_left_x:.1} {base_left_y:.1} L {base_right_x:.1} {base_right_y:.1} Z")
    }

    /// 同步朝向交互状态到 Slint 显示模型 (T19B-5B Step B)
    fn sync_orientation_display(&self, window: &AppWindow) {
        // 点显示数据 (偏移 -6 以居中渲染 12px 圆点)
        let points: Vec<OrientationPointData> = self
            .orientation_points
            .iter()
            .map(|(x, y)| OrientationPointData {
                x: *x as f32 - 6.0,
                y: *y as f32 - 6.0,
            })
            .collect();
        window.set_workspace_orientation_points(ModelRc::new(VecModel::from(points)));

        // 连线 path (两点之间)
        let path = if self.orientation_points.len() >= 2 {
            let (x0, y0) = self.orientation_points[0];
            let (x1, y1) = self.orientation_points[1];
            format!("M {x0} {y0} L {x1} {y1}")
        } else {
            String::new()
        };
        window.set_workspace_orientation_path_commands(path.into());

        // 角度值：优先用当前方案的已确认角度；否则用工作区临时角度
        let confirmed_angle = self
            .active_plan_id
            .as_ref()
            .and_then(|id| self.plan_orientation_angle(id));
        let angle = confirmed_angle.or(self.orientation_angle).unwrap_or(-1.0);
        window.set_workspace_orientation_angle(angle);
        // Step C: is_determined 由方案进度状态派生
        window.set_workspace_orientation_is_determined(self.current_plan_has_orientation());

        // 角度显示文案
        let angle_display = if angle >= 0.0 {
            format!("{:.1}\u{00b0}", angle)
        } else {
            String::new()
        };
        window.set_workspace_orientation_angle_display(angle_display.into());

        // P1-5: 方向箭头 Path commands (按角度计算三角形)
        let arrow_commands = if angle >= 0.0 {
            Self::build_arrow_commands(angle)
        } else {
            String::new()
        };
        window.set_workspace_orientation_arrow_commands(arrow_commands.into());

        // 状态文案
        let status = if self.current_plan_has_orientation() {
            self.l10n.t("orientation.status_determined").into()
        } else if self.orientation_points.is_empty() {
            self.l10n.t("orientation.status_idle").into()
        } else if self.orientation_points.len() == 1 {
            self.l10n.t("orientation.status_first_point").into()
        } else {
            self.l10n.t("orientation.status_calculated").into()
        };
        window.set_workspace_orientation_status(status);
    }

    /// 把页面回调绑到 VM（T19B-2：向导完成 + 重看教程；T19B-3：校区选择；
    /// T19B-4：方案列表 CRUD + 教程气泡钩子）。
    ///
    /// 回调闭包持 `Rc<RefCell<Self>>` 共享可变访问（Slint 单线程 UI）；
    /// 回调错误一律递 [`report_callback_error`]（弹窗铁律 ADR-0021）。
    pub(crate) fn bind(
        injector: &Rc<RefCell<Self>>,
        presentation: &Rc<RefCell<ProductionEntries>>,
        window: &AppWindow,
    ) {
        // 完成设置：读窗口选项 → F1 complete_first_run → 重判着陆跳下一屏
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let presentation_startup = Rc::clone(presentation);
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
                    crate::map_webview::hide();
                    drop(injector);
                    presentation_startup.borrow_mut().show_startup(&window);
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

        // ── T22: 高德地图密钥保存与测试──────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_gaode_save_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let api_key = window.get_gaode_api_key().to_string();
            let security_key = window.get_gaode_security_key().to_string();
            let mut injector = shared.borrow_mut();
            match injector.settings_mut().set_gaode_api_key(&api_key) {
                Ok(()) => {}
                Err(err) => {
                    report_callback_error(injector.l10n(), &err);
                    return;
                }
            }
            match injector
                .settings_mut()
                .set_gaode_security_key(&security_key)
            {
                Ok(()) => {
                    notification_center::info(
                        "应用",
                        "高德地图密钥已保存",
                        "请在需要地图功能的页面测试使用",
                    );
                }
                Err(err) => report_callback_error(injector.l10n(), &err),
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_gaode_test_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let api_key = window.get_gaode_api_key().to_string();
            let security_key = window.get_gaode_security_key().to_string();
            let mut injector = shared.borrow_mut();
            match injector
                .settings_mut()
                .test_gaode_connection(&api_key, &security_key)
            {
                Ok(()) => {
                    notification_center::info(
                        "高德地图密钥验证",
                        "连通性正常",
                        "高德地图 API 可正常使用",
                    );
                }
                Err(err) => {
                    report_callback_error(injector.l10n(), &err);
                }
            }
        });

        // T21: 高德地图嵌入探针初始化 (placeholder，待 Slint 1.17+ 升级)
        // ↻ 集成到 notification-center → T24 完成

        // ── T19B-3/T19B-5：校区选择页回调 ────────────────────────
        Self::bind_campus_select(injector, presentation, window);
        // ── T19B-4：方案列表页回调 ────────────────────────────
        Self::bind_plan_list(injector, presentation, window);
        // ── T19B-9: 右上角工具栏回调 ─────────────────────
        Self::bind_toolbar(presentation, window);
        // ── T19B-5B: 方案工作区回调绑定（Phase 1）───────────────────
        Self::bind_workspace(injector, presentation, window);
        // ── T24: 边界地图 WebView IPC 桥接注册 ─────────────────────
        Self::bind_boundary_map_ipc(injector, window);
    }

    /// T24：注册边界地图 WebView 的 IPC 处理器。
    ///
    /// 消息分发（B3 `parse_ipc_message` 解析；业务计算委托 B5）：
    /// - OsmElements → B5 BoundarySorter 排序选取 → 公告栏告知选中
    /// - ConfirmBoundary → 经纬度→平面米（B5 CoordinateConverter）→
    ///   B5 校验（闭合/面积/自相交）→ 步骤条打勾；失败 → B7 弹窗
    /// - BoundaryUpdate → 暂存编辑中坐标（壳只桥接，不做计算）
    /// - ManualPoint/Cancel/Clear → 人工圈画累积点同步到 Slint 画布状态
    /// - Error/未知 → 公告栏提示（非弹窗，非阻塞，ADR-0021 分级）
    fn bind_boundary_map_ipc(injector: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        crate::map_webview::register_ipc_handler(Rc::new(move |msg: &str| {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let Ok(parsed) = gaode_client::parse_ipc_message(msg) else {
                return; // 畸形载荷静默丢弃（B3 解析层已尽力）
            };
            match parsed {
                IpcMessage::OsmElements { elements } => {
                    // B5 排序选取（含锚点 → 名称匹配 → 距离最近；无候选列表，ADR-0029）
                    let (anchor_lon, anchor_lat) = injector.map_anchor();
                    let campus_name = injector.current_campus_name();
                    let sorted = BoundarySorter::sort_candidates(
                        elements,
                        anchor_lon,
                        anchor_lat,
                        campus_name.as_deref(),
                    );
                    match sorted.into_iter().next() {
                        Some(best) => {
                            let name =
                                best.element.tags.get("name").cloned().unwrap_or_else(|| {
                                    injector.l10n().t("boundary.unknown_campus")
                                });
                            match best.element.geometry {
                                Some(coords) => {
                                    let count = coords.len();
                                    // Rust→JS：选中的 WGS-84 坐标发回 JS，
                                    // 经 AMap.convertFrom 转 GCJ-02 后上屏
                                    // （红线：未转换坐标禁止上屏）
                                    let coords_json = serde_json::to_string(&coords)
                                        .unwrap_or_else(|_| "[]".to_string());
                                    let name_json = serde_json::to_string(&name)
                                        .unwrap_or_else(|_| "\"未知校区\"".to_string());
                                    crate::map_webview::evaluate_script(&format!(
                                        "convertAndDraw({coords_json}, {name_json});"
                                    ));
                                    let body = injector.l10n().t_with_array(
                                        "boundary.osm_auto_selected_body",
                                        &[&name, &count.to_string()],
                                    );
                                    notification_center::info(
                                        injector.l10n().t("collection.boundary_step"),
                                        injector.l10n().t("boundary.osm_auto_selected_title"),
                                        body,
                                    );
                                }
                                None => {
                                    // 候选无坐标数据 → JS 切人工圈画兜底
                                    crate::map_webview::evaluate_script("enableManualMode();");
                                    notification_center::info(
                                        injector.l10n().t("collection.boundary_step"),
                                        injector.l10n().t("boundary.osm_no_geometry_title"),
                                        injector.l10n().t("boundary.osm_no_geometry_body"),
                                    );
                                }
                            }
                        }
                        None => {
                            // 无匹配要素 → JS 切人工圈画兜底（非弹窗，非阻塞）
                            crate::map_webview::evaluate_script("enableManualMode();");
                            notification_center::info(
                                injector.l10n().t("collection.boundary_step"),
                                injector.l10n().t("boundary.osm_not_found_title"),
                                injector.l10n().t("boundary.osm_not_found_body"),
                            );
                        }
                    }
                }
                IpcMessage::ConfirmBoundary { coords } => {
                    injector.confirm_map_boundary(&window, &coords);
                }
                IpcMessage::BoundaryUpdate { coords: _ } => {
                    // No-op: 编辑中坐标不回存，确认时以载荷为准
                }
                IpcMessage::ManualPoint { lon: _, lat: _ } => {
                    // No-op: 人工落点在 JS 侧累积，确认时以载荷为准
                }
                IpcMessage::ManualCancel => {
                    // No-op
                }
                IpcMessage::ManualClear => {
                    // No-op
                }
                IpcMessage::Coordinate {
                    longitude: _,
                    latitude: _,
                } => {
                    // No-op: manual points are JS-side only
                }
                IpcMessage::Error { message } => {
                    // 公告栏提示（非弹窗、非阻塞，ADR-0021 分级）
                    notification_center::info(
                        injector.l10n().t("collection.boundary_step"),
                        injector.l10n().t("boundary.map_notice_title"),
                        message,
                    );
                }
                // T25: 朝向相关
                IpcMessage::OrientationPoints { points: _ } => {
                    // JS 已上报两点坐标，等待 confirm 后再计算
                    // 不立即处理，等 confirm_orientation
                }
                IpcMessage::ConfirmOrientation { points } => {
                    // B5 计算：两点 → 方位角
                    let (x0, y0) = (points[0][0], points[0][1]);
                    let (x1, y1) = (points[1][0], points[1][1]);

                    match OrientationLine::new(Point2D::new(x0, y0), Point2D::new(x1, y1))
                        .and_then(|line| OrientationCalculator::calculate(&line))
                    {
                        Some(orientation) => {
                            let degree = orientation.degree();
                            // T25: 按方案保存已确认朝向角度
                            if let Some(plan_id) = injector.active_plan_id.clone() {
                                let has_boundary = injector.current_plan_has_boundary();
                                injector.update_plan_progress(&plan_id, has_boundary, true);
                                injector.set_plan_orientation_angle(&plan_id, Some(degree));
                            }
                            // Rust→JS: 返回角度值供显示
                            crate::map_webview::evaluate_script(&format!(
                                "window.calculatedOrientationAngle = {};",
                                degree
                            ));
                            // 同步 UI
                            injector.sync_workspace_progress(&window);
                            injector.sync_orientation_display(&window);
                        }
                        None => {
                            report_callback_error(
                                injector.l10n(),
                                &injector.l10n().t("orientation.error_coincident_points"),
                            );
                        }
                    }
                }
                IpcMessage::OrientationClear => {
                    injector.orientation_points.clear();
                    // T25: 清除当前工作角度；按方案清除已确认角度
                    injector.orientation_angle = None;
                    if let Some(plan_id) = injector.active_plan_id.clone() {
                        let has_boundary = injector.current_plan_has_boundary();
                        injector.update_plan_progress(&plan_id, has_boundary, false);
                        injector.set_plan_orientation_angle(&plan_id, None);
                    }
                    injector.sync_orientation_display(&window);
                    injector.sync_workspace_progress(&window);
                }
            }
        }));
    }

    /// T19B-3/T19B-5：校区选择页回调绑定。
    ///
    /// 新建演示校区 → create_campus 刷新列表；点列表项 → remember_campus →
    /// 刷新方案列表 → 跳屏 2；点击设置 → 跳屏 3。
    fn bind_campus_select(
        injector: &Rc<RefCell<Self>>,
        presentation: &Rc<RefCell<ProductionEntries>>,
        window: &AppWindow,
    ) {
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let presentation_new = Rc::clone(presentation);
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
                        // 自动进入方案列表
                        crate::map_webview::hide();
                        drop(injector);
                        presentation_new.borrow_mut().show_plan_list(&window);
                    }
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        let weak = window.as_weak();
        let presentation_settings = Rc::clone(presentation);
        window.on_campus_select_settings_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            crate::map_webview::hide();
            presentation_settings.borrow_mut().show_settings(&window);
        });

        // 单击已有校区行（T19B-5B 补接）：remember_campus → 刷新方案列表 → 跳屏 2
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let presentation_plan = Rc::clone(presentation);
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
            crate::map_webview::hide();
            drop(injector);
            presentation_plan.borrow_mut().show_plan_list(&window);
        });
    }

    /// T19B-4/T19B-5A：方案列表页回调绑定。
    ///
    /// 新建方案（ADR-0010 轻创建对话框）/ 返回校区选择 / ···菜单操作
    /// （改名/复制/删除，ADR-0018 §三）/ 教程气泡钩子（F2 规矩①②）。
    fn bind_plan_list(
        injector: &Rc<RefCell<Self>>,
        presentation: &Rc<RefCell<ProductionEntries>>,
        window: &AppWindow,
    ) {
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
        let presentation_campus = Rc::clone(presentation);
        window.on_plan_list_back_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            crate::map_webview::hide();
            presentation_campus.borrow_mut().show_campus_select(&window);
        });

        // 单击方案卡片（ADR-0027 第 6 轮：单击即开，无概览层）→ 跳屏 4 工作区
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_plan_list_card_clicked(move |plan_id| {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            // Step C: 设置当前激活方案 ID，隔离多方案进度状态
            injector.set_active_plan(Some(&plan_id));
            // 记录当前方案 ID 供后续接线单使用
            window.set_active_plan_id(plan_id.clone());
            window.set_workspace_active_step(0);
            window.set_active_screen(4);
            // T24: 进入屏 4 → 显示边界地图 WebView（密钥只经 F1；
            // 空密钥则跳过创建，Slint 画布人工圈画兜底仍可用）
            {
                let api_key = injector
                    .settings()
                    .gaode_api_key()
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let security_key = injector
                    .settings()
                    .gaode_security_key()
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                if !api_key.is_empty() {
                    let (anchor_lon, anchor_lat) = injector.map_anchor();
                    crate::map_webview::show(
                        window.as_weak(),
                        api_key,
                        security_key,
                        anchor_lon,
                        anchor_lat,
                    );
                }
            }
            // Step C: 同步当前方案的进度到步骤条
            injector.sync_workspace_progress(&window);
            // F2 步骤条气泡钩子（规矩③"只教一次"：已见过则返回 None）
            if let Some(bubble) = injector
                .tutorial()
                .bubble_for(TutorialStep::StepperIntro, injector.l10n())
            {
                window.set_workspace_tutorial_text(bubble.message.into());
                window.set_workspace_tutorial_dismiss_label(bubble.dismiss_label.into());
                window.set_workspace_tutorial_skip_all_label(
                    bubble.skip_all_label.unwrap_or_default().into(),
                );
                window.set_workspace_tutorial_visible(true);
            } else {
                window.set_workspace_tutorial_visible(false);
            }
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
        let presentation_duplicate = Rc::clone(presentation);
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
                    drop(injector);
                    presentation_duplicate.borrow_mut().show_plan_list(&window);
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        // 删除（ADR-0018 §三）：先弹确认窗，确认后进回收站
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let shared_confirmed = Rc::clone(injector);
        let shared_cancel = Rc::clone(injector);
        let presentation_confirmed = Rc::clone(presentation);
        window.on_plan_list_delete_clicked(move |plan_id_str| {
            let Some(window) = weak.upgrade() else { return };
            let injector = shared.borrow();
            // 设置确认窗文案
            window.set_confirm_dialog_title(injector.l10n().t("dialog.delete_title").into());
            window.set_confirm_dialog_body(injector.l10n().t("plan.delete_confirm").into());
            window.set_active_plan_id(plan_id_str);
            window.set_confirm_dialog_visible(true);
        });

        // ── 确认窗回调（T19B-5A + P0-3 朝向重算 + T22 Gaode Key 引导）───────────────
        // 确认删除：调 F3 delete_plan（保留 30 天），或应用朝向重算值，或跳转 Gaode 设置
        let weak = window.as_weak();
        window.on_confirm_dialog_confirmed(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            let mut injector = shared_confirmed.borrow_mut();

            // T22: Gaode Key 空值引导至设置页
            if std::mem::replace(&mut injector.pending_gaode_redirect, false) {
                crate::map_webview::hide();
                drop(injector);
                presentation_confirmed.borrow_mut().show_settings(&window);
                return;
            }

            // P0-3: 如果有待应用的朝向值，优先处理
            if let Some(pending_angle) = injector.pending_orientation_angle.take() {
                // 应用新朝向值
                injector.orientation_angle = Some(pending_angle);
                // T25: 按方案保存已确认朝向角度
                if let Some(plan_id) = injector.active_plan_id.clone() {
                    let has_boundary = injector.current_plan_has_boundary();
                    injector.update_plan_progress(&plan_id, has_boundary, true);
                    injector.set_plan_orientation_angle(&plan_id, Some(pending_angle));
                }
                injector.sync_workspace_progress(&window);
                injector.sync_orientation_display(&window);
                return;
            }

            // 无 pending state → 删除计划
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
                Ok(_) => {
                    drop(injector);
                    presentation_confirmed.borrow_mut().show_plan_list(&window);
                }
                Err(error) => report_callback_error(injector.l10n(), &error),
            }
        });

        // 取消删除 / 取消朝向重算 / 取消 Gaode Key 引导
        let weak = window.as_weak();
        window.on_confirm_dialog_cancelled(move || {
            let Some(window) = weak.upgrade() else { return };
            window.set_confirm_dialog_visible(false);
            // P0-3: 重置 pending state（如果已设置）
            // T22: 同时重置 Gaode Key 引导标志
            if let Ok(mut injector) = shared_cancel.try_borrow_mut() {
                injector.pending_orientation_angle = None;
                injector.pending_gaode_redirect = false;
            }
        });

        // ── 输入窗回调（T19B-5A）───────────────────────────
        // 确认输入：根据 mode 分派新建/改名
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let presentation_input = Rc::clone(presentation);
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
                            drop(injector);
                            presentation_input.borrow_mut().show_plan_list(&window);
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
                            drop(injector);
                            presentation_input.borrow_mut().show_plan_list(&window);
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
    fn bind_toolbar(presentation: &Rc<RefCell<ProductionEntries>>, window: &AppWindow) {
        let weak = window.as_weak();

        // 公告栏入口：跳屏 5
        let weak_clone = weak.clone();
        let presentation_notice = Rc::clone(presentation);
        window.on_notice_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                crate::map_webview::hide();
                presentation_notice.borrow_mut().show_notifications(&window);
            }
        });

        // 切换校区入口：跳屏 1
        let weak_clone = weak.clone();
        let presentation_campus = Rc::clone(presentation);
        window.on_switch_campus_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                crate::map_webview::hide();
                presentation_campus.borrow_mut().show_campus_select(&window);
            }
        });

        // 回收站入口：跳屏 6
        let weak_clone = weak.clone();
        window.on_trash_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                crate::map_webview::hide();
                window.set_active_screen(6);
            }
        });

        // 设置入口：跳屏 3
        let weak_clone = weak.clone();
        let presentation_settings = Rc::clone(presentation);
        window.on_settings_toolbar_button_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                crate::map_webview::hide();
                presentation_settings.borrow_mut().show_settings(&window);
            }
        });

        // 设置页返回：经校区与方案入口刷新校区选择页
        let weak_clone = weak.clone();
        let presentation_campus_back = Rc::clone(presentation);
        window.on_settings_back_clicked(move || {
            if let Some(window) = weak_clone.upgrade() {
                crate::map_webview::hide();
                presentation_campus_back
                    .borrow_mut()
                    .show_campus_select(&window);
            }
        });
    }

    // ── T19B-5B: 方案工作区回调绑定（Phase 1）───────────────────────
    fn bind_workspace(
        injector: &Rc<RefCell<Self>>,
        presentation: &Rc<RefCell<ProductionEntries>>,
        window: &AppWindow,
    ) {
        // 步骤点击：上锁步骤不可点击（ADR-0027：前跳上锁，回跳自由，第①格永远解锁）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        let presentation_steps = Rc::clone(presentation);
        window.on_workspace_step_clicked(move |step_index| {
            let Some(window) = weak.upgrade() else { return };
            let completed = window.get_workspace_completed_steps();
            if step_index > completed {
                return; // 锁定步骤忽略点击
            }

            match step_index {
                2 => {
                    presentation_steps.borrow_mut().show_collection(&window);
                    return;
                }
                3 => {
                    presentation_steps.borrow_mut().show_review(&window);
                    return;
                }
                4 => {
                    presentation_steps.borrow_mut().show_export(&window);
                    return;
                }
                _ => {}
            }

            // T22: 空 Gaode Key 检测与引导（步骤①：圈边界编辑器）
            if step_index == 0 {
                let injector_ref = shared.borrow();
                let api_key = injector_ref
                    .settings()
                    .gaode_api_key()
                    .unwrap_or(None)
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);
                drop(injector_ref);

                if !api_key {
                    // 无密钥：弹确认窗引导至设置页
                    let mut injector_mut = shared.borrow_mut();
                    window.set_confirm_dialog_title(
                        injector_mut
                            .l10n()
                            .t("settings.gaode_empty_key_title")
                            .into(),
                    );
                    window.set_confirm_dialog_body(
                        injector_mut
                            .l10n()
                            .t("settings.gaode_empty_key_body")
                            .into(),
                    );
                    window.set_confirm_dialog_confirm_label(
                        injector_mut
                            .l10n()
                            .t("settings.gaode_go_to_settings")
                            .into(),
                    );
                    window.set_confirm_dialog_cancel_label(
                        injector_mut.l10n().t("app.cancel_button").into(),
                    );
                    injector_mut.pending_gaode_redirect = true;
                    window.set_confirm_dialog_visible(true);
                    return; // 不激活步骤
                }

                // T25: 进入步骤①后显示边界编辑地图
                {
                    let injector_ref = shared.borrow();
                    let api_key = injector_ref
                        .settings()
                        .gaode_api_key()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let security_key = injector_ref
                        .settings()
                        .gaode_security_key()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    if !api_key.is_empty() {
                        let (anchor_lon, anchor_lat) = injector_ref.map_anchor();
                        crate::map_webview::show(
                            window.as_weak(),
                            api_key,
                            security_key,
                            anchor_lon,
                            anchor_lat,
                        );
                    }
                }
            }

            // T25: 进入步骤②时显示朝向模式地图（带已确认边界作为参照）
            if step_index == 1 {
                let injector_ref = shared.borrow();
                let api_key = injector_ref
                    .settings()
                    .gaode_api_key()
                    .unwrap_or(None)
                    .map(|k| !k.is_empty())
                    .unwrap_or(false);
                drop(injector_ref);

                if api_key {
                    // 重新创建 WebView 用于朝向模式
                    crate::map_webview::hide();

                    let injector_mut = shared.borrow_mut();
                    let api_key = injector_mut
                        .settings()
                        .gaode_api_key()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let security_key = injector_mut
                        .settings()
                        .gaode_security_key()
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let (anchor_lon, anchor_lat) = injector_mut.map_anchor();

                    // T25: 取当前方案已确认边界的 GCJ-02 坐标作为朝向模式参照
                    let existing_boundary_gcj02 = injector_mut
                        .active_plan_id
                        .as_ref()
                        .and_then(|id| injector_mut.plan_boundary_gcj02(id));

                    use gaode_client::BoundaryEditPageConfig;
                    let config = BoundaryEditPageConfig::new(&api_key, &security_key)
                        .with_anchor(anchor_lon, anchor_lat)
                        .with_orientation_mode(true)
                        .with_existing_boundary(existing_boundary_gcj02);

                    // Show will rebuild with this config
                    // For now use default constructor - future refactor needed
                    crate::map_webview::show_with_config(window.as_weak(), config);
                }
            }

            window.set_workspace_active_step(step_index);
        });

        // 步骤条教程气泡“知道了”（F2 规矩①：记已见并落库，此后不再显示）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_tutorial_dismiss_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            if let Err(error) = injector.dismiss_tutorial_step(TutorialStep::StepperIntro) {
                report_callback_error(injector.l10n(), &error);
            }
            window.set_workspace_tutorial_visible(false);
        });

        // 步骤条教程气泡“跳过全部引导”（F2 规矩②）
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_tutorial_skip_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            if let Err(error) = injector.skip_all_tutorial() {
                report_callback_error(injector.l10n(), &error);
            }
            window.set_workspace_tutorial_visible(false);
        });

        // ── 屏 4 步骤①：圈边界编辑器回调（T19B-5B Phase 2 Step A）────────
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_boundary_canvas_clicked(move |x, y| {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            // B5 handle_event: ClickAt → 添加顶点 (Slint f32 → B5 f64)
            match injector
                .boundary_drawer_mut()
                .handle_event(BoundaryUiEvent::ClickAt {
                    x: x as f64,
                    y: y as f64,
                }) {
                EventResult::Accepted => {}
                EventResult::Rejected(msg) => report_callback_error(injector.l10n(), &msg),
                EventResult::Ignored => {}
            }
            injector.sync_boundary_display(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_boundary_undo_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            // B5 handle_event: Cancel → 撤销最后一点
            if let EventResult::Rejected(msg) = injector
                .boundary_drawer_mut()
                .handle_event(BoundaryUiEvent::Cancel)
            {
                report_callback_error(injector.l10n(), &msg);
            }
            injector.sync_boundary_display(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_boundary_confirm_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();
            let vertices = injector.boundary_drawer().vertices().to_vec();

            // 先验证再确认 (ADR-0021 + B7 弹窗铁律)
            let validation_result = validate_polygon_closure(&vertices);
            if !validation_result.is_valid {
                // 不合法：报错但不消耗 Confirm 事件 (drawer 保持 Drawing 状态)
                let error_detail: String = validation_result
                    .errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ");
                report_callback_error(injector.l10n(), &error_detail);
                return;
            }

            // 合法：调用 B5 Confirm 设置状态为 Determined
            match injector
                .boundary_drawer_mut()
                .handle_event(BoundaryUiEvent::Confirm)
            {
                EventResult::Accepted => {
                    // Step C: 边界确认成功 → 更新方案进度 has_boundary = true
                    if let Some(plan_id) = injector.active_plan_id.clone() {
                        let has_orientation = injector.current_plan_has_orientation();
                        injector.update_plan_progress(&plan_id, true, has_orientation);
                    }
                    injector.sync_workspace_progress(&window);
                }
                EventResult::Rejected(msg) => report_callback_error(injector.l10n(), &msg),
                EventResult::Ignored => {}
            }
            injector.sync_boundary_display(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_boundary_reset_clicked(move || {
            let mut injector = shared.borrow_mut();
            // Reset → 清空所有顶点
            injector.boundary_drawer_mut().reset();
            // Step C: 边界重置 → 更新方案进度 has_boundary = false
            // T25: 同时清除当前方案的 GCJ-02 边界坐标
            if let Some(plan_id) = injector.active_plan_id.clone() {
                let has_orientation = injector.current_plan_has_orientation();
                injector.update_plan_progress(&plan_id, false, has_orientation);
                injector.set_plan_boundary_gcj02(&plan_id, None);
            }
            // 同步 UI
            {
                let Some(window) = weak.upgrade() else { return };
                injector.sync_boundary_display(&window);
                injector.sync_workspace_progress(&window);
            }
        });

        // ── 屏 4 步骤②：定朝向回调（T19B-5B Phase 2 Step B）────────────
        // NOTE: T25 - 使用高德地图而非纯画布。方向点击由 map IPC 处理，本处仅做门控状态切换
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_orientation_canvas_clicked(move |x: f32, y: f32| {
            // T25: 地图模式已接管该交互，Slint 画布点击为 NO-OP
            // 此处保留参数避免未使用警告
            let _ = (x, y);
            let _ = weak;
            let _ = shared;
        });

        // 提交按钮：角度输入模式下校验并设定朝向
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_orientation_submit_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            let mut injector = shared.borrow_mut();

            let mode = window.get_workspace_orientation_mode().to_string();
            if mode == "bearing-angle" {
                // 角度输入模式：从 LineEdit 读取文本 → B5 normalize_angle 校验
                let text = window.get_workspace_orientation_input_text().to_string();
                let angle: f32 = match text.trim().parse() {
                    Ok(v) => v,
                    Err(_) => {
                        // P0-1/2: 硬编码英文 → i18n + B7 弹窗（非 toast）
                        window
                            .set_error_dialog_title(injector.l10n().t("dialog.error_title").into());
                        window.set_error_dialog_source(injector.l10n().t("app.source_tag").into());
                        window.set_error_dialog_body(
                            injector.l10n().t("orientation.error_invalid_angle").into(),
                        );
                        window.set_error_dialog_visible(true);
                        return;
                    }
                };
                match OrientationCalculator::normalize_angle(angle) {
                    Some(orientation) => {
                        // 已有朝向值时弹确认窗
                        if injector.current_plan_has_orientation() {
                            // P0-3: 保存待应用的朝向值供 confirm dialog 使用
                            injector.pending_orientation_angle = Some(orientation.degree());
                            window.set_confirm_dialog_title(
                                injector.l10n().t("orientation.recalc_title").into(),
                            );
                            window.set_confirm_dialog_body(
                                injector
                                    .l10n()
                                    .t("collection.orientation_recalc_notice")
                                    .into(),
                            );
                            window.set_confirm_dialog_visible(true);
                            return;
                        }
                        injector.orientation_angle = Some(orientation.degree());
                        // T25: 按方案保存已确认朝向角度
                        if let Some(plan_id) = injector.active_plan_id.clone() {
                            let has_boundary = injector.current_plan_has_boundary();
                            injector.update_plan_progress(&plan_id, has_boundary, true);
                            injector
                                .set_plan_orientation_angle(&plan_id, Some(orientation.degree()));
                        }
                        injector.sync_workspace_progress(&window);
                    }
                    None => {
                        // P0-1/2: 硬编码英文 → i18n + B7 弹窗（非 toast）
                        window
                            .set_error_dialog_title(injector.l10n().t("dialog.error_title").into());
                        window.set_error_dialog_source(injector.l10n().t("app.source_tag").into());
                        window.set_error_dialog_body(
                            injector
                                .l10n()
                                .t("orientation.error_angle_out_of_range")
                                .into(),
                        );
                        window.set_error_dialog_visible(true);
                        return;
                    }
                }
            } else if mode == "two-points" {
                // 两点模式：地图 IPC 已直接处理 confirm_orientation，
                // Slint 提交按钮在此处仅做 UI 同步兜底。
                if injector.orientation_points.len() == 2 && injector.orientation_angle.is_some() {
                    let angle = injector.orientation_angle;
                    if let Some(plan_id) = injector.active_plan_id.clone() {
                        let has_boundary = injector.current_plan_has_boundary();
                        injector.update_plan_progress(&plan_id, has_boundary, true);
                        injector.set_plan_orientation_angle(&plan_id, angle);
                    }
                    injector.sync_workspace_progress(&window);
                }
            }
            injector.sync_orientation_display(&window);
        });

        // 重置按钮：清空所有朝向状态
        let weak = window.as_weak();
        let shared = Rc::clone(injector);
        window.on_workspace_orientation_reset_clicked(move || {
            let mut injector = shared.borrow_mut();
            injector.orientation_points.clear();
            injector.orientation_angle = None;
            // T25: 按方案清除已确认朝向角度
            if let Some(plan_id) = injector.active_plan_id.clone() {
                let has_boundary = injector.current_plan_has_boundary();
                injector.update_plan_progress(&plan_id, has_boundary, false);
                injector.set_plan_orientation_angle(&plan_id, None);
            }
            {
                let Some(window) = weak.upgrade() else { return };
                window.set_workspace_orientation_input_text("".into());
                injector.sync_orientation_display(&window);
                injector.sync_workspace_progress(&window);
            }
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

    /// B5 边界绘制器（只读）
    pub fn boundary_drawer(&self) -> &BoundaryDrawer {
        &self.boundary_drawer
    }

    /// B5 边界绘制器（可变：事件处理与重置）
    pub fn boundary_drawer_mut(&mut self) -> &mut BoundaryDrawer {
        &mut self.boundary_drawer
    }

    // ── T19B-5B Step C: 方案进度内存模型访问器 ───────────────────────
    /// 获取当前激活方案的 completed_steps（派生值）
    fn current_plan_completed_steps(&self) -> u8 {
        self.active_plan_id
            .as_ref()
            .and_then(|id| self.plan_progress_states.get(id))
            .map(|state| state.completed_steps())
            .unwrap_or(0)
    }

    /// 当前激活方案是否已完成边界步骤
    fn current_plan_has_boundary(&self) -> bool {
        self.active_plan_id
            .as_ref()
            .and_then(|id| self.plan_progress_states.get(id))
            .map(|state| state.has_boundary)
            .unwrap_or(false)
    }

    /// 当前激活方案是否已完成朝向步骤
    fn current_plan_has_orientation(&self) -> bool {
        self.active_plan_id
            .as_ref()
            .and_then(|id| self.plan_progress_states.get(id))
            .map(|state| state.has_orientation)
            .unwrap_or(false)
    }

    /// 更新指定方案的进度状态
    fn update_plan_progress(&mut self, plan_id: &str, has_boundary: bool, has_orientation: bool) {
        let state = self
            .plan_progress_states
            .entry(plan_id.to_string())
            .or_default();
        state.has_boundary = has_boundary;
        state.has_orientation = has_orientation;
    }

    /// 方案卡片进度文案：沿用 T19B-5B 的内存进度覆盖，
    /// 在工单 03～09 迁移前保持既有列表数据结果不变。
    pub(crate) fn plan_card_progress_text(&self, plan_id: &str, fallback: &str) -> String {
        match self.plan_progress_states.get(plan_id) {
            Some(state) if state.has_boundary && state.has_orientation => {
                self.l10n.t("plan.progress_next_collection")
            }
            Some(state) if state.has_boundary => self.l10n.t("plan.progress_boundary_done"),
            _ => fallback.to_owned(),
        }
    }

    /// T25: 设置指定方案的已确认朝向角度
    fn set_plan_orientation_angle(&mut self, plan_id: &str, angle: Option<f32>) {
        let state = self
            .plan_progress_states
            .entry(plan_id.to_string())
            .or_default();
        state.orientation_angle = angle;
    }

    /// T25: 读取指定方案的已确认朝向角度
    fn plan_orientation_angle(&self, plan_id: &str) -> Option<f32> {
        self.plan_progress_states
            .get(plan_id)
            .and_then(|state| state.orientation_angle)
    }

    /// T25: 设置指定方案的已确认边界 GCJ-02 坐标
    fn set_plan_boundary_gcj02(&mut self, plan_id: &str, coords: Option<Vec<[f64; 2]>>) {
        let state = self
            .plan_progress_states
            .entry(plan_id.to_string())
            .or_default();
        state.boundary_gcj02 = coords;
    }

    /// T25: 读取指定方案的已确认边界 GCJ-02 坐标
    fn plan_boundary_gcj02(&self, plan_id: &str) -> Option<Vec<[f64; 2]>> {
        self.plan_progress_states
            .get(plan_id)
            .and_then(|state| state.boundary_gcj02.clone())
    }

    /// 设置当前激活方案 ID（用于多方案切换时隔离进度）
    fn set_active_plan(&mut self, plan_id: Option<&str>) {
        self.active_plan_id = plan_id.map(|s| s.to_string());
        // T25: 切换方案时清空工作区临时朝向状态，避免 A 方案未提交的点串到 B 方案
        self.orientation_points.clear();
        self.orientation_angle = None;
        self.pending_orientation_angle = None;
    }

    /// 同步工作区进度到 Slint 窗口（completed-steps + is_determined）
    fn sync_workspace_progress(&self, window: &AppWindow) {
        let steps = self.current_plan_completed_steps();
        window.set_workspace_completed_steps(steps.into());
        window.set_workspace_orientation_is_determined(self.current_plan_has_orientation());
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

    // ── T24: 地图边界辅助方法（壳只桥接，计算全在 B3/B5）──────────────

    /// T24/T05: 当前锚点经纬度 (lon, lat)。
    ///
    /// 改读当前校区在数据库中的锚点坐标；若当前无校区则保留北京默认锚点，
    /// 保证首次进入边界编辑页时仍有可定位中心。
    fn map_anchor(&self) -> (f64, f64) {
        self.projects
            .landing_campus()
            .ok()
            .flatten()
            .map(|c| (c.anchor_lng, c.anchor_lat))
            .unwrap_or((116.397, 39.916))
    }

    /// T24: 当前校区名（供 B5 排序器名称匹配权重）。
    fn current_campus_name(&self) -> Option<String> {
        self.projects
            .landing_campus()
            .ok()
            .flatten()
            .map(|c| c.name)
    }

    /// T24: 地图边界确认流程。
    ///
    /// GCJ-02 经纬度 →（B5 链式换算：Mercator → 平面米，多边形重心为
    /// 参考原点）→ B5 校验（闭合/面积/自相交）→ 失败 B7 弹窗（ADR-0021）；
    /// 成功 → 装载 B5 drawer 为 Determined → 步骤条打勾 → 同步工作区进度。
    fn confirm_map_boundary(&mut self, window: &AppWindow, coords: &[[f64; 2]]) {
        if coords.len() < 3 {
            report_callback_error(self.l10n(), &self.l10n().t("boundary.error_too_few_points"));
            return;
        }

        // 参考原点：多边形重心（校园尺度下经纬度算术平均足够）
        let (sum_lon, sum_lat) = coords
            .iter()
            .fold((0.0_f64, 0.0_f64), |(slon, slat), [lon, lat]| {
                (slon + lon, slat + lat)
            });
        let n = coords.len() as f64;
        let (center_lon, center_lat) = (sum_lon / n, sum_lat / n);

        // B5 链式换算：经纬度 → Mercator → 平面米（相对重心）
        let mut converter = CoordinateConverter::default();
        converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
        let mut vertices: Vec<Vertex> = Vec::with_capacity(coords.len());
        for [lon, lat] in coords {
            let mercator = MercatorCoord::from_lat_lon(*lat, *lon);
            let Some(plane) = converter.mercator_to_plane(mercator) else {
                report_callback_error(self.l10n(), &self.l10n().t("boundary.error_convert_failed"));
                return;
            };
            vertices.push(Vertex::new(plane.x, plane.y));
        }

        // B5 校验（闭合/面积/自相交，ADR-0029）——失败走 B7 弹窗
        let validation_result = validate_polygon_closure(&vertices);
        if !validation_result.is_valid {
            let error_detail: String = validation_result
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            report_callback_error(self.l10n(), &error_detail);
            return;
        }

        // 合法：装载 B5 drawer 为 Determined（T24 地图来源与画布来源同状态机）
        // T25: 同时按方案保存 GCJ-02 原始坐标，供朝向模式半透明参照
        if let Some(plan_id) = self.active_plan_id.clone() {
            self.set_plan_boundary_gcj02(&plan_id, Some(coords.to_vec()));
        }
        self.boundary_drawer.load_determined(vertices);
        if let Some(plan_id) = self.active_plan_id.clone() {
            let has_orientation = self.current_plan_has_orientation();
            self.update_plan_progress(&plan_id, true, has_orientation);
        }
        self.sync_workspace_progress(window);
        self.sync_boundary_display(window);
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
