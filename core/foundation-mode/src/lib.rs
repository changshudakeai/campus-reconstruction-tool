//! B5 地基模式 —— 边界绘制与朝向设定引擎
//!
//! **核心职责**（T14 / ADR-0012）：在高德地图上圈画方案边界、可选地画两点参考线
//! 设定朝向；未设置朝向时由完整导出用例采用地图正北；修改朝向触发重算并明确告知影响范围。
//!
//! ## 模块组织
//!
//! - [boundary_ui]：边界绘制 UI 状态机（多点触控/鼠标拖拽事件驱动）
//! - [orientation]：朝向两点参考线与角度计算（正北夹角，0~360°校验）
//! - [coordinate]：坐标系转换子模块（高德 Mercator → 平面米单位 → MC 块坐标）
//! - [validation]：边界有效性验证（闭合检查 + 面积计算）
//! - [warning]：朝向修改警告弹窗逻辑（告知重算影响范围）
//!
//! ## 窗口契约（缝 7）
//!
//! F3 向 B5 要朝向计算；坐标系转换是 B5 内部子模块，不独立成 crate。
//! Slint 渲染在壳层：指针事件转成 [BoundaryUiEvent] 送入 [BoundaryDrawer]，
//! 状态与顶点表由壳层绑定回 Slint 属性（缝 1：壳零业务逻辑）。
//!
//! ## 依赖纪律（ADR-0017）
//!
//! 只依赖 B1 shared-domain-types：[Boundary]/[Orientation] 直接复用 T02 类型
//! 定义。不碰 B2 data-persistence（存储归 T11），不依赖 B3/B6 等基础层横
//! 向模块（基础层横向零依赖，xtask arch 强制）——T24 OSM 候选排序因操作
//! B3 自有 `OsmElement` 类型而归 B3 gaode-client（ADR-0029 允许 B3/B5 任一）。
//! 错误文案暂为中文硬编码，待壳层经 B6 解析文本键。

pub mod boundary_export;
pub mod boundary_ui;
pub mod coordinate;
pub mod orientation;
pub mod validation;
pub mod warning;

// 重新导出公共类型，方便 crate 外使用
pub use boundary_export::{
    boundary_footprint, boundary_footprint_with_orientation, BoundaryFootprint,
    BoundaryFootprintError,
};
pub use boundary_ui::{BoundaryDrawer, BoundaryState, BoundaryUiEvent, EventResult, Vertex};
pub use coordinate::{CoordinateConverter, McBlockCoord, MercatorCoord, PlaneMileUnit};
pub use orientation::{OrientationCalculator, OrientationLine, Point2D};
pub use validation::{
    detect_self_intersection, validate_polygon_closure, BoundaryValidationError, ValidationResult,
};
pub use warning::{
    check_orientation_change_impact, format_impact_details, should_show_confirmation_dialog,
    ImpactItem, OrientationImpactReport,
};

// B1 共享类型透传：B5 的调用方（F3）拿到的边界/朝向就是全工程统一词汇
pub use shared_domain_types::{Boundary, Orientation};
