//! B3 高德地图客户端
//!
//! 高德 Web API 封装（缝 7：F3/F4 向 B3 要高德 API 封装、坐标拾取、边界绘制）。
//! 本 crate 是纯逻辑层：不发网络请求、不渲染 UI——实际的地图渲染与 POI 搜索
//! 由壳层 WebView 加载本 crate 生成的地图页（官方 CDN JS SDK）完成，
//! JS 侧结果经桥接回传后由本 crate 解析、筛选、驱动确认流程。
//!
//! ## 模块组织
//!
//! - [poi](crate::poi)：`SchoolPoi` + `parse_place_search_response`
//!   —— 高德地点搜索响应解析、教育类筛选、同名去重（ADR-0008 第 2 条）
//! - [search_flow](crate::search_flow)：`CampusSearchFlow`
//!   —— 候选列表 → 详情 → **显式确认**状态机（不自动进入校区）
//! - [record](crate::record)：`CampusPoiRecord`
//!   —— 校区 POI 持久化载荷（POI identity + coordinate lineage）
//! - [map_page](crate::map_page)：`build_map_page_html` + `build_pick_point_page_html`
//!   —— WebView 地图页（官方 CDN v2.0，安全密钥，地图最小高度 300px）
//! - [boundary_edit_map_page](crate::boundary_edit_map_page)：`build_boundary_edit_page_html`
//!   —— T24 边界编辑地图页（Overpass 查询 + PolygonEditor 编辑 + 人工圈画兼底）
//! - [review_map_page](crate::review_map_page)：`build_review_map_page_html`
//!   —— T38 评审地图页（候选三态标注：待定虚线/保留实线/剔除隐藏 + 定位跳转 +
//!   地图↔卡片双向高亮）
//! - [boundary_sorting](crate::boundary_sorting)：`BoundarySorter`
//!   —— T24 OSM 边界候选排序（包含锚点 → 名称匹配 → 距离最近，纯函数可单测）
//!
//! ## 架构边界（ADR-0017）
//!
//! B3 是基础模块：内部依赖仅 B1（共享领域类型），不依赖任何功能模块与壳。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod boundary_edit_map_page;
mod boundary_sorting;
mod coords;
mod error;
mod map_page;
mod map_viewport;
mod poi;
mod record;
mod review_map_page;
mod search_flow;

pub use boundary_edit_map_page::{
    build_boundary_edit_page_html, BoundaryEditPageConfig,
    GAODE_CDN_URL_TEMPLATE as BOUNDARY_CDN_TEMPLATE,
};
pub use boundary_sorting::{BoundaryCandidateScore, BoundarySorter};
pub use coords::{convert_coords_wgs84_to_gcj02, convert_pairs_wgs84_to_gcj02, wgs84_to_gcj02};
pub use error::{Error, Result};
pub use map_page::{
    build_map_page_html, build_pick_point_page_html, MapPageConfig, GAODE_CDN_URL_TEMPLATE,
    MAP_MIN_HEIGHT_PX,
};
pub use map_viewport::MapViewport;
pub use poi::{
    parse_ipc_message, parse_location_value, parse_place_search_response, IpcMessage, OsmElement,
    OsmMember, SchoolPoi, SCHOOL_TYPECODE_PREFIX,
};
pub use record::CampusPoiRecord;
pub use review_map_page::{
    build_review_map_page_html, ReviewMapPageConfig, REVIEW_MAP_CDN_URL_TEMPLATE,
};
pub use search_flow::{CampusSearchFlow, ConfirmedCampus, SearchFlowState};
