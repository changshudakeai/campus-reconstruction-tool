//! T19 — S1 主程序应用壳。
//!
//! ADR-0037 的目标职责是呈现页面状态、进度、导航结果与通知，并把一次用户操作
//! 转发给一个功能模块。当前 `runtime` 的着陆组合以及 [`ViewModelInjector`] 对全部
//! F 模块的持有与接线属于迁移期遗留，只为工单 01 的行为基线保留，不构成新增
//! S1 业务协调的授权。Slint 绑定、B6 文案与 B7 Presenter 仍是呈现层能力。
mod diagnostic_log;
mod map_webview;
mod presentation;
mod presenter;
mod production;
mod runtime;

pub use map_webview::MAP_LOAD_TIMEOUT_MARKER;
#[doc(hidden)]
pub use map_webview::{reset_review_push_count, review_push_count, set_review_push_probe_visible};
pub use presentation::{
    BoundaryViewState, CampusPlanPageState, CampusPlanPresentationEntry, CollectionPageState,
    CollectionPresentationEntry, CollectionRequest, ConfirmationPresentation, ExportPageState,
    ExportPresentationEntry, ExportPresentationRequest, InvalidProgress, NavigationDecision,
    NotificationFact, NotificationPageState, NotificationPresentationEntry,
    OpaqueNotificationAction, OperationState, OrientationViewState, Presentation,
    PresentationAdapter, Progress, ReviewPageState, ReviewPresentationEntry, ReviewRequest, Screen,
    SettingsPageState, SettingsPresentationEntry, SettingsRequest, StartupPageState,
    StartupPresentationEntry, StartupRequest, ToolbarPageState, TrashPageState,
    TrashPresentationEntry, TrashRequest, WorkspacePageState, WorkspacePresentationEntry,
    WorkspaceRequest,
};
pub use presenter::report_callback_error;
pub use presenter::ShellPresenter;
pub use runtime::{
    assemble_application, landing_decision, run_dev, ApplicationRuntime, LandingDecision,
};
pub use runtime::{ShellDatabases, ViewModelInjector};

// ui/main.slint 生成的 AppWindow 绑定（build.rs 产出）。生成代码的可见性
// 与属性由 Slint 生成器决定，不受本库 lint 约束，故在模块内豁免。
mod generated {
    #![allow(
        unreachable_pub,
        clippy::allow_attributes_without_reason,
        clippy::todo,
        reason = "Slint 生成代码，可见性与豁免标记由生成器决定，非手写代码；\
                  std-widgets 组件的生成代码内含 todo! 占位分支"
    )]

    slint::include_modules!();
}

pub use generated::{
    AppWindow, BoundaryPointData, CampusData, NoticeData, OperationPresentationState,
    OrientationPointData, PlanCardData, ReviewCandidateData, Theme, TrashItemData,
};
