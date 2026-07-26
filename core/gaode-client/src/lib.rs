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
//! - [map_page](crate::map_page)：`build_map_page_html`
//!   —— WebView 地图页（官方 CDN v1.2，地图最小高度 300px）
//!
//! ## 架构边界（ADR-0017）
//!
//! B3 是基础模块：内部依赖仅 B1（共享领域类型），不依赖任何功能模块与壳。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod error;
mod map_page;
mod poi;
mod record;
mod search_flow;

pub use error::{Error, Result};
pub use map_page::{build_map_page_html, MapPageConfig, GAODE_CDN_URL_TEMPLATE, MAP_MIN_HEIGHT_PX};
pub use poi::{parse_place_search_response, SchoolPoi, SCHOOL_TYPECODE_PREFIX};
pub use record::CampusPoiRecord;
pub use search_flow::{CampusSearchFlow, ConfirmedCampus, SearchFlowState};
