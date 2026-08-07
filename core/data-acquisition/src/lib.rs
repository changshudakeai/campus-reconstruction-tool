//! F4 数据采集流水线：可插拔数据源 + 增量刷新检测 + 原始观测落库。
//!
//! 窗口契约缝 3：F4 向数据源适配器要原始对象 → 交 B13 归类 → 把带完整
//! 原始标签的观测数据**当场写入** B2 原始观测表（数据粮仓，永不删除）。
//! 采集非强制（ADR-0012）：本 crate 只负责"采"，评审归 F5，互不相识。
//!
//! ## 模块组织
//!
//! - [source](crate::source)：`DataSource` 适配器接口（ADR-0013 可插拔）
//!   与默认实现 `GaodeDataSource`（复用 B3 高德客户端解析）
//! - [pipeline](crate::pipeline)：`AcquisitionPipeline` 采集流水线
//!   （边界 → 原始对象 → 归类 → 落库，一次调用走完缝 3）
//! - [refresh](crate::refresh)：`RefreshDiff` 增量刷新检测
//!   （对比上次采集 digest，展示 新增/更新/未变）
//! - [progress](crate::progress)：`CollectionProgressView` 进度反馈 UI 占位
//!   （纯数据视图 + B6 文本键，UI 层解析渲染）
//! - [error](crate::error)：带类型错误
//! - [overpass](crate::overpass)：OSM/Overpass/Nominatim Rust 侧直连
//!   （T31：端点回退、union 查询、Nominatim 校名解析、校区边界自动获取级联）
//! - [regeo](crate::regeo)：高德逆地理编码补名（T31：OSM name 优先后的第二级）
//!
//! ## 依赖边界（ADR-0017）
//!
//! F4 依赖 B1/B2/B3/B13；归类逻辑完全复用 B13 `ClassifyEngine`（不重定义）；
//! 落库完全走 B2 `RawObservationsApi`（本 crate 不触碰 SQL）；
//! 禁止依赖其他 F* 功能模块（评审台 F5 尤其）。

#![cfg_attr(not(test), warn(unreachable_pub))]

pub mod error;
pub mod overpass;
pub mod pipeline;
pub mod progress;
pub mod refresh;
pub mod regeo;
pub mod source;

// 重新导出公共类型，方便 crate 外使用
pub use error::{AcquisitionError, Result};
pub use pipeline::{AcquisitionBatch, AcquisitionPipeline, CandidateDraft, CollectionReport};
pub use progress::{
    category_text_key, text_keys, CategoryProgress, CollectionProgressView, ALL_CATEGORIES,
};
pub use refresh::{DiffEntry, DiffKind, RefreshDiff};
pub use regeo::{parse_regeo_name, polygon_centroid, RegeoNamer};
pub use source::{
    BridgeTransport, DataSource, GaodeDataSource, NameEnricher, OverpassDataSource,
    OverpassTransport, RawEntity, SourceGeometry,
};
