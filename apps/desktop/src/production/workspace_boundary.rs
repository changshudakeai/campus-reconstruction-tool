//! S1-05/06 工作区入口的状态与共享上下文：会话状态、上下文、几何/坐标辅助。
//!
//! 本模块持有工作区面向 S1 的会话状态与共享上下文（`WorkspaceProductionContext`）
//! 以及几何换算/通知事实辅助函数；一次请求一次完整呈现的适配器见
//! `super::workspace_adapter`。边界与朝向业务规则全部经 B5 foundation-mode /
//! F5 OrientationCalculator 完成，S1 不掺入业务规则。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use collection_flow::CollectionFlow;
use data_persistence::PlanWorkspaceState;
use export_flow::BoundaryExportFlow;
use foundation_mode::{
    BoundaryDrawer, BoundaryState, OrientationCalculator, OrientationLine, Point2D, Vertex,
};
use localization::Localization;
use notification_center::Notification;
use shared_domain_types::CandidateCategory;
use slint::ComponentHandle;

use crate::presentation::{
    BoundaryViewState, NotificationFact, OrientationViewState, WorkspacePageState,
};
use crate::ViewModelInjector;
use project_management::BoundaryFetchStage;

use super::workspace_leave::{
    LeaveWorkspaceDecision, LeaveWorkspaceIntent, LeaveWorkspaceUseCase, WorkspaceOperation,
};

/// 单方案进度状态（工作区功能入口持有；正式持久化归后续数据工单）。
#[derive(Debug, Clone, Default)]
pub(crate) struct PlanProgressState {
    pub(crate) has_orientation: bool,
    pub(crate) orientation_angle: Option<f32>,
}

impl PlanProgressState {
    fn completed_steps(&self, boundary_confirmed: bool) -> u8 {
        boundary_confirmed as u8 + self.has_orientation as u8
    }
}

/// 工作区会话状态：全部由功能入口持有，S1 呈现层不保存业务副本。
#[derive(Default)]
pub(crate) struct WorkspaceSessionState {
    pub(super) active_plan_id: Option<String>,
    pub(super) active_context: Option<project_management::PlanContextView>,
    pub(super) plans: HashMap<String, PlanProgressState>,
    pub(super) drawer: BoundaryDrawer,
    pub(super) orientation_points: Vec<(f64, f64)>,
    pub(super) orientation_angle: Option<f32>,
    pub(super) pending_orientation_angle: Option<f32>,
    pub(super) active_step: i32,
    /// 迁移期兼容：无已打开方案时按窗口当前呈现的已完成步数判定（旧行为基线）。
    pub(super) adopted_completed_steps: Option<u8>,
    pub(super) map_processing: bool,
    /// T34: 地图 WebView 是否可用（不可用时边界步骤显示 Slint 兜底画布）
    pub(super) map_available: bool,
    /// T34: 左侧抽屉是否展开（做法 A：展开时地图右移让位）
    pub(super) drawer_open: bool,
    /// T34: 地图侧边界绘制状态（供抽屉 ① 显示点数/状态）
    pub(super) map_draw: MapDrawState,
    pub(super) tutorial_visible: bool,
    pub(super) tutorial_text: String,
    pub(super) tutorial_dismiss_label: String,
    pub(super) tutorial_skip_all_label: String,
    /// T39：评审候选列表当前页索引（S1 呈现层分页状态；切分类时复位到 0）。
    pub(super) review_page_index: usize,
    /// B 工单：边界自动获取当前阶段（None = 无在途获取；S1 只显示阶段与耗时）
    pub(super) boundary_fetch_stage: Option<BoundaryFetchStage>,
    /// 当前阶段内端点尝试序号（1..=total；非端点阶段为 0）
    pub(super) boundary_fetch_attempt: u32,
    /// 当前阶段端点总数（非端点阶段为 0）
    pub(super) boundary_fetch_total_attempts: u32,
    /// 自阶段开始的整数秒（进度反馈用，B.9）
    pub(super) boundary_fetch_elapsed_secs: u64,
}

/// T34: 地图侧边界绘制状态（纯呈现；几何真相仍在地图 JS/B5 侧）
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct MapDrawState {
    pub(super) point_count: i32,
    pub(super) status: MapDrawStatus,
    /// T 工单：当前选中的顶点索引（None = 未选中；抽屉"删除选中点"按钮
    /// 只在选中后可用，几何真相仍在地图 JS 侧）。
    pub(super) selected_vertex: Option<i32>,
}

impl MapDrawState {
    /// 是否有选中的边界顶点（只有编辑态地图才可能有选中）。
    pub(super) fn has_selected_vertex(&self) -> bool {
        self.status == MapDrawStatus::Editing && self.selected_vertex.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum MapDrawStatus {
    /// 无地图侧绘制（使用 session.drawer 状态）
    #[default]
    Idle,
    /// 人工圈画进行中
    Drawing,
    /// OSM 边界编辑中（已自动绘制）
    Editing,
}

impl WorkspaceSessionState {
    pub(super) fn completed_steps(&self, boundary_confirmed: bool, has_collection: bool) -> u8 {
        match &self.active_plan_id {
            Some(plan_id) => self
                .plans
                .get(plan_id)
                .map(|state| state.completed_steps(boundary_confirmed) + has_collection as u8)
                .unwrap_or(boundary_confirmed as u8 + has_collection as u8),
            None => self.adopted_completed_steps.unwrap_or(0),
        }
    }

    pub(super) fn adopt(&mut self, completed: i32) {
        self.adopted_completed_steps = Some(completed.clamp(0, 5) as u8);
    }

    pub(super) fn boundary_unsaved(&self) -> bool {
        matches!(
            self.drawer.state(),
            BoundaryState::Drawing | BoundaryState::Editing { .. }
        ) && !self.drawer.vertices().is_empty()
    }

    /// 把计算好的朝向写入方案正式状态（工作区会话内正式状态；正式持久化归
    /// 后续数据工单）。没有已打开方案时拒绝保存，正式状态保持原状。
    pub(super) fn commit_orientation(&mut self, angle: f32) -> Result<(), ()> {
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

/// 工作区功能入口上下文：呈现适配器与页面状态共用。
#[derive(Clone)]
pub(crate) struct WorkspaceProductionContext {
    pub(super) injector: Rc<RefCell<ViewModelInjector>>,
    pub(super) window: slint::Weak<crate::AppWindow>,
    pub(super) session: Rc<RefCell<WorkspaceSessionState>>,
    pub(super) export_flow: Arc<BoundaryExportFlow>,
    pub(super) collection_flow: Arc<CollectionFlow>,
    leave_workspace: LeaveWorkspaceUseCase,
}

impl WorkspaceProductionContext {
    pub(crate) fn new(
        injector: Rc<RefCell<ViewModelInjector>>,
        window: &crate::AppWindow,
        export_flow: Arc<BoundaryExportFlow>,
        collection_flow: Arc<CollectionFlow>,
    ) -> Self {
        let tutorial_dismiss_label = injector.borrow().l10n().t("tutorial.dismiss_button");
        Self {
            injector,
            window: window.as_weak(),
            session: Rc::new(RefCell::new(WorkspaceSessionState {
                tutorial_dismiss_label,
                ..Default::default()
            })),
            export_flow,
            collection_flow,
            leave_workspace: LeaveWorkspaceUseCase::default(),
        }
    }

    pub(super) fn operation_started(&self, operation: WorkspaceOperation) {
        self.leave_workspace.operation_started(operation);
    }

    pub(super) fn operation_finished(&self, operation: WorkspaceOperation) {
        self.leave_workspace.operation_finished(operation);
    }

    pub(super) fn decide_leave(
        &self,
        target: crate::presentation::Screen,
    ) -> LeaveWorkspaceDecision {
        let session = self.session.borrow();
        self.leave_workspace.decide(LeaveWorkspaceIntent {
            target,
            map_processing: session.map_processing,
            active_step: session.active_step,
            boundary_unsaved: session.boundary_unsaved()
                || crate::map_session::has_boundary_draft(),
        })
    }

    pub(super) fn injector(&self) -> Rc<RefCell<ViewModelInjector>> {
        Rc::clone(&self.injector)
    }

    /// 把当前工作区会话状态写成方案级安全检查点（状态变更即落库，工单 A.6）。
    ///
    /// 边界几何取自 export-flow（正式导出输入），名称取自 F3 边界会话；
    /// 落库失败只告警不阻塞用户操作，但绝不静默伪造"已确认"。
    pub(super) fn checkpoint_workspace_session(&self) {
        let Some(plan_id) = self.session.borrow().active_plan_id.clone() else {
            return;
        };
        let (boundary, boundary_confirmed) = {
            let view = self.export_flow.boundary_view();
            let coords = view
                .as_ref()
                .and_then(polygon_coordinates)
                .unwrap_or_default();
            (coords, self.export_flow.boundary_confirmed())
        };
        let boundary_name = self
            .injector
            .borrow_mut()
            .boundary_session_mut()
            .active_view()
            .map(|view| view.name)
            .unwrap_or_default();
        let (orientation_angle, active_step) = {
            let session = self.session.borrow();
            let angle = session
                .plans
                .get(&plan_id)
                .and_then(|state| state.orientation_angle)
                .map(f64::from);
            (angle, session.active_step)
        };
        let state = PlanWorkspaceState::new(
            plan_id.clone(),
            boundary_name,
            boundary,
            boundary_confirmed,
            orientation_angle,
            active_step,
        );
        let mut injector = self.injector.borrow_mut();
        if let Err(error) = injector.save_workspace_state(&state) {
            log::warn!("工作区安全检查点落库失败（plan={plan_id}）: {error}");
        }
    }

    /// 当前打开方案 ID（评审进台/封账按此方案装载会话）。
    pub(crate) fn active_plan_id(&self) -> Option<String> {
        self.session.borrow().active_plan_id.clone()
    }

    /// 当前工作区页完整可观察状态（由功能入口侧状态派生，S1 只绘制）。
    pub(crate) fn page(&self) -> WorkspacePageState {
        let injector = self.injector.borrow();
        let l10n = injector.l10n();
        let session = self.session.borrow();
        let boundary_confirmed =
            session.active_plan_id.is_some() && self.export_flow.boundary_confirmed();
        let has_collection = session
            .active_plan_id
            .as_ref()
            .is_some_and(|plan_id| self.collection_flow.is_review_unlocked(plan_id));
        let completed = session.completed_steps(boundary_confirmed, has_collection);
        let has_orientation = session
            .active_plan_id
            .as_ref()
            .and_then(|plan_id| session.plans.get(plan_id))
            .is_some_and(|state| state.has_orientation);
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
        let boundary = self.boundary_view(l10n, &session);
        let boundary_fetch_status = fetch_status_label(l10n, &session);
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
            drawer_open: session.drawer_open,
            map_available: session.map_available,
            map_loading: session.map_processing,
            map_loading_label: l10n.t("map.loading"),
            map_failed_label: l10n.t("map.load_failed"),
            boundary_points_label: l10n
                .t_with_array("boundary.point_count", &[&boundary.point_count.to_string()]),
            orientation_current_angle_label: l10n.t("orientation.current_angle_label"),
            orientation_confirm_two_points_label: l10n.t("orientation.confirm_two_points_button"),
            drawer_expand_tooltip: l10n.t("workspace.drawer_expand_tooltip"),
            drawer_collapse_tooltip: l10n.t("workspace.drawer_collapse_tooltip"),
            boundary_fetch_status,
            boundary,
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
        if self.collection_flow.is_review_unlocked(plan_id) {
            return self.injector.borrow().l10n().t("plan.progress_next_review");
        }
        if self.export_flow.plan_boundary_confirmed(plan_id) {
            if session
                .plans
                .get(plan_id)
                .is_some_and(|state| state.has_orientation)
            {
                return self
                    .injector
                    .borrow()
                    .l10n()
                    .t("plan.progress_next_collection");
            }
            return self
                .injector
                .borrow()
                .l10n()
                .t("plan.progress_boundary_done");
        }
        fallback.to_owned()
    }

    pub(super) fn boundary_view(
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
        // T34：地图侧绘制（人工圈画/OSM 编辑）时以地图桥接上报的点数/状态为准，
        // 否则回落到 Slint 兜底画布（session.drawer）状态。
        let (point_count, status) = if session.map_draw.status != MapDrawStatus::Idle {
            let count = session.map_draw.point_count.max(0);
            let status = match session.map_draw.status {
                MapDrawStatus::Drawing => {
                    l10n.t_with_array("boundary.status_drawing", &[&count.to_string()])
                }
                MapDrawStatus::Editing => {
                    if session.map_draw.has_selected_vertex() {
                        l10n.t("boundary.status_vertex_selected")
                    } else {
                        l10n.t("boundary.status_editing")
                    }
                }
                MapDrawStatus::Idle => l10n.t("boundary.status_idle"),
            };
            (count, status)
        } else {
            let status = match session.drawer.state() {
                BoundaryState::Idle => l10n.t("boundary.status_idle"),
                BoundaryState::Drawing => {
                    l10n.t_with_array("boundary.status_drawing", &[&vertices.len().to_string()])
                }
                BoundaryState::Determined => l10n.t("boundary.status_determined"),
                BoundaryState::Editing { .. } => l10n.t("boundary.status_editing"),
            };
            (vertices.len() as i32, status)
        };
        // 已确认边界被再次编辑（地图侧拖拽/增删顶点后 map_draw 离开 Idle）时，
        // 确认按钮必须重新可用，让用户把调整后的顶点重新确认并覆盖落库。
        let edited_since_confirmed = is_closed && session.map_draw.status != MapDrawStatus::Idle;
        BoundaryViewState {
            points,
            path_commands,
            title: l10n.t("boundary.step_title"),
            hint: l10n.t("boundary.hint"),
            undo_label: l10n.t("boundary.undo_button"),
            confirm_label: l10n.t("boundary.confirm_button"),
            reset_label: l10n.t("boundary.reset_button"),
            refresh_label: l10n.t("boundary.refresh_button"),
            delete_selected_label: l10n.t("boundary.delete_selected_button"),
            delete_selected_enabled: session.map_draw.has_selected_vertex(),
            edited_since_confirmed,
            status,
            map_placeholder: l10n.t("boundary.map_placeholder"),
            is_determined: is_closed,
            point_count,
        }
    }

    pub(super) fn orientation_view(
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
            angle,
            is_determined,
            title: l10n.t("orientation.step_title"),
            two_points_hint: l10n.t("orientation.two_points_hint"),
            bearing_angle_hint: l10n.t("orientation.bearing_angle_hint"),
            angle_input_placeholder: l10n.t("orientation.angle_input_placeholder"),
            angle_display,
            clear_input: false,
            fill_input: None,
            submit_label: l10n.t("orientation.submit_button"),
            reset_label: l10n.t("orientation.reset_button"),
            status,
        }
    }

    /// 地图密钥与校区锚点（锚点优先取当前方案的校区，其次上次校区）。
    ///
    /// 没有真实校区锚点时返回 `None`：不得用固定坐标初始化地图或参与边界换算
    /// （ADR-0042 §7 禁止以固定坐标/锚点代替用户数据）。
    pub(super) fn map_credentials(&self) -> Option<((String, String), (f64, f64))> {
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
            .or_else(|| {
                injector
                    .projects()
                    .landing_campus()
                    .ok()
                    .flatten()
                    .map(|campus| (campus.anchor_lng, campus.anchor_lat))
            })?;
        Some(((api_key, security_key), anchor))
    }

    pub(super) fn mark_map_loading(&self) {
        let mut session = self.session.borrow_mut();
        session.map_processing = true;
    }
}

/// 由 F5 计算地图两点的朝向角（正北为 0°、顺时针增加；重合两点返回 None）。
pub(super) fn calculate_orientation_angle(points: [[f64; 2]; 2]) -> Option<f32> {
    let (x0, y0) = (points[0][0], points[0][1]);
    let (x1, y1) = (points[1][0], points[1][1]);
    OrientationLine::new(Point2D::new(x0, y0), Point2D::new(x1, y1))
        .and_then(|line| OrientationCalculator::calculate(&line))
        .map(|orientation| orientation.degree())
}

pub(super) fn polygon_coordinates(boundary: &export_flow::BoundaryView) -> Option<Vec<[f64; 2]>> {
    first_outer_ring(&boundary.r#type, &boundary.coordinates)
}

pub(super) fn first_outer_ring(
    boundary_type: &str,
    coordinates: &serde_json::Value,
) -> Option<Vec<[f64; 2]>> {
    match boundary_type {
        "Polygon" => serde_json::from_value::<Vec<Vec<[f64; 2]>>>(coordinates.clone())
            .ok()
            .and_then(|rings| rings.into_iter().next()),
        "MultiPolygon" => serde_json::from_value::<Vec<Vec<Vec<[f64; 2]>>>>(coordinates.clone())
            .ok()
            .and_then(|polygons| polygons.into_iter().next())
            .and_then(|rings| rings.into_iter().next()),
        _ => None,
    }
}

/// 坐标数组的简单重心（经纬度平均值）。
pub(super) fn centroid(coords: &[[f64; 2]]) -> Option<(f64, f64)> {
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

/// 重算确认窗正文：沿用既有重算提示（collection.orientation_recalc_notice），
/// 并按 F5 影响报告逐项列出已生成数据（类别名经 B6 本地化，ADR-0005）。
pub(super) fn orientation_recalc_body(
    l10n: &Localization,
    report: &foundation_mode::OrientationImpactReport,
) -> String {
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

pub(super) fn validation_detail(result: &foundation_mode::ValidationResult) -> String {
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

pub(super) fn info_fact(l10n: &Localization, title_key: &str, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::info(
        l10n.t("app.source_tag"),
        l10n.t(title_key),
        body.to_owned(),
    ))
}

pub(super) fn error_fact(l10n: &Localization, body: &str) -> NotificationFact {
    NotificationFact::new(Notification::error(
        l10n.t("app.source_tag"),
        l10n.t("dialog.error_title"),
        body.to_owned(),
    ))
}

/// 边界获取阶段 → 本地化文本键（B6；S1 只按键取文案）。
pub(super) fn fetch_stage_key(stage: BoundaryFetchStage) -> &'static str {
    match stage {
        BoundaryFetchStage::CampusName => "boundary.stage_campus_name",
        BoundaryFetchStage::ByElementId => "boundary.stage_by_id",
        BoundaryFetchStage::Amenity => "boundary.stage_amenity",
        BoundaryFetchStage::Landuse => "boundary.stage_landuse",
    }
}

/// 恢复的已确认边界环是否可信（≥3 个有限坐标；A.7：数据损坏不得伪造已确认）。
pub(super) fn valid_restored_ring(coords: &[[f64; 2]]) -> bool {
    coords.len() >= 3
        && coords
            .iter()
            .all(|[lon, lat]| lon.is_finite() && lat.is_finite())
}

/// 边界自动获取的进度文案（阶段 + 端点尝试 + 已耗时；无在途获取时为空串）。
fn fetch_status_label(l10n: &Localization, session: &WorkspaceSessionState) -> String {
    let Some(stage) = session.boundary_fetch_stage else {
        return String::new();
    };
    let stage_label = l10n.t(fetch_stage_key(stage));
    if session.boundary_fetch_total_attempts > 0 {
        l10n.t_with_args(
            "boundary.fetch_progress_endpoint",
            serde_json::json!({
                "stage": stage_label,
                "attempt": session.boundary_fetch_attempt,
                "total": session.boundary_fetch_total_attempts,
                "seconds": session.boundary_fetch_elapsed_secs,
            }),
        )
    } else {
        l10n.t_with_args(
            "boundary.fetch_progress_stage",
            serde_json::json!({
                "stage": stage_label,
                "seconds": session.boundary_fetch_elapsed_secs,
            }),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use foundation_mode::{check_orientation_change_impact, Orientation};
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
