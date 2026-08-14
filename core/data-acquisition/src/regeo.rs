//! 高德逆地理编码（regeo）补名（T31 命名第二级；T36 有界并发 + 持久化缓存）。
//!
//! 命名两级：优先 OSM `name` 标签（零成本、与几何同源）；无名字的关键建筑
//! （教学楼/图书馆/宿舍等）用高德 regeo 反查最近 POI/地址补名并**持久化缓存**
//! （SQLite，B2 [`data_persistence::RegeoNameCache`]），重复采集不再重复调用。
//!
//! T36 铁律：
//! - 生产补名为**有界并发批量补名**（默认 8 路），不串行阻塞采集主链路；
//! - 单次 regeo 调用超时 10s → 5s；失败立即降级为不补名（名称保持 #id）；
//! - 每次采集运行设总体截止时间（默认 ≤60s，由 A1 传入）与补名调用上限
//!   （默认 256 次），超限立即结束并如实标注“部分建筑未命名”，禁止无限等待；
//! - 未配置 Key / 调用失败时返回 `None`，名称保持“未命名建筑 #id”，不阻断导出。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use data_persistence::{CandidateNameSource, RegeoNameCacheApi};

use crate::source::{BatchEnrichment, NameEnricher, RawEntity, SourceGeometry};

/// 高德 Web 服务 regeo 端点
pub const REGEO_ENDPOINT: &str = "https://restapi.amap.com/v3/geocode/regeo";

/// 单次 regeo 调用超时（工单：10s → 5s）
pub const REGEO_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// 生产补名并发路数（有界并发）
pub const REGEO_CONCURRENCY: usize = 8;

/// 单次采集运行的补名调用上限（超限立即结束）
pub const REGEO_MAX_CALLS_PER_RUN: usize = 256;

/// regeo HTTP 传输（生产为 ureq；测试注入罐头）
pub type RegeoTransport =
    Box<dyn Fn(&str, Duration) -> std::result::Result<String, String> + Send + Sync>;

/// Web 服务 Key 提供器（只经设置页录入；生产按数据库路径实时读取）
pub type KeyProvider = Box<dyn Fn() -> Option<String> + Send + Sync>;

/// 一次 regeo 调用的最终去向（名称 + 来源 + 是否失败）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct LookupOutcome {
    name: Option<String>,
    source: CandidateNameSource,
}

impl LookupOutcome {
    fn named(name: String, source: CandidateNameSource) -> Self {
        Self {
            name: Some(name),
            source,
        }
    }

    fn unnamed(source: CandidateNameSource) -> Self {
        Self { name: None, source }
    }
}

/// regeo 补名器：面几何中心点反查 + 持久化缓存 + 有界并发批量补名
pub struct RegeoNamer {
    transport: RegeoTransport,
    key_provider: KeyProvider,
    cache: Arc<dyn RegeoNameCacheApi>,
}

impl RegeoNamer {
    /// 测试 / 默认：注入传输 + 内存缓存（语义与 SQLite 持久化版一致）
    pub fn new(transport: RegeoTransport, key_provider: KeyProvider) -> Self {
        Self {
            transport,
            key_provider,
            cache: Arc::new(InMemoryRegeoCache::default()),
        }
    }

    /// 生产：ureq 直连 + 设置页 Key 提供器 + 持久化 SQLite 缓存
    pub fn production(key_provider: KeyProvider, cache: Arc<dyn RegeoNameCacheApi>) -> Self {
        Self {
            transport: Box::new(|url: &str, timeout: Duration| {
                let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
                let agent = ureq::AgentBuilder::new()
                    .timeout(timeout)
                    .tls_connector(Arc::new(tls))
                    .build();
                let response = agent.get(url).call().map_err(|error| error.to_string())?;
                response.into_string().map_err(|error| error.to_string())
            }),
            key_provider,
            cache,
        }
    }

    /// 为面几何补名：仅 Polygon（点位只作证据，不做 regeo 扩面/补名主体）。
    /// 返回 `None` 表示无 Key、无结果或几何不是面。
    pub fn name_for_geometry(&self, geometry: &SourceGeometry) -> Option<String> {
        let (lon, lat) = polygon_centroid(geometry)?;
        let cache_key = format!("{lon:.5},{lat:.5}");
        if let Ok(Some(cached)) = self.cache.get_regeo_name(&cache_key) {
            return cached;
        }
        self.lookup_uncached(&cache_key, lon, lat).name
    }

    /// 单次网络调用（5s 超时；失败降级为不补名）
    fn lookup_uncached(&self, cache_key: &str, lon: f64, lat: f64) -> LookupOutcome {
        let Some(key) = (self.key_provider)() else {
            return LookupOutcome::unnamed(CandidateNameSource::Failed);
        };
        self.lookup_uncached_with_key(&key, cache_key, lon, lat)
    }

    /// 用已知 Key 执行一次网络调用并写缓存
    fn lookup_uncached_with_key(
        &self,
        key: &str,
        cache_key: &str,
        lon: f64,
        lat: f64,
    ) -> LookupOutcome {
        let url =
            format!("{REGEO_ENDPOINT}?key={key}&location={lon},{lat}&radius=200&extensions=base");
        match (self.transport)(&url, REGEO_HTTP_TIMEOUT) {
            Ok(body) => {
                let parsed = parse_regeo_name_outcome(&body);
                let outcome = match parsed {
                    RegeoNameOutcome {
                        name: Some(name),
                        failed: false,
                    } => LookupOutcome::named(name, CandidateNameSource::Gaode),
                    RegeoNameOutcome {
                        name: None,
                        failed: true,
                    } => LookupOutcome::unnamed(CandidateNameSource::Failed),
                    RegeoNameOutcome {
                        name: None,
                        failed: false,
                    } => LookupOutcome::unnamed(CandidateNameSource::Unnamed),
                    RegeoNameOutcome {
                        name: Some(_),
                        failed: true,
                    } => LookupOutcome::unnamed(CandidateNameSource::Failed),
                };
                let _ = self
                    .cache
                    .put_regeo_name(cache_key, outcome.name.as_deref());
                outcome
            }
            Err(_) => LookupOutcome::unnamed(CandidateNameSource::Failed),
        }
    }
}

/// 内存版缓存接口实现（测试与降级；语义与 SQLite 持久化版一致）
#[derive(Default)]
struct InMemoryRegeoCache {
    inner: Mutex<std::collections::HashMap<String, Option<String>>>,
}

impl RegeoNameCacheApi for InMemoryRegeoCache {
    fn get_regeo_name(&self, cache_key: &str) -> data_persistence::Result<Option<Option<String>>> {
        Ok(self
            .inner
            .lock()
            .expect("in-memory regeo cache lock")
            .get(cache_key)
            .cloned())
    }

    fn put_regeo_name(&self, cache_key: &str, name: Option<&str>) -> data_persistence::Result<()> {
        self.inner
            .lock()
            .expect("in-memory regeo cache lock")
            .insert(cache_key.to_owned(), name.map(str::to_owned));
        Ok(())
    }
}

impl NameEnricher for RegeoNamer {
    /// 有界并发批量补名：
    /// - 8 路 worker 共享派发游标，不串行阻塞；
    /// - 派发前检查 `deadline` 与调用上限，超限立即停止并标记 partial；
    /// - 单次调用 5s 超时，失败降级为不补名（名称保持 #id）；
    /// - 命中持久化缓存的坐标不再调用。
    fn enrich_batch(&self, entities: &[RawEntity], deadline: Instant) -> BatchEnrichment {
        let mut names: Vec<Option<String>> = vec![None; entities.len()];
        let mut name_sources = vec![CandidateNameSource::Unnamed; entities.len()];
        let mut misses: Vec<(usize, String, f64, f64)> = Vec::new();
        for (index, entity) in entities.iter().enumerate() {
            let Some((lon, lat)) = entity.source_geometry.as_ref().and_then(polygon_centroid)
            else {
                continue;
            };
            let cache_key = format!("{lon:.5},{lat:.5}");
            match self.cache.get_regeo_name(&cache_key) {
                Ok(Some(Some(cached))) => {
                    names[index] = Some(cached);
                    name_sources[index] = CandidateNameSource::Cache;
                }
                Ok(Some(None)) => {
                    name_sources[index] = CandidateNameSource::Unnamed;
                }
                Ok(None) | Err(_) => misses.push((index, cache_key, lon, lat)),
            }
        }

        let Some(key) = (self.key_provider)() else {
            for (index, _, _, _) in &misses {
                name_sources[*index] = CandidateNameSource::Failed;
            }
            return BatchEnrichment {
                names,
                name_sources,
                partial: false,
                attempted: 0,
                key_missing: true,
                skipped_count: misses.len(),
            };
        };
        if misses.is_empty() {
            return BatchEnrichment {
                names,
                name_sources,
                partial: false,
                attempted: 0,
                key_missing: false,
                skipped_count: 0,
            };
        }

        let missed_total = misses.len();
        let next = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let limit_hit = Arc::new(AtomicBool::new(false));
        let results = Arc::new(Mutex::new(Vec::<(usize, LookupOutcome)>::new()));
        let workers = REGEO_CONCURRENCY.min(missed_total.max(1));
        let key_ref = &key;
        let misses_ref = &misses;
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let next = Arc::clone(&next);
                let calls = Arc::clone(&calls);
                let limit_hit = Arc::clone(&limit_hit);
                let results = Arc::clone(&results);
                scope.spawn(move || loop {
                    if Instant::now() >= deadline {
                        limit_hit.store(true, Ordering::SeqCst);
                        return;
                    }
                    let position = next.fetch_add(1, Ordering::SeqCst);
                    if position >= missed_total {
                        return;
                    }
                    if calls.fetch_add(1, Ordering::SeqCst) >= REGEO_MAX_CALLS_PER_RUN {
                        limit_hit.store(true, Ordering::SeqCst);
                        return;
                    }
                    let (index, cache_key, lon, lat) = &misses_ref[position];
                    let outcome = self.lookup_uncached_with_key(key_ref, cache_key, *lon, *lat);
                    results
                        .lock()
                        .expect("regeo batch results lock")
                        .push((*index, outcome));
                });
            }
        });

        let mut named = 0usize;
        let mut attempted = 0usize;
        let mut attempted_indices = std::collections::HashSet::new();
        {
            let mut results = results.lock().expect("regeo batch results lock");
            for (index, outcome) in results.drain(..) {
                attempted += 1;
                attempted_indices.insert(index);
                name_sources[index] = outcome.source;
                names[index] = outcome.name;
                if names[index].is_some() {
                    named += 1;
                }
            }
        }
        for (index, _, _, _) in &misses {
            if !attempted_indices.contains(index) {
                name_sources[*index] = CandidateNameSource::Failed;
            }
        }
        BatchEnrichment {
            names,
            name_sources,
            partial: limit_hit.load(Ordering::SeqCst) || attempted > named,
            attempted,
            key_missing: false,
            skipped_count: 0,
        }
    }
}

/// 面几何中心点（GCJ-02；简单平均，校区尺度足够）
pub fn polygon_centroid(geometry: &SourceGeometry) -> Option<(f64, f64)> {
    let SourceGeometry::Polygon(points) = geometry else {
        return None;
    };
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f64;
    let (sum_lon, sum_lat) = points.iter().fold((0.0, 0.0), |(lon, lat), point| {
        (lon + point.0, lat + point.1)
    });
    Some((sum_lon / n, sum_lat / n))
}

/// regeo 响应解析结果：名称是否为可接受建筑名 + 本次调用是否失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegeoNameOutcome {
    pub name: Option<String>,
    pub failed: bool,
}

/// 解析 regeo 响应：只把满足可接受名称规则的最近 POI 名称作为建筑名。
///
/// `formatted_address` 只能作为地址辅助信息，绝不回退为正式建筑名。
pub fn parse_regeo_name_outcome(json: &str) -> RegeoNameOutcome {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return RegeoNameOutcome {
            name: None,
            failed: true,
        };
    };
    if value.get("status").and_then(serde_json::Value::as_str) != Some("1") {
        return RegeoNameOutcome {
            name: None,
            failed: true,
        };
    }
    let Some(regeocode) = value.get("regeocode") else {
        return RegeoNameOutcome {
            name: None,
            failed: false,
        };
    };
    let formatted_address = regeocode
        .get("formatted_address")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    let name = regeocode
        .get("pois")
        .and_then(serde_json::Value::as_array)
        .and_then(|pois| pois.first())
        .and_then(|poi| poi.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .filter(|name| acceptable_poi_name(name, &formatted_address));
    RegeoNameOutcome {
        name,
        failed: false,
    }
}

/// 仅返回可接受的最近 POI 名称；无法接受时返回 `None`。
pub fn parse_regeo_name(json: &str) -> Option<String> {
    parse_regeo_name_outcome(json).name
}

/// 高德最近 POI 名称是否可作为建筑名（保守规则，避免地址/校名污染）。
///
/// 产品规则 #3/#4：名称非空、长度合理、不包含控制字符、不等于格式化地址、
/// 且不是“道路/行政区 + 门牌号”这类地址表达。
pub fn acceptable_poi_name(name: &str, formatted_address: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return false;
    }
    if name.chars().any(char::is_control) {
        return false;
    }
    let address = formatted_address.trim();
    if !address.is_empty() && name == address {
        return false;
    }
    !is_address_like(name)
}

fn is_address_like(name: &str) -> bool {
    const AREA_TOKENS: &[&str] = &[
        "路", "街", "道", "弄", "巷", "村", "镇", "乡", "县", "区", "市", "省",
    ];
    AREA_TOKENS.iter().any(|token| name.contains(token)) && name.ends_with('号')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn polygon() -> SourceGeometry {
        SourceGeometry::Polygon(vec![
            (121.42, 31.02),
            (121.44, 31.02),
            (121.44, 31.04),
            (121.42, 31.04),
            (121.42, 31.02),
        ])
    }

    #[test]
    fn parse_regeo_prefers_nearest_poi_name() {
        let json = r#"{"status":"1","info":"OK","regeocode":{"formatted_address":"上海市闵行区东川路800号","pois":[{"name":"第一教学楼"},{"name":"上海交通大学"}]}}"#;
        assert_eq!(parse_regeo_name(json).as_deref(), Some("第一教学楼"));
    }

    #[test]
    fn parse_regeo_does_not_fall_back_to_formatted_address() {
        let json = r#"{"status":"1","info":"OK","regeocode":{"formatted_address":"上海市闵行区东川路800号","pois":[]}}"#;
        let outcome = parse_regeo_name_outcome(json);
        assert_eq!(outcome.name, None, "formatted_address 不得伪装成建筑名");
        assert!(!outcome.failed, "成功响应但无可接受名称不算失败");
        assert_eq!(parse_regeo_name(json), None);
    }

    #[test]
    fn parse_regeo_rejects_failed_status() {
        assert_eq!(
            parse_regeo_name(r#"{"status":"0","info":"INVALID_USER_KEY"}"#),
            None
        );
        assert_eq!(parse_regeo_name("not json"), None);
        assert!(parse_regeo_name_outcome("not json").failed);
        assert!(
            parse_regeo_name_outcome(r#"{"status":"0","info":"CUQPS_HAS_EXCEEDED_THE_LIMIT"}"#)
                .failed
        );
    }

    #[test]
    fn acceptable_poi_name_rejects_address_like_and_control_characters() {
        assert!(acceptable_poi_name("第一教学楼", "上海市闵行区东川路800号"));
        assert!(!acceptable_poi_name(
            "东川路800号",
            "上海市闵行区东川路800号"
        ));
        assert!(!acceptable_poi_name(
            "上海市闵行区东川路800号",
            "上海市闵行区东川路800号"
        ));
        assert!(!acceptable_poi_name("", ""));
        assert!(!acceptable_poi_name("第一教学楼\u{0}", ""));
    }

    #[test]
    fn batch_enrichment_reports_missing_key_without_network() {
        use std::sync::Arc;
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);
        let transport = Box::new(move |_: &str, _: Duration| {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            Ok("unused".to_owned())
        });
        let namer = RegeoNamer::new(transport, Box::new(|| None));
        let entities = vec![
            building_entity("a", 121.41, 31.21),
            building_entity("b", 121.42, 31.22),
        ];
        let batch = namer.enrich_batch(&entities, Instant::now() + Duration::from_secs(60));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(batch.key_missing);
        assert_eq!(batch.skipped_count, 2);
        assert_eq!(batch.attempted, 0);
        assert!(!batch.partial);
        assert!(batch
            .name_sources
            .iter()
            .all(|source| *source == CandidateNameSource::Failed));
    }

    #[test]
    fn centroid_is_computed_for_polygon_only() {
        let (lon, lat) = polygon_centroid(&polygon()).unwrap();
        assert!((lon - 121.428).abs() < 1e-9);
        assert!((lat - 31.028).abs() < 1e-9);
        assert!(polygon_centroid(&SourceGeometry::Point((1.0, 2.0))).is_none());
        assert!(polygon_centroid(&SourceGeometry::LineString(vec![(1.0, 2.0)])).is_none());
    }

    #[test]
    fn cache_avoids_duplicate_calls_for_same_centroid() {
        use std::sync::Arc;
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        let transport = Box::new(move |_: &str, _: Duration| {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            Ok(
                r#"{"status":"1","info":"OK","regeocode":{"pois":[{"name":"第一教学楼"}]}}"#
                    .to_owned(),
            )
        });
        let namer = RegeoNamer::new(transport, Box::new(|| Some("web-key".to_owned())));
        let name = namer.name_for_geometry(&polygon()).unwrap();
        assert_eq!(name, "第一教学楼");
        let second = namer.name_for_geometry(&polygon()).unwrap();
        assert_eq!(second, "第一教学楼");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "同坐标只调一次 regeo");
    }

    #[test]
    fn missing_key_skips_network_and_returns_none() {
        use std::sync::Arc;
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        let transport = Box::new(move |_: &str, _: Duration| {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            Ok("unused".to_owned())
        });
        let namer = RegeoNamer::new(transport, Box::new(|| None));
        assert!(namer.name_for_geometry(&polygon()).is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "无 Key 不发请求");
    }

    #[test]
    fn point_geometry_is_not_named_via_regeo() {
        let transport = Box::new(|_: &str, _: Duration| Ok("unused".to_owned()));
        let namer = RegeoNamer::new(transport, Box::new(|| Some("key".to_owned())));
        assert!(namer
            .name_for_geometry(&SourceGeometry::Point((121.4, 31.2)))
            .is_none());
    }

    fn building_entity(id: &str, lon: f64, lat: f64) -> RawEntity {
        RawEntity::with_geometry(
            id,
            id.to_owned(),
            data_transformers::TagMap::new(),
            serde_json::json!({"id": id}),
            Some(SourceGeometry::Polygon(vec![
                (lon, lat),
                (lon + 0.001, lat),
                (lon + 0.001, lat + 0.001),
                (lon, lat),
            ])),
            "polygon",
        )
    }

    #[test]
    fn production_limits_are_bounded() {
        // 工单硬约束：单次 5s 超时、8 路并发、总体 60s 由 A1 传参、调用上限 256
        assert_eq!(REGEO_HTTP_TIMEOUT, Duration::from_secs(5));
        assert_eq!(REGEO_CONCURRENCY, 8);
        assert_eq!(REGEO_MAX_CALLS_PER_RUN, 256);
    }

    #[test]
    fn batch_enrichment_stops_at_deadline_and_marks_partial() {
        // 注入“慢/挂起 regeo transport”：每次调用睡满超时后失败。
        // 8 路并发下第一波 5s 后到达截止，超限立即结束并如实标注“部分建筑未命名”。
        let transport = Box::new(|_: &str, timeout: Duration| {
            // 无卡顿铁律禁用 std::thread::sleep；用 recv_timeout 等满超时
            let (_tx, rx) = std::sync::mpsc::channel::<()>();
            let _ = rx.recv_timeout(timeout);
            Err("模拟 regeo 超时".to_owned())
        });
        let namer = RegeoNamer::new(transport, Box::new(|| Some("web-key".to_owned())));
        let entities: Vec<RawEntity> = (0..20)
            .map(|i| building_entity(&format!("b{i}"), 121.40 + i as f64 * 0.01, 31.20))
            .collect();
        let started = Instant::now();
        let deadline = started + Duration::from_secs(6);
        let batch = namer.enrich_batch(&entities, deadline);
        let elapsed = started.elapsed();

        assert!(batch.partial, "截止前未补完必须标记“部分建筑未命名”");
        assert!(
            batch.attempted < entities.len(),
            "截止后不得继续派发：attempted={} total={}",
            batch.attempted,
            entities.len()
        );
        assert!(
            elapsed
                < deadline.duration_since(started) + REGEO_HTTP_TIMEOUT + Duration::from_secs(1),
            "含在飞波次的最坏耗时受单波 5s 上界约束：{elapsed:?}"
        );
        assert!(
            batch.names.iter().all(Option::is_none),
            "失败降级为不补名（名称保持 #id）"
        );
    }

    #[test]
    fn persistent_cache_avoids_repeated_calls_across_sessions() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let cache: Arc<dyn RegeoNameCacheApi> =
            Arc::new(data_persistence::RegeoNameCache::open_in_memory().unwrap());
        let entities: Vec<RawEntity> = vec![
            building_entity("a", 121.41, 31.21),
            building_entity("b", 121.42, 31.22),
        ];

        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&calls);
        let first = RegeoNamer {
            transport: Box::new(move |_: &str, _: Duration| {
                first_calls.fetch_add(1, Ordering::SeqCst);
                Ok(
                    r#"{"status":"1","info":"OK","regeocode":{"pois":[{"name":"教学楼"}]}}"#
                        .to_owned(),
                )
            }),
            key_provider: Box::new(|| Some("web-key".to_owned())),
            cache: Arc::clone(&cache),
        };
        let first_batch = first.enrich_batch(&entities, Instant::now() + Duration::from_secs(60));
        assert_eq!(first_batch.attempted, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(first_batch
            .name_sources
            .iter()
            .all(|source| *source == CandidateNameSource::Gaode));

        // 第二次“会话”共享同一持久化缓存：不再调用 regeo
        let second = RegeoNamer {
            transport: Box::new(|_: &str, _: Duration| {
                panic!("持久化缓存命中后不得再次调用 regeo");
            }),
            key_provider: Box::new(|| Some("web-key".to_owned())),
            cache: Arc::clone(&cache),
        };
        let second_batch = second.enrich_batch(&entities, Instant::now() + Duration::from_secs(60));
        assert_eq!(second_batch.attempted, 0, "重复采集不再重复调用");
        assert_eq!(second_batch.names[0].as_deref(), Some("教学楼"));
        assert_eq!(second_batch.names[1].as_deref(), Some("教学楼"));
        assert!(second_batch
            .name_sources
            .iter()
            .all(|source| *source == CandidateNameSource::Cache));
        assert!(!second_batch.partial);
    }

    #[test]
    fn cached_miss_does_not_call_network_again() {
        let cache: Arc<dyn RegeoNameCacheApi> =
            Arc::new(data_persistence::RegeoNameCache::open_in_memory().unwrap());
        let entities = vec![building_entity("a", 121.41, 31.21)];
        let first = RegeoNamer {
            transport: Box::new(|_: &str, _: Duration| {
                Ok(r#"{"status":"0","info":"NO_DATA"}"#.to_owned())
            }),
            key_provider: Box::new(|| Some("web-key".to_owned())),
            cache: Arc::clone(&cache),
        };
        let first_batch = first.enrich_batch(&entities, Instant::now() + Duration::from_secs(60));
        assert!(first_batch.names[0].is_none());

        let second = RegeoNamer {
            transport: Box::new(|_: &str, _: Duration| {
                panic!("已查无名称也必须缓存，不得再次调用");
            }),
            key_provider: Box::new(|| Some("web-key".to_owned())),
            cache: Arc::clone(&cache),
        };
        let second_batch = second.enrich_batch(&entities, Instant::now() + Duration::from_secs(60));
        assert_eq!(second_batch.attempted, 0);
        assert!(second_batch.names[0].is_none());
    }
}
