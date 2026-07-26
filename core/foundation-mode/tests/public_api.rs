//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use foundation_mode::{
    check_orientation_change_impact, format_impact_details, should_show_confirmation_dialog,
    validate_polygon_closure, Boundary, BoundaryDrawer, BoundaryState, BoundaryUiEvent,
    BoundaryValidationError, CoordinateConverter, EventResult, MercatorCoord, Orientation,
    OrientationCalculator, OrientationLine, PlaneMileUnit, Point2D, Vertex,
};
use shared_domain_types::CandidateCategory;
use std::collections::HashMap;

#[test]
fn public_api_types_exist() {
    // MercatorCoord / PlaneMileUnit / McBlockCoord：三级坐标链
    let mut converter = CoordinateConverter::default();
    assert_eq!(converter.blocks_per_meter(), 1.0);
    converter.set_center(MercatorCoord::from_lat_lon(39.9042, 116.4074));
    converter.set_blocks_per_meter(1.0);
    let (plane, mc) = converter
        .convert(MercatorCoord::from_lat_lon(39.9042, 116.4084))
        .expect("已设中心点，转换必有值");
    assert!(plane.distance_to(&PlaneMileUnit::new(0.0, 0.0)) > 0.0);
    assert!(mc.block_x > 0);

    // OrientationLine + OrientationCalculator：产出 B1 Orientation（T02 复用）
    let line = OrientationLine::new(Point2D::new(0.0, 0.0), Point2D::new(100.0, 0.0))
        .expect("非重合两点");
    assert_eq!(line.length(), 100.0);
    let orientation: Orientation = OrientationCalculator::calculate(&line).expect("正东 90°");
    assert_eq!(orientation.degree(), 90.0);
    assert_eq!(
        OrientationCalculator::normalize_angle(-90.0).unwrap().degree(),
        270.0
    );

    // BoundaryDrawer：事件驱动的绘制状态机
    let mut drawer = BoundaryDrawer::new();
    assert_eq!(drawer.state(), BoundaryState::Idle);
    assert_eq!(
        drawer.handle_event(BoundaryUiEvent::ClickAt { x: 0.0, y: 0.0 }),
        EventResult::Accepted
    );
    drawer.handle_event(BoundaryUiEvent::ClickAt { x: 50.0, y: 0.0 });
    drawer.handle_event(BoundaryUiEvent::ClickAt { x: 25.0, y: 40.0 });
    assert_eq!(drawer.handle_event(BoundaryUiEvent::Confirm), EventResult::Accepted);
    assert_eq!(drawer.state(), BoundaryState::Determined);
    assert_eq!(drawer.vertices().len(), 3);

    // validate_polygon_closure：闭合检查 + 面积
    let result = validate_polygon_closure(drawer.vertices());
    assert!(result.is_valid);
    assert!(result.area.unwrap() > 100.0);
    let too_few = validate_polygon_closure(&[Vertex::new(0.0, 0.0)]);
    assert!(matches!(
        too_few.errors[0],
        BoundaryValidationError::InsufficientVertices(1)
    ));

    // 朝向修改影响报告（类别沿用 B1 CandidateCategory）
    let mut existing = HashMap::new();
    existing.insert(CandidateCategory::Building, 15);
    let report = check_orientation_change_impact(
        &existing,
        Some(Orientation::new(0.0).unwrap()),
        Orientation::new(90.0).unwrap(),
    );
    assert!(!report.title.is_empty());
    assert!(should_show_confirmation_dialog(&report));
    assert!(format_impact_details(&report).contains("建筑"));

    // B1 共享类型透传
    let boundary = Boundary::empty();
    assert!(boundary.is_empty());
}
