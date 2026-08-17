//! OSM/Overpass/Nominatim Rust 侧直连（T31）。
//!
//! 调研根因（`docs/research/candidate-data-sources-and-naming.md` §4.2）：
//! 1. 请求 URL 缺 `data=` 参数 → 服务器把参数名当查询体，必报 parse error；
//! 2. `amenity~"university|college|school"` 的 `|` 正则被新版 Overpass 拒绝
//!    （版本相关，端点滚动更新）→ 一律 union 写法；
//! 3. overpass-api.de / kumi 不给浏览器 CORS 头 → 一律 Rust 侧直连，
//!    不再依赖 WebView fetch。
//!
//! 端点按本次运行健康度顺序回退并有限重试，每端点超时 [`OVERPASS_HTTP_TIMEOUT`]；
//! Nominatim 按 OSMF 政策 ≤1 次/秒并带 User-Agent。
//!
//! 校区边界自动获取级联（ADR-0029 + T31）：
//! Nominatim 校名 → osm_type/osm_id → Overpass 按 ID 拉取；
//! 失败回退 Overpass `amenity=university|college|school` 锚点近域查询；
//! 再失败回退 `landuse=education`；均无数据 → 人工圈画兜底（由调用方决定）。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gaode_client::{convert_coords_wgs84_to_gcj02, BoundarySorter, OsmElement, OsmMember};

mod query;
mod reliability;

use query::campus_object_query_shards;
pub use query::{
    bbox_around, boundary_bbox, campus_objects_query, element_by_id_query, landuse_education_query,
    university_query,
};
use reliability::{
    FailureKind, ReliableExecutor, RequestFailure, RequestTransport, RetryPolicy, RunHealth,
};

/// Overpass 公共端点（按 de → kumi → mail.ru 顺序回退；上海网络实测见调研 §4.1）
pub const OVERPASS_ENDPOINTS: [&str; 3] = [
    "https://overpass-api.de",
    "https://overpass.kumi.systems",
    "https://maps.mail.ru/osm/tools/overpass",
];

/// 单次公共节点请求预算。大型 `out geom` 查询需要给服务端现实执行时间。
pub const OVERPASS_HTTP_TIMEOUT: Duration = Duration::from_secs(25);

/// 单个边界查询的整体预算；候选分片由 A1 传入更大的用户操作总体截止。
pub const OVERPASS_QUERY_DEADLINE: Duration = Duration::from_secs(90);

/// 校区边界整条级联（校名解析 + by-id + 近域兜底）的统一总体预算。
pub const BOUNDARY_FETCH_DEADLINE: Duration = Duration::from_secs(150);

/// Overpass 服务端超时（秒；必须比客户端超时小，避免客户端已放弃后公共端点
/// 仍继续占用算力——服务端 20s < 客户端 25s）。
pub const OVERPASS_SERVER_TIMEOUT_SECS: u32 = 20;

const OVERPASS_MAX_ROUNDS: u8 = 2;
const OVERPASS_RETRY_BACKOFF: Duration = Duration::from_secs(1);
const OVERPASS_TRANSIENT_COOLDOWN: Duration = Duration::from_secs(1);
const OVERPASS_OVERLOADED_COOLDOWN: Duration = Duration::from_secs(30);

/// 边界获取的阶段（分阶段耗时记录与用户可见反馈用，工单 workspace-restore B.8/B.9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchStage {
    /// Nominatim 校名解析
    CampusName,
    /// Overpass 按元素 ID 拉取边界
    ByElementId,
    /// Overpass amenity=university|college|school 锚点近域
    Amenity,
    /// Overpass landuse=education 兜底
    Landuse,
}

impl FetchStage {
    /// 稳定文本键（B6 zh-CN.json；S1 只按键取文案）
    pub fn label_key(self) -> &'static str {
        match self {
            Self::CampusName => "boundary.stage_campus_name",
            Self::ByElementId => "boundary.stage_by_id",
            Self::Amenity => "boundary.stage_amenity",
            Self::Landuse => "boundary.stage_landuse",
        }
    }
}

/// 一次获取进度事件（阶段 + 端点尝试 + 自请求开始已耗时）。
#[derive(Debug, Clone, PartialEq)]
pub struct FetchProgress {
    /// 当前阶段
    pub stage: FetchStage,
    /// 该阶段内第几次尝试（端点回退时 1..=total_attempts；非端点阶段为 0）
    pub attempt: u32,
    /// 该阶段端点总数（非端点阶段为 0）
    pub total_attempts: u32,
    /// 自阶段开始（端点阶段自端点查询开始）的整数秒
    pub elapsed_secs: u64,
}

/// Nominatim 端点（OSMF 公共实例）
pub const NOMINATIM_ENDPOINT: &str = "https://nominatim.openstreetmap.org/search";

/// 所有 OSM 请求的 User-Agent（Nominatim 政策要求；Overpass 礼貌请求也带）
pub const USER_AGENT: &str = "MCRebuildV2/2.0.0 (campus-reconstruction-tool; desktop)";

/// 可注入的 HTTP 传输（生产为 ureq；测试注入罐头）
pub type HttpTransport =
    Box<dyn Fn(&str, Duration) -> std::result::Result<String, String> + Send + Sync>;

/// 校区边界自动获取结果（来源标注与自动绘制所需全部事实）
#[derive(Debug, Clone, PartialEq)]
pub enum CampusBoundaryResult {
    /// 自动选取到最佳匹配（坐标已转 GCJ-02，可直接上屏）
    AutoSelected {
        /// OSM name 标签（评审/来源标注）
        name: String,
        /// GCJ-02 坐标环
        gcj02: Vec<[f64; 2]>,
        /// 数据来源（来源标注）
        source: BoundarySourceKind,
        /// 参与排序的候选元素数（排序证据）
        candidate_count: usize,
    },
    /// 各数据源均无该校区边界 → 人工圈画兜底
    NotFound,
    /// 网络/解析失败（保留错误文本供诊断；仍按 ADR-0029 回退人工圈画）
    Unreachable { message: String },
}

/// 边界数据来源（来源标注）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundarySourceKind {
    /// Nominatim 校名解析 → Overpass 按元素 ID 拉取
    NominatimByElementId,
    /// Overpass `amenity=university|college|school` 锚点近域查询
    OverpassAmenity,
    /// Overpass `landuse=education` 兜底查询
    LanduseEducation,
}

impl std::fmt::Display for BoundarySourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NominatimByElementId => f.write_str("Nominatim + Overpass by-id"),
            Self::OverpassAmenity => f.write_str("Overpass amenity=university|college|school"),
            Self::LanduseEducation => f.write_str("Overpass landuse=education"),
        }
    }
}

/// Nominatim 校名解析命中
#[derive(Debug, Clone, PartialEq)]
pub struct NominatimMatch {
    pub osm_type: String,
    pub osm_id: i64,
    pub display_name: String,
    pub class: String,
    pub kind: String,
}

/// Overpass 客户端：端点回退 + union 查询（全部可单测）。
///
/// 会话内自适应端点排序（工单 B.12 端点选择/重试策略）：公共端点负载起伏是
/// “时快时慢”的主要来源；同一会话内记住最近成功端点，后续查询优先试它，
/// 避免每次都在已知慢的端点上空等一个超时周期。
pub struct OverpassClient {
    executor: ReliableExecutor,
    /// 会话内端点尝试顺序（最近成功端点前置；初始为 [`OVERPASS_ENDPOINTS`] 原序）
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl OverpassClient {
    /// 生产传输：复用一个 ureq Agent/TLS 连接池，以 POST 请求体发送查询。
    pub fn production() -> Self {
        let order = Arc::new(Mutex::new(OVERPASS_ENDPOINTS.to_vec()));
        let agent = ureq_agent().expect("Overpass TLS client must initialize");
        Self {
            executor: ReliableExecutor::new(
                reliability::production_transport(agent),
                Arc::clone(&order),
                production_retry_policy(),
            ),
            order,
        }
    }

    /// 兼容测试注入。生产路径不使用此 GET 形态 seam。
    pub fn with_transport(transport: HttpTransport) -> Self {
        let order = Arc::new(Mutex::new(OVERPASS_ENDPOINTS.to_vec()));
        let transport: RequestTransport = Arc::new(move |request, timeout| {
            let url = format!("{}/api/interpreter?{}", request.endpoint, request.body);
            transport(&url, timeout).map_err(|message| classify_legacy_failure(&message))
        });
        Self {
            executor: ReliableExecutor::new(
                transport,
                Arc::clone(&order),
                production_retry_policy(),
            ),
            order,
        }
    }

    #[cfg(test)]
    fn with_request_transport_and_policy(transport: RequestTransport, policy: RetryPolicy) -> Self {
        let order = Arc::new(Mutex::new(OVERPASS_ENDPOINTS.to_vec()));
        Self {
            executor: ReliableExecutor::new(transport, Arc::clone(&order), policy),
            order,
        }
    }

    /// 当前端点尝试顺序（最近成功端点在前；测试断言用）。
    pub fn endpoint_order(&self) -> Vec<&'static str> {
        self.order
            .lock()
            .expect("overpass endpoint order lock")
            .clone()
    }

    /// 按端点回退执行查询：de → kumi → mail.ru；
    /// 错误页（parse error / Runtime error / HTML / 504）视为失败，换下一端点；
    /// 整体查询不得超过 [`OVERPASS_QUERY_DEADLINE`]，并记录每端点耗时。
    pub fn query_with_fallback(&self, query: &str) -> std::result::Result<String, String> {
        self.query_with_fallback_progress(query, FetchStage::ByElementId, &|_| {})
    }

    /// 同 [`Self::query_with_fallback`]，但逐端点上报进度（阶段 + 尝试序号 + 已耗时）。
    pub fn query_with_fallback_progress(
        &self,
        query: &str,
        stage: FetchStage,
        on_progress: &dyn Fn(FetchProgress),
    ) -> std::result::Result<String, String> {
        self.query_with_fallback_progress_until(
            query,
            stage,
            Instant::now() + OVERPASS_QUERY_DEADLINE,
            on_progress,
        )
    }

    fn query_with_fallback_progress_until(
        &self,
        query: &str,
        stage: FetchStage,
        deadline: Instant,
        on_progress: &dyn Fn(FetchProgress),
    ) -> std::result::Result<String, String> {
        let mut health = RunHealth::default();
        self.query_with_fallback_progress_until_with_health(
            query,
            stage,
            deadline,
            &mut health,
            on_progress,
        )
    }

    fn query_with_fallback_progress_until_with_health(
        &self,
        query: &str,
        stage: FetchStage,
        deadline: Instant,
        health: &mut RunHealth,
        on_progress: &dyn Fn(FetchProgress),
    ) -> std::result::Result<String, String> {
        let started = Instant::now();
        self.executor
            .query(query, deadline, health, &|attempt, total, _| {
                on_progress(FetchProgress {
                    stage,
                    attempt,
                    total_attempts: total,
                    elapsed_secs: started.elapsed().as_secs(),
                });
            })
    }

    /// 按有界子查询获取六类校园对象。各分片顺序执行并共享本次运行节点健康；
    /// 已成功分片只保留在内存，不会因后续分片重试而重复下载。
    pub fn query_campus_objects(
        &self,
        bbox: (f64, f64, f64, f64),
        deadline: Instant,
        on_retry: &dyn Fn(),
    ) -> std::result::Result<String, String> {
        let shards = campus_object_query_shards(bbox)
            .map_err(|error| format!("集中标签规则无法生成采集查询：{error}"))?;
        let mut health = RunHealth::default();
        let mut elements = Vec::new();
        let mut identities = BTreeSet::new();
        for shard in shards {
            let body =
                self.executor
                    .query(&shard.query, deadline, &mut health, &|attempt, _, _| {
                        if attempt > 1 {
                            on_retry();
                        }
                    })?;
            let value: serde_json::Value = serde_json::from_str(&body)
                .map_err(|error| format!("成功响应解析失败：{error}"))?;
            for element in value
                .get("elements")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let identity = (
                    element
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    element
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or_default(),
                );
                if identities.insert(identity) {
                    elements.push(element.clone());
                }
            }
        }
        serde_json::to_string(&serde_json::json!({ "elements": elements }))
            .map_err(|error| format!("合并 Overpass 子查询失败：{error}"))
    }
}

fn production_retry_policy() -> RetryPolicy {
    RetryPolicy {
        request_timeout: OVERPASS_HTTP_TIMEOUT,
        max_rounds: OVERPASS_MAX_ROUNDS,
        retry_backoff: OVERPASS_RETRY_BACKOFF,
        transient_cooldown: OVERPASS_TRANSIENT_COOLDOWN,
        overloaded_cooldown: OVERPASS_OVERLOADED_COOLDOWN,
    }
}

fn classify_legacy_failure(message: &str) -> RequestFailure {
    let lower = message.to_ascii_lowercase();
    let kind = if lower.contains("429") {
        FailureKind::RateLimited
    } else if lower.contains("504") {
        FailureKind::GatewayTimeout
    } else if lower.contains("timeout") || message.contains("超时") || message.contains("挂起")
    {
        FailureKind::Timeout
    } else {
        FailureKind::Connection
    };
    RequestFailure {
        kind,
        message: message.to_owned(),
    }
}

/// Nominatim 客户端：校名 → 校区元素（OSMF 政策：≤1 次/秒、带 User-Agent）
pub struct NominatimClient {
    transport: HttpTransport,
    endpoint: String,
    cache: Mutex<BTreeMap<String, Option<NominatimMatch>>>,
    last_request: Option<Mutex<Option<Instant>>>,
}

impl NominatimClient {
    /// 生产传输：ureq 直连，调用前按政策休眠 1 秒
    pub fn production() -> Self {
        Self {
            transport: ureq_transport(),
            endpoint: NOMINATIM_ENDPOINT.to_owned(),
            cache: Mutex::new(BTreeMap::new()),
            last_request: Some(Mutex::new(None)),
        }
    }

    /// 测试注入（不要求休眠）
    pub fn with_transport(transport: HttpTransport) -> Self {
        Self::with_endpoint_and_transport(NOMINATIM_ENDPOINT, transport)
    }

    /// 可切换的 Nominatim 兼容服务 seam；调用方仍须遵守所选服务政策。
    pub fn with_endpoint_and_transport(endpoint: &str, transport: HttpTransport) -> Self {
        Self {
            transport,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            cache: Mutex::new(BTreeMap::new()),
            last_request: None,
        }
    }

    /// 解析校名：先精确查询，无校区命中时去掉括号后缀再查一次。
    /// 命中规则：`class=amenity` 且 `type` 为 university/college/school。
    pub fn resolve_campus(
        &self,
        campus_name: &str,
    ) -> std::result::Result<Option<NominatimMatch>, String> {
        self.resolve_campus_until(campus_name, Instant::now() + Duration::from_secs(45))
    }

    fn resolve_campus_until(
        &self,
        campus_name: &str,
        deadline: Instant,
    ) -> std::result::Result<Option<NominatimMatch>, String> {
        if let Some(cached) = self
            .cache
            .lock()
            .expect("nominatim cache lock")
            .get(campus_name)
            .cloned()
        {
            return Ok(cached);
        }
        for candidate in campus_name_candidates(campus_name) {
            let url = format!(
                "{}?q={}&format=json&limit=5",
                self.endpoint,
                encode_query(&candidate)
            );
            let mut last_error = None;
            for _ in 0..2 {
                self.wait_for_rate_limit(deadline)?;
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err("Nominatim 校名解析达到边界获取总体截止".to_owned());
                }
                match (self.transport)(&url, Duration::from_secs(15).min(remaining)) {
                    Ok(body) => match parse_nominatim_results_checked(&body) {
                        Ok(results) => {
                            if let Some(matched) = results.into_iter().find(is_campus_like) {
                                self.cache
                                    .lock()
                                    .expect("nominatim cache lock")
                                    .insert(campus_name.to_owned(), Some(matched.clone()));
                                return Ok(Some(matched));
                            }
                            last_error = None;
                            break;
                        }
                        Err(error) => last_error = Some(error),
                    },
                    Err(error) => last_error = Some(error),
                }
            }
            if let Some(error) = last_error {
                return Err(format!("Nominatim 请求或响应失败：{error}"));
            }
        }
        Ok(None)
    }

    fn wait_for_rate_limit(&self, deadline: Instant) -> std::result::Result<(), String> {
        let Some(last_request) = &self.last_request else {
            return Ok(());
        };
        let mut last = last_request.lock().expect("nominatim rate limit lock");
        if let Some(previous) = *last {
            let required = Duration::from_secs(1).saturating_sub(previous.elapsed());
            if required > deadline.saturating_duration_since(Instant::now()) {
                return Err("Nominatim 限速等待将超过边界获取总体截止".to_owned());
            }
            if !required.is_zero() {
                let (_sender, receiver) = std::sync::mpsc::channel::<()>();
                let _ = receiver.recv_timeout(required);
            }
        }
        *last = Some(Instant::now());
        Ok(())
    }
}

/// 校区边界自动获取器：Nominatim → by-id → amenity 近域 → landuse 级联
pub struct CampusBoundaryFetcher {
    overpass: Arc<OverpassClient>,
    nominatim: NominatimClient,
}

impl CampusBoundaryFetcher {
    pub fn production() -> Self {
        Self {
            overpass: Arc::new(OverpassClient::production()),
            nominatim: NominatimClient::production(),
        }
    }

    /// 生产构造：注入共享 Overpass 客户端（会话内端点自适应排序跨查询生效）。
    pub fn production_with_overpass(overpass: Arc<OverpassClient>) -> Self {
        Self {
            overpass,
            nominatim: NominatimClient::production(),
        }
    }

    pub fn with_clients(overpass: OverpassClient, nominatim: NominatimClient) -> Self {
        Self {
            overpass: Arc::new(overpass),
            nominatim,
        }
    }

    /// 级联获取校区边界（锚点为 GCJ-02；OSM 元素 WGS-84 先转 GCJ-02 再排序）。
    pub fn fetch_campus(
        &self,
        campus_name: &str,
        anchor_lon: f64,
        anchor_lat: f64,
    ) -> CampusBoundaryResult {
        self.fetch_campus_with_progress(campus_name, anchor_lon, anchor_lat, &|_| {})
    }

    /// 同 [`Self::fetch_campus`]，但分阶段上报进度（工单 B.9：超过 15s 必须给
    /// 用户明确阶段反馈，不是干等）并记录每阶段耗时（工单 B.8）。
    pub fn fetch_campus_with_progress(
        &self,
        campus_name: &str,
        anchor_lon: f64,
        anchor_lat: f64,
        on_progress: &dyn Fn(FetchProgress),
    ) -> CampusBoundaryResult {
        let fetch_started = Instant::now();
        let overall_deadline = fetch_started + BOUNDARY_FETCH_DEADLINE;
        let mut overpass_health = RunHealth::default();
        on_progress(FetchProgress {
            stage: FetchStage::CampusName,
            attempt: 0,
            total_attempts: 0,
            elapsed_secs: 0,
        });
        // 1. Nominatim 校名 → 元素 ID → Overpass 按 ID 拉取
        let campus_name_result = self
            .nominatim
            .resolve_campus_until(campus_name, overall_deadline);
        log::info!(
            "边界获取阶段 校名解析 完成，耗时 {:?}",
            fetch_started.elapsed()
        );
        match campus_name_result {
            Ok(Some(matched)) => {
                let query = element_by_id_query(&matched.osm_type, matched.osm_id);
                match self
                    .overpass
                    .as_ref()
                    .query_with_fallback_progress_until_with_health(
                        &query,
                        FetchStage::ByElementId,
                        overall_deadline,
                        &mut overpass_health,
                        on_progress,
                    ) {
                    Ok(body) => {
                        if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name)
                        {
                            return CampusBoundaryResult::AutoSelected {
                                name: best.name,
                                gcj02: best.geometry,
                                source: BoundarySourceKind::NominatimByElementId,
                                candidate_count: best.candidate_count,
                            };
                        }
                    }
                    Err(message) => {
                        return fallback_after_error(
                            self.overpass.as_ref(),
                            anchor_lon,
                            anchor_lat,
                            campus_name,
                            format!("边界 by-ID 查询失败：{message}"),
                            overall_deadline,
                            &mut overpass_health,
                            on_progress,
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(message) => {
                return fallback_after_error(
                    self.overpass.as_ref(),
                    anchor_lon,
                    anchor_lat,
                    campus_name,
                    format!("校名解析失败：{message}"),
                    overall_deadline,
                    &mut overpass_health,
                    on_progress,
                );
            }
        }
        // 2. ADR-0029 主路径：amenity=university|college|school 锚点近域
        match self.query_and_select(
            &university_query(bbox_around(anchor_lon, anchor_lat, 1500.0)),
            anchor_lon,
            anchor_lat,
            campus_name,
            BoundarySourceKind::OverpassAmenity,
            FetchStage::Amenity,
            overall_deadline,
            &mut overpass_health,
            on_progress,
        ) {
            Some(result) => result,
            None => CampusBoundaryResult::NotFound,
        }
    }

    /// 执行一次 Overpass 查询并自动选取最佳边界；查询失败返回 Unreachable。
    #[allow(
        clippy::too_many_arguments,
        reason = "级联查询需要：查询/锚点/校名/来源 + 进度阶段与回调；保持平铺可读"
    )]
    fn query_and_select(
        &self,
        query: &str,
        anchor_lon: f64,
        anchor_lat: f64,
        campus_name: &str,
        source: BoundarySourceKind,
        stage: FetchStage,
        deadline: Instant,
        health: &mut RunHealth,
        on_progress: &dyn Fn(FetchProgress),
    ) -> Option<CampusBoundaryResult> {
        match self
            .overpass
            .query_with_fallback_progress_until_with_health(
                query,
                stage,
                deadline,
                health,
                on_progress,
            ) {
            Ok(body) => select_best(&body, anchor_lon, anchor_lat, campus_name).map(|best| {
                CampusBoundaryResult::AutoSelected {
                    name: best.name,
                    gcj02: best.geometry,
                    source,
                    candidate_count: best.candidate_count,
                }
            }),
            Err(message) => Some(CampusBoundaryResult::Unreachable {
                message: format!(
                    "{}查询失败：{message}",
                    match stage {
                        FetchStage::Amenity => "amenity 近域",
                        FetchStage::Landuse => "landuse 兜底",
                        FetchStage::ByElementId => "边界 by-ID",
                        FetchStage::CampusName => "校名解析",
                    }
                ),
            }),
        }
    }
}

/// Nominatim 失败后按级联继续：amenity 近域 → landuse=education → NotFound/Unreachable
#[allow(
    clippy::too_many_arguments,
    reason = "级联回退需要保留锚点、首段错误、统一截止、运行健康和 UI 进度上下文"
)]
fn fallback_after_error(
    overpass: &OverpassClient,
    anchor_lon: f64,
    anchor_lat: f64,
    campus_name: &str,
    first_error: String,
    deadline: Instant,
    health: &mut RunHealth,
    on_progress: &dyn Fn(FetchProgress),
) -> CampusBoundaryResult {
    let amenity = overpass.query_with_fallback_progress_until_with_health(
        &university_query(bbox_around(anchor_lon, anchor_lat, 1500.0)),
        FetchStage::Amenity,
        deadline,
        health,
        on_progress,
    );
    match amenity {
        Ok(body) => {
            if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name) {
                return CampusBoundaryResult::AutoSelected {
                    name: best.name,
                    gcj02: best.geometry,
                    source: BoundarySourceKind::OverpassAmenity,
                    candidate_count: best.candidate_count,
                };
            }
            let landuse = overpass.query_with_fallback_progress_until_with_health(
                &landuse_education_query(bbox_around(anchor_lon, anchor_lat, 1500.0)),
                FetchStage::Landuse,
                deadline,
                health,
                on_progress,
            );
            match landuse {
                Ok(body) => {
                    if let Some(best) = select_best(&body, anchor_lon, anchor_lat, campus_name) {
                        return CampusBoundaryResult::AutoSelected {
                            name: best.name,
                            gcj02: best.geometry,
                            source: BoundarySourceKind::LanduseEducation,
                            candidate_count: best.candidate_count,
                        };
                    }
                    CampusBoundaryResult::NotFound
                }
                Err(message) => CampusBoundaryResult::Unreachable {
                    message: format!("{first_error}；landuse 兜底查询失败：{message}"),
                },
            }
        }
        Err(message) => CampusBoundaryResult::Unreachable {
            message: format!("{first_error}；amenity 近域查询失败：{message}"),
        },
    }
}

struct SelectedBoundary {
    name: String,
    geometry: Vec<[f64; 2]>,
    candidate_count: usize,
}

/// 归一化坐标环：截断到第一次回到首点的位置。
///
/// 真实 OSM 几何的闭合节点可能不在末尾（环在中途闭合后仍带尾点，如
/// 华东师大普陀校区边界 90 点中第 87 点即首点、其后还带 2 个尾点）。
/// 归一化保证进入桌面链路的边界是干净单环（去掉重复闭合点与尾点），
/// 避免确认校验把共享端点的尾边误判为自相交（T33）。
fn normalize_closed_ring(coords: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let Some(first) = coords.first().copied() else {
        return Vec::new();
    };
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(coords.len());
    for point in &coords {
        if out.len() > 1 && *point == first {
            break;
        }
        out.push(*point);
    }
    out
}

/// 解析 Overpass 响应 → WGS-84 转 GCJ-02 → 按 ADR-0029 排序（锚点包含 →
/// 名称匹配 → 距离最近）→ 取最佳。
fn select_best(
    json: &str,
    anchor_lon: f64,
    anchor_lat: f64,
    campus_name: &str,
) -> Option<SelectedBoundary> {
    let elements = elements_to_gcj02(parse_elements(json));
    let candidate_count = elements.len();
    let mut sorted =
        BoundarySorter::sort_candidates(elements, anchor_lon, anchor_lat, Some(campus_name));
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| {
        b.contains_anchor
            .cmp(&a.contains_anchor)
            .then_with(|| b.name_match_score.total_cmp(&a.name_match_score))
            .then_with(|| a.distance_to_anchor_m.total_cmp(&b.distance_to_anchor_m))
    });
    let best = sorted.remove(0);
    Some(SelectedBoundary {
        name: best
            .element
            .tags
            .get("name")
            .cloned()
            .unwrap_or_else(|| best.element.id.to_string()),
        geometry: normalize_closed_ring(best.element.geometry?),
        candidate_count,
    })
}

/// 解析 Overpass `out geom` JSON 为 OSM 元素（way 用 geometry；relation 拼接外环成员）
pub fn parse_elements(json: &str) -> Vec<OsmElement> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(elements) = value.get("elements").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    elements.iter().filter_map(parse_element).collect()
}

fn parse_element(value: &serde_json::Value) -> Option<OsmElement> {
    let kind = value.get("type")?.as_str()?;
    let id = value.get("id")?.as_i64()?;
    let tags = value
        .get("tags")
        .and_then(serde_json::Value::as_object)
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let members = value
        .get("members")
        .and_then(serde_json::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|m| {
                    Some(OsmMember {
                        r#type: m.get("type")?.as_str()?.to_owned(),
                        reference: m.get("ref")?.as_i64()?,
                        role: m
                            .get("role")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let geometry = match kind {
        "way" => value
            .get("geometry")
            .and_then(serde_json::Value::as_array)
            .map(|points| geometry_points(points)),
        "relation" => value
            .get("members")
            .and_then(serde_json::Value::as_array)
            .map(|members| {
                let mut points = Vec::new();
                for member in members {
                    let role = member
                        .get("role")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if role == "outer" || role.is_empty() {
                        if let Some(part) =
                            member.get("geometry").and_then(serde_json::Value::as_array)
                        {
                            points.extend(geometry_points(part));
                        }
                    }
                }
                points
            })
            .filter(|points| !points.is_empty()),
        _ => None,
    };
    Some(OsmElement {
        r#type: kind.to_owned(),
        id,
        geometry,
        members,
        tags,
    })
}

fn geometry_points(points: &[serde_json::Value]) -> Vec<[f64; 2]> {
    points
        .iter()
        .filter_map(|point| Some([point.get("lon")?.as_f64()?, point.get("lat")?.as_f64()?]))
        .collect()
}

/// WGS-84 元素几何就地转 GCJ-02（原始 WGS-84 由原始载荷另行保全）
pub fn elements_to_gcj02(mut elements: Vec<OsmElement>) -> Vec<OsmElement> {
    for element in &mut elements {
        if let Some(geometry) = element.geometry.as_mut() {
            convert_coords_wgs84_to_gcj02(geometry);
        }
    }
    elements
}

/// 校名候选：精确名 → 去括号后缀的基础名（高德校区名常带“(闵行本部校区)”后缀）
pub fn campus_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_owned()];
    let stripped = strip_parenthetical_suffix(name);
    if stripped != name && !stripped.is_empty() {
        candidates.push(stripped);
    }
    candidates
}

fn strip_parenthetical_suffix(name: &str) -> String {
    // 全角（…）或半角（…）后缀，保留括号前主体
    for (open, close) in [('（', '）'), ('(', ')')] {
        if let Some(open_index) = name.rfind(open) {
            if name[open_index..].contains(close) {
                return name[..open_index].trim().to_owned();
            }
        }
    }
    name.to_owned()
}

/// 解析 Nominatim JSON 结果
pub fn parse_nominatim_results(json: &str) -> Vec<NominatimMatch> {
    parse_nominatim_results_checked(json).unwrap_or_default()
}

fn parse_nominatim_results_checked(json: &str) -> std::result::Result<Vec<NominatimMatch>, String> {
    let value = serde_json::from_str::<serde_json::Value>(json)
        .map_err(|error| format!("JSON 解析失败：{error}"))?;
    let results = value
        .as_array()
        .ok_or_else(|| "响应顶层不是数组".to_owned())?;
    Ok(results
        .iter()
        .filter_map(|item| {
            let osm_type = item.get("osm_type")?.as_str()?.to_owned();
            let osm_id = item.get("osm_id")?.as_i64()?;
            if osm_type == "node" {
                // 校名解析只认面元素（way/relation）；地铁站/公交站 node 会干扰
                return None;
            }
            Some(NominatimMatch {
                osm_type,
                osm_id,
                display_name: item
                    .get("display_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                class: item
                    .get("class")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                kind: item
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect())
}

fn is_campus_like(matched: &NominatimMatch) -> bool {
    matched.class == "amenity"
        && matches!(matched.kind.as_str(), "university" | "college" | "school")
}

/// 百分号编码（覆盖 UTF-8 中文字节与 Overpass 语法字符）
pub fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

fn first_error_line(body: &str) -> String {
    body.lines()
        .find(|line| {
            let normalized = line.to_ascii_lowercase();
            normalized.contains("parse error") || normalized.contains("error")
        })
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn ureq_agent() -> std::result::Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
    Ok(ureq::AgentBuilder::new()
        .user_agent(USER_AGENT)
        .tls_connector(Arc::new(tls))
        .build())
}

/// Nominatim 生产 HTTP 传输（复用同一个 ureq Agent/连接池）。
fn ureq_transport() -> HttpTransport {
    let agent = ureq_agent().expect("Nominatim TLS client must initialize");
    Box::new(move |url: &str, timeout: Duration| {
        let response = agent
            .get(url)
            .timeout(timeout)
            .call()
            .map_err(|error| error.to_string())?;
        response.into_string().map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod overpass_tests;
