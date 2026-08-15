//! 方案边界获取会话。
//!
//! 本模块在 F3 接口后拥有方案级边界缓存、在途去重和陈旧结果隔离；调用方只
//! 提交打开方案、获取、刷新、确认和清除意图，不管理 receiver 或缓存键。

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use shared_domain_types::PlanId;

/// 边界获取阶段（分阶段反馈与耗时记录，工单 B.8/B.9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryFetchStage {
    /// Nominatim 校名解析
    CampusName,
    /// Overpass 按元素 ID 拉取边界
    ByElementId,
    /// Overpass amenity 近域
    Amenity,
    /// Overpass landuse 兜底
    Landuse,
}

/// 一次边界获取进度事件（阶段 + 端点尝试 + 已耗时）。
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryFetchProgress {
    /// 当前阶段
    pub stage: BoundaryFetchStage,
    /// 该阶段内第几次尝试（端点回退时 1..=total_attempts；非端点阶段为 0）
    pub attempt: u32,
    /// 该阶段端点总数（非端点阶段为 0）
    pub total_attempts: u32,
    /// 自请求开始（或该阶段端点查询开始）的整数秒
    pub elapsed_secs: u64,
}

/// 外部边界来源返回的结构化结果。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryFetchOutcome {
    AutoSelected {
        name: String,
        gcj02: Vec<[f64; 2]>,
        source: String,
        candidate_count: usize,
    },
    NotFound,
    Unreachable {
        message: String,
    },
}

/// 后台线程上报的进度通道（S1 只显示阶段与耗时，不判断业务条件）。
pub type BoundaryProgressSink = Arc<dyn Fn(BoundaryFetchProgress) + Send + Sync>;

/// 真实 OSM 适配器与测试 fake 共用的外部来源 seam。
pub type BoundarySource =
    Arc<dyn Fn(&str, f64, f64, BoundaryProgressSink) -> BoundaryFetchOutcome + Send + Sync>;

/// S1 恢复边界页面所需的只读状态；正式获取状态仍由本模块持有。
#[derive(Debug, Clone, PartialEq)]
pub struct PlanBoundaryView {
    pub name: String,
    pub gcj02: Vec<[f64; 2]>,
    pub confirmed: bool,
}

/// 获取/刷新请求的同步决定。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryRequest {
    Ready(PlanBoundaryView),
    Started,
    Loading,
    MissingContext,
}

/// 后台边界请求的非阻塞轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryPoll {
    Idle,
    /// 仍在获取；带最近一次进度事件（无新事件时为 None，呈现层沿用上次阶段）
    Loading {
        progress: Option<BoundaryFetchProgress>,
    },
    Ready(BoundaryFetchOutcome),
    Stale,
}

/// 后台线程经通道上报的事件（进度 + 最终结果）。
enum BoundaryFetchEvent {
    Progress(BoundaryFetchProgress),
    Done(BoundaryFetchOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BoundaryCacheKey {
    plan_id: PlanId,
    campus_name: String,
    anchor_lon_bits: u64,
    anchor_lat_bits: u64,
}

impl BoundaryCacheKey {
    fn new(plan_id: PlanId, campus_name: String, anchor_lon: f64, anchor_lat: f64) -> Self {
        Self {
            plan_id,
            campus_name,
            anchor_lon_bits: anchor_lon.to_bits(),
            anchor_lat_bits: anchor_lat.to_bits(),
        }
    }

    fn anchor(&self) -> (f64, f64) {
        (
            f64::from_bits(self.anchor_lon_bits),
            f64::from_bits(self.anchor_lat_bits),
        )
    }
}

#[derive(Debug, Clone)]
struct PlanBoundaryState {
    key: BoundaryCacheKey,
    official: Option<PlanBoundaryView>,
    candidate: Option<PlanBoundaryView>,
}

impl PlanBoundaryState {
    fn visible(&self) -> Option<PlanBoundaryView> {
        self.candidate.clone().or_else(|| self.official.clone())
    }
}

struct PendingBoundaryFetch {
    receiver: mpsc::Receiver<BoundaryFetchEvent>,
}

/// F3 的方案级边界会话入口。
pub struct PlanBoundarySession {
    source: BoundarySource,
    active_key: Option<BoundaryCacheKey>,
    plans: HashMap<PlanId, PlanBoundaryState>,
    pending: HashMap<BoundaryCacheKey, PendingBoundaryFetch>,
}

impl PlanBoundarySession {
    pub fn new(source: BoundarySource) -> Self {
        Self {
            source,
            active_key: None,
            plans: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// 切换方案并返回该方案可恢复的最新边界；不同来源配置会使旧结果失效。
    pub fn open_plan(
        &mut self,
        plan_id: PlanId,
        campus_name: String,
        anchor_lon: f64,
        anchor_lat: f64,
    ) -> Option<PlanBoundaryView> {
        let key = BoundaryCacheKey::new(plan_id, campus_name, anchor_lon, anchor_lat);
        self.active_key = Some(key.clone());
        self.pending
            .retain(|pending_key, _| pending_key.plan_id != plan_id || *pending_key == key);
        if self
            .plans
            .get(&plan_id)
            .is_some_and(|state| state.key != key)
        {
            self.plans.remove(&plan_id);
        }
        self.visible_active()
    }

    /// 地图就绪触发的普通获取：完成结果和同键在途请求都会直接复用。
    pub fn request(&mut self) -> BoundaryRequest {
        if self
            .active_key
            .as_ref()
            .is_some_and(|active| self.pending.contains_key(active))
        {
            return BoundaryRequest::Loading;
        }
        if let Some(view) = self.visible_active() {
            return BoundaryRequest::Ready(view);
        }
        self.start_active_request()
    }

    /// 用户明确刷新：Ready 结果失效并启动一次请求；同键 Loading 不重复请求。
    /// 已确认正式边界保留到新候选确认，刷新候选与正式边界不会混为一个状态。
    pub fn refresh(&mut self) -> BoundaryRequest {
        let Some(key) = self.active_key.clone() else {
            return BoundaryRequest::MissingContext;
        };
        if self.pending.contains_key(&key) {
            return BoundaryRequest::Loading;
        }
        if let Some(state) = self.plans.get_mut(&key.plan_id) {
            state.candidate = None;
        }
        self.spawn_request(key)
    }

    /// 用户清除/重画正式边界：该方案的完成结果与在途请求一起失效。
    pub fn clear_active(&mut self) {
        if let Some(key) = self.active_key.as_ref() {
            self.plans.remove(&key.plan_id);
            self.pending.remove(key);
        }
    }

    /// 用户确认当前编辑几何后，原子替换本会话的正式边界。
    pub fn confirm(&mut self, gcj02: Vec<[f64; 2]>) {
        let Some(key) = self.active_key.clone() else {
            return;
        };
        let state = self
            .plans
            .entry(key.plan_id)
            .or_insert_with(|| PlanBoundaryState {
                key: key.clone(),
                official: None,
                candidate: None,
            });
        let name = state
            .candidate
            .as_ref()
            .or(state.official.as_ref())
            .map(|view| view.name.clone())
            .unwrap_or_default();
        state.key = key;
        state.official = Some(PlanBoundaryView {
            name,
            gcj02,
            confirmed: true,
        });
        state.candidate = None;
    }

    /// 非阻塞轮询；失败不写完成缓存，过期键的结果不进入活动方案。
    pub fn poll(&mut self) -> BoundaryPoll {
        let Some(key) = self.active_key.clone() else {
            return BoundaryPoll::Idle;
        };
        let Some(pending) = self.pending.get_mut(&key) else {
            return BoundaryPoll::Idle;
        };
        let event = match pending.receiver.try_recv() {
            Ok(event) => event,
            Err(mpsc::TryRecvError::Empty) => return BoundaryPoll::Loading { progress: None },
            Err(mpsc::TryRecvError::Disconnected) => {
                BoundaryFetchEvent::Done(BoundaryFetchOutcome::Unreachable {
                    message: "boundary fetch worker disconnected".to_owned(),
                })
            }
        };
        // 进度事件只透传，不终结在途请求（最终 Done 事件才移除 pending）。
        if let BoundaryFetchEvent::Progress(progress) = &event {
            return BoundaryPoll::Loading {
                progress: Some(progress.clone()),
            };
        }
        let BoundaryFetchEvent::Done(outcome) = event else {
            unreachable!("event handled above");
        };
        self.pending.remove(&key).expect("pending boundary fetch");
        if self.active_key.as_ref() != Some(&key) {
            return BoundaryPoll::Stale;
        }
        if let BoundaryFetchOutcome::AutoSelected { name, gcj02, .. } = &outcome {
            let state = self
                .plans
                .entry(key.plan_id)
                .or_insert_with(|| PlanBoundaryState {
                    key: key.clone(),
                    official: None,
                    candidate: None,
                });
            state.key = key;
            state.candidate = Some(PlanBoundaryView {
                name: name.clone(),
                gcj02: gcj02.clone(),
                confirmed: false,
            });
        }
        BoundaryPoll::Ready(outcome)
    }

    /// 工作现场恢复：把已确认边界预置为当前方案的正式状态（重启后不再
    /// 重复校名解析/Overpass 查询；map_ready 直接命中缓存）。
    pub fn restore_confirmed(&mut self, view: PlanBoundaryView) {
        let Some(key) = self.active_key.clone() else {
            return;
        };
        let state = self
            .plans
            .entry(key.plan_id)
            .or_insert_with(|| PlanBoundaryState {
                key: key.clone(),
                official: None,
                candidate: None,
            });
        state.key = key;
        state.official = Some(view);
        state.candidate = None;
    }

    /// 当前活动方案的可见边界（恢复/落库读取边界名用）。
    pub fn active_view(&self) -> Option<PlanBoundaryView> {
        self.visible_active()
    }

    fn start_active_request(&mut self) -> BoundaryRequest {
        let Some(key) = self.active_key.clone() else {
            return BoundaryRequest::MissingContext;
        };
        if self.pending.contains_key(&key) {
            return BoundaryRequest::Loading;
        }
        self.spawn_request(key)
    }

    fn spawn_request(&mut self, key: BoundaryCacheKey) -> BoundaryRequest {
        let (anchor_lon, anchor_lat) = key.anchor();
        let campus_name = key.campus_name.clone();
        let source = Arc::clone(&self.source);
        let (sender, receiver) = mpsc::channel();
        let progress_sender = sender.clone();
        let progress_sink: BoundaryProgressSink = Arc::new(move |progress| {
            let _ = progress_sender.send(BoundaryFetchEvent::Progress(progress));
        });
        std::thread::spawn(move || {
            let outcome = source(&campus_name, anchor_lon, anchor_lat, progress_sink);
            let _ = sender.send(BoundaryFetchEvent::Done(outcome));
        });
        self.pending.insert(key, PendingBoundaryFetch { receiver });
        BoundaryRequest::Started
    }

    fn visible_active(&self) -> Option<PlanBoundaryView> {
        let key = self.active_key.as_ref()?;
        self.plans
            .get(&key.plan_id)
            .filter(|state| state.key == *key)
            .and_then(PlanBoundaryState::visible)
    }
}
