//! 方案边界外部来源适配器。
//!
//! 这里仅做构造期端口适配：生产环境连接 OSM，测试注入 fake；获取状态、缓存、
//! 刷新和陈旧结果规则全部由 F3 `PlanBoundarySession` 持有。

use std::sync::Arc;

use data_acquisition::overpass::{
    CampusBoundaryFetcher, CampusBoundaryResult, FetchProgress, FetchStage, OverpassClient,
};
use project_management::{
    BoundaryFetchOutcome, BoundaryFetchProgress, BoundaryFetchStage, BoundaryProgressSink,
    BoundarySource,
};

/// 方案边界外部数据源 seam。生产环境直连 OSM；桌面集成测试注入计数 fake。
///
/// 第四个参数是获取阶段进度回调（后台线程调用；S1 只显示阶段与已耗时）。
pub type BoundaryFetchSource =
    Arc<dyn Fn(&str, f64, f64, &dyn Fn(FetchProgress)) -> CampusBoundaryResult + Send + Sync>;

/// 生产边界源：共享 Overpass 客户端（会话内端点自适应排序跨查询生效）。
pub(crate) fn production_boundary_source(overpass: Arc<OverpassClient>) -> BoundaryFetchSource {
    let fetcher = Arc::new(CampusBoundaryFetcher::production_with_overpass(overpass));
    Arc::new(move |campus_name, anchor_lon, anchor_lat, on_progress| {
        fetcher.fetch_campus_with_progress(campus_name, anchor_lon, anchor_lat, on_progress)
    })
}

pub(crate) fn boundary_session_source(source: BoundaryFetchSource) -> BoundarySource {
    Arc::new(
        move |campus_name, anchor_lon, anchor_lat, sink: BoundaryProgressSink| {
            let mapped_sink = |progress: FetchProgress| {
                sink(BoundaryFetchProgress {
                    stage: match progress.stage {
                        FetchStage::CampusName => BoundaryFetchStage::CampusName,
                        FetchStage::ByElementId => BoundaryFetchStage::ByElementId,
                        FetchStage::Amenity => BoundaryFetchStage::Amenity,
                        FetchStage::Landuse => BoundaryFetchStage::Landuse,
                    },
                    attempt: progress.attempt,
                    total_attempts: progress.total_attempts,
                    elapsed_secs: progress.elapsed_secs,
                });
            };
            match source(campus_name, anchor_lon, anchor_lat, &mapped_sink) {
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
        },
    )
}
