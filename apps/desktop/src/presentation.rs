//! S1 按功能划分的呈现接缝。
//!
//! 每个入口把一次请求交给一个可替换适配器，并一次取得页面、操作、导航、
//! 确认与通知的完整呈现结果。入口只认识呈现数据，不持有正式业务数据，
//! 也不协调功能模块内部步骤。

use std::fmt;
use std::marker::PhantomData;

use notification_center::{NotificationCenter, NotificationRecord};
use slint::{ModelRc, VecModel};

use crate::{
    AppWindow, CampusData, NoticeData, OperationPresentationState, PlanCardData, TrashItemData,
};

pub use notification_center::OpaqueNotificationAction;

/// B7 留存并呈现的一条通知事实。
pub type NotificationFact = NotificationRecord;

/// 功能模块与 S1 共同使用的单一呈现接口。
pub trait PresentationAdapter<Request, Page> {
    /// 处理一次请求并返回可直接呈现的完整结果。
    fn present(&mut self, request: Request) -> Presentation<Page>;
}

/// 合法的用户可见进度百分比。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress(u8);

impl Progress {
    /// 尚未取得可见进展的合法起点。
    pub const ZERO: Self = Self(0);

    /// 百分比数值，恒在 0～100。
    pub fn percent(self) -> u8 {
        self.0
    }
}

/// 输入超出 0～100 时返回的进度错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidProgress(u8);

impl fmt::Display for InvalidProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "progress must be between 0 and 100, got {}",
            self.0
        )
    }
}

impl std::error::Error for InvalidProgress {}

impl TryFrom<u8> for Progress {
    type Error = InvalidProgress;

    fn try_from(percent: u8) -> Result<Self, Self::Error> {
        if percent <= 100 {
            Ok(Self(percent))
        } else {
            Err(InvalidProgress(percent))
        }
    }
}

/// 用户操作的可观察状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationState {
    /// 页面已就绪，当前没有操作结果。
    Ready,
    /// 操作成功完成。
    Succeeded,
    /// 耗时操作已经开始并立即返回进度。
    Processing {
        progress: Progress,
    },
    /// 操作失败；反馈由随结果返回的 B7 通知决定。
    Failed,
    /// 操作尚未提交，必须先由用户确认。
    /// 操作需要用户输入（新建/改名输入窗），页面已就绪等待提交。
    NeedsInput,
    NeedsConfirmation,
}

/// S1 可呈现的页面位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// 首次设置。
    FirstRunSetup,
    /// 校区选择。
    CampusSelect,
    /// 方案列表。
    PlanList,
    /// 常规设置。
    Settings,
    /// 方案五步工作区。
    Workspace,
    /// 通知中心。
    Notifications,
    /// 回收站。
    Trash,
}

impl Screen {
    fn index(self) -> i32 {
        match self {
            Self::FirstRunSetup => 0,
            Self::CampusSelect => 1,
            Self::PlanList => 2,
            Self::Settings => 3,
            Self::Workspace => 4,
            Self::Notifications => 5,
            Self::Trash => 6,
        }
    }
}

/// 功能模块返回的导航决定；S1 只执行，不自行判断业务条件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    /// 留在当前页面。
    Stay,
    /// 显示指定页面。
    Show(Screen),
}

/// 需要确认时一次返回的完整弹窗内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationPresentation {
    title: String,
    body: String,
    confirm_label: String,
    cancel_label: String,
}

impl ConfirmationPresentation {
    /// 建立一份不含业务判断的确认窗呈现数据。
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
        }
    }
}

/// 输入窗呈现数据（新建/改名共用；`text` 为预填默认值或失败后回显的用户草稿）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDialogPresentation {
    title: String,
    label: String,
    text: String,
    confirm_label: String,
    cancel_label: String,
    /// 输入窗模式：0 = 新建方案，1 = 改名（Rust 侧据此分派提交逻辑）
    mode: i32,
}

impl InputDialogPresentation {
    /// 建立一份输入窗呈现数据。
    pub fn new(
        title: impl Into<String>,
        label: impl Into<String>,
        text: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
        mode: i32,
    ) -> Self {
        Self {
            title: title.into(),
            label: label.into(),
            text: text.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            mode,
        }
    }
}
/// 一个入口一次返回的完整呈现结果。
#[derive(Debug, Clone)]
pub struct Presentation<Page> {
    page: Page,
    operation: OperationState,
    navigation: NavigationDecision,
    confirmation: Option<ConfirmationPresentation>,
    input: Option<InputDialogPresentation>,
    notifications: Vec<NotificationFact>,
}

impl<Page> Presentation<Page> {
    /// 返回就绪页面。
    pub fn ready(page: Page) -> Self {
        Self::new(page, OperationState::Ready)
    }

    /// 返回成功结果及成功后的完整页面。
    pub fn succeeded(page: Page) -> Self {
        Self::new(page, OperationState::Succeeded)
    }

    /// 立即返回处理中页面和合法进度。
    pub fn processing(page: Page, progress: Progress) -> Self {
        Self::new(page, OperationState::Processing { progress })
    }

    /// 返回失败页面；当前行为有反馈时，作为 B7 通知事实一同返回。
    pub fn failed(page: Page) -> Self {
        Self::new(page, OperationState::Failed)
    }

    /// 返回未提交页面与完整确认内容。
    pub fn needs_confirmation(page: Page, confirmation: ConfirmationPresentation) -> Self {
        let mut result = Self::new(page, OperationState::NeedsConfirmation);
        result.confirmation = Some(confirmation);
        result
    }

    /// 返回等待用户输入的页面与完整输入窗内容。
    pub fn needs_input(page: Page, input: InputDialogPresentation) -> Self {
        let mut result = Self::new(page, OperationState::NeedsInput);
        result.input = Some(input);
        result
    }

    fn new(page: Page, operation: OperationState) -> Self {
        Self {
            page,
            operation,
            navigation: NavigationDecision::Stay,
            confirmation: None,
            input: None,
            notifications: Vec::new(),
        }
    }

    /// 附加由功能模块作出的导航决定。
    pub fn with_navigation(mut self, navigation: NavigationDecision) -> Self {
        self.navigation = navigation;
        self
    }

    /// 附加输入窗内容（失败后回显用户草稿时使用）。
    pub fn with_input(mut self, input: InputDialogPresentation) -> Self {
        self.input = Some(input);
        self
    }
    /// 附加一条通知事实，供 B7 统一留存和呈现。
    pub fn with_notification(mut self, notification: NotificationFact) -> Self {
        self.notifications.push(notification);
        self
    }

    /// 完整页面状态。
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// 当前操作状态。
    pub fn operation(&self) -> &OperationState {
        &self.operation
    }

    /// 功能模块作出的导航决定。
    pub fn navigation(&self) -> NavigationDecision {
        self.navigation
    }

    /// 本次操作产生的通知事实。
    pub fn notifications(&self) -> &[NotificationFact] {
        &self.notifications
    }

    /// 本次操作要求用户填写的输入窗内容（无则返回 None）。
    pub fn input(&self) -> Option<&InputDialogPresentation> {
        self.input.as_ref()
    }
}

trait WindowPageState {
    fn render(&self, window: &AppWindow);
}

fn render_presentation<Page>(
    presentation: &Presentation<Page>,
    window: &AppWindow,
    center: &NotificationCenter,
) where
    Page: WindowPageState,
{
    let (state, progress) = operation_presentation(&presentation.operation);
    window.set_operation_state(state);
    window.set_operation_progress(progress);
    window.set_confirm_dialog_visible(false);
    window.set_confirm_dialog_title("".into());
    window.set_confirm_dialog_body("".into());
    window.set_confirm_dialog_confirm_label("".into());
    window.set_confirm_dialog_cancel_label("".into());
    window.set_input_dialog_visible(false);
    window.set_input_dialog_title("".into());
    window.set_input_dialog_text("".into());
    window.set_input_dialog_mode(0);
    presentation.page.render(window);

    if let NavigationDecision::Show(screen) = presentation.navigation {
        window.set_active_screen(screen.index());
    }

    for fact in &presentation.notifications {
        match fact.diagnostic_action().cloned() {
            Some(action) => {
                center.publish_with_action(fact.notification().clone(), action);
            }
            None => center.publish(fact.notification().clone()),
        }
    }

    if let Some(confirmation) = &presentation.confirmation {
        window.set_confirm_dialog_title(confirmation.title.clone().into());
        window.set_confirm_dialog_body(confirmation.body.clone().into());
        window.set_confirm_dialog_confirm_label(confirmation.confirm_label.clone().into());
        window.set_confirm_dialog_cancel_label(confirmation.cancel_label.clone().into());
        window.set_confirm_dialog_visible(true);
    }

    if let Some(input) = &presentation.input {
        window.set_input_dialog_title(input.title.clone().into());
        window.set_input_dialog_text(input.text.clone().into());
        window.set_input_dialog_mode(input.mode);
        window.set_input_dialog_visible(true);
    }
}

fn operation_presentation(operation: &OperationState) -> (OperationPresentationState, i32) {
    match operation {
        OperationState::Ready => (OperationPresentationState::Ready, 0),
        OperationState::Succeeded => (OperationPresentationState::Succeeded, 0),
        OperationState::Processing { progress } => (
            OperationPresentationState::Processing,
            i32::from(progress.percent()),
        ),
        OperationState::Failed => (OperationPresentationState::Failed, 0),
        OperationState::NeedsConfirmation => (OperationPresentationState::NeedsConfirmation, 0),
        OperationState::NeedsInput => (OperationPresentationState::NeedsInput, 0),
    }
}

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
        window.set_toolbar_title(self.title.clone().into());
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
    pub new_demo_campus_label: String,
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
        window.set_campus_select_new_demo_campus_button_text(
            self.new_demo_campus_label.clone().into(),
        );
        window.set_campus_select_settings_button_text(self.campus_settings_label.clone().into());
        window.set_campus_select_model(ModelRc::new(VecModel::from(self.campuses.clone())));
        window.set_campus_search_text(self.campus_search_query.clone().into());
        window.set_campus_search_placeholder(self.campus_search_placeholder.clone().into());
        window.set_campus_search_button_text(self.campus_search_button_label.clone().into());
        window.set_campus_recent_title(self.campus_recent_title.clone().into());
        window.set_campus_search_results_model(ModelRc::new(VecModel::from(
            self.campus_search_results.clone(),
        )));
        window.set_campus_show_results(self.campus_show_results);
        window.set_plan_list_model(ModelRc::new(VecModel::from(self.plans.clone())));
        window.set_plan_list_tutorial_visible(self.tutorial_visible);
        window.set_plan_list_tutorial_text(self.tutorial_text.clone().into());
        window.set_plan_list_tutorial_dismiss_label(self.tutorial_dismiss_label.clone().into());
        window.set_plan_list_tutorial_skip_all_label(self.tutorial_skip_all_label.clone().into());
    }
}

/// 当前五步工作区占位页的全部可观察状态。
#[derive(Clone)]
pub struct WorkspacePageState {
    pub toolbar: ToolbarPageState,
    pub completed_steps: i32,
    pub placeholder_title: String,
    pub placeholder_subtitle: String,
    pub pending_notice: String,
    pub title_step_label: String,
    pub boundary_step_label: String,
    pub orientation_step_label: String,
    pub collection_step_label: String,
    pub review_step_label: String,
    pub export_step_label: String,
    pub tutorial_visible: bool,
    pub tutorial_text: String,
    pub tutorial_dismiss_label: String,
    pub tutorial_skip_all_label: String,
}

impl WorkspacePageState {
    fn render(&self, window: &AppWindow, active_step: i32) {
        self.toolbar.render(window);
        window.set_workspace_active_step(active_step);
        window.set_workspace_completed_steps(self.completed_steps);
        window.set_workspace_placeholder_title(self.placeholder_title.clone().into());
        window.set_workspace_placeholder_subtitle(self.placeholder_subtitle.clone().into());
        window.set_workspace_step_pending_notice(self.pending_notice.clone().into());
        window.set_workspace_stepper_title_label(self.title_step_label.clone().into());
        window.set_workspace_stepper_boundary_label(self.boundary_step_label.clone().into());
        window.set_workspace_stepper_orientation_label(self.orientation_step_label.clone().into());
        window.set_workspace_stepper_collection_label(self.collection_step_label.clone().into());
        window.set_workspace_stepper_review_label(self.review_step_label.clone().into());
        window.set_workspace_stepper_export_label(self.export_step_label.clone().into());
        window.set_workspace_tutorial_visible(self.tutorial_visible);
        window.set_workspace_tutorial_text(self.tutorial_text.clone().into());
        window.set_workspace_tutorial_dismiss_label(self.tutorial_dismiss_label.clone().into());
        window.set_workspace_tutorial_skip_all_label(self.tutorial_skip_all_label.clone().into());
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
                self.workspace.render(window, $step);
            }
        }
    };
}

workspace_page_state!(CollectionPageState, 2, "采集入口的当前完整占位页状态。");
workspace_page_state!(ReviewPageState, 3, "评审入口的当前完整占位页状态。");
workspace_page_state!(CoveragePageState, 2, "覆盖率入口的当前完整占位页状态。");
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

macro_rules! presentation_entry {
    ($name:ident, $page:ty, $doc:literal) => {
        #[doc = $doc]
        pub struct $name<'a, Request> {
            adapter: Box<dyn PresentationAdapter<Request, $page> + 'a>,
            request: PhantomData<fn(Request)>,
        }

        impl<'a, Request> $name<'a, Request> {
            /// 使用生产或测试适配器建立入口；调用者不保存具体适配器类型。
            pub fn new(adapter: impl PresentationAdapter<Request, $page> + 'a) -> Self {
                Self {
                    adapter: Box::new(adapter),
                    request: PhantomData,
                }
            }

            /// 在同一个入口上替换生产或测试适配器。
            pub fn replace_adapter(
                &mut self,
                adapter: impl PresentationAdapter<Request, $page> + 'a,
            ) {
                self.adapter = Box::new(adapter);
            }

            /// 处理一次请求、呈现完整结果，并把同一结果交还调用者观察。
            pub fn show(
                &mut self,
                window: &AppWindow,
                center: &NotificationCenter,
                request: Request,
            ) -> Presentation<$page> {
                let presentation = self.adapter.present(request);
                render_presentation(&presentation, window, center);
                presentation
            }
        }
    };
}

presentation_entry!(
    StartupPresentationEntry,
    StartupPageState,
    "首次启动呈现入口。"
);
presentation_entry!(
    SettingsPresentationEntry,
    SettingsPageState,
    "设置呈现入口。"
);
presentation_entry!(
    CampusPlanPresentationEntry,
    CampusPlanPageState,
    "校区与方案呈现入口。"
);
presentation_entry!(TrashPresentationEntry, TrashPageState, "回收站呈现入口。");
presentation_entry!(
    CollectionPresentationEntry,
    CollectionPageState,
    "采集呈现入口。"
);
presentation_entry!(ReviewPresentationEntry, ReviewPageState, "评审呈现入口。");
presentation_entry!(
    CoveragePresentationEntry,
    CoveragePageState,
    "覆盖率检查呈现入口。"
);

impl<'a, Request> NotificationPresentationEntry<'a, Request> {
    /// 呈现独立的后台故障操作状态，不改写普通页面操作或临时弹窗。
    pub(crate) fn show_diagnostic(
        &mut self,
        window: &AppWindow,
        center: &NotificationCenter,
        request: Request,
    ) -> Presentation<NotificationPageState> {
        let presentation = self.adapter.present(request);
        let (state, progress) = operation_presentation(&presentation.operation);
        window.set_diagnostic_operation_state(state);
        window.set_diagnostic_operation_progress(progress);
        presentation.page.render(window);

        for fact in &presentation.notifications {
            match fact.diagnostic_action().cloned() {
                Some(action) => {
                    center.publish_with_action(fact.notification().clone(), action);
                }
                None => center.publish(fact.notification().clone()),
            }
        }

        presentation
    }
}
presentation_entry!(ExportPresentationEntry, ExportPageState, "导出呈现入口。");
presentation_entry!(
    NotificationPresentationEntry,
    NotificationPageState,
    "通知呈现入口。"
);

fn string_model(values: &[String]) -> ModelRc<slint::SharedString> {
    let values = values.iter().cloned().map(Into::into).collect::<Vec<_>>();
    ModelRc::new(VecModel::from(values))
}
