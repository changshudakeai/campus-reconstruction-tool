//! 一个方案连续使用的地图会话（ADR-0045）。
//!
//! 外层页面只提交“显示意图”和结构化命令。本模块独占 WebView 页种类、
//! 代际、弹窗遮挡、视野恢复及 JavaScript 协议；功能入口不再拼生命周期。
// ignore-tidy-filelength: 地图会话是“小入口、大实现”的深适配器：对外只有
// 显示意图/命令/观测入口，内部集中 ADR-0045 现场、预览负载与定位现场；
// T52 预览现场与 WebView 生命周期耦合紧密，集中维护比强行拆散更可审计。

use std::cell::RefCell;
use std::rc::Rc;

use slint::Weak;

pub const MAP_LOAD_TIMEOUT_MARKER: &str = crate::map_webview::MAP_LOAD_TIMEOUT_MARKER;

/// 方案内需要地图的三个可见现场。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapScene {
    Boundary,
    Orientation,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapDestination {
    Review,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapTransition {
    Ready,
    ConfirmBoundaryDraftDiscard,
}

/// 调用方只表达用户命令，不学习任何 JavaScript 函数名。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MapCommand {
    BoundaryUndo,
    BoundaryDeleteSelected,
    SubmitBoundary,
    BoundaryClear,
    BoundaryEnableManual,
    BoundaryDraw {
        coordinates: Vec<[f64; 2]>,
        label: String,
        restored: bool,
    },
    OrientationClear,
    OrientationActivate,
    SubmitOrientation,
    ReviewReplace(Vec<serde_json::Value>),
    ReviewUpdate(serde_json::Value),
    ReviewHighlight(Option<String>),
    ReviewLocate(String),
    ReviewMapText(bool),
    CampusSearch {
        request_id: u64,
        query: String,
    },
    /// 第五步 3D 预览：复位视角到自动取景。
    PreviewReset,
    /// 第五步 3D 预览：按倍率缩放（>1 拉远，<1 拉近）。
    PreviewZoom(f32),
    /// 第五步 3D 预览：定位到保留候选要素（按预览负载 features 的 id）。
    PreviewLocate(String),
}

impl MapCommand {
    fn scene(&self) -> Option<MapScene> {
        match self {
            Self::BoundaryUndo
            | Self::BoundaryDeleteSelected
            | Self::SubmitBoundary
            | Self::BoundaryClear
            | Self::BoundaryEnableManual
            | Self::BoundaryDraw { .. } => Some(MapScene::Boundary),
            Self::OrientationClear | Self::OrientationActivate | Self::SubmitOrientation => {
                Some(MapScene::Orientation)
            }
            Self::ReviewReplace(_)
            | Self::ReviewUpdate(_)
            | Self::ReviewHighlight(_)
            | Self::ReviewLocate(_)
            | Self::ReviewMapText(_) => Some(MapScene::Review),
            Self::CampusSearch { .. }
            | Self::PreviewReset
            | Self::PreviewZoom(_)
            | Self::PreviewLocate(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapCommandResult {
    Allowed,
    Unavailable,
}

/// 可在 WebView 安全重建后恢复的地图视野。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MapViewport {
    pub(crate) lon: f64,
    pub(crate) lat: f64,
    pub(crate) zoom: f64,
}

impl MapViewport {
    fn gaode(self) -> gaode_client::MapViewport {
        gaode_client::MapViewport::new(self.lon, self.lat, self.zoom)
    }
}

/// 页面层提交的完整显示意图。密钥只留在内存，不参与日志或错误文本。
#[derive(Clone, PartialEq)]
pub(crate) enum MapDisplayIntent {
    Boundary {
        plan_id: String,
        api_key: String,
        security_key: String,
        anchor: (f64, f64),
    },
    Orientation {
        plan_id: String,
        config: gaode_client::BoundaryEditPageConfig,
    },
    Review {
        plan_id: String,
        api_key: String,
        security_key: String,
        anchor: (f64, f64),
        map_text_label: String,
    },
    CampusSearch {
        api_key: String,
        security_key: String,
    },
    /// 第五步 3D 方块预览（T52）：本地 Three.js 页，无需地图凭据。
    BlockPreview { plan_id: String },
}

impl MapDisplayIntent {
    fn scene(&self) -> Option<MapScene> {
        match self {
            Self::Boundary { .. } => Some(MapScene::Boundary),
            Self::Orientation { .. } => Some(MapScene::Orientation),
            Self::Review { .. } => Some(MapScene::Review),
            Self::CampusSearch { .. } | Self::BlockPreview { .. } => None,
        }
    }

    fn plan_id(&self) -> Option<&str> {
        match self {
            Self::Boundary { plan_id, .. }
            | Self::Orientation { plan_id, .. }
            | Self::Review { plan_id, .. }
            | Self::BlockPreview { plan_id } => Some(plan_id),
            Self::CampusSearch { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MapEvent {
    Workspace {
        scene: MapScene,
        message: gaode_client::IpcMessage,
    },
    CampusSearch(String),
    /// 第五步 3D 预览页回传的原始 IPC（preview_stats / preview_error / preview_loaded）。
    Preview(String),
}

pub(crate) type MapEventHandler = Rc<dyn Fn(MapEvent)>;
pub(crate) type MapAvailabilityHandler = Rc<dyn Fn(MapScene, bool)>;

#[derive(Default)]
struct MapSessionRuntime {
    state: MapSessionState,
    desired: Option<(Weak<crate::AppWindow>, MapDisplayIntent)>,
    covered: bool,
    failed: bool,
    event_handler: Option<MapEventHandler>,
    availability_handler: Option<MapAvailabilityHandler>,
}

thread_local! {
    static SESSION: RefCell<MapSessionRuntime> = RefCell::new(MapSessionRuntime::default());
}

thread_local! {
    /// 第五步预览负载推送到 WebView 的累计次数（契约测试观测）。
    static PREVIEW_PUSH_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// 第五步预览定位命令推送到 WebView 的累计次数（契约测试观测）。
    static PREVIEW_LOCATE_PUSH_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// 预览页最近一次上报的 (fps, 方块数, 可见面四边形数)。
    static PREVIEW_STATS: RefCell<Option<(f32, usize, usize)>> = const { RefCell::new(None) };
    /// 预览页最近一次上报的渲染错误。
    static PREVIEW_RENDER_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 不含 WebView 句柄的连续会话状态；真实适配器只消费它决定好的现场。
#[derive(Debug, Default)]
pub(crate) struct MapSessionState {
    plan_id: Option<String>,
    campus_viewport: Option<MapViewport>,
    review_viewport: Option<MapViewport>,
    boundary_draft_dirty: bool,
    boundary_draft: Option<Vec<[f64; 2]>>,
    /// 第五步 3D 预览的最新渲染负载（呈现临时状态；方案切换即清除）。
    preview_payload: Option<String>,
    /// 等待预览页就绪后执行的定位目标（生成完成/页面重建后补推）。
    preview_locate: Option<String>,
    available: bool,
    generation: u64,
}

impl MapSessionState {
    /// 进入方案。只有方案身份真正变化时才清空全部临时现场。
    pub(crate) fn enter_plan(&mut self, plan_id: impl Into<String>) {
        let plan_id = plan_id.into();
        if self.plan_id.as_deref() == Some(plan_id.as_str()) {
            return;
        }
        self.plan_id = Some(plan_id);
        self.campus_viewport = None;
        self.review_viewport = None;
        self.boundary_draft_dirty = false;
        self.boundary_draft = None;
        self.preview_payload = None;
        self.preview_locate = None;
        self.available = false;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn remember_viewport(&mut self, scene: MapScene, viewport: MapViewport) {
        match scene {
            MapScene::Boundary | MapScene::Orientation => {
                self.campus_viewport = Some(viewport);
            }
            MapScene::Review => self.review_viewport = Some(viewport),
        }
    }

    pub(crate) fn viewport_for(&self, scene: MapScene) -> Option<MapViewport> {
        match scene {
            MapScene::Boundary | MapScene::Orientation => self.campus_viewport,
            MapScene::Review => self.review_viewport,
        }
    }

    pub(crate) fn boundary_draft_changed(&mut self) {
        self.boundary_draft_dirty = true;
    }

    fn remember_boundary_draft(&mut self, coordinates: Vec<[f64; 2]>) {
        self.boundary_draft_dirty = true;
        self.boundary_draft = Some(coordinates);
    }

    pub(crate) fn boundary_committed(&mut self) {
        self.boundary_draft_dirty = false;
        self.boundary_draft = None;
    }

    pub(crate) fn discard_boundary_draft(&mut self) {
        self.boundary_draft_dirty = false;
        self.boundary_draft = None;
    }

    pub(crate) fn prepare(&self, destination: MapDestination) -> MapTransition {
        if self.boundary_draft_dirty
            && matches!(destination, MapDestination::Review | MapDestination::Export)
        {
            MapTransition::ConfirmBoundaryDraftDiscard
        } else {
            MapTransition::Ready
        }
    }

    pub(crate) fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    pub(crate) fn command_allowed(&self, _command: &MapCommand) -> MapCommandResult {
        if self.available {
            MapCommandResult::Allowed
        } else {
            MapCommandResult::Unavailable
        }
    }

    pub(crate) fn begin_generation(&mut self, _scene: MapScene) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.available = false;
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn accepts_event(&self, generation: u64) -> bool {
        generation == self.generation
    }
}

fn scene_for_page(page: crate::map_webview::MapPageKind) -> Option<MapScene> {
    match page {
        crate::map_webview::MapPageKind::Boundary => Some(MapScene::Boundary),
        crate::map_webview::MapPageKind::Orientation => Some(MapScene::Orientation),
        crate::map_webview::MapPageKind::Review => Some(MapScene::Review),
        crate::map_webview::MapPageKind::CampusSearch
        | crate::map_webview::MapPageKind::BlockPreview => None,
    }
}

/// 把适配器事件收进会话：退休代已由适配器淘汰，视野与草稿状态在转发前更新。
pub(crate) fn register_handlers(
    event_handler: MapEventHandler,
    availability_handler: MapAvailabilityHandler,
) {
    SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.event_handler = Some(event_handler);
        runtime.availability_handler = Some(availability_handler);
    });
    crate::map_webview::register_ipc_handler(Rc::new(|page, body| {
        if page == crate::map_webview::MapPageKind::BlockPreview {
            let handler = SESSION.with(|s| s.borrow().event_handler.clone());
            if let Some(handler) = handler {
                handler(MapEvent::Preview(body.to_owned()));
            }
            return;
        }
        if page == crate::map_webview::MapPageKind::CampusSearch {
            let handler = SESSION.with(|s| s.borrow().event_handler.clone());
            if let Some(handler) = handler {
                handler(MapEvent::CampusSearch(body.to_owned()));
            }
            return;
        }
        let Ok(message) = gaode_client::parse_ipc_message(body) else {
            return;
        };
        let Some(scene) = scene_for_page(page) else {
            return;
        };
        SESSION.with(|session| {
            let mut runtime = session.borrow_mut();
            match &message {
                gaode_client::IpcMessage::ViewportChanged {
                    longitude,
                    latitude,
                    zoom,
                } => runtime.state.remember_viewport(
                    scene,
                    MapViewport {
                        lon: *longitude,
                        lat: *latitude,
                        zoom: *zoom,
                    },
                ),
                gaode_client::IpcMessage::BoundaryUpdate { coords } => {
                    runtime.state.remember_boundary_draft(coords.clone());
                }
                gaode_client::IpcMessage::ManualPoint { lon, lat, .. } => {
                    runtime.state.boundary_draft_changed();
                    runtime
                        .state
                        .boundary_draft
                        .get_or_insert_with(Vec::new)
                        .push([*lon, *lat]);
                }
                gaode_client::IpcMessage::ManualCancel => {
                    runtime.state.boundary_draft_changed();
                    if let Some(draft) = runtime.state.boundary_draft.as_mut() {
                        draft.pop();
                    }
                }
                gaode_client::IpcMessage::ManualClear => {
                    runtime.state.boundary_draft_changed();
                    runtime.state.boundary_draft = Some(Vec::new());
                }
                gaode_client::IpcMessage::BoundaryGeometryUpdate { .. } => {
                    runtime.state.boundary_draft_changed();
                }
                gaode_client::IpcMessage::ConfirmBoundary { .. }
                | gaode_client::IpcMessage::ConfirmBoundaryGeometry { .. } => {
                    runtime.state.boundary_committed();
                }
                _ => {}
            }
        });
        let handler = SESSION.with(|s| s.borrow().event_handler.clone());
        if let Some(handler) = handler {
            handler(MapEvent::Workspace {
                scene,
                message: message.clone(),
            });
        }
        if scene == MapScene::Boundary && matches!(message, gaode_client::IpcMessage::MapReady) {
            let draft = SESSION.with(|s| s.borrow().state.boundary_draft.clone());
            if let Some(coordinates) = draft {
                let _ = command(MapCommand::BoundaryDraw {
                    coordinates,
                    label: String::new(),
                    restored: false,
                });
            }
        }
    }));
    crate::map_webview::register_status_handler(Rc::new(|page, available| {
        if page == crate::map_webview::MapPageKind::BlockPreview {
            if available {
                let payload = SESSION.with(|s| s.borrow().state.preview_payload.clone());
                if let Some(payload) = payload {
                    push_preview_payload(&payload);
                }
                let locate = SESSION.with(|s| s.borrow().state.preview_locate.clone());
                if let Some(feature_id) = locate {
                    push_preview_locate(&feature_id);
                }
            }
            return;
        }
        let Some(scene) = scene_for_page(page) else {
            return;
        };
        let handler = SESSION.with(|session| {
            let mut runtime = session.borrow_mut();
            runtime.state.set_available(available);
            runtime.failed = !available;
            runtime.availability_handler.clone()
        });
        if let Some(handler) = handler {
            handler(scene, available);
        }
    }));
}

/// Slint 契约测试保留的注入入口；生产 WebView 事件不走这里。
pub(crate) fn dispatch_contract_ipc(raw: String) {
    let event = SESSION.with(|session| {
        let runtime = session.borrow();
        if matches!(
            runtime.desired.as_ref().map(|(_, intent)| intent),
            Some(MapDisplayIntent::BlockPreview { .. })
        ) {
            // 预览页自身只回传 preview_* 消息；契约测试直接注入的地图 IPC
            // （如 confirm_boundary）仍按边界场景路由，不得被预览分支吞掉。
            return match gaode_client::parse_ipc_message(&raw) {
                Ok(message) => Some(MapEvent::Workspace {
                    scene: MapScene::Boundary,
                    message,
                }),
                Err(_) => Some(MapEvent::Preview(raw)),
            };
        }
        match runtime.desired.as_ref().map(|(_, intent)| intent) {
            Some(MapDisplayIntent::CampusSearch { .. }) => Some(MapEvent::CampusSearch(raw)),
            desired => {
                let scene = desired
                    .and_then(MapDisplayIntent::scene)
                    .unwrap_or(MapScene::Boundary);
                gaode_client::parse_ipc_message(&raw)
                    .ok()
                    .map(|message| MapEvent::Workspace { scene, message })
            }
        }
    });
    if let Some(MapEvent::Workspace { scene, message, .. }) = event.as_ref() {
        SESSION.with(|session| {
            let mut runtime = session.borrow_mut();
            match message {
                gaode_client::IpcMessage::MapReady => {
                    runtime.state.set_available(true);
                    runtime.failed = false;
                }
                gaode_client::IpcMessage::BoundaryUpdate { coords }
                    if *scene == MapScene::Boundary =>
                {
                    runtime.state.remember_boundary_draft(coords.clone());
                }
                gaode_client::IpcMessage::ConfirmBoundary { .. }
                | gaode_client::IpcMessage::ConfirmBoundaryGeometry { .. } => {
                    runtime.state.boundary_committed();
                }
                _ => {}
            }
        });
    }
    let handler = SESSION.with(|s| s.borrow().event_handler.clone());
    if let (Some(handler), Some(event)) = (handler, event) {
        handler(event);
    }
}

pub(crate) fn set_contract_available(available: bool) {
    SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.state.set_available(available);
        runtime.failed = !available;
    });
}

/// 提交最终显示意图。同一意图幂等；方案变化会清空上一方案全部临时现场。
pub(crate) fn present(window: Weak<crate::AppWindow>, intent: MapDisplayIntent) -> bool {
    let should_reconcile = SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        if let Some(plan_id) = intent.plan_id() {
            if runtime.state.plan_id.as_deref() != Some(plan_id) {
                crate::map_webview::set_review_map_text_visible(false);
            }
            runtime.state.enter_plan(plan_id.to_owned());
        }
        let unchanged = runtime
            .desired
            .as_ref()
            .is_some_and(|(_, desired)| desired == &intent);
        runtime.desired = Some((window, intent));
        runtime.failed = false;
        !runtime.covered && (!unchanged || !crate::map_webview::is_visible())
    });
    if should_reconcile {
        reconcile();
    }
    should_reconcile
}

fn reconcile() {
    let desired = SESSION.with(|session| {
        let runtime = session.borrow();
        (!runtime.covered && !runtime.failed)
            .then(|| runtime.desired.clone())
            .flatten()
    });
    let Some((window, intent)) = desired else {
        return;
    };
    let viewport = intent
        .scene()
        .and_then(|scene| SESSION.with(|s| s.borrow().state.viewport_for(scene)))
        .map(MapViewport::gaode);
    if let Some(scene) = intent.scene() {
        SESSION.with(|s| {
            s.borrow_mut().state.begin_generation(scene);
        });
    }
    match intent {
        MapDisplayIntent::Boundary {
            api_key,
            security_key,
            anchor,
            ..
        } => crate::map_webview::show_boundary_with_viewport(
            window,
            api_key,
            security_key,
            anchor.0,
            anchor.1,
            viewport,
        ),
        MapDisplayIntent::Orientation { mut config, .. } => {
            if let Some(viewport) = viewport {
                config = config.with_initial_viewport(viewport);
            }
            crate::map_webview::show_with_config(window, config);
        }
        MapDisplayIntent::Review {
            api_key,
            security_key,
            anchor,
            map_text_label,
            ..
        } => crate::map_webview::show_review_with_viewport(
            window,
            api_key,
            security_key,
            anchor.0,
            anchor.1,
            map_text_label,
            viewport,
        ),
        MapDisplayIntent::CampusSearch {
            api_key,
            security_key,
        } => crate::map_webview::show_campus_search(window, api_key, security_key),
        MapDisplayIntent::BlockPreview { .. } => {
            let payload = SESSION.with(|s| s.borrow().state.preview_payload.clone());
            let outcome = crate::map_webview_preview::show_block_preview(window, payload);
            if outcome == crate::map_webview::ShowOutcome::RetainedPreview {
                // 离屏保留页已就绪：立即补推最新负载与挂起定位
                // （新建页面由状态回调在 available 后推送）。
                let (payload, locate) = SESSION.with(|s| {
                    (
                        s.borrow().state.preview_payload.clone(),
                        s.borrow().state.preview_locate.clone(),
                    )
                });
                if let Some(payload) = payload {
                    push_preview_payload(&payload);
                }
                if let Some(locate) = locate {
                    push_preview_locate(&locate);
                }
            }
        }
    }
}

/// 模态窗口只改变遮挡状态；关闭后按最后的显示意图统一恢复。
pub(crate) fn cover_for_modal() {
    SESSION.with(|s| s.borrow_mut().covered = true);
    crate::map_webview::hide();
}

pub(crate) fn uncover_after_modal() {
    SESSION.with(|s| s.borrow_mut().covered = false);
    reconcile();
}

/// 离开地图场景，清除显示意图；方案内视野仍保留到切换方案为止。
pub(crate) fn hide() {
    SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.desired = None;
        runtime.state.set_available(false);
    });
    crate::map_webview::hide();
}

pub(crate) fn mark_failed() {
    SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.failed = true;
        runtime.state.set_available(false);
    });
    crate::map_webview::mark_map_failed();
    crate::map_webview::hide();
}

pub(crate) fn available() -> bool {
    SESSION.with(|s| s.borrow().state.available)
}

/// 记下最新一次预览渲染负载（第五步页面临时状态）。
///
/// 数据落会话后，若预览页当前可见则立即推送；页面尚在创建时由状态回调
/// 在就绪后补推；重建（弹窗恢复/回到第五步）时按会话负载重新内嵌。
pub(crate) fn remember_preview_payload(payload: String) {
    let should_push = SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.state.preview_payload = Some(payload.clone());
        let desired_preview = matches!(
            runtime.desired.as_ref().map(|(_, intent)| intent),
            Some(MapDisplayIntent::BlockPreview { .. })
        );
        desired_preview
            && !runtime.covered
            && !runtime.failed
            && (crate::map_webview::is_visible()
                || crate::map_webview_preview::webview_creation_disabled())
    });
    if should_push {
        push_preview_payload(&payload);
    }
}

/// 记下等待预览页就绪后执行的定位目标（卡片定位在预览生成完成后触发；
/// 页面尚在创建/重建时由状态回调补推，保证定位不丢失）。
pub(crate) fn remember_preview_locate(feature_id: String) {
    let should_push = SESSION.with(|session| {
        let mut runtime = session.borrow_mut();
        runtime.state.preview_locate = Some(feature_id.clone());
        let desired_preview = matches!(
            runtime.desired.as_ref().map(|(_, intent)| intent),
            Some(MapDisplayIntent::BlockPreview { .. })
        );
        desired_preview
            && !runtime.covered
            && !runtime.failed
            && (crate::map_webview::is_visible()
                || crate::map_webview_preview::webview_creation_disabled())
            && runtime.state.preview_payload.is_some()
    });
    if should_push {
        push_preview_locate(&feature_id);
    }
}

fn push_preview_payload(payload: &str) {
    PREVIEW_PUSH_COUNT.with(|counter| counter.set(counter.get() + 1));
    if crate::map_webview_preview::webview_creation_disabled() {
        // 契约测试探针：只记录负载推送意图，不真实执行 WebView 脚本。
        return;
    }
    // 队列安全：脚本早于页面解析到达时先把负载写进占位变量，viewer.js
    // 就绪后消费；已就绪则直接装载。
    let script = format!(
        "(function(p){{if(window.loadPreviewData){{window.loadPreviewData(p);}}else{{window.__previewPending=p;}}}})({payload});"
    );
    crate::map_webview::evaluate_script(&script);
}

fn push_preview_locate(feature_id: &str) {
    PREVIEW_LOCATE_PUSH_COUNT.with(|counter| counter.set(counter.get() + 1));
    if crate::map_webview_preview::webview_creation_disabled() {
        // 契约测试探针：只记录定位意图下发，不真实执行 WebView 脚本。
        return;
    }
    let script = format!(
        "(function(id){{if(window.locatePreviewFeature){{window.locatePreviewFeature(id);}}}})({});",
        serde_json::to_string(feature_id).unwrap_or_else(|_| "\"\"".into())
    );
    crate::map_webview::evaluate_script(&script);
}

/// 契约测试观测：会话当前记住的预览负载（可能很大，仅测试使用）。
#[doc(hidden)]
pub fn preview_payload() -> Option<String> {
    SESSION.with(|s| s.borrow().state.preview_payload.clone())
}

/// 契约测试观测：预览负载推送次数。
#[doc(hidden)]
pub fn preview_push_count() -> usize {
    PREVIEW_PUSH_COUNT.with(|counter| counter.get())
}

/// 契约测试观测：预览定位命令推送次数。
#[doc(hidden)]
pub fn preview_locate_push_count() -> usize {
    PREVIEW_LOCATE_PUSH_COUNT.with(|counter| counter.get())
}

/// 契约测试观测：清零预览推送计数。
#[doc(hidden)]
pub fn reset_preview_push_count() {
    PREVIEW_PUSH_COUNT.with(|counter| counter.set(0));
    PREVIEW_LOCATE_PUSH_COUNT.with(|counter| counter.set(0));
}

/// 记录预览页回传的统计/错误（帧率证据 + 渲染故障观测，供日志与契约测试）。
pub(crate) fn record_preview_ipc(raw: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    match value.get("type").and_then(|kind| kind.as_str()) {
        Some("preview_stats") => {
            let fps = value
                .pointer("/payload/fps")
                .and_then(|fps| fps.as_f64())
                .unwrap_or(0.0) as f32;
            let blocks = value
                .pointer("/payload/blocks")
                .and_then(|blocks| blocks.as_u64())
                .unwrap_or(0) as usize;
            let quads = value
                .pointer("/payload/quads")
                .and_then(|quads| quads.as_u64())
                .unwrap_or(0) as usize;
            PREVIEW_STATS.with(|stats| *stats.borrow_mut() = Some((fps, blocks, quads)));
        }
        Some("preview_error") => {
            let message = value
                .pointer("/payload/message")
                .and_then(|message| message.as_str())
                .unwrap_or_default()
                .to_owned();
            PREVIEW_RENDER_ERROR.with(|error| *error.borrow_mut() = Some(message));
        }
        _ => {}
    }
}

/// 契约测试/验收观测：预览页最近上报的帧率与体量统计。
#[doc(hidden)]
pub fn preview_stats() -> Option<(f32, usize, usize)> {
    PREVIEW_STATS.with(|stats| *stats.borrow())
}

/// 契约测试/验收观测：预览页最近上报的渲染错误。
#[doc(hidden)]
pub fn preview_render_error() -> Option<String> {
    PREVIEW_RENDER_ERROR.with(|error| error.borrow().clone())
}

pub(crate) fn campus_search_ready() -> bool {
    crate::map_webview::campus_search_ready()
}

pub(crate) fn prepare(destination: MapDestination) -> MapTransition {
    SESSION.with(|s| s.borrow().state.prepare(destination))
}

pub(crate) fn has_boundary_draft() -> bool {
    SESSION.with(|s| s.borrow().state.boundary_draft_dirty)
}

pub(crate) fn discard_boundary_draft() {
    SESSION.with(|s| s.borrow_mut().state.discard_boundary_draft());
}

pub(crate) fn boundary_committed() {
    SESSION.with(|s| s.borrow_mut().state.boundary_committed());
}

pub(crate) fn is_load_timeout(message: &str) -> bool {
    message == MAP_LOAD_TIMEOUT_MARKER
}

pub fn shutdown() {
    crate::map_webview::shutdown();
}

#[doc(hidden)]
pub fn map_visible() -> bool {
    crate::map_webview::map_visible()
}

#[doc(hidden)]
pub fn set_map_visible_probe(visible: bool) {
    crate::map_webview::set_map_visible_probe(visible);
}

#[doc(hidden)]
pub fn reset_review_push_count() {
    crate::map_webview::reset_review_push_count();
}

#[doc(hidden)]
pub fn review_push_count() -> usize {
    crate::map_webview::review_push_count()
}

#[doc(hidden)]
pub fn review_pushed_scripts() -> Vec<String> {
    crate::map_webview::review_pushed_scripts()
}

#[doc(hidden)]
pub fn set_review_push_probe_visible(visible: bool) {
    crate::map_webview::set_review_push_probe_visible(visible);
}

#[doc(hidden)]
pub fn review_map_text_visible() -> bool {
    crate::map_webview::review_map_text_visible()
}

pub(crate) fn command(command: MapCommand) -> MapCommandResult {
    let expected_scene = command.scene();
    let campus_command = matches!(command, MapCommand::CampusSearch { .. });
    let preview_command = matches!(
        command,
        MapCommand::PreviewReset | MapCommand::PreviewZoom(_) | MapCommand::PreviewLocate(_)
    );
    let review_command = matches!(
        command,
        MapCommand::ReviewReplace(_)
            | MapCommand::ReviewUpdate(_)
            | MapCommand::ReviewHighlight(_)
            | MapCommand::ReviewLocate(_)
            | MapCommand::ReviewMapText(_)
    );
    let allowed = SESSION.with(|session| {
        let runtime = session.borrow();
        if runtime.covered || runtime.failed {
            return false;
        }
        match runtime.desired.as_ref().map(|(_, intent)| intent) {
            Some(MapDisplayIntent::CampusSearch { .. }) => campus_command,
            Some(MapDisplayIntent::BlockPreview { .. }) => {
                preview_command
                    && (crate::map_webview::is_visible()
                        || crate::map_webview_preview::webview_creation_disabled())
            }
            Some(intent) => {
                intent.scene() == expected_scene
                    && runtime.state.command_allowed(&command) == MapCommandResult::Allowed
            }
            None => false,
        }
    });
    if !allowed {
        return MapCommandResult::Unavailable;
    }
    if let MapCommand::ReviewMapText(visible) = &command {
        crate::map_webview::set_review_map_text_visible(*visible);
    }
    let script = match command {
        MapCommand::BoundaryUndo => "undoManualPointFromDrawer();".to_owned(),
        MapCommand::BoundaryDeleteSelected => "deleteSelectedVertexFromDrawer();".to_owned(),
        MapCommand::SubmitBoundary => "submitBoundaryFromDrawer();".to_owned(),
        MapCommand::BoundaryClear => "clearManualDrawingFromDrawer();".to_owned(),
        MapCommand::BoundaryEnableManual => "enableManualMode();".to_owned(),
        MapCommand::BoundaryDraw {
            coordinates,
            label,
            restored,
        } => {
            let coordinates = serde_json::to_string(&coordinates).unwrap_or_else(|_| "[]".into());
            let label = serde_json::to_string(&label).unwrap_or_else(|_| "\"\"".into());
            let function = if restored {
                "drawRestoredBoundaryGcj"
            } else {
                "drawBoundaryGcj"
            };
            format!("{function}({coordinates}, {label});")
        }
        MapCommand::OrientationClear => "clearOrientationFromDrawer();".to_owned(),
        MapCommand::OrientationActivate => "initOrientationMode();".to_owned(),
        MapCommand::SubmitOrientation => "submitOrientationFromDrawer();".to_owned(),
        MapCommand::ReviewReplace(objects) => format!(
            "window.setReviewCandidates({});",
            serde_json::to_string(&objects).unwrap_or_else(|_| "[]".into())
        ),
        MapCommand::ReviewUpdate(object) => format!(
            "window.updateReviewCandidate({});",
            serde_json::to_string(&object).unwrap_or_else(|_| "{}".into())
        ),
        MapCommand::ReviewHighlight(Some(id)) => format!(
            "window.highlightReviewCandidate({});",
            serde_json::to_string(&id).unwrap_or_else(|_| "\"\"".into())
        ),
        MapCommand::ReviewHighlight(None) => "window.clearReviewHighlight();".to_owned(),
        MapCommand::ReviewLocate(id) => format!(
            "window.locateReviewCandidate({});",
            serde_json::to_string(&id).unwrap_or_else(|_| "\"\"".into())
        ),
        MapCommand::ReviewMapText(visible) => {
            format!("window.setReviewMapText({visible});")
        }
        MapCommand::CampusSearch { request_id, query } => {
            let query = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_owned());
            format!(
                "(function(){{if (typeof searchCampus !== 'function') return;searchCampus({request_id},{query});}})();"
            )
        }
        MapCommand::PreviewReset => {
            "(window.__previewReady ? window.resetPreviewView() : void 0);".to_owned()
        }
        MapCommand::PreviewZoom(delta) => {
            let delta = delta.clamp(0.05, 20.0);
            format!("(window.__previewReady ? window.zoomPreview({delta}) : void 0);")
        }
        MapCommand::PreviewLocate(feature_id) => {
            // 统一走 push_preview_locate：计数 + 契约探针 + 页面脚本推送。
            push_preview_locate(&feature_id);
            String::new()
        }
    };
    if review_command {
        crate::map_webview::note_review_push(&script);
    }
    if !script.is_empty() {
        crate::map_webview::evaluate_script(&script);
    }
    MapCommandResult::Allowed
}

#[cfg(test)]
mod tests {
    use super::{
        command, preview_locate_push_count, reset_preview_push_count, MapCommand, MapCommandResult,
        MapDestination, MapDisplayIntent, MapScene, MapSessionState, MapTransition, MapViewport,
        SESSION,
    };

    fn viewport(lon: f64, lat: f64, zoom: f64) -> MapViewport {
        MapViewport { lon, lat, zoom }
    }

    #[test]
    fn one_plan_shares_campus_view_and_keeps_review_view_separate() {
        let mut session = MapSessionState::default();
        session.enter_plan("plan-a");
        session.remember_viewport(MapScene::Boundary, viewport(121.44, 31.03, 17.0));

        assert_eq!(
            session.viewport_for(MapScene::Orientation),
            Some(viewport(121.44, 31.03, 17.0)),
            "边界与朝向应像同一张持续使用的地图"
        );

        session.remember_viewport(MapScene::Review, viewport(121.46, 31.05, 19.0));
        session.remember_viewport(MapScene::Orientation, viewport(121.45, 31.04, 16.0));

        assert_eq!(
            session.viewport_for(MapScene::Boundary),
            Some(viewport(121.45, 31.04, 16.0))
        );
        assert_eq!(
            session.viewport_for(MapScene::Review),
            Some(viewport(121.46, 31.05, 19.0)),
            "评审视野不能被边界/朝向覆盖"
        );
    }

    #[test]
    fn switching_plan_clears_every_temporary_map_scene() {
        let mut session = MapSessionState::default();
        session.enter_plan("plan-a");
        session.remember_viewport(MapScene::Boundary, viewport(121.44, 31.03, 17.0));
        session.remember_viewport(MapScene::Review, viewport(121.46, 31.05, 19.0));

        session.enter_plan("plan-b");

        assert_eq!(session.viewport_for(MapScene::Boundary), None);
        assert_eq!(session.viewport_for(MapScene::Orientation), None);
        assert_eq!(session.viewport_for(MapScene::Review), None);
    }

    #[test]
    fn boundary_draft_must_be_discarded_before_review_or_export() {
        let mut session = MapSessionState::default();
        session.enter_plan("plan-a");
        session.boundary_draft_changed();

        assert_eq!(
            session.prepare(MapDestination::Review),
            MapTransition::ConfirmBoundaryDraftDiscard
        );
        assert_eq!(
            session.prepare(MapDestination::Export),
            MapTransition::ConfirmBoundaryDraftDiscard
        );
        session.discard_boundary_draft();
        assert_eq!(
            session.prepare(MapDestination::Review),
            MapTransition::Ready
        );
    }

    #[test]
    fn unavailable_map_rejects_fact_changing_commands_but_keeps_saved_state() {
        let mut session = MapSessionState::default();
        session.enter_plan("plan-a");
        session.set_available(false);

        assert_eq!(
            session.command_allowed(&MapCommand::SubmitBoundary),
            MapCommandResult::Unavailable
        );
        assert_eq!(
            session.command_allowed(&MapCommand::SubmitOrientation),
            MapCommandResult::Unavailable
        );
        assert_eq!(
            session.command_allowed(&MapCommand::ReviewLocate("candidate-1".into())),
            MapCommandResult::Unavailable
        );
    }

    #[test]
    fn events_from_retired_generation_are_ignored() {
        let mut session = MapSessionState::default();
        session.enter_plan("plan-a");
        let old = session.begin_generation(MapScene::Boundary);
        let current = session.begin_generation(MapScene::Review);

        assert!(!session.accepts_event(old));
        assert!(session.accepts_event(current));
    }

    #[test]
    fn preview_locate_command_reaches_webview_under_creation_probe() {
        use crate::map_webview_preview::set_webview_creation_probe;
        use slint::Weak;

        set_webview_creation_probe(true);
        reset_preview_push_count();
        SESSION.with(|session| {
            let mut runtime = session.borrow_mut();
            runtime.desired = Some((
                Weak::<crate::AppWindow>::default(),
                MapDisplayIntent::BlockPreview {
                    plan_id: "plan-a".to_owned(),
                },
            ));
            runtime.state.preview_payload = Some("{}".to_owned());
            runtime.state.preview_locate = None;
        });

        assert_eq!(
            command(MapCommand::PreviewLocate("way/1".to_owned())),
            MapCommandResult::Allowed,
            "第五步预览页定位命令必须放行"
        );
        assert_eq!(
            preview_locate_push_count(),
            1,
            "定位命令必须下发一次到预览页"
        );

        SESSION.with(|session| {
            let mut runtime = session.borrow_mut();
            runtime.desired = None;
            runtime.state.preview_payload = None;
            runtime.state.preview_locate = None;
        });
        set_webview_creation_probe(false);
    }
}
