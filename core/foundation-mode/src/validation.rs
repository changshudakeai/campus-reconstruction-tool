//! 边界有效性验证子模块
//!
//! **核心功能**：验证绘制边界的几何正确性——
//! - 多边形闭合检查（至少 3 个点；GeoJSON 环自动首尾闭合）
//! - 面积计算（鞋带公式，经 geo crate；用于提示用户绘制范围大小）
//!
//! 自相交等拓扑检查属 B14 geometry-validator 职责（ADR-0017 依赖 DAG
//! 禁止 B5→B14 横向依赖），由上层功能模块在两者之间编排。

use crate::boundary_ui::Vertex;
use geo::{Area, Coord, LineString, Polygon};

/// 最小有效面积（平方米）：小于此值的"边界"多半是误触
const MIN_VALID_AREA_M2: f64 = 100.0;

/// 边界验证错误类型
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum BoundaryValidationError {
    /// 顶点数不足，无法闭合成多边形
    #[error("顶点数不足：需要至少 3 个，实际 {0} 个")]
    InsufficientVertices(usize),

    /// 面积过小（共线/误触产生的退化多边形也落在这里）
    #[error("边界面积过小：{area:.1} 平方米，至少需要 {min:.0} 平方米")]
    AreaTooSmall {
        /// 实际面积（平方米）
        area: f64,
        /// 最小要求（平方米）
        min: f64,
    },
}

/// 验证结果
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationResult {
    /// 是否有效
    pub is_valid: bool,
    /// 如果无效，错误原因列表
    pub errors: Vec<BoundaryValidationError>,
    /// 多边形面积（平方米；顶点不足时为 None）
    pub area: Option<f64>,
}

/// 验证边界多边形的几何正确性
///
/// 参数 `vertices` 为按绘制顺序排列的顶点（顺/逆时针均可，未闭合会自动闭合）。
pub fn validate_polygon_closure(vertices: &[Vertex]) -> ValidationResult {
    // 检查 1：至少 3 个点才能形成闭合多边形
    if vertices.len() < 3 {
        return ValidationResult {
            is_valid: false,
            errors: vec![BoundaryValidationError::InsufficientVertices(
                vertices.len(),
            )],
            area: None,
        };
    }

    // 构建 geo Polygon（LineString::from 自动处理坐标序列；环需首尾闭合）
    let mut ring: Vec<Coord<f64>> = vertices.iter().map(|v| Coord { x: v.x, y: v.y }).collect();
    if ring.first() != ring.last() {
        ring.push(ring[0]);
    }
    let polygon = Polygon::new(LineString::from(ring), vec![]);

    // 检查 2：面积（无符号鞋带公式；退化多边形面积为 0）
    let area = polygon.unsigned_area();
    let mut errors = Vec::new();
    if area < MIN_VALID_AREA_M2 {
        errors.push(BoundaryValidationError::AreaTooSmall {
            area,
            min: MIN_VALID_AREA_M2,
        });
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        area: Some(area),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_boundary_is_valid_with_exact_area() {
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(10.0, 0.0),
            Vertex::new(10.0, 10.0),
            Vertex::new(0.0, 10.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(result.is_valid);
        assert_eq!(result.area, Some(100.0));
        assert!(result.errors.is_empty());
    }

    #[test]
    fn two_points_are_insufficient() {
        let vertices = vec![Vertex::new(0.0, 0.0), Vertex::new(10.0, 0.0)];

        let result = validate_polygon_closure(&vertices);

        assert!(!result.is_valid);
        assert_eq!(
            result.errors[0],
            BoundaryValidationError::InsufficientVertices(2)
        );
        assert!(result.area.is_none());
    }

    #[test]
    fn triangle_area_uses_shoelace_formula() {
        // 三角形面积 = 底×高/2 = 20×30/2 = 300
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(10.0, 30.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(result.is_valid);
        assert!((result.area.unwrap() - 300.0).abs() < 1e-9);
    }

    #[test]
    fn tiny_polygon_fails_area_threshold() {
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(1.0, 0.0),
            Vertex::new(0.0, 1.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(!result.is_valid);
        assert!(matches!(
            result.errors[0],
            BoundaryValidationError::AreaTooSmall { min, .. } if min == MIN_VALID_AREA_M2
        ));
    }

    #[test]
    fn collinear_points_degenerate_to_zero_area() {
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(5.0, 5.0),
            Vertex::new(10.0, 10.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(!result.is_valid);
        assert_eq!(result.area, Some(0.0));
    }

    #[test]
    fn pre_closed_ring_is_not_double_closed() {
        // 用户传入已闭合的环（首尾同点）也应算出正确面积
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(10.0, 0.0),
            Vertex::new(10.0, 10.0),
            Vertex::new(0.0, 10.0),
            Vertex::new(0.0, 0.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(result.is_valid);
        assert_eq!(result.area, Some(100.0));
    }
}
