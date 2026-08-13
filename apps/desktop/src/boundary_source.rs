//! 方案边界外部来源适配器。
//!
//! 这里仅做构造期端口适配：生产环境连接 OSM，测试注入 fake；获取状态、缓存、
//! 刷新和陈旧结果规则全部由 F3 `PlanBoundarySession` 持有。

use std::sync::Arc;

use data_acquisition::overpass::{CampusBoundaryFetcher, CampusBoundaryResult};
use project_management::{BoundaryFetchOutcome, BoundarySource};

/// 方案边界外部数据源 seam。生产环境直连 OSM；桌面集成测试注入计数 fake。
pub type BoundaryFetchSource = Arc<dyn Fn(&str, f64, f64) -> CampusBoundaryResult + Send + Sync>;

pub(crate) fn production_boundary_source() -> BoundaryFetchSource {
    Arc::new(|campus_name, anchor_lon, anchor_lat| {
        CampusBoundaryFetcher::production().fetch_campus(campus_name, anchor_lon, anchor_lat)
    })
}

pub(crate) fn boundary_session_source(source: BoundaryFetchSource) -> BoundarySource {
    Arc::new(move |campus_name, anchor_lon, anchor_lat| {
        match source(campus_name, anchor_lon, anchor_lat) {
            CampusBoundaryResult::AutoSelected {
                name,
                gcj02,
                source,
                candidate_count,
            } => BoundaryFetchOutcome::AutoSelected {
                name,
                gcj02,
                source: source.to_string(),
                candidate_count,
            },
            CampusBoundaryResult::NotFound => BoundaryFetchOutcome::NotFound,
            CampusBoundaryResult::Unreachable { message } => {
                BoundaryFetchOutcome::Unreachable { message }
            }
        }
    })
}
