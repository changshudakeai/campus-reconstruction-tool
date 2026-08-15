//! 方案边界获取的呈现适配。
//!
//! F3 `PlanBoundarySession` 拥有缓存、刷新、在途去重与陈旧结果状态机；本文件只
//! 把其结构化结果翻译为工作区页面状态、通知与地图绘制命令。

use foundation_mode::{CoordinateConverter, MercatorCoord, Vertex};
use localization::Localization;
use notification_center::Notification;
use project_management::{
    BoundaryFetchOutcome, BoundaryFetchStage, BoundaryPoll, BoundaryRequest, PlanBoundaryView,
};
use shared_domain_types::{Boundary, PlanId};

use crate::presentation::{
    NavigationDecision, NotificationFact, Presentation, Progress, Screen, WorkspacePageState,
};

use super::workspace_adapter::WorkspaceProductionAdapter;
use super::workspace_boundary::{
    centroid, info_fact, valid_restored_ring, MapDrawState, MapDrawStatus,
};

/// 工作现场恢复失败的明确提示（A.7：不静默伪造"已确认"）。
fn restore_failed_fact(l10n: &Localization) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("app.source_tag"),
        l10n.t("boundary.restore_failed_title"),
        l10n.t("boundary.restore_failed_body"),
    ))
}

impl WorkspaceProductionAdapter {
    /// open_plan 收尾：恢复的步骤非 0 且边界已确认时，直接导航到该步骤
    /// （工作区步骤一并恢复，A.1）；否则按首开落在边界页。
    pub(super) fn finish_open_plan(
        &mut self,
        presentation: Presentation<WorkspacePageState>,
        restored_step: i32,
    ) -> Presentation<WorkspacePageState> {
        if restored_step != 0 && self.context.export_flow.boundary_confirmed() {
            return self.navigate(restored_step);
        }
        presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    /// 读回持久化工作区状态并重新种子会话/导出/采集输入。
    ///
    /// 返回 (恢复步骤, 恢复失败提示)；数据损坏/缺失时明确提示且绝不伪造
    /// "已确认"（A.7）。
    pub(super) fn restore_persisted_workspace(
        &mut self,
        plan_id: &PlanId,
    ) -> (i32, Option<NotificationFact>) {
        let plan_key = plan_id.to_string();
        let loaded = self
            .context
            .injector
            .borrow()
            .load_workspace_state(&plan_key);
        let state = match loaded {
            Ok(Some(state)) => state,
            Ok(None) => return (0, None),
            Err(error) => {
                log::warn!("工作现场读取失败（plan={plan_key}）: {error}");
                return (0, Some(restore_failed_fact(&self.l10n())));
            }
        };
        if !state.boundary_confirmed {
            if let Some(angle) = state.orientation_angle {
                self.restore_orientation(&plan_key, angle);
            }
            return (0, None);
        }
        if !valid_restored_ring(&state.boundary_gcj02) {
            return (0, Some(restore_failed_fact(&self.l10n())));
        }
        let view = PlanBoundaryView {
            name: state.boundary_name.clone(),
            gcj02: state.boundary_gcj02.clone(),
            confirmed: true,
        };
        self.context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .restore_confirmed(view);
        // 同步会话画布状态：地图未就绪（或用户不触发 map_ready）时抽屉也应
        // 立即呈现"边界已确认"（A.2）。
        self.restore_cached_drawer_state(&PlanBoundaryView {
            name: state.boundary_name.clone(),
            gcj02: state.boundary_gcj02.clone(),
            confirmed: true,
        });
        let coordinates = serde_json::json!([state.boundary_gcj02.clone()]);
        self.context
            .export_flow
            .confirm_boundary("Polygon", coordinates.clone());
        self.context.collection_flow.confirm_boundary(Boundary {
            r#type: "Polygon".to_owned(),
            coordinates,
        });
        if let Some(angle) = state.orientation_angle {
            self.restore_orientation(&plan_key, angle);
        }
        (state.active_step, None)
    }

    /// 恢复方案朝向（会话正式状态 + 导出输入；未设置过朝向仍按地图正北）。
    pub(super) fn restore_orientation(&mut self, plan_id: &str, angle: f64) {
        let angle = angle as f32;
        {
            let mut session = self.context.session.borrow_mut();
            session.orientation_angle = Some(angle);
            let state = session.plans.entry(plan_id.to_owned()).or_default();
            state.has_orientation = true;
            state.orientation_angle = Some(angle);
        }
        self.context.export_flow.set_orientation(Some(angle));
    }

    pub(super) fn boundary_refresh(&mut self) -> Presentation<WorkspacePageState> {
        let request = self
            .context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .refresh();
        match request {
            BoundaryRequest::Started => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.drawer.reset();
                    session.map_draw = MapDrawState::default();
                    session.map_processing = true;
                    session.boundary_fetch_stage = Some(BoundaryFetchStage::CampusName);
                    session.boundary_fetch_attempt = 0;
                    session.boundary_fetch_total_attempts = 0;
                    session.boundary_fetch_elapsed_secs = 0;
                }
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryRequest::Loading => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_processing = true;
                    if session.boundary_fetch_stage.is_none() {
                        session.boundary_fetch_stage = Some(BoundaryFetchStage::CampusName);
                    }
                }
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryRequest::Ready(cached) => {
                self.restore_cached_boundary(&cached);
                self.clear_fetch_status();
                Presentation::ready(self.context.page())
            }
            BoundaryRequest::MissingContext => self.boundary_source_context_missing(),
        }
    }

    /// 地图就绪触发一次边界获取；F3 对 Ready 与同键 Loading 统一去重。
    pub(super) fn start_boundary_fetch(&mut self) -> Presentation<WorkspacePageState> {
        let request = self
            .context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .request();
        match request {
            BoundaryRequest::Ready(cached) => {
                self.restore_cached_boundary(&cached);
                self.clear_fetch_status();
                Presentation::ready(self.context.page())
            }
            BoundaryRequest::Started | BoundaryRequest::Loading => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_processing = true;
                    if session.boundary_fetch_stage.is_none() {
                        session.boundary_fetch_stage = Some(BoundaryFetchStage::CampusName);
                    }
                }
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryRequest::MissingContext => self.boundary_source_context_missing(),
        }
    }

    /// 非阻塞轮询 F3 会话入口；S1 不接触 receiver 或请求 generation。
    pub(super) fn poll_boundary_fetch(&mut self) -> Presentation<WorkspacePageState> {
        let poll = self
            .context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .poll();
        match poll {
            BoundaryPoll::Loading { progress } => {
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_processing = true;
                    if let Some(progress) = progress {
                        session.boundary_fetch_stage = Some(progress.stage);
                        session.boundary_fetch_attempt = progress.attempt;
                        session.boundary_fetch_total_attempts = progress.total_attempts;
                        session.boundary_fetch_elapsed_secs = progress.elapsed_secs;
                    }
                }
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryPoll::Ready(outcome) => {
                self.clear_fetch_status();
                self.apply_boundary_fetch_outcome(outcome)
            }
            BoundaryPoll::Idle | BoundaryPoll::Stale => {
                self.clear_fetch_status();
                Presentation::ready(self.context.page())
            }
        }
    }

    /// 结束在途获取的进度展示（不改变 map_available 语义）。
    fn clear_fetch_status(&self) {
        let mut session = self.context.session.borrow_mut();
        session.map_processing = false;
        session.boundary_fetch_stage = None;
        session.boundary_fetch_attempt = 0;
        session.boundary_fetch_total_attempts = 0;
        session.boundary_fetch_elapsed_secs = 0;
    }

    fn apply_boundary_fetch_outcome(
        &mut self,
        outcome: BoundaryFetchOutcome,
    ) -> Presentation<WorkspacePageState> {
        let l10n = self.l10n();
        let mut notifications = Vec::new();
        match outcome {
            BoundaryFetchOutcome::AutoSelected {
                name,
                gcj02,
                source,
                candidate_count,
            } => {
                let coords_json = serde_json::to_string(&gcj02).unwrap_or_else(|_| "[]".to_owned());
                let name_json =
                    serde_json::to_string(&name).unwrap_or_else(|_| "\"未知校区\"".to_owned());
                crate::map_webview::evaluate_script(&format!(
                    "drawBoundaryGcj({coords_json}, {name_json});"
                ));
                {
                    let mut session = self.context.session.borrow_mut();
                    session.map_draw.point_count = gcj02.len() as i32;
                    session.map_draw.status = MapDrawStatus::Editing;
                }
                let body = l10n.t_with_array(
                    "boundary.osm_auto_selected_body",
                    &[&name, &candidate_count.to_string(), &source],
                );
                notifications.push(info_fact(&l10n, "boundary.osm_auto_selected_title", &body));
            }
            BoundaryFetchOutcome::NotFound => {
                crate::map_webview::evaluate_script("enableManualMode();");
                notifications.push(info_fact(
                    &l10n,
                    "boundary.osm_not_found_title",
                    &l10n.t("boundary.osm_not_found_body"),
                ));
            }
            BoundaryFetchOutcome::Unreachable { message } => {
                log::warn!("OSM 边界自动获取失败（人工圈画兜底）: {message}");
                crate::map_webview::evaluate_script("enableManualMode();");
                // B.10：失败原因明确到阶段（校名解析失败/端点不可达/解析失败），
                // 不得笼统提示"超时"。
                let body = l10n.t_with_array("boundary.osm_unreachable_detail_body", &[&message]);
                notifications.push(info_fact(&l10n, "boundary.osm_unreachable_title", &body));
            }
        }
        let mut presentation = Presentation::ready(self.context.page());
        for notification in notifications {
            presentation = presentation.with_notification(notification);
        }
        presentation
    }

    fn boundary_source_context_missing(&self) -> Presentation<WorkspacePageState> {
        crate::map_webview::evaluate_script("enableManualMode();");
        let l10n = self.l10n();
        Presentation::ready(self.context.page()).with_notification(info_fact(
            &l10n,
            "boundary.osm_not_found_title",
            &l10n.t("boundary.osm_not_found_body"),
        ))
    }

    pub(super) fn cache_confirmed_boundary(&self, gcj02: &[[f64; 2]]) {
        self.context
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .confirm(gcj02.to_vec());
    }

    pub(super) fn restore_cached_boundary(&self, cached: &PlanBoundaryView) {
        let coords_json = serde_json::to_string(&cached.gcj02).unwrap_or_else(|_| "[]".to_owned());
        if cached.confirmed {
            let status = self.l10n().t("boundary.session_restored_status");
            let status_json = serde_json::to_string(&status).unwrap_or_else(|_| "\"\"".to_owned());
            crate::map_webview::evaluate_script(&format!(
                "drawRestoredBoundaryGcj({coords_json}, {status_json});"
            ));
        } else {
            let name_json =
                serde_json::to_string(&cached.name).unwrap_or_else(|_| "\"\"".to_owned());
            crate::map_webview::evaluate_script(&format!(
                "drawBoundaryGcj({coords_json}, {name_json});"
            ));
        }
        self.restore_cached_drawer_state(cached);
    }

    pub(super) fn restore_cached_drawer_state(&self, cached: &PlanBoundaryView) {
        if cached.confirmed {
            if let Some((center_lon, center_lat)) = centroid(&cached.gcj02) {
                let mut converter = CoordinateConverter::default();
                converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
                let vertices = cached
                    .gcj02
                    .iter()
                    .map(|[lon, lat]| {
                        converter
                            .mercator_to_plane(MercatorCoord::from_lat_lon(*lat, *lon))
                            .map(|plane| Vertex::new(plane.x, plane.y))
                    })
                    .collect::<Option<Vec<_>>>();
                if let Some(vertices) = vertices {
                    let mut session = self.context.session.borrow_mut();
                    session.drawer.load_determined(vertices);
                    session.map_draw = MapDrawState::default();
                    return;
                }
            }
        }
        let mut session = self.context.session.borrow_mut();
        session.map_draw.point_count = cached.gcj02.len() as i32;
        session.map_draw.status = MapDrawStatus::Editing;
    }
}
