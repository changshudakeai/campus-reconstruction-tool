//! T14 验收测试：四点边界面积计算 + 朝向角度范围校验 + 坐标链一致性

use foundation_mode::{
    check_orientation_change_impact, validate_polygon_closure, CoordinateConverter, MercatorCoord,
    Orientation, OrientationCalculator, OrientationLine, PlaneMileUnit, Point2D, Vertex,
};
use shared_domain_types::CandidateCategory;
use std::collections::HashMap;

// ==================== 验收点 1：四点边界 → 面积计算正确 ====================

#[test]
fn four_point_boundary_area_is_exact() {
    // 10×10 正方形边界 → 100 平方米
    let vertices = vec![
        Vertex::new(0.0, 0.0),
        Vertex::new(10.0, 0.0),
        Vertex::new(10.0, 10.0),
        Vertex::new(0.0, 10.0),
    ];

    let result = validate_polygon_closure(&vertices);

    assert!(result.is_valid);
    assert_eq!(result.area, Some(100.0));
}

#[test]
fn four_point_irregular_quadrilateral_area() {
    // 不规则四边形（梯形）：上底 10 + 下底 20，高 10 → 面积 150
    let vertices = vec![
        Vertex::new(0.0, 0.0),
        Vertex::new(20.0, 0.0),
        Vertex::new(15.0, 10.0),
        Vertex::new(5.0, 10.0),
    ];

    let result = validate_polygon_closure(&vertices);

    assert!(result.is_valid);
    assert!((result.area.unwrap() - 150.0).abs() < 1e-9);
}

#[test]
fn campus_scale_boundary_area() {
    // 校园尺度：400×300 米矩形 → 12 万平方米
    let vertices = vec![
        Vertex::new(0.0, 0.0),
        Vertex::new(400.0, 0.0),
        Vertex::new(400.0, 300.0),
        Vertex::new(0.0, 300.0),
    ];

    let result = validate_polygon_closure(&vertices);

    assert!(result.is_valid);
    assert_eq!(result.area, Some(120_000.0));
}

// ==================== 验收点 2：朝向范围校验（-90° / 400°）====================

#[test]
fn minus_90_degrees_is_rejected_by_shared_type() {
    // B1 Orientation 直接拒绝越界角度——不允许 -90° 溜进领域模型
    assert!(Orientation::new(-90.0).is_none());
}

#[test]
fn plus_400_degrees_is_rejected_by_shared_type() {
    assert!(Orientation::new(400.0).is_none());
}

#[test]
fn minus_90_degrees_can_be_corrected_to_270() {
    // 经 B5 修正入口归一化后合法
    let corrected = OrientationCalculator::normalize_angle(-90.0).expect("可修正");
    assert_eq!(corrected.degree(), 270.0);
}

#[test]
fn plus_400_degrees_can_be_corrected_to_40() {
    let corrected = OrientationCalculator::normalize_angle(400.0).expect("可修正");
    assert_eq!(corrected.degree(), 40.0);
}

#[test]
fn calculated_orientation_is_always_in_range() {
    // 参考线扫一整圈，计算出的朝向永远落在 [0, 360)
    for i in 0..36 {
        let theta = f64::from(i) * 10.0_f64.to_radians();
        let line = OrientationLine::new(
            Point2D::new(0.0, 0.0),
            Point2D::new(theta.sin() * 100.0, theta.cos() * 100.0),
        )
        .expect("半径 100 的非重合点");

        let orientation = OrientationCalculator::calculate(&line).expect("范围内角度");
        assert!((0.0..360.0).contains(&orientation.degree()));
    }
}

// ==================== 坐标链：高德 Mercator → 平面米 → MC 块 ====================

#[test]
fn full_conversion_chain_produces_plausible_blocks() {
    let mut converter = CoordinateConverter::default();
    converter.set_center(MercatorCoord::from_lat_lon(39.9042, 116.4074));

    // 校园东北角（约东 85 米、北 111 米）→ (x=East, y=North)
    let corner = MercatorCoord::from_lat_lon(39.9052, 116.4084);
    let (plane, mc) = converter.convert(corner).expect("已设中心");

    assert!((plane.x - 85.4).abs() < 20.0, "东向 {} 米", plane.x);
    assert!((plane.y - 111.0).abs() < 20.0, "北向 {} 米", plane.y);
    // MC 轴向：东 → X 正，北 → Z 负
    assert!(mc.block_x > 0);
    assert!(mc.block_z < 0);
}

#[test]
fn scale_ratio_shrinks_block_counts() {
    let mut converter = CoordinateConverter::default();
    converter.set_blocks_per_meter(0.5); // 2 米 = 1 块

    let mc = converter.plane_to_mc(&PlaneMileUnit::new(100.0, 60.0)); // 东 100 米，北 60 米
    assert_eq!(mc.block_x, 50); // x=East → MC.X
    assert_eq!(mc.block_z, -30); // y=North → MC.Z 负
}

// ==================== 朝向修改 → 重算影响告知（ADR-0012）====================

#[test]
fn changing_orientation_reports_affected_buildings() {
    // 负责人验收点：改朝向时告诉用户"这会重算你之前画的 XX 栋楼"
    let mut existing = HashMap::new();
    existing.insert(CandidateCategory::Building, 12);

    let report = check_orientation_change_impact(
        &existing,
        Some(Orientation::new(0.0).unwrap()),
        Orientation::new(90.0).unwrap(),
    );

    assert_eq!(report.items.len(), 1);
    assert_eq!(report.items[0].count, 12);
    assert!(report.items[0].requires_confirmation);
    assert!(report.title.contains("重算"));
}
