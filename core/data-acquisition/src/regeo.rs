//! 高德逆地理编码（regeo）补名（T31 命名第二级）。
//!
//! 命名两级：优先 OSM `name` 标签（零成本、与几何同源）；无名字的关键建筑
//! （教学楼/图书馆/宿舍等）用高德 regeo 反查最近 POI/地址补名并**缓存**。
//!
//! - regeo 需要独立 Web 服务 Key（与 JS API Key 不同；ADR-0004 设置页
//!   “高德 Web服务 Key（开发人员使用）”）；
//! - 配额：个人 5000 次/日（调研 §3.2），一所学校几百栋建筑够用；
//! - 缓存按坐标键（GCJ-02，5 位小数 ≈ 1 米）存放本次会话结果，避免重复调用；
//! - 未配置 Key / 调用失败时返回 `None`，名称保持“未命名建筑 #id”，不阻塞导出。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::source::SourceGeometry;

/// 高德 Web 服务 regeo 端点
pub const REGEO_ENDPOINT: &str = "https://restapi.amap.com/v3/geocode/regeo";

/// regeo HTTP 传输（生产为 ureq；测试注入罐头）
pub type RegeoTransport =
    Box<dyn Fn(&str, Duration) -> std::result::Result<String, String> + Send + Sync>;

/// Web 服务 Key 提供器（只经设置页录入；生产按数据库路径实时读取）
pub type KeyProvider = Box<dyn Fn() -> Option<String> + Send + Sync>;

/// regeo 补名器：面几何中心点反查 + 会话级缓存
pub struct RegeoNamer {
    transport: RegeoTransport,
    key_provider: KeyProvider,
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl RegeoNamer {
    pub fn new(transport: RegeoTransport, key_provider: KeyProvider) -> Self {
        Self {
            transport,
            key_provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 生产：ureq 直连 + 设置页 Key 提供器
    pub fn production(key_provider: KeyProvider) -> Self {
        Self::new(
            Box::new(|url: &str, timeout: Duration| {
                let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
                let agent = ureq::AgentBuilder::new()
                    .timeout(timeout)
                    .tls_connector(Arc::new(tls))
                    .build();
                let response = agent.get(url).call().map_err(|error| error.to_string())?;
                response.into_string().map_err(|error| error.to_string())
            }),
            key_provider,
        )
    }

    /// 为面几何补名：仅 Polygon（点位只作证据，不做 regeo 扩面/补名主体）。
    /// 返回 `None` 表示无 Key、无结果或几何不是面。
    pub fn name_for_geometry(&self, geometry: &SourceGeometry) -> Option<String> {
        let (lon, lat) = polygon_centroid(geometry)?;
        let cache_key = format!("{lon:.5},{lat:.5}");
        if let Some(cached) = self.cache.lock().ok()?.get(&cache_key).cloned() {
            return cached;
        }
        let key = (self.key_provider)()?;
        let url =
            format!("{REGEO_ENDPOINT}?key={key}&location={lon},{lat}&radius=200&extensions=base");
        let body = (self.transport)(&url, Duration::from_secs(10)).ok()?;
        let name = parse_regeo_name(&body);
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(cache_key, name.clone());
        }
        name
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

/// 解析 regeo 响应：优先最近 POI 名称，其次格式化地址。
pub fn parse_regeo_name(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.get("status").and_then(serde_json::Value::as_str) != Some("1") {
        return None;
    }
    let regeocode = value.get("regeocode")?;
    let poi_name = regeocode
        .get("pois")
        .and_then(serde_json::Value::as_array)
        .and_then(|pois| pois.first())
        .and_then(|poi| poi.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .filter(|name| !name.is_empty());
    poi_name.or_else(|| {
        regeocode
            .get("formatted_address")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .filter(|address| !address.is_empty())
    })
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
    fn parse_regeo_falls_back_to_formatted_address() {
        let json = r#"{"status":"1","info":"OK","regeocode":{"formatted_address":"上海市闵行区东川路800号","pois":[]}}"#;
        assert_eq!(
            parse_regeo_name(json).as_deref(),
            Some("上海市闵行区东川路800号")
        );
    }

    #[test]
    fn parse_regeo_rejects_failed_status() {
        assert_eq!(
            parse_regeo_name(r#"{"status":"0","info":"INVALID_USER_KEY"}"#),
            None
        );
        assert_eq!(parse_regeo_name("not json"), None);
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
}
