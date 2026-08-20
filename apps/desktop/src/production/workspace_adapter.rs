//! S1-05/06 工作区生产适配器：把一次工作区请求转成一次完整呈现。
//!
//! 适配器只转发完整用户意图并呈现结果：边界闭合、有效性、重置与保存全部经
//! B5 foundation-mode 完成；朝向校验与保存经 F5 OrientationCalculator / B1
//! Orientation；地图通道只负责显示与转交原始动作。状态与上下文见
//! `super::workspace_boundary`（会话状态 / 共享上下文 / 几何与通知辅助）。
// ignore-tidy-filelength: 工作区适配器是“小入口、大实现”的深适配器：对外只有
// `PresentationAdapter<WorkspaceRequest, WorkspacePageState>` 一个小接口，背后
// 承载 open/navigate/边界/朝向/地图事件等完整呈现实现。地图 IPC 与生命周期已由
// 地图会话（map_session，T46/ADR-0045）集中拥有，本文件不再出现按页种类判断或
// 原始 JS；未出现真实的独立变化轴，不为追求行数机械拆分（T47/T48 已并入 T46）。
// 2026-08-18 删除失效期限：此有期限豁免因结构目标已达成而转为可持续理由。
use std::collections::HashMap;

use foundation_mode::{
    check_orientation_change_impact, validate_polygon_closure, BoundaryUiEvent,
    CoordinateConverter, EventResult, MercatorCoord, Orientation, OrientationCalculator, Vertex,
};
use gaode_client::{BoundaryEditPageConfig, IpcMessage};
use localization::Localization;
use onboarding_tutorial::TutorialStep;
use shared_domain_types::PlanId;

use crate::presentation::{
    ConfirmationPresentation, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, Progress, Screen, WorkspacePageState, WorkspaceRequest,
};

#[cfg(test)]
use super::record_entry_call;

use super::workspace_boundary::{
    calculate_orientation_angle, centroid, error_fact, first_outer_ring, info_fact,
    orientation_recalc_body, polygon_coordinates, validation_detail, MapDrawState, MapDrawStatus,
    WorkspaceProductionContext,
};
use super::workspace_leave::{LeaveConfirmationReason, LeaveWorkspaceDecision};

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
            WorkspaceRequest::Resume => self.resume(),
            WorkspaceRequest::Navigate { step } => self.navigate(step),
            WorkspaceRequest::Leave { target } => self.leave(target),
            WorkspaceRequest::BoundaryCanvasClick { x, y } => self.boundary_canvas_click(x, y),
            WorkspaceRequest::BoundaryUndo => self.boundary_undo(),
            WorkspaceRequest::BoundaryDeleteSelected => self.boundary_delete_selected(),
            WorkspaceRequest::BoundaryConfirm => self.boundary_confirm(),
            WorkspaceRequest::BoundaryReset => self.boundary_reset(),
            WorkspaceRequest::BoundaryRefresh => self.boundary_refresh(),
            WorkspaceRequest::OrientationSubmit { mode, angle_text } => {
                self.orientation_submit(&mode, &angle_text)
            }
            WorkspaceRequest::OrientationReset => self.orientation_reset(),
            WorkspaceRequest::DrawerToggle => self.drawer_toggle(),
            WorkspaceRequest::ConfirmOrientation => self.confirm_orientation(),
            WorkspaceRequest::CancelConfirmation => {
                self.context.session.borrow_mut().pending_orientation_angle = None;
                Presentation::ready(self.context.page())
            }
            WorkspaceRequest::TutorialDismiss => self.tutorial_dismiss(),
            WorkspaceRequest::TutorialSkipAll => self.tutorial_skip_all(),
            WorkspaceRequest::MapStatus { available } => self.map_status(available),
            WorkspaceRequest::MapEvent { message } => self.map_event(message),
            WorkspaceRequest::PollBoundaryFetch => self.poll_boundary_fetch(),
        }
    }
}

impl WorkspaceProductionAdapter {
    fn map_plan_id(&self) -> String {
        self.context
            .active_plan_id()
            .unwrap_or_else(|| "__adopted_workspace__".to_owned())
    }

    fn present_boundary_map(&self, keys: (String, String), anchor: (f64, f64)) -> bool {
        crate::map_session::present(
            self.context.window.clone(),
            crate::map_session::MapDisplayIntent::Boundary {
                plan_id: self.map_plan_id(),
                api_key: keys.0,
                security_key: keys.1,
                anchor,
            },
        )
    }

    /// 第五步显示本地 3D 方块预览页（T52）：无需高德密钥，未生成时为空页。
    /// 边界/朝向/评审的连续地图现场保留在地图会话里，切回其他步骤恢复。
    fn present_block_preview(&self) {
        crate::map_session::present(
            self.context.window.clone(),
            crate::map_session::MapDisplayIntent::BlockPreview {
                plan_id: self.map_plan_id(),
            },
        );
    }

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
            session.active_context = Some(context.clone());
            session.orientation_points.clear();
            session.orientation_angle = None;
            session.pending_orientation_angle = None;
            session.adopted_completed_steps = None;
            session.active_step = 0;
            // 切换方案立即作废旧画布；F3 会话入口负责旧请求过期与按方案恢复。
            session.map_processing = false;
            session.drawer.reset();
            session.map_draw = MapDrawState::default();
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
        self.context.export_flow.set_plan(&context);
        self.context.collection_flow.set_plan(&parsed);
        let cached = self
            .context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .open_plan(
                parsed,
                context.campus_name.clone(),
                context.anchor_lng,
                context.anchor_lat,
            );
        if let Some(cached) = cached.as_ref() {
            self.restore_cached_drawer_state(cached);
        }
        // 工作现场恢复：读回上次打开该方案时落库的已确认边界/朝向/步骤。
        // 边界确认后重启不再重复校名解析/Overpass 查询（B.12 修复之一）。
        let (restored_step, restore_notice) = self.restore_persisted_workspace(&parsed);
        self.context.checkpoint_workspace_session();
        if let Err(error) = self
            .context
            .injector
            .borrow_mut()
            .save_last_active_plan(Some(plan_id))
        {
            log::warn!("记录上次打开方案失败: {error}");
        }
        let Some((keys, anchor)) = self.context.map_credentials() else {
            let map_notice_body = {
                let l10n = self.l10n();
                l10n.t("boundary.map_load_failed")
            };
            let mut page = self.context.page();
            // T40：切换方案是输入框显式清空入口（一次性请求，随本次呈现消费；
            // 渲染不再回写输入文本）。
            page.orientation.clear_input = true;
            let mut presentation = Presentation::ready(page).with_notification(info_fact(
                &self.l10n(),
                "boundary.map_notice_title",
                &map_notice_body,
            ));
            if let Some(notice) = restore_notice {
                presentation = presentation.with_notification(notice);
            }
            return self.finish_open_plan(presentation, restored_step);
        };
        // 地图会话按方案身份决定是否复用；同类页面的新方案不会沿用旧现场。
        let mut presentation = if keys.0.is_empty() {
            let mut page = self.context.page();
            page.orientation.clear_input = true;
            Presentation::ready(page)
        } else {
            let changed = self.present_boundary_map(keys, anchor);
            self.context.session.borrow_mut().map_available = false;
            if changed {
                self.context.mark_map_loading();
            }
            let mut page = self.context.page();
            page.orientation.clear_input = true;
            if changed {
                Presentation::processing(page, Progress::ZERO)
            } else {
                Presentation::ready(page)
            }
        };
        if let Some(notice) = restore_notice {
            presentation = presentation.with_notification(notice);
        }
        self.finish_open_plan(presentation, restored_step)
    }

    pub(super) fn navigate(&mut self, step: i32) -> Presentation<WorkspacePageState> {
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
            if session.active_plan_id.is_some() {
                self.context.export_flow.boundary_confirmed()
            } else {
                session
                    .adopted_completed_steps
                    .is_some_and(|completed| completed > 0)
            }
        };
        // ADR-0041：边界确认是唯一门槛；其余步骤不再形成线性强制链。
        if step != 0 && !boundary_confirmed {
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Blocked);
        }
        let destination = match step {
            3 => Some(crate::map_session::MapDestination::Review),
            4 => Some(crate::map_session::MapDestination::Export),
            _ => None,
        };
        if destination.is_some_and(|destination| {
            crate::map_session::prepare(destination)
                == crate::map_session::MapTransition::ConfirmBoundaryDraftDiscard
        }) {
            let l10n = self.l10n();
            return Presentation::needs_confirmation(
                self.context.page(),
                ConfirmationPresentation::new(
                    l10n.t("boundary.draft_leave_title"),
                    l10n.t("boundary.draft_leave_body"),
                    l10n.t("boundary.draft_leave_confirm"),
                    l10n.t("boundary.draft_leave_cancel"),
                ),
            );
        }
        if step == 0 {
            let Some((keys, anchor)) = self.context.map_credentials() else {
                let l10n = self.l10n();
                return Presentation::ready(self.context.page())
                    .with_notification(info_fact(
                        &l10n,
                        "boundary.map_notice_title",
                        &l10n.t("boundary.map_load_failed"),
                    ))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
            };
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
            self.context.checkpoint_workspace_session();
            let changed = self.present_boundary_map(keys, anchor);
            let presentation = if changed {
                self.context.session.borrow_mut().map_available = false;
                self.context.mark_map_loading();
                Presentation::processing(self.context.page(), Progress::ZERO)
            } else {
                Presentation::ready(self.context.page())
            };
            return presentation.with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        if step == 1 {
            let Some((keys, anchor)) = self.context.map_credentials() else {
                let l10n = self.l10n();
                return Presentation::ready(self.context.page())
                    .with_notification(info_fact(
                        &l10n,
                        "boundary.map_notice_title",
                        &l10n.t("boundary.map_load_failed"),
                    ))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
            };
            self.context.session.borrow_mut().active_step = 1;
            self.context.checkpoint_workspace_session();
            let presentation = if keys.0.is_empty() {
                Presentation::ready(self.context.page())
            } else {
                let existing_boundary = self
                    .context
                    .export_flow
                    .boundary_view()
                    .as_ref()
                    .and_then(polygon_coordinates);
                let config = BoundaryEditPageConfig::new(&keys.0, &keys.1)
                    .with_anchor(anchor.0, anchor.1)
                    .with_orientation_mode(true)
                    .with_existing_boundary(existing_boundary);
                crate::map_session::present(
                    self.context.window.clone(),
                    crate::map_session::MapDisplayIntent::Orientation {
                        plan_id: self.map_plan_id(),
                        config,
                    },
                );
                self.context.session.borrow_mut().map_available = false;
                self.context.mark_map_loading();
                Presentation::processing(self.context.page(), Progress::ZERO)
            };
            return presentation.with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        // T38：步骤 ④（索引 3）评审显示评审地图；步骤 ③ 采集与步骤 ⑤ 导出
        // 继续让位显示边界页——从评审步返回时若当前不是边界页则重建，避免
        // 候选标注页串到采集/导出步。
        if step == 3 {
            // 评审地图由评审入口（ReviewRequest::Open）在候选装载后创建并
            // 内嵌标注（候选在 navigate 时尚未载入；T38 弹窗恢复复用缓存）。
            self.context.session.borrow_mut().active_step = 3;
            self.context.checkpoint_workspace_session();
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        if step == 2 {
            // 从评审步返回（或地图页为评审/朝向页）时重建边界页，保证
            // 采集步骤的让位地图始终是边界页。
            if let Some((keys, anchor)) = self.context.map_credentials() {
                if !keys.0.is_empty() && self.present_boundary_map(keys, anchor) {
                    self.context.session.borrow_mut().map_available = false;
                    self.context.mark_map_loading();
                }
            }
        } else if step == 4 {
            // 第五步把地图显示替换为 3D 预览页；预览页不依赖高德地图，
            // 不进入“地图加载中”状态，也不改变边界/朝向/评审现场。
            {
                let mut session = self.context.session.borrow_mut();
                session.map_available = true;
                session.map_processing = false;
            }
            self.present_block_preview();
        }
        self.context.session.borrow_mut().active_step = step;
        self.context.checkpoint_workspace_session();
        Presentation::ready(self.context.page())
            .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    fn leave(&mut self, target: Screen) -> Presentation<WorkspacePageState> {
        match self.context.decide_leave(target) {
            LeaveWorkspaceDecision::Allow { target } => Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Show(target)),
            LeaveWorkspaceDecision::Blocked(_) => Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Blocked),
            LeaveWorkspaceDecision::NeedsConfirmation(reason) => {
                let l10n = self.l10n();
                let (title_key, body_key) = match reason {
                    LeaveConfirmationReason::UnsavedBoundary => (
                        "workspace.leave_discard_title",
                        "workspace.leave_discard_body",
                    ),
                    LeaveConfirmationReason::OperationRunning => (
                        "workspace.leave_running_title",
                        "workspace.leave_running_body",
                    ),
                };
                Presentation::needs_confirmation(
                    self.context.page(),
                    ConfirmationPresentation::new(
                        l10n.t(title_key),
                        l10n.t(body_key),
                        l10n.t("dialog.confirm_button"),
                        l10n.t("dialog.cancel_button"),
                    ),
                )
            }
        }
    }

    fn boundary_canvas_click(&mut self, x: f32, y: f32) -> Presentation<WorkspacePageState> {
        if !crate::map_session::available() {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")));
        }
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
        if crate::map_session::command(crate::map_session::MapCommand::BoundaryUndo)
            == crate::map_session::MapCommandResult::Allowed
        {
            return Presentation::ready(self.context.page());
        }
        let l10n = self.l10n();
        Presentation::failed(self.context.page())
            .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")))
    }

    /// 抽屉"删除选中点"：地图可用时走 JS 桥接命令（JS 校验剩余点数并删除
    /// 选中顶点后经 boundary_update/vertex_deselected 回传）；地图不可用时
    /// 无可删对象，保持现状。
    fn boundary_delete_selected(&mut self) -> Presentation<WorkspacePageState> {
        let result =
            crate::map_session::command(crate::map_session::MapCommand::BoundaryDeleteSelected);
        if result == crate::map_session::MapCommandResult::Allowed {
            Presentation::ready(self.context.page())
        } else {
            let l10n = self.l10n();
            Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")))
        }
    }

    fn boundary_confirm(&mut self) -> Presentation<WorkspacePageState> {
        // T34：地图可用时确认走抽屉按钮 → JS 桥接命令（JS 读取当前多边形/
        // 人工点序列后经 confirm_boundary IPC 回传，由 B5 校验并落库）。
        if crate::map_session::command(crate::map_session::MapCommand::SubmitBoundary)
            == crate::map_session::MapCommandResult::Allowed
        {
            return Presentation::ready(self.context.page());
        }
        let l10n = self.l10n();
        Presentation::failed(self.context.page())
            .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")))
    }

    fn boundary_reset(&mut self) -> Presentation<WorkspacePageState> {
        if crate::map_session::command(crate::map_session::MapCommand::BoundaryClear)
            == crate::map_session::MapCommandResult::Unavailable
        {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")));
        }
        {
            let mut session = self.context.session.borrow_mut();
            session.drawer.reset();
            session.map_draw = MapDrawState::default();
            session.map_processing = false;
        }
        self.context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .clear_active();
        self.context.export_flow.reset_boundary();
        self.context.collection_flow.reset_boundary();
        self.context.checkpoint_workspace_session();
        Presentation::ready(self.context.page())
    }

    fn orientation_submit(
        &mut self,
        mode: &str,
        angle_text: &str,
    ) -> Presentation<WorkspacePageState> {
        if !crate::map_session::available() {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")));
        }
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
            let result =
                crate::map_session::command(crate::map_session::MapCommand::SubmitOrientation);
            return if result == crate::map_session::MapCommandResult::Allowed {
                Presentation::ready(self.context.page())
            } else {
                let l10n = self.l10n();
                Presentation::failed(self.context.page())
                    .with_notification(error_fact(&l10n, &l10n.t("boundary.map_load_failed")))
            };
        }
        Presentation::ready(self.context.page())
    }

    /// 两点模式或方位角模式提交的统一决策：首次设定直接保存；覆盖已有朝向
    /// 时先返回 F5 影响报告驱动的确认请求，确认后才落库（ADR-0027）。
    fn apply_orientation(&mut self, angle: f32) -> Presentation<WorkspacePageState> {
        let (has_orientation, old_angle) = {
            let session = self.context.session.borrow();
            match session
                .active_plan_id
                .as_ref()
                .and_then(|plan_id| session.plans.get(plan_id))
            {
                Some(state) => (state.has_orientation, state.orientation_angle),
                None => (false, None),
            }
        };
        if has_orientation {
            let Some(old) = old_angle.and_then(Orientation::new) else {
                return self.orientation_save_failed();
            };
            let Some(new_orientation) = Orientation::new(angle) else {
                return self.orientation_save_failed();
            };
            // 采集迁出后：已生成数据分布由 A1/F5 完整用例持有，本入口不再
            // 拼接影响报告输入（当前空分布，与 S1-07 前行为一致）。
            let counts = HashMap::new();
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
                self.context.checkpoint_workspace_session();
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
        let mut page = self.context.page();
        page.orientation.fill_input = Some(format!("{degree:.1}"));
        Presentation::ready(page)
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
        let mut page = self.context.page();
        // T40：地图"清除重来"是输入框显式清空入口（一次性请求，随本次呈现消费）。
        page.orientation.clear_input = true;
        Presentation::ready(page)
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
        let boundary_center = self
            .context
            .export_flow
            .boundary_view()
            .as_ref()
            .and_then(polygon_coordinates)
            .and_then(|coords| centroid(&coords));
        let center = {
            let session = self.context.session.borrow();
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
            if let Some(plan_id) = session.active_plan_id.clone() {
                let state = session.plans.entry(plan_id).or_default();
                state.has_orientation = false;
                state.orientation_angle = None;
            }
        }
        // T34：地图可用时同步清空 JS 侧朝向两点草稿（纯画布 + 消息桥）
        let _ = crate::map_session::command(crate::map_session::MapCommand::OrientationClear);
        self.context.export_flow.set_orientation(None);
        self.context.checkpoint_workspace_session();
        let mut page = self.context.page();
        // T40：抽屉"重置"是输入框显式清空入口（一次性请求，随本次呈现消费）。
        page.orientation.clear_input = true;
        Presentation::ready(page)
    }

    /// T34：左侧抽屉开合（做法 A：展开时地图右移让位，收起时恢复原宽）。
    /// 纯页面临时状态，由工作区入口持有，S1 只呈现。
    fn drawer_toggle(&mut self) -> Presentation<WorkspacePageState> {
        {
            let mut session = self.context.session.borrow_mut();
            session.drawer_open = !session.drawer_open;
        }
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
            session.map_available = available;
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

    fn map_event(&mut self, message: IpcMessage) -> Presentation<WorkspacePageState> {
        match message {
            // T37：地图就绪后按当前步骤激活对应交互模式——朝向步（步骤②）
            // 显式调用 ORIENTATION_SCRIPT 的 initOrientationMode 挂接两点
            // 选择点击处理器；边界步仍走 T31 Rust 侧 OSM 自动获取。
            IpcMessage::MapReady => self.map_ready_for_active_step(),
            // T24 旧路径已由 T31 Rust 侧 Overpass 取代：HTML 不再发送
            // osm_elements，保留解析兼容但不再走 convertAndDraw 死接线。
            IpcMessage::OsmElements { .. } => Presentation::ready(self.context.page()),
            IpcMessage::ConfirmBoundary { coords } => self.confirm_map_boundary(&coords),
            IpcMessage::ConfirmBoundaryGeometry {
                r#type,
                coordinates,
            } => self.confirm_map_geometry(&r#type, coordinates),
            // T34：编辑/圈画 IPC 同步抽屉 ① 的点数与状态（纯呈现，几何真相
            // 仍在地图 JS/B5 侧）。
            IpcMessage::BoundaryUpdate { coords } => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = coords.len() as i32;
                    session.map_draw.status = MapDrawStatus::Editing;
                }
                Presentation::ready(self.context.page())
            }
            // 顶点选中：抽屉"删除选中点"按钮可用（几何真相仍在地图 JS 侧）。
            IpcMessage::VertexSelected { index, count } => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = count as i32;
                    session.map_draw.status = MapDrawStatus::Editing;
                    session.map_draw.selected_vertex = Some(index as i32);
                }
                Presentation::ready(self.context.page())
            }
            IpcMessage::VertexDeselected => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.selected_vertex = None;
                }
                Presentation::ready(self.context.page())
            }
            // 删除被拒绝（剩余点数 < 3）：明确提示，不破坏边界。
            IpcMessage::DeleteVertexRejected { .. } => {
                let l10n = self.l10n();
                Presentation::failed(self.context.page()).with_notification(error_fact(
                    &l10n,
                    &l10n.t("boundary.error_too_few_after_delete"),
                ))
            }
            IpcMessage::BoundaryGeometryUpdate { .. } => Presentation::ready(self.context.page()),
            IpcMessage::ManualPoint { total, .. } => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = total as i32;
                    session.map_draw.status = MapDrawStatus::Drawing;
                }
                Presentation::ready(self.context.page())
            }
            IpcMessage::ManualCancel => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = (session.map_draw.point_count - 1).max(0);
                    session.map_draw.status = MapDrawStatus::Drawing;
                }
                Presentation::ready(self.context.page())
            }
            IpcMessage::ManualClear => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw = MapDrawState::default();
                    session.map_processing = false;
                }
                self.context
                    .injector
                    .borrow_mut()
                    .boundary_session_mut()
                    .clear_active();
                self.context.checkpoint_workspace_session();
                Presentation::ready(self.context.page())
            }
            IpcMessage::Coordinate { .. } => Presentation::ready(self.context.page()),
            IpcMessage::OrientationPoints { points } => self.orientation_points(points),
            IpcMessage::ConfirmOrientation { points } => self.confirm_orientation_points(points),
            IpcMessage::OrientationClear => self.orientation_clear(),
            // 地图会话按场景路由评审事件，不会把它交给工作区适配器；此处
            // 保留显式空分支满足穷尽匹配。
            IpcMessage::ReviewObjectClicked { .. } => Presentation::ready(self.context.page()),
            // 评审地图文字开关只由评审入口处理；此处保留空分支满足穷尽匹配。
            IpcMessage::ReviewMapTextToggled { .. } => Presentation::ready(self.context.page()),
            // 视野属于地图会话现场，进入工作区前已由 map_session 消费。
            IpcMessage::ViewportChanged { .. } => Presentation::ready(self.context.page()),
            IpcMessage::Error { message } => {
                let l10n = self.l10n();
                // T36：页面 onerror / 5s SDK 超时（及 Rust 侧 10s 加载超时）→
                // 明确错误对话框，不静默；隐藏损坏地图并如实上报不可用，用户
                // 依赖地图事实的新操作必须暂停。超时标记由地图会话的底层
                // WebView 适配器回传，这里只负责本地化用户反馈。
                let body = if crate::map_session::is_load_timeout(&message) {
                    l10n.t("map.load_timeout_body")
                } else {
                    l10n.t_with_array("map.load_failed_body", &[&message])
                };
                crate::map_session::mark_failed();
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_processing = false;
                    session.map_available = false;
                }
                Presentation::ready(self.context.page()).with_notification(error_fact(&l10n, &body))
            }
        }
    }

    /// T37：地图就绪后按当前步骤激活交互模式。
    ///
    /// 朝向步（步骤②）：向地图会话下发激活命令，挂接两点选择点击处理器并
    /// 回显已确认边界半透明参照，确保朝向步地图点击可点。
    /// 边界步（其余步骤）：仍走 [`Self::start_boundary_fetch`]（T31 Rust
    /// 侧 Nominatim → Overpass → WGS→GCJ 自动获取）。
    fn map_ready_for_active_step(&mut self) -> Presentation<WorkspacePageState> {
        let active_step = self.context.session.borrow().active_step;
        if active_step == 1 {
            let _ =
                crate::map_session::command(crate::map_session::MapCommand::OrientationActivate);
            Presentation::ready(self.context.page())
        } else if active_step == 0 && !crate::map_session::has_boundary_draft() {
            self.start_boundary_fetch()
        } else {
            // 采集/评审/导出步骤的地图只是让位显示：迟到的 map_ready 既不应
            // 重新触发边界获取（边界只属于步骤①，B 工单“避免重复查询”），
            // 也不得用工作区页面覆盖采集/导出入口的进度呈现（恢复工作流实测
            // 暴露的竞态：导出进行中 map_ready 会把状态重置回 Ready）。
            Presentation::ready(self.context.page())
        }
    }

    fn confirm_map_boundary(&mut self, coords: &[[f64; 2]]) -> Presentation<WorkspacePageState> {
        self.confirm_map_geometry("Polygon", serde_json::json!([coords]))
    }

    fn confirm_map_geometry(
        &mut self,
        boundary_type: &str,
        coordinates: serde_json::Value,
    ) -> Presentation<WorkspacePageState> {
        let Some(coords) = first_outer_ring(boundary_type, &coordinates) else {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.error_too_few_points")));
        };
        if coords.len() < 3 {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.error_too_few_points")));
        }
        let Some((center_lon, center_lat)) = centroid(&coords) else {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &l10n.t("boundary.error_convert_failed")));
        };
        let mut converter = CoordinateConverter::default();
        converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
        let mut vertices = Vec::with_capacity(coords.len());
        for [lon, lat] in &coords {
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
            session.drawer.load_determined(vertices);
            // T34：确认后由 session.drawer（已确定态）接管点数/状态显示
            session.map_draw = MapDrawState::default();
        }
        let boundary = shared_domain_types::Boundary {
            r#type: boundary_type.to_owned(),
            coordinates: coordinates.clone(),
        };
        self.context
            .collection_flow
            .confirm_boundary(boundary.clone());
        self.context
            .export_flow
            .confirm_boundary(boundary_type, coordinates);
        self.cache_confirmed_boundary(&coords);
        crate::map_session::boundary_committed();
        self.context.checkpoint_workspace_session();
        let mut presentation = Presentation::ready(self.context.page());
        if let Some(notice) = self.revalidate_boundary_after_confirm(&boundary) {
            presentation = presentation.with_notification(notice);
        }
        presentation
    }

    /// 边界确认后触发本地候选资格重验证（D 工单）。
    ///
    /// 指纹相同（边界未变化）时不触发任何计算；失败只提示不阻断：已确认
    /// 边界与现有评审状态保持不变（验收 7）。全程不联网。
    fn revalidate_boundary_after_confirm(
        &self,
        boundary: &shared_domain_types::Boundary,
    ) -> Option<NotificationFact> {
        let plan_id = self.context.session.borrow().active_plan_id.clone()?;
        let Ok(plan_id) = PlanId::parse(&plan_id) else {
            return None;
        };
        match self
            .context
            .collection_flow
            .revalidate_boundary_if_changed(&plan_id, boundary)
        {
            Ok(report) => {
                if report.boundary_changed {
                    log::info!(
                        "边界候选资格本地重验证完成（plan={plan_id}）：examined={}，\
                         isolated={}，reviewable={}，voided={}，pending={}",
                        report.examined,
                        report.newly_isolated.len(),
                        report.newly_reviewable.len(),
                        report.decisions_voided,
                        report.decisions_reset_to_pending
                    );
                }
                None
            }
            Err(error) => {
                log::warn!("边界候选资格本地重验证失败（plan={plan_id}）: {error}");
                let l10n = self.l10n();
                Some(error_fact(
                    &l10n,
                    &l10n.t("boundary.error_revalidation_failed"),
                ))
            }
        }
    }

    pub(crate) fn l10n(&self) -> std::cell::Ref<'_, Localization> {
        std::cell::Ref::map(self.context.injector.borrow(), |injector| injector.l10n())
    }
}
