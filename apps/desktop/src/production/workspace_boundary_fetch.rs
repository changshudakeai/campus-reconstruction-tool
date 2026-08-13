//! 方案边界获取的呈现适配。
//!
//! F3 `PlanBoundarySession` 拥有缓存、刷新、在途去重与陈旧结果状态机；本文件只
//! 把其结构化结果翻译为工作区页面状态、通知与地图绘制命令。

use foundation_mode::{CoordinateConverter, MercatorCoord, Vertex};
use project_management::{BoundaryFetchOutcome, BoundaryPoll, BoundaryRequest, PlanBoundaryView};

use crate::presentation::{Presentation, Progress, WorkspacePageState};

use super::workspace_adapter::WorkspaceProductionAdapter;
use super::workspace_boundary::{centroid, info_fact, MapDrawState, MapDrawStatus};

impl WorkspaceProductionAdapter {
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
                }
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryRequest::Loading => {
                self.context.session.borrow_mut().map_processing = true;
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryRequest::Ready(cached) => {
                self.restore_cached_boundary(&cached);
                self.context.session.borrow_mut().map_processing = false;
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
                self.context.session.borrow_mut().map_processing = false;
                Presentation::ready(self.context.page())
            }
            BoundaryRequest::Started | BoundaryRequest::Loading => {
                self.context.session.borrow_mut().map_processing = true;
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
            BoundaryPoll::Loading => {
                self.context.session.borrow_mut().map_processing = true;
                Presentation::processing(self.context.page(), Progress::ZERO)
            }
            BoundaryPoll::Ready(outcome) => {
                self.context.session.borrow_mut().map_processing = false;
                self.apply_boundary_fetch_outcome(outcome)
            }
            BoundaryPoll::Idle | BoundaryPoll::Stale => {
                self.context.session.borrow_mut().map_processing = false;
                Presentation::ready(self.context.page())
            }
        }
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
                notifications.push(info_fact(
                    &l10n,
                    "boundary.osm_not_found_title",
                    &l10n.t("boundary.osm_unreachable_body"),
                ));
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
