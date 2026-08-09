//! S1-05/06 工作区生产适配器：把一次工作区请求转成一次完整呈现。
//!
//! 适配器只转发完整用户意图并呈现结果：边界闭合、有效性、重置与保存全部经
//! B5 foundation-mode 完成；朝向校验与保存经 F5 OrientationCalculator / B1
//! Orientation；地图通道只负责显示与转交原始动作。状态与上下文见
//! `super::workspace_boundary`（会话状态 / 共享上下文 / 几何与通知辅助）。
// ignore-tidy-filelength: T38 评审步地图让位（评审地图 + 返回采集/导出重建边界页）并入导航
// 后短暂超限；失效里程碑：v2.1.0（2026-12-31），届时按职责拆出地图导航决策助手后消除

use std::collections::HashMap;
use std::sync::mpsc;

use data_acquisition::overpass::{CampusBoundaryFetcher, CampusBoundaryResult};
use foundation_mode::{
    check_orientation_change_impact, validate_polygon_closure, BoundaryUiEvent,
    CoordinateConverter, EventResult, MercatorCoord, Orientation, OrientationCalculator, Vertex,
};
use gaode_client::{parse_ipc_message, BoundaryEditPageConfig, IpcMessage};
use localization::Localization;
use onboarding_tutorial::TutorialStep;
use shared_domain_types::PlanId;

use crate::presentation::{
    ConfirmationPresentation, NavigationDecision, Presentation, PresentationAdapter, Progress,
    Screen, WorkspacePageState, WorkspaceRequest,
};

#[cfg(test)]
use super::record_entry_call;

use super::workspace_boundary::{
    calculate_orientation_angle, centroid, error_fact, first_outer_ring, info_fact,
    orientation_recalc_body, plane_vertices_to_gcj02, polygon_coordinates, validation_detail,
    MapDrawState, MapDrawStatus, WorkspaceProductionContext,
};

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
            WorkspaceRequest::DrawerToggle => self.drawer_toggle(),
            WorkspaceRequest::ConfirmOrientation => self.confirm_orientation(),
            WorkspaceRequest::CancelConfirmation => {
                self.context.session.borrow_mut().pending_orientation_angle = None;
                Presentation::ready(self.context.page())
            }
            WorkspaceRequest::TutorialDismiss => self.tutorial_dismiss(),
            WorkspaceRequest::TutorialSkipAll => self.tutorial_skip_all(),
            WorkspaceRequest::MapStatus { available } => self.map_status(available),
            WorkspaceRequest::MapIpc { message } => self.map_ipc(&message),
            WorkspaceRequest::PollBoundaryFetch => self.poll_boundary_fetch(),
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
            if let Ok(plan_id) = shared_domain_types::PlanId::parse(&context.plan_id) {
                self.context.collection_flow.set_plan(&plan_id);
            }
        }
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
        let page = self.context.page();
        // T34：打开方案必须落在边界页——若残留其他页 WebView（如朝向页），
        // 先重建为边界页，避免"恢复错页"。
        let map_is_boundary =
            crate::map_webview::is_visible() && crate::map_webview::is_boundary_page();
        let presentation = if keys.0.is_empty() || map_is_boundary {
            Presentation::ready(page)
        } else {
            crate::map_webview::hide();
            crate::map_webview::show(
                self.context.window.clone(),
                keys.0,
                keys.1,
                anchor.0,
                anchor.1,
            );
            self.context.session.borrow_mut().map_available = true;
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
            // T34：进入边界步骤必须落在边界页（朝向页 WebView 不能留在本步骤）
            let map_is_boundary =
                crate::map_webview::is_visible() && crate::map_webview::is_boundary_page();
            let presentation = if map_is_boundary {
                Presentation::ready(self.context.page())
            } else {
                crate::map_webview::hide();
                crate::map_webview::show(
                    self.context.window.clone(),
                    keys.0,
                    keys.1,
                    anchor.0,
                    anchor.1,
                );
                self.context.session.borrow_mut().map_available = true;
                self.context.mark_map_loading();
                Presentation::processing(self.context.page(), Progress::ZERO)
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
            let presentation = if keys.0.is_empty() {
                Presentation::ready(self.context.page())
            } else {
                crate::map_webview::hide();
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
                crate::map_webview::show_with_config(self.context.window.clone(), config);
                self.context.session.borrow_mut().map_available = true;
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
            return Presentation::ready(self.context.page())
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        }
        if matches!(step, 2 | 4) {
            // 从评审步返回（或地图页为评审/朝向页）时重建边界页，保证
            // 采集/导出步骤的让位地图始终是边界页。
            let map_is_boundary =
                crate::map_webview::is_visible() && crate::map_webview::is_boundary_page();
            if !map_is_boundary {
                if let Some((keys, anchor)) = self.context.map_credentials() {
                    if !keys.0.is_empty() {
                        crate::map_webview::hide();
                        crate::map_webview::show(
                            self.context.window.clone(),
                            keys.0,
                            keys.1,
                            anchor.0,
                            anchor.1,
                        );
                        self.context.session.borrow_mut().map_available = true;
                        self.context.mark_map_loading();
                    }
                }
            }
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
        // T34：地图可用时撤销走抽屉按钮 → JS 桥接命令（地图退化为纯画布 +
        // 消息桥）；地图不可用时回落到 Slint 兜底画布状态机。
        if crate::map_webview::is_visible() {
            crate::map_webview::evaluate_script("undoManualPointFromDrawer();");
            return Presentation::ready(self.context.page());
        }
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
        // T34：地图可用时确认走抽屉按钮 → JS 桥接命令（JS 读取当前多边形/
        // 人工点序列后经 confirm_boundary IPC 回传，由 B5 校验并落库）。
        if crate::map_webview::is_visible() {
            crate::map_webview::evaluate_script("submitBoundaryFromDrawer();");
            return Presentation::ready(self.context.page());
        }
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
        let rejected = {
            let mut session = self.context.session.borrow_mut();
            match session.drawer.handle_event(BoundaryUiEvent::Confirm) {
                EventResult::Accepted => None,
                EventResult::Rejected(message) => Some(message),
                EventResult::Ignored => None,
            }
        };
        if let Some(message) = rejected {
            let l10n = self.l10n();
            return Presentation::failed(self.context.page())
                .with_notification(error_fact(&l10n, &message));
        }
        let coordinates = {
            let session = self.context.session.borrow();
            let Some(context) = session.active_context.as_ref() else {
                return Presentation::failed(self.context.page());
            };
            serde_json::json!([plane_vertices_to_gcj02(
                session.drawer.vertices(),
                context.anchor_lng,
                context.anchor_lat,
            )])
        };
        self.context
            .collection_flow
            .confirm_boundary(shared_domain_types::Boundary {
                r#type: "Polygon".to_owned(),
                coordinates: coordinates.clone(),
            });
        self.context
            .export_flow
            .confirm_boundary("Polygon", coordinates);
        Presentation::ready(self.context.page())
    }

    fn boundary_reset(&mut self) -> Presentation<WorkspacePageState> {
        {
            let mut session = self.context.session.borrow_mut();
            // T34：地图可用时清空 JS 侧绘制（多边形/编辑器/人工点），
            // 并同时复位本地兜底画布与地图侧呈现状态。
            if crate::map_webview::is_visible() {
                crate::map_webview::evaluate_script("clearManualDrawingFromDrawer();");
            }
            session.drawer.reset();
            session.map_draw = MapDrawState::default();
        }
        self.context.export_flow.reset_boundary();
        self.context.collection_flow.reset_boundary();
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
            session.orientation_input_text.clear();
            if let Some(plan_id) = session.active_plan_id.clone() {
                let state = session.plans.entry(plan_id).or_default();
                state.has_orientation = false;
                state.orientation_angle = None;
            }
        }
        // T34：地图可用时同步清空 JS 侧朝向两点草稿（纯画布 + 消息桥）
        if crate::map_webview::is_visible() {
            crate::map_webview::evaluate_script("clearOrientationFromDrawer();");
        }
        self.context.export_flow.set_orientation(None);
        Presentation::ready(self.context.page())
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

    fn map_ipc(&mut self, message: &str) -> Presentation<WorkspacePageState> {
        let Ok(parsed) = parse_ipc_message(message) else {
            return Presentation::ready(self.context.page());
        };
        match parsed {
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
                }
                Presentation::ready(self.context.page())
            }
            IpcMessage::Coordinate { .. } => Presentation::ready(self.context.page()),
            IpcMessage::OrientationPoints { points } => self.orientation_points(points),
            IpcMessage::ConfirmOrientation { points } => self.confirm_orientation_points(points),
            IpcMessage::OrientationClear => self.orientation_clear(),
            // T38：评审地图 IPC 由 handle_map_ipc 在 is_review_page 分支路由，
            // 不会到达工作区适配器；此处保留显式空分支满足穷尽匹配。
            IpcMessage::ReviewObjectClicked { .. } => Presentation::ready(self.context.page()),
            IpcMessage::Error { message } => {
                let l10n = self.l10n();
                // T36：页面 onerror / 5s SDK 超时（及 Rust 侧 10s 加载超时）→
                // 明确错误对话框，不静默；隐藏损坏地图并如实上报不可用，用户
                // 可退回左侧抽屉"方位角手动输入"完成朝向。超时标记由
                // map_webview 在 Rust 侧兜底超时时回传，这里本地化。
                let body = if message == crate::map_webview::MAP_LOAD_TIMEOUT_MARKER {
                    l10n.t("map.load_timeout_body")
                } else {
                    l10n.t_with_array("map.load_failed_body", &[&message])
                };
                crate::map_webview::mark_map_failed();
                crate::map_webview::hide();
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
    /// 朝向步（步骤②）：经 `map_webview::evaluate_script` 显式调用页面
    /// `initOrientationMode()`（页面 ORIENTATION_SCRIPT 已定义，:154），
    /// 挂接两点选择点击处理器并回显已确认边界半透明参照；这是对页面自身
    /// `onMapReadyForMode` 自动激活的防御性兜底，确保朝向步地图点击可点。
    /// 边界步（其余步骤）：仍走 [`Self::start_boundary_fetch`]（T31 Rust
    /// 侧 Nominatim → Overpass → WGS→GCJ 自动获取）。
    fn map_ready_for_active_step(&mut self) -> Presentation<WorkspacePageState> {
        let active_step = self.context.session.borrow().active_step;
        if active_step == 1 {
            crate::map_webview::evaluate_script("initOrientationMode();");
            Presentation::ready(self.context.page())
        } else {
            self.start_boundary_fetch()
        }
    }

    /// T31：地图就绪 → 后台线程执行 OSM 边界自动获取（Nominatim → Overpass
    /// 端点回退 → WGS→GCJ → ADR-0029 排序），不阻塞 UI 线程；结果经
    /// [`WorkspaceRequest::PollBoundaryFetch`] 轮询取回。
    fn start_boundary_fetch(&mut self) -> Presentation<WorkspacePageState> {
        let Some((campus_name, anchor_lon, anchor_lat)) = ({
            let session = self.context.session.borrow();
            session
                .active_context
                .as_ref()
                .map(|context| {
                    (
                        context.campus_name.clone(),
                        context.anchor_lng,
                        context.anchor_lat,
                    )
                })
                .or_else(|| {
                    self.context
                        .injector
                        .borrow()
                        .projects()
                        .landing_campus()
                        .ok()
                        .flatten()
                        .map(|campus| (campus.name, campus.anchor_lng, campus.anchor_lat))
                })
        }) else {
            // 没有真实校区上下文时不得用固定坐标参与 OSM 查询：直接人工圈画。
            crate::map_webview::evaluate_script("enableManualMode();");
            let l10n = self.l10n();
            return Presentation::ready(self.context.page()).with_notification(info_fact(
                &l10n,
                "boundary.osm_not_found_title",
                &l10n.t("boundary.osm_not_found_body"),
            ));
        };
        {
            let session = self.context.session.borrow();
            if session.pending_boundary_fetch.is_some() {
                // 已有进行中的获取：保持处理态，不重复发起。
                return Presentation::processing(self.context.page(), Progress::ZERO);
            }
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let fetcher = CampusBoundaryFetcher::production();
            let outcome = fetcher.fetch_campus(&campus_name, anchor_lon, anchor_lat);
            let _ = tx.send(outcome);
        });
        {
            let mut session = self.context.session.borrow_mut();
            session.pending_boundary_fetch = Some(rx);
            session.map_processing = true;
        }
        Presentation::processing(self.context.page(), Progress::ZERO)
    }

    /// 轮询后台边界获取结果：终态到达即应用（绘制/人工圈画兜底），未到保持处理态。
    fn poll_boundary_fetch(&mut self) -> Presentation<WorkspacePageState> {
        // T38 根因：无待处理的后台获取（评审步等非边界场景误触发轮询）时，
        // 必须先释放 session 借用再呈现页面——旧实现直接在
        // `session.borrow_mut()` 存活期间调用 `self.context.page()`，触发
        // RefCell already mutably borrowed → 主线程 panic → 进程退出时
        // TLS 析构 drop WebView → Close() → combase.dll 0xc0000005 崩溃。
        if self
            .context
            .session
            .borrow()
            .pending_boundary_fetch
            .is_none()
        {
            return Presentation::ready(self.context.page());
        }
        let outcome = {
            let mut session = self.context.session.borrow_mut();
            match session.pending_boundary_fetch.as_mut() {
                // 双检兜底（主线程无并发，理论不可达）：复位处理态并让
                // 借用在块尾释放，绝不在此处调用 page()。
                None => {
                    session.map_processing = false;
                    None
                }
                Some(receiver) => match receiver.try_recv() {
                    Ok(outcome) => {
                        session.pending_boundary_fetch = None;
                        session.map_processing = false;
                        Some(outcome)
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        session.pending_boundary_fetch = None;
                        session.map_processing = false;
                        None
                    }
                },
            }
        };
        let Some(outcome) = outcome else {
            return Presentation::processing(self.context.page(), Progress::ZERO);
        };
        self.apply_boundary_fetch_outcome(outcome)
    }

    /// 应用边界获取结果：自动绘制（来源标注）或人工圈画兜底（明确提示）。
    fn apply_boundary_fetch_outcome(
        &mut self,
        outcome: CampusBoundaryResult,
    ) -> Presentation<WorkspacePageState> {
        let l10n = self.l10n();
        let mut presentation = Presentation::ready(self.context.page());
        match outcome {
            CampusBoundaryResult::AutoSelected {
                name,
                gcj02,
                source,
                candidate_count,
            } => {
                let coords_json =
                    serde_json::to_string(&gcj02).unwrap_or_else(|_| "[]".to_string());
                let name_json =
                    serde_json::to_string(&name).unwrap_or_else(|_| "\"未知校区\"".to_string());
                crate::map_webview::evaluate_script(&format!(
                    "drawBoundaryGcj({coords_json}, {name_json});"
                ));
                // T34：抽屉 ① 点数/状态跟随 OSM 自动绘制（编辑模式）
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = gcj02.len() as i32;
                    session.map_draw.status = MapDrawStatus::Editing;
                }
                let body = l10n.t_with_array(
                    "boundary.osm_auto_selected_body",
                    &[&name, &candidate_count.to_string(), &source.to_string()],
                );
                presentation = presentation.with_notification(info_fact(
                    &l10n,
                    "boundary.osm_auto_selected_title",
                    &body,
                ));
            }
            CampusBoundaryResult::NotFound => {
                crate::map_webview::evaluate_script("enableManualMode();");
                presentation = presentation.with_notification(info_fact(
                    &l10n,
                    "boundary.osm_not_found_title",
                    &l10n.t("boundary.osm_not_found_body"),
                ));
            }
            CampusBoundaryResult::Unreachable { message } => {
                log::warn!("OSM 边界自动获取失败（人工圈画兜底）: {message}");
                crate::map_webview::evaluate_script("enableManualMode();");
                presentation = presentation.with_notification(info_fact(
                    &l10n,
                    "boundary.osm_not_found_title",
                    &l10n.t("boundary.osm_unreachable_body"),
                ));
            }
        }
        presentation
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
        self.context
            .collection_flow
            .confirm_boundary(shared_domain_types::Boundary {
                r#type: boundary_type.to_owned(),
                coordinates: coordinates.clone(),
            });
        self.context
            .export_flow
            .confirm_boundary(boundary_type, coordinates);
        Presentation::ready(self.context.page())
    }

    pub(crate) fn l10n(&self) -> std::cell::Ref<'_, Localization> {
        std::cell::Ref::map(self.context.injector.borrow(), |injector| injector.l10n())
    }
}
