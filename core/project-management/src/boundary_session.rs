//! 方案边界获取会话。
//!
//! 本模块在 F3 接口后拥有方案级边界缓存、在途去重和陈旧结果隔离；调用方只
//! 提交打开方案、获取、刷新、确认和清除意图，不管理 receiver 或缓存键。

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use shared_domain_types::PlanId;

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

/// 真实 OSM 适配器与测试 fake 共用的外部来源 seam。
pub type BoundarySource = Arc<dyn Fn(&str, f64, f64) -> BoundaryFetchOutcome + Send + Sync>;

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
    Loading,
    Ready(BoundaryFetchOutcome),
    Stale,
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
    receiver: mpsc::Receiver<BoundaryFetchOutcome>,
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
        let outcome = match pending.receiver.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return BoundaryPoll::Loading,
            Err(mpsc::TryRecvError::Disconnected) => BoundaryFetchOutcome::Unreachable {
                message: "boundary fetch worker disconnected".to_owned(),
            },
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
        std::thread::spawn(move || {
            let _ = sender.send(source(&campus_name, anchor_lon, anchor_lat));
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
