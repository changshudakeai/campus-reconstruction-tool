//! S1 按功能划分的呈现接缝。
//!
//! 每个入口把一次请求交给一个可替换适配器，并一次取得页面、操作、导航、
//! 确认与通知的完整呈现结果。入口只认识呈现数据，不持有正式业务数据，
//! 也不协调功能模块内部步骤。

mod pages;

pub use pages::{
    BoundaryViewState, CampusPlanPageState, CollectionPageState, CollectionRequest,
    ExportPageState, ExportPresentationRequest, NotificationPageState, OrientationViewState,
    ReviewPageState, ReviewRequest, SettingsPageState, SettingsRequest, StartupPageState,
    StartupRequest, ToolbarPageState, TrashPageState, TrashRequest, WorkspacePageState,
    WorkspaceRequest,
};

use std::fmt;
use std::marker::PhantomData;

use notification_center::{NotificationCenter, NotificationRecord};

use crate::{AppWindow, OperationPresentationState};

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

    pub(crate) fn from_index(index: i32) -> Option<Self> {
        Some(match index {
            0 => Self::FirstRunSetup,
            1 => Self::CampusSelect,
            2 => Self::PlanList,
            3 => Self::Settings,
            4 => Self::Workspace,
            5 => Self::Notifications,
            6 => Self::Trash,
            _ => return None,
        })
    }
}

/// 功能模块返回的导航决定；S1 只执行，不自行判断业务条件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    /// 留在当前页面。
    Stay,
    /// 条件不足：功能入口拒绝进入，留在当前页面且不改变页面内容。
    Blocked,
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

pub(crate) trait WindowPageState {
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
        // T34：弹窗遮挡统一机制——确认弹窗显示前隐藏地图 WebView
        crate::map_session::cover_for_modal();
    }

    if let Some(input) = &presentation.input {
        window.set_input_dialog_title(input.title.clone().into());
        window.set_input_dialog_text(input.text.clone().into());
        window.set_input_dialog_mode(input.mode);
        window.set_input_dialog_visible(true);
        // T34：弹窗遮挡统一机制——输入弹窗显示前隐藏地图 WebView
        crate::map_session::cover_for_modal();
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
    WorkspacePresentationEntry,
    WorkspacePageState,
    "方案工作区呈现入口（步骤导航、边界与朝向门控）。"
);
presentation_entry!(
    CollectionPresentationEntry,
    CollectionPageState,
    "采集呈现入口。"
);
presentation_entry!(ReviewPresentationEntry, ReviewPageState, "评审呈现入口。");

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
