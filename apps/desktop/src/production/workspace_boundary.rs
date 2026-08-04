//! S1-05/06 生产呈现装配：方案工作区、步骤导航、边界与朝向流程。
// ignore-tidy-filelength: 工作区功能入口的单一呈现接口（步骤导航/边界/朝向全流程/地图 IPC），朝向流程于工单 06 迁入后仍超 1000 行；拆文件会触发 desktop-shell crate 源文件数上限（10/10），随工单 07/08/09 迁出后收窄
//!
//! 本模块是工作区面向 S1 的功能入口：步骤点击/“下一步”返回允许进入、条件不足
//! 或需要确认的导航决定；边界闭合、有效性、重置与保存全部经 B5 foundation-mode
//! 完成；朝向两点参考线与方位角输入的校验、影响说明与保存也全部由本入口
//! 完成（F5 OrientationCalculator / B1 Orientation）；地图通道只负责显示与
//! 转交原始动作（map_webview / B3 页面），S1 不掺入边界或朝向业务规则。
//! 离开边界页的可以离开/需要确认/必须停留也由本入口判定。
//!
//! 方案进度沿用 S1-04 前已确认的内存模型（本工单不改变数据结果；正式持久化
//! 归后续数据工单），状态由功能入口侧持有，S1 呈现层不保存业务副本。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use foundation_mode::{
    check_orientation_change_impact, validate_polygon_closure, BoundaryDrawer, BoundaryState,
    BoundaryUiEvent, CoordinateConverter, EventResult, MercatorCoord, Orientation,
    OrientationCalculator, OrientationImpactReport, OrientationLine, Point2D, Vertex,
};
use gaode_client::{parse_ipc_message, BoundaryEditPageConfig, BoundarySorter, IpcMessage};
use localization::Localization;
use notification_center::Notification;
use onboarding_tutorial::TutorialStep;
use shared_domain_types::{CandidateCategory, PlanId};
use slint::ComponentHandle;

use crate::presentation::{
    BoundaryViewState, ConfirmationPresentation, NavigationDecision, NotificationFact,
    OrientationViewState, Presentation, PresentationAdapter, Progress, Screen, WorkspacePageState,
    WorkspaceRequest,
};
use crate::production::BoundaryExportCapability;
use crate::ViewModelInjector;

#[cfg(test)]
use super::record_entry_call;

/// 单方案进度状态（工作区功能入口持有；正式持久化归后续数据工单）。
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanProgressState {
    pub(crate) has_boundary: bool,
    pub(crate) has_orientation: bool,
    pub(crate) has_collection: bool,
    pub(crate) orientation_angle: Option<f32>,
    pub(crate) boundary_gcj02: Option<Vec<[f64; 2]>>,
    /// 已生成数据分布（F5 重算影响报告的输入；采集/评审迁出后填充，当前为空）。
    pub(crate) generated_category_counts: HashMap<CandidateCategory, usize>,
}

impl PlanProgressState {
    fn completed_steps(&self) -> u8 {
        self.has_boundary as u8 + self.has_orientation as u8 + self.has_collection as u8
    }
}

/// 工作区会话状态：全部由功能入口持有，S1 呈现层不保存业务副本。
#[derive(Default)]
pub(crate) struct WorkspaceSessionState {
    pub(super) active_plan_id: Option<String>,
    pub(super) active_context: Option<project_management::PlanContextView>,
    pub(super) plans: HashMap<String, PlanProgressState>,
    drawer: BoundaryDrawer,
    orientation_points: Vec<(f64, f64)>,
    orientation_angle: Option<f32>,
    orientation_input_text: String,
    orientation_mode: String,
    pending_orientation_angle: Option<f32>,
    active_step: i32,
    /// 迁移期兼容：无已打开方案时按窗口当前呈现的已完成步数判定（旧行为基线）。
    adopted_completed_steps: Option<u8>,
    map_processing: bool,
    tutorial_visible: bool,
    tutorial_text: String,
    tutorial_dismiss_label: String,
    tutorial_skip_all_label: String,
}

impl WorkspaceSessionState {
    fn completed_steps(&self) -> u8 {
        match &self.active_plan_id {
            Some(plan_id) => self
                .plans
                .get(plan_id)
                .map(PlanProgressState::completed_steps)
                .unwrap_or(0),
            None => self.adopted_completed_steps.unwrap_or(0),
        }
    }

    fn adopt(&mut self, completed: i32) {
        self.adopted_completed_steps = Some(completed.clamp(0, 5) as u8);
    }

    fn boundary_unsaved(&self) -> bool {
        matches!(
            self.drawer.state(),
            BoundaryState::Drawing | BoundaryState::Editing { .. }
        ) && !self.drawer.vertices().is_empty()
    }

    /// 把计算好的朝向写入方案正式状态（工作区会话内正式状态；正式持久化归
    /// 后续数据工单）。没有已打开方案时拒绝保存，正式状态保持原状。
    fn commit_orientation(&mut self, angle: f32) -> Result<(), ()> {
        let Some(plan_id) = self.active_plan_id.clone() else {
            return Err(());
        };
        self.orientation_angle = Some(angle);
        self.pending_orientation_angle = None;
        let state = self.plans.entry(plan_id).or_default();
        state.has_orientation = true;
        state.orientation_angle = Some(angle);
        Ok(())
    }
}

/// 工作区功能入口上下文：呈现适配器与占位步骤页共用。
#[derive(Clone)]
pub(crate) struct WorkspaceProductionContext {
    injector: Rc<RefCell<ViewModelInjector>>,
    window: slint::Weak<crate::AppWindow>,
    pub(super) session: Rc<RefCell<WorkspaceSessionState>>,
    pub(super) export_flow: Arc<dyn BoundaryExportCapability>,
}

impl WorkspaceProductionContext {
    pub(crate) fn new(
        injector: Rc<RefCell<ViewModelInjector>>,
        window: &crate::AppWindow,
        export_flow: Arc<dyn BoundaryExportCapability>,
    ) -> Self {
        let tutorial_dismiss_label = injector.borrow().l10n().t("tutorial.dismiss_button");
        Self {
            injector,
            window: window.as_weak(),
            session: Rc::new(RefCell::new(WorkspaceSessionState {
                orientation_mode: "two-points".to_string(),
                tutorial_dismiss_label,
                ..Default::default()
            })),
            export_flow,
        }
    }

    pub(super) fn injector(&self) -> Rc<RefCell<ViewModelInjector>> {
        Rc::clone(&self.injector)
    }

    /// 当前工作区页完整可观察状态（由功能入口侧状态派生，S1 只绘制）。
    pub(crate) fn page(&self) -> WorkspacePageState {
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        let session = self.session.borrow();
        let completed = session.completed_steps();
        let boundary_confirmed = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .is_some_and(|state| state.has_boundary);
        let has_orientation = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .is_some_and(|state| state.has_orientation);
        let has_collection = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .is_some_and(|state| state.has_collection);
        let (campus_name, plan_name) = session
            .active_context
            .as_ref()
            .map(|context| (context.campus_name.clone(), context.plan_name.clone()))
            .unwrap_or_default();
        let context_label = if campus_name.is_empty() || plan_name.is_empty() {
            String::new()
        } else {
            l10n.t_with_array("workspace.context_campus_plan", &[&campus_name, &plan_name])
        };
        WorkspacePageState {
            toolbar: super::toolbar(l10n, true),
            campus_name,
            plan_name,
            context_label,
            active_step: session.active_step,
            completed_steps: i32::from(completed),
            // ADR-0041：边界确认后朝向、采集、评审与导出都可进入；
            // 未确认边界时只有边界步骤可用。
            step_locked: (0..5)
                .map(|index| index != 0 && !boundary_confirmed)
                .collect(),
            step_completed: vec![
                boundary_confirmed,
                has_orientation,
                has_collection,
                false,
                false,
            ],
            placeholder_title: l10n.t("workspace.placeholder_title"),
            placeholder_subtitle: l10n.t("workspace.placeholder_subtitle"),
            pending_notice: l10n.t("workspace.step_pending_notice"),
            title_step_label: l10n.t("collection.title"),
            boundary_step_label: l10n.t("collection.boundary_step"),
            orientation_step_label: l10n.t("collection.orientation_step"),
            collection_step_label: l10n.t("collection.collect_button"),
            review_step_label: l10n.t("review.workbench_title"),
            export_step_label: l10n.t("export.start_button"),
            boundary: self.boundary_view(l10n, &session),
            orientation: self.orientation_view(l10n, &session),
            tutorial_visible: session.tutorial_visible,
            tutorial_text: session.tutorial_text.clone(),
            tutorial_dismiss_label: session.tutorial_dismiss_label.clone(),
            tutorial_skip_all_label: session.tutorial_skip_all_label.clone(),
        }
    }

    /// 方案卡片进度文案（S1-04 前由注入器内存进度覆盖；本工单把该状态迁到
    /// 功能入口侧，数据结果不变）。
    pub(crate) fn plan_card_progress_text(&self, plan_id: &str, fallback: &str) -> String {
        let session = self.session.borrow();
        match session.plans.get(plan_id) {
            Some(state) if state.has_collection => {
                self.injector.borrow().l10n().t("plan.progress_next_review")
            }
            Some(state) if state.has_boundary && state.has_orientation => self
                .injector
                .borrow()
                .l10n()
                .t("plan.progress_next_collection"),
            Some(state) if state.has_boundary => self
                .injector
                .borrow()
                .l10n()
                .t("plan.progress_boundary_done"),
            _ => fallback.to_owned(),
        }
    }

    fn boundary_view(
        &self,
        l10n: &Localization,
        session: &WorkspaceSessionState,
    ) -> BoundaryViewState {
        let vertices = session.drawer.vertices();
        let is_closed = matches!(session.drawer.state(), BoundaryState::Determined);
        let points = vertices
            .iter()
            .map(|vertex| crate::BoundaryPointData {
                x: vertex.x as f32 - 5.0,
                y: vertex.y as f32 - 5.0,
            })
            .collect();
        let path_commands = build_path_commands(vertices, is_closed);
        let status = match session.drawer.state() {
            BoundaryState::Idle => l10n.t("boundary.status_idle"),
            BoundaryState::Drawing => {
                l10n.t_with_array("boundary.status_drawing", &[&vertices.len().to_string()])
            }
            BoundaryState::Determined => l10n.t("boundary.status_determined"),
            BoundaryState::Editing { .. } => l10n.t("boundary.status_editing"),
        };
        BoundaryViewState {
            points,
            path_commands,
            title: l10n.t("boundary.step_title"),
            hint: l10n.t("boundary.hint"),
            undo_label: l10n.t("boundary.undo_button"),
            confirm_label: l10n.t("boundary.confirm_button"),
            reset_label: l10n.t("boundary.reset_button"),
            status,
            map_placeholder: l10n.t("boundary.map_placeholder"),
            is_determined: is_closed,
            point_count: vertices.len() as i32,
        }
    }

    fn orientation_view(
        &self,
        l10n: &Localization,
        session: &WorkspaceSessionState,
    ) -> OrientationViewState {
        let points: Vec<crate::OrientationPointData> = session
            .orientation_points
            .iter()
            .map(|(x, y)| crate::OrientationPointData {
                x: *x as f32 - 6.0,
                y: *y as f32 - 6.0,
            })
            .collect();
        let path = if session.orientation_points.len() >= 2 {
            let (x0, y0) = session.orientation_points[0];
            let (x1, y1) = session.orientation_points[1];
            format!("M {x0} {y0} L {x1} {y1}")
        } else {
            String::new()
        };
        let confirmed_angle = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .and_then(|state| state.orientation_angle);
        let angle = confirmed_angle
            .or(session.orientation_angle)
            .unwrap_or(-1.0);
        let is_determined = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .map(|state| state.has_orientation)
            .unwrap_or(false);
        let angle_display = if angle >= 0.0 {
            format!("{angle:.1}\u{00b0}")
        } else {
            String::new()
        };
        let arrow_commands = if angle >= 0.0 {
            build_arrow_commands(angle)
        } else {
            String::new()
        };
        let status = if is_determined {
            l10n.t("orientation.status_determined")
        } else if session.orientation_points.is_empty() {
            l10n.t("orientation.status_idle")
        } else if session.orientation_points.len() == 1 {
            l10n.t("orientation.status_first_point")
        } else {
            l10n.t("orientation.status_calculated")
        };
        OrientationViewState {
            points,
            path_commands: path,
            arrow_commands,
            mode: session.orientation_mode.clone(),
            angle,
            is_determined,
            title: l10n.t("orientation.step_title"),
            two_points_hint: l10n.t("orientation.two_points_hint"),
            bearing_angle_hint: l10n.t("orientation.bearing_angle_hint"),
            angle_input_placeholder: l10n.t("orientation.angle_input_placeholder"),
            angle_display,
            input_text: session.orientation_input_text.clone(),
            submit_label: l10n.t("orientation.submit_button"),
            reset_label: l10n.t("orientation.reset_button"),
            status,
            mode_two_points_label: l10n.t("orientation.mode_two_points"),
            mode_bearing_angle_label: l10n.t("orientation.mode_bearing_angle"),
        }
    }

    /// 地图密钥与校区锚点（锚点优先取当前方案的校区；兜底取上次校区/默认点）。
    fn map_credentials(&self) -> ((String, String), (f64, f64)) {
        let injector = self.injector.borrow();
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
        let anchor = self
            .session
            .borrow()
            .active_context
            .as_ref()
            .map(|context| (context.anchor_lng, context.anchor_lat))
            .unwrap_or_else(|| {
                injector
                    .projects()
                    .landing_campus()
                    .ok()
                    .flatten()
                    .map(|campus| (campus.anchor_lng, campus.anchor_lat))
                    .unwrap_or((116.397, 39.916))
            });
        ((api_key, security_key), anchor)
    }

    fn mark_map_loading(&self) {
        let mut session = self.session.borrow_mut();
        session.map_processing = true;
    }
}

/// 工作区生产适配器：一次请求一次完整呈现（导航决定、边界动作、离开判定）。
pub(crate) struct WorkspaceProductionAdapter {
    pub(crate) context: WorkspaceProductionContext,
}

impl PresentationAdapter<WorkspaceRequest, WorkspacePageState> for WorkspaceProductionAdapter {
    fn present(&mut self, request: WorkspaceRequest) -> Presentation<WorkspacePageState> {
        #[cfg(test)]
        record_entry_call(9);
        match request {
            WorkspaceRequest::OpenPlan { plan_id } => self.open_plan(&plan_id),
            WorkspaceRequest::Navigate { step } => self.navigate(step),
            WorkspaceRequest::Leave { target } => self.leave(target),
            WorkspaceRequest::BoundaryCanvasClick { x, y } => self.boundary_canvas_click(x, y),
            WorkspaceRequest::BoundaryUndo => self.boundary_undo(),
            WorkspaceRequest::BoundaryConfirm => self.boundary_confirm(),
            WorkspaceRequest::BoundaryReset => self.boundary_reset(),
            WorkspaceRequest::OrientationSubmit { mode, angle_text } => {
                self.orientation_submit(&mode, &angle_text)
            }
            WorkspaceRequest::OrientationReset => self.orientation_reset(),
            WorkspaceRequest::OrientationModeChanged { mode } => {
                self.context.session.borrow_mut().orientation_mode = mode;
                Presentation::ready(self.context.page())
            }
            WorkspaceRequest::ConfirmOrientation => self.confirm_orientation(),
            WorkspaceRequest::CancelConfirmation => {
                self.context.session.borrow_mut().pending_orientation_angle = None;
                Presentation::ready(self.context.page())
            }
            WorkspaceRequest::TutorialDismiss => self.tutorial_dismiss(),
            WorkspaceRequest::TutorialSkipAll => self.tutorial_skip_all(),
            WorkspaceRequest::MapStatus { available } => self.map_status(available),
            WorkspaceRequest::MapIpc { message } => self.map_ipc(&message),
        }
    }
}

impl WorkspaceProductionAdapter {
    fn open_plan(&mut self, plan_id: &str) -> Presentation<WorkspacePageState> {
        let parsed = match PlanId::parse(plan_id) {
            Ok(parsed) => parsed,
            Err(_) => {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &l10n.t("workspace.plan_not_found")));
            }
        };
        let context = match self
            .context
            .injector
            .borrow()
            .projects()
            .plan_context(&parsed)
        {
            Ok(Some(context)) => context,
            Ok(None) => {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &l10n.t("workspace.plan_not_found")));
            }
            Err(error) => {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &error.to_string()));
            }
        };
        {
            let mut session = self.context.session.borrow_mut();
            session.active_plan_id = Some(plan_id.to_string());
            session.active_context = Some(context);
            session.orientation_points.clear();
            session.orientation_angle = None;
            session.pending_orientation_angle = None;
            session.orientation_input_text.clear();
            session.adopted_completed_steps = None;
            session.active_step = 0;
            let injector = self.context.injector.borrow();
            match injector
                .tutorial()
                .bubble_for(TutorialStep::StepperIntro, injector.l10n())
            {
                Some(bubble) => {
                    session.tutorial_visible = true;
                    session.tutorial_text = bubble.message;
                    session.tutorial_dismiss_label = bubble.dismiss_label;
                    session.tutorial_skip_all_label = bubble.skip_all_label.unwrap_or_default();
                }
                None => {
                    session.tutorial_visible = false;
                    session.tutorial_text.clear();
                    session.tutorial_skip_all_label.clear();
                    session.tutorial_dismiss_label = injector.l10n().t("tutorial.dismiss_button");
                }
            }
        }
        if let Some(context) = self.context.session.borrow().active_context.clone() {
            self.context.export_flow.set_plan(&context);
        }
        let (keys, anchor) = self.context.map_credentials();
        let page = self.context.page();
        let presentation = if keys.0.is_empty() || crate::map_webview::is_visible() {
            Presentation::ready(page)
        } else {
            crate::map_webview::show(
                self.context.window.clone(),
                keys.0,
                keys.1,
                anchor.0,
                anchor.1,
            );
            self.context.mark_map_loading();
            Presentation::processing(self.context.page(), Progress::ZERO)
        };
        presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    fn navigate(&mut self, step: i32) -> Presentation<WorkspacePageState> {
        if !(0..=4).contains(&step) {
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Blocked);
        }
        {
            let mut session = self.context.session.borrow_mut();
            if session.active_plan_id.is_none() {
                let completed = self
                    .context
                    .window
                    .upgrade()
                    .map(|window| window.get_workspace_completed_steps())
                    .unwrap_or(0);
                session.adopt(completed);
            }
        }
        let boundary_confirmed = {
            let session = self.context.session.borrow();
            match session.active_plan_id.as_ref() {
                Some(plan_id) => session
                    .plans
                    .get(plan_id)
                    .is_some_and(|state| state.has_boundary),
                // 迁移期无方案测试/占位路径沿用窗口注入的已完成状态。
                None => session
                    .adopted_completed_steps
                    .is_some_and(|completed| completed > 0),
            }
        };
        // ADR-0041：边界确认是唯一门槛；其余步骤不再形成线性强制链。
        if step != 0 && !boundary_confirmed {
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Blocked);
        }
        if step == 0 {
            let (keys, anchor) = self.context.map_credentials();
            if keys.0.is_empty() {
                let l10n = self.l10n();
                return Presentation::needs_confirmation(
                    self.context.page(),
                    ConfirmationPresentation::new(
                        l10n.t("settings.gaode_empty_key_title"),
                        l10n.t("settings.gaode_empty_key_body"),
                        l10n.t("settings.gaode_go_to_settings"),
                        l10n.t("app.cancel_button"),
                    ),
                );
            }
            self.context.session.borrow_mut().active_step = 0;
            let presentation = if crate::map_webview::is_visible() {
                Presentation::ready(self.context.page())
            } else {
                crate::map_webview::show(
                    self.context.window.clone(),
                    keys.0,
                    keys.1,
                    anchor.0,
                    anchor.1,
                );
                self.context.mark_map_loading();
                Presentation::processing(self.context.page(), Progress::ZERO)
            };
            return presentation.with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        if step == 1 {
            let (keys, anchor) = self.context.map_credentials();
            self.context.session.borrow_mut().active_step = 1;
            let presentation = if keys.0.is_empty() {
                Presentation::ready(self.context.page())
            } else {
                crate::map_webview::hide();
                let existing_boundary_gcj02 = self
                    .context
                    .session
                    .borrow()
                    .active_plan_id
                    .as_ref()
                    .and_then(|plan_id| {
                        self.context
                            .session
                            .borrow()
                            .plans
                            .get(plan_id)
                            .and_then(|state| state.boundary_gcj02.clone())
                    });
                let config = BoundaryEditPageConfig::new(&keys.0, &keys.1)
                    .with_anchor(anchor.0, anchor.1)
                    .with_orientation_mode(true)
                    .with_existing_boundary(existing_boundary_gcj02);
                crate::map_webview::show_with_config(self.context.window.clone(), config);
                self.context.mark_map_loading();
                Presentation::processing(self.context.page(), Progress::ZERO)
            };
            return presentation.with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        self.context.session.borrow_mut().active_step = step;
        Presentation::ready(self.context.page())
            .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    fn leave(&mut self, target: Screen) -> Presentation<WorkspacePageState> {
        let session = self.context.session.borrow();
        // 地图加载中离开边界页：必须停留（ADR-0037 用户故事 18）
        if session.map_processing && session.active_step == 0 {
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Blocked);
        }
        // 存在未确认的边界绘制：需要确认后再离开
        if session.boundary_unsaved() {
            let l10n = self.l10n();
            return Presentation::needs_confirmation(
                self.context.page(),
                ConfirmationPresentation::new(
                    l10n.t("workspace.leave_discard_title"),
                    l10n.t("workspace.leave_discard_body"),
                    l10n.t("dialog.confirm_button"),
                    l10n.t("dialog.cancel_button"),
                ),
            );
        }
        Presentation::ready(self.context.page()).with_navigation(NavigationDecision::Show(target))
    }

    fn boundary_canvas_click(&mut self, x: f32, y: f32) -> Presentation<WorkspacePageState> {
        let rejected = {
            let mut session = self.context.session.borrow_mut();
            match session.drawer.handle_event(BoundaryUiEvent::ClickAt {
                x: f64::from(x),
                y: f64::from(y),
            }) {
                EventResult::Rejected(message) => Some(message),
                EventResult::Accepted | EventResult::Ignored => None,
            }
        };
        if let Some(message) = rejected {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &message));
        }
        Presentation::ready(self.context.page())
    }

    fn boundary_undo(&mut self) -> Presentation<WorkspacePageState> {
        let rejected = {
            let mut session = self.context.session.borrow_mut();
            match session.drawer.handle_event(BoundaryUiEvent::Cancel) {
                EventResult::Rejected(message) => Some(message),
                EventResult::Accepted | EventResult::Ignored => None,
            }
        };
        if let Some(message) = rejected {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &message));
        }
        Presentation::ready(self.context.page())
    }

    fn boundary_confirm(&mut self) -> Presentation<WorkspacePageState> {
        let invalid = {
            let session = self.context.session.borrow();
            let vertices = session.drawer.vertices().to_vec();
            let result = validate_polygon_closure(&vertices);
            if !result.is_valid {
                Some(validation_detail(&result))
            } else {
                None
            }
        };
        if let Some(detail) = invalid {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &detail));
        }
        let (rejected, confirmed_boundary) = {
            let mut session = self.context.session.borrow_mut();
            match session.drawer.handle_event(BoundaryUiEvent::Confirm) {
                EventResult::Accepted => {
                    let fallback_boundary = session.active_context.as_ref().map(|context| {
                        plane_vertices_to_gcj02(
                            session.drawer.vertices(),
                            context.anchor_lng,
                            context.anchor_lat,
                        )
                    });
                    if let Some(plan_id) = session.active_plan_id.clone() {
                        let state = session.plans.entry(plan_id).or_default();
                        state.has_boundary = fallback_boundary.is_some();
                        state.boundary_gcj02 = fallback_boundary.clone();
                    }
                    (None, fallback_boundary)
                }
                EventResult::Rejected(message) => (Some(message), None),
                EventResult::Ignored => (None, None),
            }
        };
        if let Some(message) = rejected {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &message));
        }
        if let Some(coordinates) = confirmed_boundary {
            self.context
                .export_flow
                .set_boundary(Some(coordinates), true);
        }
        Presentation::ready(self.context.page())
    }

    fn boundary_reset(&mut self) -> Presentation<WorkspacePageState> {
        {
            let mut session = self.context.session.borrow_mut();
            session.drawer.reset();
            if let Some(plan_id) = session.active_plan_id.clone() {
                let state = session.plans.entry(plan_id).or_default();
                state.has_boundary = false;
                state.boundary_gcj02 = None;
            }
        }
        self.context.export_flow.set_boundary(None, false);
        Presentation::ready(self.context.page())
    }

    fn orientation_submit(
        &mut self,
        mode: &str,
        angle_text: &str,
    ) -> Presentation<WorkspacePageState> {
        self.context.session.borrow_mut().orientation_input_text = angle_text.to_string();
        if mode == "bearing-angle" {
            let angle: f32 = match angle_text.trim().parse() {
                Ok(angle) => angle,
                Err(_) => {
                    let l10n = self.l10n();
                    return Presentation::failed(self.context.page()).with_notification(
                        error_fact(&l10n, &l10n.t("orientation.error_invalid_angle")),
                    );
                }
            };
            let Some(orientation) = OrientationCalculator::normalize_angle(angle) else {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page()).with_notification(error_fact(
                    &l10n,
                    &l10n.t("orientation.error_angle_out_of_range"),
                ));
            };
            return self.apply_orientation(orientation.degree());
        }
        if mode == "two-points" {
            let angle = {
                let session = self.context.session.borrow();
                if session.orientation_points.len() == 2 {
                    session.orientation_angle
                } else {
                    None
                }
            };
            if let Some(angle) = angle {
                return self.apply_orientation(angle);
            }
        }
        Presentation::ready(self.context.page())
    }

    /// 两点模式或方位角模式提交的统一决策：首次设定直接保存；覆盖已有朝向
    /// 时先返回 F5 影响报告驱动的确认请求，确认后才落库（ADR-0027）。
    fn apply_orientation(&mut self, angle: f32) -> Presentation<WorkspacePageState> {
        let (has_orientation, old_angle, counts) = {
            let session = self.context.session.borrow();
            match session
                .active_plan_id
                .as_ref()
                .and_then(|plan_id| session.plans.get(plan_id))
            {
                Some(state) => (
                    state.has_orientation,
                    state.orientation_angle,
                    state.generated_category_counts.clone(),
                ),
                None => (false, None, HashMap::new()),
            }
        };
        if has_orientation {
            let Some(old) = old_angle.and_then(Orientation::new) else {
                return self.orientation_save_failed();
            };
            let Some(new_orientation) = Orientation::new(angle) else {
                return self.orientation_save_failed();
            };
            let report = check_orientation_change_impact(&counts, Some(old), new_orientation);
            self.context.session.borrow_mut().pending_orientation_angle = Some(angle);
            let l10n = self.l10n();
            return Presentation::needs_confirmation(
                self.context.page(),
                ConfirmationPresentation::new(
                    l10n.t("orientation.recalc_title"),
                    orientation_recalc_body(&l10n, &report),
                    l10n.t("dialog.confirm_button"),
                    l10n.t("dialog.cancel_button"),
                ),
            );
        }
        self.commit_orientation(angle)
    }

    /// 保存朝向到方案正式状态；保存失败时正式状态保持不变并显示明确错误。
    fn commit_orientation(&mut self, angle: f32) -> Presentation<WorkspacePageState> {
        let result = self.context.session.borrow_mut().commit_orientation(angle);
        match result {
            Ok(()) => {
                self.context.export_flow.set_orientation(Some(angle));
                Presentation::ready(self.context.page())
            }
            Err(()) => self.orientation_save_failed(),
        }
    }

    fn orientation_save_failed(&mut self) -> Presentation<WorkspacePageState> {
        let l10n = self.l10n();
        Presentation::failed(self.context.page())
            .with_notification(error_fact(&l10n, &l10n.t("orientation.error_save_failed")))
    }

    /// 地图两点参考线（orientation_points IPC）：F5 计算并把路径/箭头/角度
    /// 回填页面，尚未保存。
    fn orientation_points(&mut self, points: [[f64; 2]; 2]) -> Presentation<WorkspacePageState> {
        if self.orientation_draft_from_points(points).is_err() {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page()).with_notification(error_fact(
                &l10n,
                &l10n.t("orientation.error_coincident_points"),
            ));
        }
        Presentation::ready(self.context.page())
    }

    /// 地图确认朝向（confirm_orientation IPC）：先回填草稿，再走统一的
    /// 首次保存/覆盖确认决策。
    fn confirm_orientation_points(
        &mut self,
        points: [[f64; 2]; 2],
    ) -> Presentation<WorkspacePageState> {
        let degree = match self.orientation_draft_from_points(points) {
            Ok(degree) => degree,
            Err(()) => {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page()).with_notification(error_fact(
                    &l10n,
                    &l10n.t("orientation.error_coincident_points"),
                ));
            }
        };
        self.apply_orientation(degree)
    }

    /// 计算并回填两点草稿（点/角度）；重合或不可计算时清空草稿并返回错误。
    fn orientation_draft_from_points(&mut self, points: [[f64; 2]; 2]) -> Result<f32, ()> {
        let Some(degree) = calculate_orientation_angle(points) else {
            self.clear_orientation_draft();
            return Err(());
        };
        let overlay = self.orientation_overlay_points(points);
        {
            let mut session = self.context.session.borrow_mut();
            session.orientation_points = overlay;
            session.orientation_angle = Some(degree);
        }
        Ok(degree)
    }

    /// 地图“清除重来”（orientation_clear IPC）：只清草稿，不清已保存的
    /// 正式状态。
    fn orientation_clear(&mut self) -> Presentation<WorkspacePageState> {
        self.clear_orientation_draft();
        Presentation::ready(self.context.page())
    }

    /// 只清朝向草稿（点/计算角度/待定角度），不清方案正式状态。
    fn clear_orientation_draft(&self) {
        let mut session = self.context.session.borrow_mut();
        session.orientation_points.clear();
        session.orientation_angle = None;
        session.pending_orientation_angle = None;
    }

    /// 把地图经纬度两点换算成画布平面坐标（原点取已确认边界重心，否则取
    /// 校区锚点；与边界顶点同一坐标系）。
    fn orientation_overlay_points(&self, points: [[f64; 2]; 2]) -> Vec<(f64, f64)> {
        let center = {
            let session = self.context.session.borrow();
            let boundary_center = session
                .active_plan_id
                .as_ref()
                .and_then(|plan_id| session.plans.get(plan_id))
                .and_then(|state| state.boundary_gcj02.as_ref())
                .and_then(|coords| centroid(coords));
            boundary_center.or_else(|| {
                session
                    .active_context
                    .as_ref()
                    .map(|context| (context.anchor_lng, context.anchor_lat))
            })
        };
        let Some((center_lon, center_lat)) = center else {
            return Vec::new();
        };
        let mut converter = CoordinateConverter::default();
        converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
        points
            .iter()
            .filter_map(|[lon, lat]| {
                converter
                    .mercator_to_plane(MercatorCoord::from_lat_lon(*lat, *lon))
                    .map(|plane| (plane.x, plane.y))
            })
            .collect()
    }

    fn orientation_reset(&mut self) -> Presentation<WorkspacePageState> {
        {
            let mut session = self.context.session.borrow_mut();
            session.orientation_points.clear();
            session.orientation_angle = None;
            session.pending_orientation_angle = None;
            session.orientation_input_text.clear();
            if let Some(plan_id) = session.active_plan_id.clone() {
                let state = session.plans.entry(plan_id).or_default();
                state.has_orientation = false;
                state.orientation_angle = None;
                state.generated_category_counts.clear();
            }
        }
        self.context.export_flow.set_orientation(None);
        Presentation::ready(self.context.page())
    }

    fn confirm_orientation(&mut self) -> Presentation<WorkspacePageState> {
        let pending = self
            .context
            .session
            .borrow_mut()
            .pending_orientation_angle
            .take();
        match pending {
            Some(angle) => self.commit_orientation(angle),
            None => Presentation::ready(self.context.page()),
        }
    }

    fn tutorial_dismiss(&mut self) -> Presentation<WorkspacePageState> {
        let result = self
            .context
            .injector
            .borrow_mut()
            .dismiss_tutorial_step(TutorialStep::StepperIntro);
        match result {
            Ok(()) => {
                self.context.session.borrow_mut().tutorial_visible = false;
                Presentation::ready(self.context.page())
            }
            Err(error) => {
                let l10n = self.l10n();
                Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &error.to_string()))
            }
        }
    }

    fn tutorial_skip_all(&mut self) -> Presentation<WorkspacePageState> {
        let result = self.context.injector.borrow_mut().skip_all_tutorial();
        match result {
            Ok(()) => {
                self.context.session.borrow_mut().tutorial_visible = false;
                Presentation::ready(self.context.page())
            }
            Err(error) => {
                let l10n = self.l10n();
                Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &error.to_string()))
            }
        }
    }

    fn map_status(&mut self, available: bool) -> Presentation<WorkspacePageState> {
        {
            let mut session = self.context.session.borrow_mut();
            session.map_processing = false;
        }
        let mut presentation = Presentation::ready(self.context.page());
        if !available {
            let l10n = self.l10n();
            presentation = presentation.with_notification(info_fact(
                &l10n,
                "boundary.map_notice_title",
                &l10n.t("boundary.map_load_failed"),
            ));
        }
        presentation
    }

    fn map_ipc(&mut self, message: &str) -> Presentation<WorkspacePageState> {
        let Ok(parsed) = parse_ipc_message(message) else {
            return Presentation::ready(self.context.page());
        };
        match parsed {
            IpcMessage::OsmElements { elements } => self.osm_elements(elements),
            IpcMessage::ConfirmBoundary { coords } => self.confirm_map_boundary(&coords),
            IpcMessage::BoundaryUpdate { .. }
            | IpcMessage::ManualPoint { .. }
            | IpcMessage::ManualCancel
            | IpcMessage::ManualClear
            | IpcMessage::Coordinate { .. } => Presentation::ready(self.context.page()),
            IpcMessage::OrientationPoints { points } => self.orientation_points(points),
            IpcMessage::ConfirmOrientation { points } => self.confirm_orientation_points(points),
            IpcMessage::OrientationClear => self.orientation_clear(),
            IpcMessage::Error { message } => {
                let l10n = self.l10n();
                Presentation::ready(self.context.page()).with_notification(info_fact(
                    &l10n,
                    "boundary.map_notice_title",
                    &message,
                ))
            }
        }
    }

    fn osm_elements(
        &mut self,
        elements: Vec<gaode_client::OsmElement>,
    ) -> Presentation<WorkspacePageState> {
        let (anchor_lon, anchor_lat, campus_name) = {
            let session = self.context.session.borrow();
            match &session.active_context {
                Some(context) => (
                    context.anchor_lng,
                    context.anchor_lat,
                    Some(context.campus_name.clone()),
                ),
                None => {
                    let injector = self.context.injector.borrow();
                    match injector.projects().landing_campus().ok().flatten() {
                        Some(campus) => (campus.anchor_lng, campus.anchor_lat, Some(campus.name)),
                        None => (116.397, 39.916, None),
                    }
                }
            }
        };
        let sorted = BoundarySorter::sort_candidates(
            elements,
            anchor_lon,
            anchor_lat,
            campus_name.as_deref(),
        );
        let l10n = self.l10n();
        let mut presentation = Presentation::ready(self.context.page());
        match sorted.into_iter().next() {
            Some(best) => {
                let name = best
                    .element
                    .tags
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| l10n.t("boundary.unknown_campus"));
                match best.element.geometry {
                    Some(coords) => {
                        let count = coords.len();
                        let coords_json =
                            serde_json::to_string(&coords).unwrap_or_else(|_| "[]".to_string());
                        let name_json = serde_json::to_string(&name)
                            .unwrap_or_else(|_| "\"未知校区\"".to_string());
                        crate::map_webview::evaluate_script(&format!(
                            "convertAndDraw({coords_json}, {name_json});"
                        ));
                        let body = l10n.t_with_array(
                            "boundary.osm_auto_selected_body",
                            &[&name, &count.to_string()],
                        );
                        presentation = presentation.with_notification(info_fact(
                            &l10n,
                            "boundary.osm_auto_selected_title",
                            &body,
                        ));
                    }
                    None => {
                        crate::map_webview::evaluate_script("enableManualMode();");
                        presentation = presentation.with_notification(info_fact(
                            &l10n,
                            "boundary.osm_no_geometry_title",
                            &l10n.t("boundary.osm_no_geometry_body"),
                        ));
                    }
                }
            }
            None => {
                crate::map_webview::evaluate_script("enableManualMode();");
                presentation = presentation.with_notification(info_fact(
                    &l10n,
                    "boundary.osm_not_found_title",
                    &l10n.t("boundary.osm_not_found_body"),
                ));
            }
        }
        presentation
    }

    fn confirm_map_boundary(&mut self, coords: &[[f64; 2]]) -> Presentation<WorkspacePageState> {
        if coords.len() < 3 {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.error_too_few_points")));
        }
        let Some((center_lon, center_lat)) = centroid(coords) else {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.error_convert_failed")));
        };
        let mut converter = CoordinateConverter::default();
        converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
        let mut vertices = Vec::with_capacity(coords.len());
        for [lon, lat] in coords {
            let mercator = MercatorCoord::from_lat_lon(*lat, *lon);
            let Some(plane) = converter.mercator_to_plane(mercator) else {
                let l10n = self.l10n();
                return Presentation::failed(self.context.page()).with_notification(error_fact(
                    &l10n,
                    &l10n.t("boundary.error_convert_failed"),
                ));
            };
            vertices.push(Vertex::new(plane.x, plane.y));
        }
        let validation = validate_polygon_closure(&vertices);
        if !validation.is_valid {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &validation_detail(&validation)));
        }
        {
            let mut session = self.context.session.borrow_mut();
            if let Some(plan_id) = session.active_plan_id.clone() {
                let state = session.plans.entry(plan_id).or_default();
                state.boundary_gcj02 = Some(coords.to_vec());
                state.has_boundary = true;
            }
            session.drawer.load_determined(vertices);
        }
        self.context
            .export_flow
            .set_boundary(Some(coords.to_vec()), true);
        Presentation::ready(self.context.page())
    }

    pub(crate) fn l10n(&self) -> std::cell::Ref<'_, Localization> {
        std::cell::Ref::map(self.context.injector.borrow(), |injector| injector.l10n())
    }
}

/// 由 F5 计算地图两点的朝向角（正北为 0°、顺时针增加；重合两点返回 None）。
fn calculate_orientation_angle(points: [[f64; 2]; 2]) -> Option<f32> {
    let (x0, y0) = (points[0][0], points[0][1]);
    let (x1, y1) = (points[1][0], points[1][1]);
    OrientationLine::new(Point2D::new(x0, y0), Point2D::new(x1, y1))
        .and_then(|line| OrientationCalculator::calculate(&line))
        .map(|orientation| orientation.degree())
}

/// 坐标数组的简单重心（经纬度平均值）。
fn centroid(coords: &[[f64; 2]]) -> Option<(f64, f64)> {
    if coords.is_empty() {
        return None;
    }
    let (sum_lon, sum_lat) = coords
        .iter()
        .fold((0.0_f64, 0.0_f64), |(slon, slat), point| {
            (slon + point[0], slat + point[1])
        });
    let count = coords.len() as f64;
    Some((sum_lon / count, sum_lat / count))
}

/// 手动画布没有地图经纬度时，沿用 B5 的平面米语义以校区锚点反投影，
/// 让“已确认边界”仍能交给 F9 完整导出用例，不在壳层计算导出尺寸。
fn plane_vertices_to_gcj02(vertices: &[Vertex], anchor_lng: f64, anchor_lat: f64) -> Vec<[f64; 2]> {
    let center = MercatorCoord::from_lat_lon(anchor_lat, anchor_lng);
    let scale = anchor_lat.to_radians().cos();
    vertices
        .iter()
        .map(|vertex| {
            let mercator = MercatorCoord {
                x: center.x + vertex.x / scale,
                y: center.y + vertex.y / scale,
            };
            let (lat, lon) = mercator.to_lat_lon();
            [lon, lat]
        })
        .collect()
}

/// 重算确认窗正文：沿用既有重算提示（collection.orientation_recalc_notice），
/// 并按 F5 影响报告逐项列出已生成数据（类别名经 B6 本地化，ADR-0005）。
fn orientation_recalc_body(l10n: &Localization, report: &OrientationImpactReport) -> String {
    let mut body = l10n.t("collection.orientation_recalc_notice");
    for item in &report.items {
        let count = item.count.to_string();
        body.push('\n');
        body.push_str(&l10n.t_with_array(
            "orientation.impact_item_line",
            &[&l10n.t(category_key(item.category)), &count],
        ));
    }
    body
}

/// B1 六类别 → B6 本地化键（与 collection.category_* 文案一致）。
fn category_key(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "collection.category_building",
        CandidateCategory::Road => "collection.category_road",
        CandidateCategory::Water => "collection.category_water",
        CandidateCategory::Vegetation => "collection.category_vegetation",
        CandidateCategory::Sports => "collection.category_sports",
        _ => "collection.category_other",
    }
}

fn validation_detail(result: &foundation_mode::ValidationResult) -> String {
    result
        .errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// 构建 SVG path 命令字符串（用于 Slint Path 元素渲染连线）。
fn build_path_commands(vertices: &[Vertex], is_closed: bool) -> String {
    if vertices.is_empty() {
        return String::new();
    }
    let mut commands = format!("M {} {}", vertices[0].x, vertices[0].y);
    for vertex in &vertices[1..] {
        commands.push_str(&format!(" L {} {}", vertex.x, vertex.y));
    }
    if is_closed && vertices.len() >= 3 {
        commands.push_str(" Z");
    }
    commands
}

/// 构建方向箭头 Path commands（三角形，按角度旋转；罗盘中心 (50, 50)）。
fn build_arrow_commands(angle: f32) -> String {
    let rad = angle.to_radians();
    let (cx, cy) = (50.0_f32, 50.0_f32);
    let radius = 40.0_f32;
    let base_radius = 25.0_f32;
    let half_width = 8.0_f32;
    let tip_x = cx + radius * rad.sin();
    let tip_y = cy - radius * rad.cos();
    let base_left_x = cx + base_radius * rad.sin() + half_width * rad.cos();
    let base_left_y = cy - base_radius * rad.cos() + half_width * rad.sin();
    let base_right_x = cx + base_radius * rad.sin() - half_width * rad.cos();
    let base_right_y = cy - base_radius * rad.cos() - half_width * rad.sin();
    format!("M {tip_x:.1} {tip_y:.1} L {base_left_x:.1} {base_left_y:.1} L {base_right_x:.1} {base_right_y:.1} Z")
}

fn info_fact(l10n: &Localization, title_key: &str, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::info(
        l10n.t("app.source_tag"),
        l10n.t(title_key),
        body.to_owned(),
    ))
}

fn error_fact(l10n: &Localization, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("app.source_tag"),
        l10n.t("dialog.error_title"),
        body.to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use localization::Language;

    #[test]
    fn commit_orientation_requires_active_plan_and_keeps_formal_state() {
        let mut session = WorkspaceSessionState::default();
        assert!(session.commit_orientation(90.0).is_err());
        assert!(session.plans.is_empty());
        assert!(session.orientation_angle.is_none());

        session.active_plan_id = Some("plan-1".to_string());
        assert!(session.commit_orientation(90.0).is_ok());
        let state = session.plans.get("plan-1").expect("方案正式状态");
        assert!(state.has_orientation);
        assert_eq!(state.orientation_angle, Some(90.0));
        assert_eq!(session.orientation_angle, Some(90.0));
    }

    #[test]
    fn recalc_body_lists_impact_items_with_localized_category_names() {
        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");
        let mut existing = HashMap::new();
        existing.insert(CandidateCategory::Building, 3);
        let report = check_orientation_change_impact(
            &existing,
            Some(Orientation::new(0.0).expect("合法角度")),
            Orientation::new(90.0).expect("合法角度"),
        );
        let body = orientation_recalc_body(&l10n, &report);
        let count = 3usize.to_string();
        let expected_line = l10n.t_with_array(
            "orientation.impact_item_line",
            &[&l10n.t("collection.category_building"), &count],
        );
        assert!(body.contains(&l10n.t("collection.orientation_recalc_notice")));
        assert!(body.contains(&expected_line));
    }
}
