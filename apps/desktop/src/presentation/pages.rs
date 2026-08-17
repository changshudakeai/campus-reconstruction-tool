//! S1 各功能入口的页面状态与请求（呈现接缝的数据面）。
//!
//! 页面状态只描述“一次请求后应显示什么”：适配器返回完整状态，`super` 的呈现
//! 机制负责渲染。状态不持有正式业务数据，也不协调功能模块内部步骤。

use slint::{Model, ModelRc, VecModel};

use super::{Screen, WindowPageState};
use crate::{
    AppWindow, BoundaryPointData, CampusData, NoticeData, OrientationPointData, PlanCardData,
    ReviewCandidateData, TrashItemData,
};

/// 校区上下文页面共用的工具栏状态。
#[derive(Clone)]
pub struct ToolbarPageState {
    pub title: String,
    pub notice_visible: bool,
    pub notice_label: String,
    pub switch_campus_visible: bool,
    pub switch_campus_label: String,
    pub trash_visible: bool,
    pub trash_label: String,
    pub settings_visible: bool,
    pub settings_label: String,
}

impl ToolbarPageState {
    fn render(&self, window: &AppWindow) {
        // 标题已改由 main.slint 按当前屏计算（方案列表/校区选择/设置/通知中心/
        // 回收站/工作区=校区/方案名），不再由页面状态注入。
        window.set_notice_toolbar_button_visible(self.notice_visible);
        window.set_notice_toolbar_label(self.notice_label.clone().into());
        window.set_switch_campus_toolbar_button_visible(self.switch_campus_visible);
        window.set_switch_campus_toolbar_label(self.switch_campus_label.clone().into());
        window.set_trash_toolbar_button_visible(self.trash_visible);
        window.set_trash_toolbar_label(self.trash_label.clone().into());
        window.set_settings_toolbar_button_visible(self.settings_visible);
        window.set_settings_toolbar_label(self.settings_label.clone().into());
    }
}

/// 启动入口一次返回的完整首开页面状态。
#[derive(Clone)]
pub struct StartupPageState {
    pub app_title: String,
    pub status_text: String,
    pub wizard_title: String,
    pub language_label: String,
    pub version_label: String,
    pub notice_text: String,
    pub continue_label: String,
    // 首启向导高德地图配置区（ADR-0004：JS API Key + 安全密钥必填）
    pub wizard_gaode_group_title: String,
    pub wizard_gaode_api_key_label: String,
    pub wizard_gaode_api_key_placeholder: String,
    pub wizard_gaode_security_key_label: String,
    pub wizard_gaode_security_key_placeholder: String,
    pub wizard_gaode_web_service_key_label: String,
    pub wizard_gaode_web_service_key_placeholder: String,
    pub wizard_gaode_api_key: String,
    pub wizard_gaode_security_key: String,
    pub wizard_gaode_web_service_key: String,
    pub language_options: Vec<String>,
    pub version_options: Vec<String>,
    pub selected_language: String,
    pub selected_version: String,
    pub acknowledged: bool,
    /// 启动决定指向校区或方案页时，该目标页的完整状态。
    pub landing_page: Option<CampusPlanPageState>,
}

impl WindowPageState for StartupPageState {
    fn render(&self, window: &AppWindow) {
        window.set_app_title(self.app_title.clone().into());
        window.set_status_text(self.status_text.clone().into());
        window.set_wizard_title(self.wizard_title.clone().into());
        window.set_wizard_language_label(self.language_label.clone().into());
        window.set_wizard_version_label(self.version_label.clone().into());
        window.set_wizard_notice_text(self.notice_text.clone().into());
        window.set_wizard_continue_label(self.continue_label.clone().into());
        window.set_wizard_gaode_group_title(self.wizard_gaode_group_title.clone().into());
        window.set_wizard_gaode_api_key_label(self.wizard_gaode_api_key_label.clone().into());
        window.set_wizard_gaode_api_key_placeholder(
            self.wizard_gaode_api_key_placeholder.clone().into(),
        );
        window.set_wizard_gaode_security_key_label(
            self.wizard_gaode_security_key_label.clone().into(),
        );
        window.set_wizard_gaode_security_key_placeholder(
            self.wizard_gaode_security_key_placeholder.clone().into(),
        );
        window.set_wizard_gaode_web_service_key_label(
            self.wizard_gaode_web_service_key_label.clone().into(),
        );
        window.set_wizard_gaode_web_service_key_placeholder(
            self.wizard_gaode_web_service_key_placeholder.clone().into(),
        );
        window.set_wizard_gaode_api_key(self.wizard_gaode_api_key.clone().into());
        window.set_wizard_gaode_security_key(self.wizard_gaode_security_key.clone().into());
        window.set_wizard_gaode_web_service_key(self.wizard_gaode_web_service_key.clone().into());
        window.set_wizard_language_options(string_model(&self.language_options));
        window.set_wizard_version_options(string_model(&self.version_options));
        window.set_wizard_language(self.selected_language.clone().into());
        window.set_wizard_version(self.selected_version.clone().into());
        window.set_wizard_acknowledged(self.acknowledged);
        if let Some(landing_page) = &self.landing_page {
            landing_page.render(window);
        }
    }
}

/// 启动入口的一次请求：读取着陆结果或提交首次设置。
#[derive(Debug, Clone)]
pub enum StartupRequest {
    /// 读取并显示启动着陆结果。
    Show,
    /// 提交首次设置（页面兼任知情告知）后重新取得着陆结果。
    CompleteFirstRun {
        language: String,
        minecraft_version: String,
        acknowledged: bool,
        api_key: String,
        security_key: String,
        web_service_key: String,
    },
}

/// 设置入口的一次请求：读取页面或执行一次设置操作。
#[derive(Debug, Clone)]
pub enum SettingsRequest {
    /// 读取并显示设置页。
    Show,
    /// 保存常规设置（语言、Minecraft 版本、默认导出位置）。
    SaveGeneral {
        language: String,
        minecraft_version: String,
        default_export_location: String,
    },
    /// 保存高德密钥（与测试连通性分开，ADR-0004）。
    SaveKeys {
        api_key: String,
        security_key: String,
        web_service_key: String,
    },
    /// 测试高德连通性（使用页面当前输入）。
    TestConnection {
        api_key: String,
        security_key: String,
    },
    /// 请求清除全部高德密钥（先返回确认窗）。
    ClearKeys,
    /// 用户确认后执行清除。
    ConfirmClearKeys,
    /// 重新查看新手教程（F2 进度清零）。
    ReplayTutorial,
}

/// 设置入口一次返回的完整当前设置页状态。
#[derive(Clone)]
pub struct SettingsPageState {
    pub title: String,
    pub back_label: String,
    pub tutorial_replay_label: String,
    pub general_group_title: String,
    pub language_label: String,
    pub language_options: Vec<String>,
    pub selected_language: String,
    pub version_label: String,
    pub version_options: Vec<String>,
    pub selected_version: String,
    pub export_location_label: String,
    pub export_location_placeholder: String,
    pub default_export_location: String,
    pub save_settings_label: String,
    pub gaode_group_title: String,
    pub api_key_label: String,
    pub api_key_placeholder: String,
    pub api_key: String,
    pub security_key_label: String,
    pub security_key_placeholder: String,
    pub security_key: String,
    pub web_service_key_label: String,
    pub web_service_key_placeholder: String,
    pub web_service_key: String,
    pub save_label: String,
    pub test_label: String,
    pub clear_keys_label: String,
    pub status_message: String,
}

impl WindowPageState for SettingsPageState {
    fn render(&self, window: &AppWindow) {
        window.set_settings_title(self.title.clone().into());
        window.set_settings_back_label(self.back_label.clone().into());
        window.set_tutorial_replay_label(self.tutorial_replay_label.clone().into());
        window.set_settings_general_group_title(self.general_group_title.clone().into());
        window.set_settings_language_label(self.language_label.clone().into());
        window.set_settings_language_options(string_model(&self.language_options));
        window.set_settings_language(self.selected_language.clone().into());
        window.set_settings_version_label(self.version_label.clone().into());
        window.set_settings_version_options(string_model(&self.version_options));
        window.set_settings_version(self.selected_version.clone().into());
        window.set_settings_export_location_label(self.export_location_label.clone().into());
        window.set_settings_export_location_placeholder(
            self.export_location_placeholder.clone().into(),
        );
        window.set_settings_export_location(self.default_export_location.clone().into());
        window.set_settings_save_button_label(self.save_settings_label.clone().into());
        window.set_gaode_group_title(self.gaode_group_title.clone().into());
        window.set_gaode_api_key_label(self.api_key_label.clone().into());
        window.set_gaode_api_key_placeholder(self.api_key_placeholder.clone().into());
        window.set_gaode_api_key(self.api_key.clone().into());
        window.set_gaode_security_key_label(self.security_key_label.clone().into());
        window.set_gaode_security_key_placeholder(self.security_key_placeholder.clone().into());
        window.set_gaode_security_key(self.security_key.clone().into());
        window.set_gaode_web_service_key_label(self.web_service_key_label.clone().into());
        window
            .set_gaode_web_service_key_placeholder(self.web_service_key_placeholder.clone().into());
        window.set_gaode_web_service_key(self.web_service_key.clone().into());
        window.set_gaode_save_button_label(self.save_label.clone().into());
        window.set_gaode_test_button_label(self.test_label.clone().into());
        window.set_gaode_clear_button_label(self.clear_keys_label.clone().into());
        window.set_gaode_status_message(self.status_message.clone().into());
    }
}

/// 校区选择和方案列表入口一次返回的完整当前状态。
#[derive(Clone)]
pub struct CampusPlanPageState {
    pub toolbar: ToolbarPageState,
    pub campus_select_title: String,
    pub campus_empty_text: String,
    pub campus_settings_label: String,
    pub campuses: Vec<CampusData>,
    /// 校区搜索框当前文本（S1 未提交页面临时状态）
    pub campus_search_query: String,
    /// 搜索框占位文案（ADR-0006）
    pub campus_search_placeholder: String,
    /// "搜索"按钮文案
    pub campus_search_button_label: String,
    /// "最近使用的校区"区块标题（ADR-0006）
    pub campus_recent_title: String,
    /// 搜索结果（只在用户点击搜索/回车后填充）
    pub campus_search_results: Vec<CampusData>,
    /// 搜索结果区状态文案（如"正在搜索学校…"；空串 = 无状态）
    pub campus_search_status: String,
    /// 是否正在展示搜索结果（否则展示最近使用记录）
    pub campus_show_results: bool,
    pub plan_list_title: String,
    pub campus_name: String,
    pub create_plan_label: String,
    pub back_to_campus_label: String,
    pub plan_empty_text: String,
    pub rename_label: String,
    pub duplicate_label: String,
    pub delete_label: String,
    pub plans: Vec<PlanCardData>,
    pub tutorial_visible: bool,
    pub tutorial_text: String,
    pub tutorial_dismiss_label: String,
    pub tutorial_skip_all_label: String,
}

impl WindowPageState for CampusPlanPageState {
    fn render(&self, window: &AppWindow) {
        self.toolbar.render(window);
        window.set_campus_select_title(self.campus_select_title.clone().into());
        window.set_campus_select_empty_list_text(self.campus_empty_text.clone().into());
        window.set_campus_select_settings_button_text(self.campus_settings_label.clone().into());
        window.set_campus_select_model(ModelRc::new(VecModel::from(self.campuses.clone())));
        window.set_campus_search_text(self.campus_search_query.clone().into());
        window.set_campus_search_placeholder(self.campus_search_placeholder.clone().into());
        window.set_campus_search_button_text(self.campus_search_button_label.clone().into());
        window.set_campus_recent_title(self.campus_recent_title.clone().into());
        window.set_campus_search_status(self.campus_search_status.clone().into());
        window.set_campus_search_results_model(ModelRc::new(VecModel::from(
            self.campus_search_results.clone(),
        )));
        window.set_campus_show_results(self.campus_show_results);
        window.set_plan_list_model(ModelRc::new(VecModel::from(self.plans.clone())));
        window.set_plan_list_title(self.plan_list_title.clone().into());
        window.set_plan_list_campus_name(self.campus_name.clone().into());
        window.set_plan_list_create_button_text(self.create_plan_label.clone().into());
        window.set_plan_list_back_button_text(self.back_to_campus_label.clone().into());
        window.set_plan_list_empty_text(self.plan_empty_text.clone().into());
        window.set_plan_list_rename_label(self.rename_label.clone().into());
        window.set_plan_list_duplicate_label(self.duplicate_label.clone().into());
        window.set_plan_list_delete_label(self.delete_label.clone().into());
        window.set_plan_list_tutorial_visible(self.tutorial_visible);
        window.set_plan_list_tutorial_text(self.tutorial_text.clone().into());
        window.set_plan_list_tutorial_dismiss_label(self.tutorial_dismiss_label.clone().into());
        window.set_plan_list_tutorial_skip_all_label(self.tutorial_skip_all_label.clone().into());
    }
}

/// 边界步骤的完整可观察状态（S1-05：由功能入口返回，S1 只绘制）。
#[derive(Clone)]
pub struct BoundaryViewState {
    pub points: Vec<BoundaryPointData>,
    pub path_commands: String,
    pub title: String,
    pub hint: String,
    pub undo_label: String,
    pub confirm_label: String,
    pub reset_label: String,
    pub refresh_label: String,
    /// 抽屉"删除选中点"按钮文案
    pub delete_selected_label: String,
    /// 抽屉"删除选中点"按钮是否可用（有选中顶点时可用）
    pub delete_selected_enabled: bool,
    /// 已确认边界被再次编辑（拖拽/增删顶点）后为 true：确认按钮需重新可用，
    /// 以便把调整后的顶点重新确认并覆盖落库。
    pub edited_since_confirmed: bool,
    pub status: String,
    pub map_placeholder: String,
    pub is_determined: bool,
    pub point_count: i32,
}

impl BoundaryViewState {
    fn render(&self, window: &AppWindow) {
        window.set_workspace_boundary_points(ModelRc::new(VecModel::from(self.points.clone())));
        window.set_workspace_boundary_path_commands(self.path_commands.clone().into());
        window.set_workspace_boundary_title(self.title.clone().into());
        window.set_workspace_boundary_hint(self.hint.clone().into());
        window.set_workspace_boundary_undo_label(self.undo_label.clone().into());
        window.set_workspace_boundary_confirm_label(self.confirm_label.clone().into());
        window.set_workspace_boundary_reset_label(self.reset_label.clone().into());
        window.set_workspace_boundary_refresh_label(self.refresh_label.clone().into());
        window.set_workspace_boundary_delete_selected_label(
            self.delete_selected_label.clone().into(),
        );
        window.set_workspace_boundary_delete_selected_enabled(self.delete_selected_enabled);
        window.set_workspace_boundary_edited_since_confirmed(self.edited_since_confirmed);
        window.set_workspace_boundary_status(self.status.clone().into());
        window.set_workspace_boundary_map_placeholder(self.map_placeholder.clone().into());
        window.set_workspace_boundary_is_determined(self.is_determined);
        window.set_workspace_boundary_point_count(self.point_count);
    }
}

/// 朝向步骤的完整可观察状态（S1-05 必要的朝向门控；交互迁移归工单 06）。
#[derive(Clone)]
pub struct OrientationViewState {
    pub points: Vec<OrientationPointData>,
    pub path_commands: String,
    pub arrow_commands: String,
    pub angle: f32,
    pub is_determined: bool,
    pub title: String,
    pub two_points_hint: String,
    pub bearing_angle_hint: String,
    pub angle_input_placeholder: String,
    pub angle_display: String,
    /// T40：一次性"清空朝向输入框"请求（重置/清除/切换方案时由功能入口置位，
    /// 渲染只在此请求下写空串；正常渲染绝不回写输入文本，避免覆盖用户键入值）。
    pub clear_input: bool,
    /// 两点计算后把角度回填进输入框（一次性，随本次呈现消费；与 clear_input 互斥）。
    pub fill_input: Option<String>,
    pub submit_label: String,
    pub reset_label: String,
    pub status: String,
}

impl OrientationViewState {
    fn render(&self, window: &AppWindow) {
        window.set_workspace_orientation_points(ModelRc::new(VecModel::from(self.points.clone())));
        window.set_workspace_orientation_path_commands(self.path_commands.clone().into());
        window.set_workspace_orientation_arrow_commands(self.arrow_commands.clone().into());
        window.set_workspace_orientation_angle(self.angle);
        window.set_workspace_orientation_is_determined(self.is_determined);
        window.set_workspace_orientation_title(self.title.clone().into());
        window.set_workspace_orientation_two_points_hint(self.two_points_hint.clone().into());
        window.set_workspace_orientation_bearing_angle_hint(self.bearing_angle_hint.clone().into());
        window.set_workspace_orientation_angle_input_placeholder(
            self.angle_input_placeholder.clone().into(),
        );
        window.set_workspace_orientation_angle_display(self.angle_display.clone().into());
        // T40：输入值只活在窗口（提交时读取），渲染不得回写——任意呈现
        // （map_status / orientation_points IPC / 切步）都不重置输入框；
        // 只有显式的清空请求（重置/清除/切换方案）才写一次空串。
        if self.clear_input {
            window.set_workspace_orientation_input_text("".into());
        }
        if let Some(text) = &self.fill_input {
            window.set_workspace_orientation_input_text(text.clone().into());
        }
        window.set_workspace_orientation_submit_label(self.submit_label.clone().into());
        window.set_workspace_orientation_reset_label(self.reset_label.clone().into());
        window.set_workspace_orientation_status(self.status.clone().into());
    }
}

/// 方案工作区入口的一次请求（S1-05）。
///
/// 步骤点击/“下一步”统一走 [`WorkspaceRequest::Navigate`]；边界闭合、有效性、
/// 重置与保存走 [`WorkspaceRequest::BoundaryConfirm`] 等请求；离开边界页走
/// [`WorkspaceRequest::Leave`]。S1 只转交动作并绘制返回状态，不判断业务条件。
#[derive(Debug, Clone)]
pub enum WorkspaceRequest {
    /// 从方案列表打开方案工作区（新方案首次打开即第①步，ADR-0027 第 6 轮）。
    OpenPlan { plan_id: String },
    /// 历史栈返回工作区：复用当前内存会话原样重绘（同一方案/步骤/未保存
    /// 边界点），不重新打开方案；地图按当前步骤重建。
    Resume,
    /// 点击步骤（或“下一步”）：功能入口返回允许进入 / 条件不足 / 需要确认。
    Navigate { step: i32 },
    /// 离开工作区：S1 只提交目标；功能入口判断可以离开、需要确认或必须停留。
    Leave { target: Screen },
    /// 边界画布原始绘制动作转交（S1 不掺入边界规则）。
    BoundaryCanvasClick { x: f32, y: f32 },
    /// 撤销最后一个绘制点。
    BoundaryUndo,
    /// 删除当前选中的边界顶点（地图编辑态；无选中时按钮禁用）。
    BoundaryDeleteSelected,
    /// 边界闭合 + 有效性校验 + 保存（B5 状态机与校验）。
    BoundaryConfirm,
    /// 重置边界并清除已保存的方案边界。
    BoundaryReset,
    /// 明确重新请求当前方案的 OSM 边界；不把普通页面重建当作刷新。
    BoundaryRefresh,
    /// 朝向提交（方位角输入模式；两点模式由地图 IPC 计算，见工单 06）。
    OrientationSubmit { mode: String, angle_text: String },
    /// 重置朝向并清除已保存的方案朝向。
    OrientationReset,
    /// 用户在修改朝向的重算确认窗中点了“确认”。
    ConfirmOrientation,
    /// 用户取消确认窗：功能入口清除待定状态（S1-05）。
    CancelConfirmation,
    /// 步骤条教程气泡“知道了”（F2 规矩①）。
    TutorialDismiss,
    /// 步骤条教程气泡“跳过全部引导”（F2 规矩②）。
    TutorialSkipAll,
    /// 地图加载完成状态（成功或故障；故障只暂停地图相关操作）。
    MapStatus { available: bool },
    /// 地图会话已经过代际过滤、场景路由和解析的结构化页面事件。
    #[doc(hidden)]
    MapEvent { message: gaode_client::IpcMessage },
    /// T31：轮询 Rust 侧 OSM 边界自动获取的后台结果（不阻塞 UI 线程）。
    #[doc(hidden)]
    PollBoundaryFetch,
    /// 朝向模式切换（两点/方位角；S1 未提交页面临时状态）。
    /// T34: 左侧抽屉开合（做法 A：展开时地图右移让位；S1 页面临时状态）
    DrawerToggle,
}

/// S1 导出页面只提交“显示确认”或“一次开始导出”意图；完整业务链由 F9 接管。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportPresentationRequest {
    /// 进入导出步骤，显示可导出的确认状态。
    Open,
    /// 用户点击一次开始导出按钮。
    Start,
    /// S1 内部观察 F9 已提交操作的真实进度或终态。
    Poll,
    /// 离开导出上下文，丢弃旧页面的结果交付。
    Abandon,
}

/// S1 采集页只提交一次完整用户意图（开始采集/查看采集报告/轮询/离开）；
/// F4 → B2 → B14 → F7 的完整业务链由 A1 collection-flow 接管（ADR-0039）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionRequest {
    /// 进入采集页，呈现 A1 已决定的当前状态。
    Open,
    /// 用户点击一次“采集”按钮（完整开始意图）。
    Start,
    /// S1 内部轮询 A1 后台操作的真实进度或终态。
    Poll,
    /// 用户点击“查看采集报告”（完整报告操作）。
    ShowReport,
    /// 用户点击“取消采集”（T36：A1 CollectionFlow::cancel 已存在，接到抽屉按钮）。
    Cancel,
    /// 离开采集上下文，过期旧结果交付（export-flow 模板）。
    Abandon,
}

/// 当前五步工作区页的全部可观察状态。
#[derive(Clone)]
pub struct WorkspacePageState {
    pub toolbar: ToolbarPageState,
    /// 校区名（五个步骤顶部始终同时显示，ADR-0027）
    pub campus_name: String,
    /// 方案名（五个步骤顶部始终同时显示，ADR-0027）
    pub plan_name: String,
    /// 顶部上下文合成文案（“校区名 / 方案名”，经 B6 文本键生成）
    pub context_label: String,
    /// 当前步骤索引（由功能入口决定，S1 只绘制）
    pub active_step: i32,
    pub completed_steps: i32,
    /// 每个步骤是否锁定（由功能入口判定，S1 只绘制）
    pub step_locked: Vec<bool>,
    /// 每个步骤是否已完成（由功能入口判定，S1 只绘制）
    pub step_completed: Vec<bool>,
    pub placeholder_title: String,
    pub placeholder_subtitle: String,
    pub pending_notice: String,
    pub title_step_label: String,
    pub boundary_step_label: String,
    pub orientation_step_label: String,
    pub collection_step_label: String,
    pub review_step_label: String,
    pub export_step_label: String,
    /// T34: 左侧抽屉是否展开（做法 A：展开时地图右移让位）
    pub drawer_open: bool,
    /// T34: 地图 WebView 是否可用（不可用时边界步骤显示 Slint 兜底画布）
    pub map_available: bool,
    /// T36: 地图是否仍在加载（步骤②地图不可用且加载中时显示"地图加载中…"）
    pub map_loading: bool,
    /// T36: 地图加载中文案（map.loading）
    pub map_loading_label: String,
    /// T36: 地图加载失败占位文案（map.load_failed，提示可退回手动输入）
    pub map_failed_label: String,
    /// T34: 抽屉① 当前点数文案（已格式化：当前点数：{n}）
    pub boundary_points_label: String,
    /// T34: 抽屉② 当前角度标签
    pub orientation_current_angle_label: String,
    /// T34: 抽屉② 确认两点朝向按钮文案
    pub orientation_confirm_two_points_label: String,
    /// T34: 抽屉开合箭头提示文案（展开）
    pub drawer_expand_tooltip: String,
    /// T34: 抽屉开合箭头提示文案（收起）
    pub drawer_collapse_tooltip: String,
    /// B 工单：边界自动获取的进度文案（阶段 + 端点尝试 + 已耗时；空串 = 无在途）
    pub boundary_fetch_status: String,
    pub boundary: BoundaryViewState,
    pub orientation: OrientationViewState,
    pub tutorial_visible: bool,
    pub tutorial_text: String,
    pub tutorial_dismiss_label: String,
    pub tutorial_skip_all_label: String,
}

impl WorkspacePageState {
    fn render_with_step(&self, window: &AppWindow, active_step: i32) {
        self.toolbar.render(window);
        window.set_workspace_active_step(active_step);
        window.set_workspace_completed_steps(self.completed_steps);
        window.set_workspace_campus_name(self.campus_name.clone().into());
        window.set_workspace_plan_name(self.plan_name.clone().into());
        window.set_workspace_context_label(self.context_label.clone().into());
        window.set_workspace_step_locked(ModelRc::new(VecModel::from(self.step_locked.clone())));
        window.set_workspace_step_completed(ModelRc::new(VecModel::from(
            self.step_completed.clone(),
        )));
        window.set_workspace_placeholder_title(self.placeholder_title.clone().into());
        window.set_workspace_placeholder_subtitle(self.placeholder_subtitle.clone().into());
        window.set_workspace_step_pending_notice(self.pending_notice.clone().into());
        window.set_workspace_stepper_title_label(self.title_step_label.clone().into());
        window.set_workspace_stepper_boundary_label(self.boundary_step_label.clone().into());
        window.set_workspace_stepper_orientation_label(self.orientation_step_label.clone().into());
        window.set_workspace_stepper_collection_label(self.collection_step_label.clone().into());
        window.set_workspace_stepper_review_label(self.review_step_label.clone().into());
        window.set_workspace_stepper_export_label(self.export_step_label.clone().into());
        window.set_workspace_export_start_label(self.export_step_label.clone().into());
        window.set_workspace_drawer_open(self.drawer_open);
        window.set_workspace_map_available(self.map_available);
        window.set_workspace_map_loading(self.map_loading);
        window.set_workspace_map_loading_label(self.map_loading_label.clone().into());
        window.set_workspace_map_failed_label(self.map_failed_label.clone().into());
        window.set_workspace_boundary_points_label(self.boundary_points_label.clone().into());
        window.set_workspace_orientation_current_angle_label(
            self.orientation_current_angle_label.clone().into(),
        );
        window.set_workspace_orientation_confirm_two_points_label(
            self.orientation_confirm_two_points_label.clone().into(),
        );
        window.set_workspace_drawer_expand_tooltip(self.drawer_expand_tooltip.clone().into());
        window.set_workspace_drawer_collapse_tooltip(self.drawer_collapse_tooltip.clone().into());
        window.set_workspace_boundary_fetch_status(self.boundary_fetch_status.clone().into());
        window.set_workspace_tutorial_visible(self.tutorial_visible);
        window.set_workspace_tutorial_text(self.tutorial_text.clone().into());
        window.set_workspace_tutorial_dismiss_label(self.tutorial_dismiss_label.clone().into());
        window.set_workspace_tutorial_skip_all_label(self.tutorial_skip_all_label.clone().into());
        self.boundary.render(window);
        self.orientation.render(window);
    }
}

impl WindowPageState for WorkspacePageState {
    fn render(&self, window: &AppWindow) {
        self.render_with_step(window, self.active_step);
    }
}
macro_rules! workspace_page_state {
    ($name:ident, $step:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone)]
        pub struct $name {
            pub workspace: WorkspacePageState,
        }

        impl WindowPageState for $name {
            fn render(&self, window: &AppWindow) {
                self.workspace.render_with_step(window, $step);
            }
        }
    };
}

/// 采集步骤的完整可观察状态。
#[derive(Clone)]
pub struct CollectionPageState {
    pub workspace: WorkspacePageState,
    pub source_label: String,
    pub collect_label: String,
    pub progress_label: String,
    pub category_labels: Vec<String>,
    pub category_statuses: Vec<String>,
    pub category_skip_label: String,
    pub diff_summary: String,
    pub report_entry_label: String,
    pub report_body: String,
    /// 当前阶段文案（拉取数据 / 补名 / 写库 / 完成）
    pub stage_label: String,
    /// 已用时长文案（处理中实时更新）
    pub elapsed_label: String,
    /// “取消采集”按钮文案
    pub cancel_label: String,
    /// 处理中是否显示取消按钮
    pub cancel_visible: bool,
    /// “部分建筑未命名”提示（无则空串）
    pub partial_naming_label: String,
}

impl WindowPageState for CollectionPageState {
    fn render(&self, window: &AppWindow) {
        self.workspace.render_with_step(window, 2);
        window.set_collection_source_label(self.source_label.clone().into());
        window.set_collection_collect_label(self.collect_label.clone().into());
        window.set_collection_progress_label(self.progress_label.clone().into());
        window.set_collection_category_labels(string_model(&self.category_labels));
        window.set_collection_category_statuses(string_model(&self.category_statuses));
        window.set_collection_category_skip_label(self.category_skip_label.clone().into());
        window.set_collection_diff_summary(self.diff_summary.clone().into());
        window.set_collection_report_entry_label(self.report_entry_label.clone().into());
        window.set_collection_report_body(self.report_body.clone().into());
        window.set_collection_stage_label(self.stage_label.clone().into());
        window.set_collection_elapsed_label(self.elapsed_label.clone().into());
        window.set_collection_cancel_label(self.cancel_label.clone().into());
        window.set_collection_cancel_visible(self.cancel_visible);
        window.set_collection_partial_naming_label(self.partial_naming_label.clone().into());
    }
}
/// 评审页一次返回的完整页面状态（F5 WorkbenchView 的呈现层）。
#[derive(Clone)]
pub struct ReviewPageState {
    pub workspace: WorkspacePageState,
    /// 评审台标题。
    pub title: String,
    /// 无候选空态文案（不得阻塞导出、不得伪造评审完成）。
    pub empty_text: String,
    /// 可评审候选总数（0 时显示空态）。
    pub candidate_count: i32,
    /// 六类标签页（按 ADR-0016 固定顺序）。
    pub category_labels: Vec<String>,
    /// 每类候选数。
    pub category_counts: Vec<i32>,
    /// 当前激活类别索引。
    pub active_category: i32,
    /// 当前类别候选卡片（T39 分页：只含当前页切片，页大小 50–100）。
    pub cards: Vec<ReviewCandidateData>,
    /// 每页候选卡片数（分页常量；Slint 无虚拟化，避免一次实例化千级控件）。
    pub page_size: i32,
    /// 当前页码（0 起）。
    pub page_index: i32,
    /// 总页数（至少 1）。
    pub page_total: i32,
    /// 页码文案（如"第 1/17 页"）。
    pub page_label: String,
    /// 上一页按钮文案。
    pub page_prev_label: String,
    /// 下一页按钮文案。
    pub page_next_label: String,
    /// 已选数量文案。
    pub selected_count_label: String,
    /// 勾选 >=2 时显示批量操作按钮。
    pub bulk_buttons_visible: bool,
    /// 批量按钮文案。
    pub set_keep_label: String,
    pub set_reject_label: String,
    pub set_pending_label: String,
    pub select_all_label: String,
    pub deselect_all_label: String,
    /// 单卡三态按钮文案。
    pub card_pending_label: String,
    pub card_keep_label: String,
    pub card_reject_label: String,
    /// 卡片"定位到地图"按钮文案。
    pub locate_label: String,
    /// 地图三态/选中图例（虚线=待定、实线=保留、红色轮廓=选中/定位）。
    pub legend: String,
    /// 选中候选详情是否可见（存在高亮候选时）。
    pub detail_visible: bool,
    /// 详情标题（未命名显示"未命名建筑 #id"）。
    pub detail_title: String,
    /// 详情类别行（含类别名）。
    pub detail_category_label: String,
    /// 详情"标签与属性"行标签。
    pub detail_tags_label: String,
    /// 标签与属性行（key=value）。
    pub detail_tags: Vec<String>,
    /// 详情"来源"行标签。
    pub detail_source_label: String,
    /// 详情来源值（data_source_tag）。
    pub detail_source: String,
    /// 详情状态行。
    pub detail_state_label: String,
    /// 暂停/恢复/封账按钮文案。
    pub pause_label: String,
    pub resume_label: String,
    pub seal_label: String,
    /// 是否已封账（评审入口禁用信号）。
    pub sealed: bool,
    /// 建议筛选区标题。
    pub suggestion_filters_label: String,
    /// 建议筛选器标签（固定顺序，与 F5 `SuggestFilter::ALL` 一致）。
    pub suggestion_filter_labels: Vec<String>,
    /// 每个建议筛选器的候选总数。
    pub suggestion_filter_counts: Vec<i32>,
    /// 每个建议筛选器是否激活（1=激活，0=未激活）。
    pub suggestion_filter_active: Vec<i32>,
    /// "应用建议"按钮文案。
    pub apply_suggestions_label: String,
    /// "撤销上一批"按钮文案。
    pub undo_suggestions_label: String,
    /// 一键应用是否可用（未封账且当前筛选范围内有可执行建议）。
    pub apply_suggestions_enabled: bool,
    /// 是否存在可撤销的上一批（封账后不可撤销）。
    pub undo_available: bool,
    /// 封账成功后显示导出摘要。
    pub summary_visible: bool,
    /// 导出摘要文案。
    pub summary_text: String,
}

impl WindowPageState for ReviewPageState {
    fn render(&self, window: &AppWindow) {
        self.workspace.render_with_step(window, 3);
        window.set_review_title(self.title.clone().into());
        window.set_review_empty_text(self.empty_text.clone().into());
        window.set_review_candidate_count(self.candidate_count);
        window.set_review_category_labels(string_model(&self.category_labels));
        window
            .set_review_category_counts(ModelRc::new(VecModel::from(self.category_counts.clone())));
        window.set_review_active_category(self.active_category);
        // T39：三态/高亮/复选变更走单卡更新（set_row_data），不得整表重建；
        // 仅当行身份变化（切分类/翻页）时才整体替换模型。
        let current = window.get_review_cards();
        let same_identity = current.row_count() == self.cards.len()
            && current
                .row_data(0)
                .is_some_and(|first| first.candidate_id == self.cards[0].candidate_id);
        if same_identity && !self.cards.is_empty() {
            for (index, card) in self.cards.iter().enumerate() {
                if current.row_data(index).as_ref() != Some(card) {
                    current.set_row_data(index, card.clone());
                }
            }
        } else {
            window.set_review_cards(ModelRc::new(VecModel::from(self.cards.clone())));
        }
        window.set_review_page_size(self.page_size);
        window.set_review_page_index(self.page_index);
        window.set_review_page_total(self.page_total);
        window.set_review_page_label(self.page_label.clone().into());
        window.set_review_page_prev_label(self.page_prev_label.clone().into());
        window.set_review_page_next_label(self.page_next_label.clone().into());
        window.set_review_selected_count_label(self.selected_count_label.clone().into());
        window.set_review_bulk_buttons_visible(self.bulk_buttons_visible);
        window.set_review_set_keep_label(self.set_keep_label.clone().into());
        window.set_review_set_reject_label(self.set_reject_label.clone().into());
        window.set_review_set_pending_label(self.set_pending_label.clone().into());
        window.set_review_select_all_label(self.select_all_label.clone().into());
        window.set_review_deselect_all_label(self.deselect_all_label.clone().into());
        window.set_review_card_pending_label(self.card_pending_label.clone().into());
        window.set_review_card_keep_label(self.card_keep_label.clone().into());
        window.set_review_card_reject_label(self.card_reject_label.clone().into());
        window.set_review_locate_label(self.locate_label.clone().into());
        window.set_review_legend(self.legend.clone().into());
        window.set_review_detail_visible(self.detail_visible);
        window.set_review_detail_title(self.detail_title.clone().into());
        window.set_review_detail_category_label(self.detail_category_label.clone().into());
        window.set_review_detail_tags_label(self.detail_tags_label.clone().into());
        window.set_review_detail_tags(string_model(&self.detail_tags));
        window.set_review_detail_source_label(self.detail_source_label.clone().into());
        window.set_review_detail_source(self.detail_source.clone().into());
        window.set_review_detail_state_label(self.detail_state_label.clone().into());
        window.set_review_pause_label(self.pause_label.clone().into());
        window.set_review_resume_label(self.resume_label.clone().into());
        window.set_review_seal_label(self.seal_label.clone().into());
        window.set_review_sealed(self.sealed);
        window.set_review_suggestion_filters_label(self.suggestion_filters_label.clone().into());
        window.set_review_suggestion_filter_labels(string_model(&self.suggestion_filter_labels));
        window.set_review_suggestion_filter_counts(ModelRc::new(VecModel::from(
            self.suggestion_filter_counts.clone(),
        )));
        window.set_review_suggestion_filter_active(ModelRc::new(VecModel::from(
            self.suggestion_filter_active.clone(),
        )));
        window.set_review_apply_suggestions_label(self.apply_suggestions_label.clone().into());
        window.set_review_undo_suggestions_label(self.undo_suggestions_label.clone().into());
        window.set_review_apply_suggestions_enabled(self.apply_suggestions_enabled);
        window.set_review_undo_available(self.undo_available);
        window.set_review_summary_visible(self.summary_visible);
        window.set_review_summary_text(self.summary_text.clone().into());
    }
}

/// 评审页一次请求：S1 只转发用户操作，F5 返回完整页面状态与通知事实。
#[derive(Debug, Clone)]
pub enum ReviewRequest {
    /// 进入评审台：按当前方案从 B2 一次性装载 Reviewable 候选。
    Open,
    /// 切换六类标签页。
    SetCategory { index: usize },
    /// 候选列表上一页（T39 分页；翻页时才重建当前页卡片切片）。
    PagePrev,
    /// 候选列表下一页（T39 分页）。
    PageNext,
    /// 逐项判定三态（state: pending/keep/remove）。
    SetState { candidate_id: String, state: String },
    /// 高亮一个候选（点卡片或地图对象；双向联动共用同一份高亮）。
    Highlight { candidate_id: String },
    /// 地图对象点击（IPC）：JS 已自高亮，Rust 只同步卡片高亮与详情，
    /// 不在 WebView2 IPC 回调栈内回推 evaluate_script。
    MapObjectHighlight { candidate_id: String },
    /// 卡片"定位到地图"：地图中心跳转并高亮该候选。
    Locate { candidate_id: String },
    /// 评审地图就绪（map_ready IPC）：排定一次全量候选推送（事件循环安全
    /// 上下文执行，不在 IPC 回调栈内 evaluate_script）。
    MapReady,
    /// 评审地图加载失败（页面 onerror / SDK 超时 / Rust 侧加载超时）。
    MapFailed { message: String },
    /// 切换单卡复选。
    ToggleSelected { candidate_id: String },
    /// 全选当前类别。
    SelectAllActive,
    /// 取消全选当前类别。
    DeselectAllActive,
    /// 批量改为目标三态（state: pending/keep/remove）。
    SetBulk { state: String },
    /// 二次确认弹窗点了“确认”（批量剔除 >=5 项）。
    ConfirmPending,
    /// 二次确认弹窗点了“取消”。
    CancelPending,
    /// 切换建议筛选器（index 对应 F5 `SuggestFilter::ALL` 顺序）。
    ToggleSuggestionFilter { index: usize },
    /// 一键应用建议：对当前筛选范围生成保留/剔除批次并请求确认。
    ApplySuggestions,
    /// 建议应用确认弹窗点了“确认”。
    ConfirmSuggestionApply,
    /// 建议应用确认弹窗点了“取消”。
    CancelSuggestionApply,
    /// 撤销上一批建议应用（封账前最近一批）。
    UndoSuggestionApply,
    /// 暂停：会话状态写入临时文件。
    Pause,
    /// 恢复：从临时文件恢复会话状态。
    Resume,
    /// 封账：终态批量写回 B2。
    Seal,
}
workspace_page_state!(ExportPageState, 4, "导出入口的当前完整占位页状态。");

/// 通知入口一次返回的完整公告栏状态。
#[derive(Clone)]
pub struct NotificationPageState {
    pub toolbar: ToolbarPageState,
    pub title: String,
    pub empty_list_text: String,
    pub archive_label: String,
    pub date_today: String,
    pub date_yesterday: String,
    pub importance_high_label: String,
    pub unread_marker: String,
    pub diagnostic_action_label: String,
    pub notices: Vec<NoticeData>,
}

impl WindowPageState for NotificationPageState {
    fn render(&self, window: &AppWindow) {
        self.toolbar.render(window);
        window.set_notice_board_title(self.title.clone().into());
        window.set_notice_board_empty_list_text(self.empty_list_text.clone().into());
        window.set_notice_board_archive_button_text(self.archive_label.clone().into());
        window.set_notice_board_date_today(self.date_today.clone().into());
        window.set_notice_board_date_yesterday(self.date_yesterday.clone().into());
        window.set_notice_board_importance_high_label(self.importance_high_label.clone().into());
        window.set_notice_board_unread_marker(self.unread_marker.clone().into());
        window
            .set_notice_board_diagnostic_action_label(self.diagnostic_action_label.clone().into());
        window
            .set_error_dialog_diagnostic_action_label(self.diagnostic_action_label.clone().into());
        window.set_notice_board_model(ModelRc::new(VecModel::from(self.notices.clone())));
    }
}

/// 回收站入口一次返回的完整回收站页状态。
#[derive(Clone)]
pub struct TrashPageState {
    pub toolbar: ToolbarPageState,
    pub title: String,
    pub empty_list_text: String,
    pub restore_button_text: String,
    pub purge_button_text: String,
    pub retention_notice_text: String,
    pub campus_prefix: String,
    pub items: Vec<TrashItemData>,
}

impl WindowPageState for TrashPageState {
    fn render(&self, window: &AppWindow) {
        self.toolbar.render(window);
        window.set_trash_page_title(self.title.clone().into());
        window.set_trash_page_empty_list_text(self.empty_list_text.clone().into());
        window.set_trash_page_restore_button_text(self.restore_button_text.clone().into());
        window.set_trash_page_purge_button_text(self.purge_button_text.clone().into());
        window.set_trash_page_retention_notice_text(self.retention_notice_text.clone().into());
        window.set_trash_page_campus_prefix(self.campus_prefix.clone().into());
        window.set_trash_page_model(ModelRc::new(VecModel::from(self.items.clone())));
    }
}

/// 回收站入口的一次请求：读取页面或执行一次回收站操作。
#[derive(Debug, Clone)]
pub enum TrashRequest {
    /// 读取并显示回收站页。
    Show,
    /// 恢复方案（同名自动加"（恢复 N）"后缀，ADR-0018 §五）。
    Restore { trash_id: String },
    /// 请求立即永久删除（先返回确认窗）。
    RequestPurge { trash_id: String },
    /// 用户确认后执行永久删除。
    ConfirmPurge { trash_id: String },
    /// 请求清空回收站（先返回确认窗）。
    RequestClearAll,
    /// 用户确认后执行清空。
    ConfirmClearAll,
}

fn string_model(values: &[String]) -> ModelRc<slint::SharedString> {
    let values = values.iter().cloned().map(Into::into).collect::<Vec<_>>();
    ModelRc::new(VecModel::from(values))
}
