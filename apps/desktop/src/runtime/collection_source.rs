//! 生产候选数据源装配（F4 `DataSource` 适配器，T31/T36）。
//!
//! 组合根创建数据源适配器并注入 A1（ADR-0037 构造期接线）；共享 Overpass
//! 客户端使会话内端点自适应排序跨边界/候选查询生效（B 工单 B.12）。

use std::sync::Arc;

use data_acquisition::overpass::{boundary_bbox, OverpassClient};
use data_acquisition::{
    CollectionStage, DeadlineOverpassTransport, NameEnricher, OverpassDataSource, RegeoNamer,
};
use data_persistence::{AppSettingKey, AppSettingsApi, Database};
use shared_domain_types::Boundary;

use super::DEV_DB_FILE;

/// 生产候选数据源：F4 `DataSource` 适配器（OSM/Overpass，T31）。
///
/// - Overpass 传输走 **Rust 侧直连**（四个六类分片顺序执行、节点健康排序、
///   有限退避重试，失败返回结构化 `SourceUnreachable`），不再依赖 WebView fetch/CORS；
/// - selector 由 B13 集中分类规则生成，不复制六类标签表；
/// - 命名两级：OSM `name` 优先；缺名关键建筑由 regeo 补名并缓存
///   （Web 服务 Key 只经设置页录入，这里按数据库路径实时读取）。
pub(super) fn production_collection_source(
    overpass: Arc<OverpassClient>,
) -> Arc<dyn data_acquisition::DataSource + Send + Sync> {
    let transport: DeadlineOverpassTransport =
        Box::new(move |boundary: &Boundary, deadline, on_stage| {
            // 边界为 GCJ-02；工单禁止 GCJ→WGS 反向，查询窗口用“边界包围盒 +
            // ~1km 外扩余量”覆盖 GCJ 偏移（见 data-acquisition::overpass::boundary_bbox）。
            let bbox = boundary_bbox(boundary, 0.01)
                .ok_or_else(|| "边界坐标无法计算查询包围盒".to_owned())?;
            overpass
                .query_campus_objects(bbox, deadline, &|| {
                    on_stage(CollectionStage::RetryingSource)
                })
                .map_err(|message| format!("Overpass 采集查询失败：{message}"))
        });
    // regeo Web 服务 Key 提供器：只经设置页录入（B2 app_settings），实时读取。
    let key_provider: data_acquisition::regeo::KeyProvider = Box::new(move || {
        Database::open(DEV_DB_FILE)
            .ok()
            .and_then(|db| {
                AppSettingsApi::get_setting(&db, AppSettingKey::GaodeWebServiceKey)
                    .ok()
                    .flatten()
            })
            .filter(|key| !key.is_empty())
    });
    // T36：补名缓存从“会话级”改为“持久化 SQLite”（同一开发库文件）。
    // 打开失败只降级为内存缓存并告警，不阻断采集（缓存非正式业务数据）。
    let cache: Arc<dyn data_persistence::RegeoNameCacheApi> =
        match data_persistence::RegeoNameCache::open(DEV_DB_FILE) {
            Ok(cache) => Arc::new(cache),
            Err(error) => {
                log::warn!("regeo 持久化缓存打开失败，本次会话降级为内存缓存：{error}");
                Arc::new(
                    data_persistence::RegeoNameCache::open_in_memory()
                        .expect("内存 regeo 缓存必须可用"),
                )
            }
        };
    let namer = Arc::new(RegeoNamer::production(key_provider, cache));
    let enricher: Option<Arc<dyn NameEnricher>> = Some(namer);
    Arc::new(OverpassDataSource::with_deadline_transport(transport).with_name_enricher(enricher))
}
