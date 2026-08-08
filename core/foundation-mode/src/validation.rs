//! 边界有效性验证子模块
//!
//! **核心功能**：验证绘制边界的几何正确性——
//! - 多边形闭合检查（至少 3 个点；GeoJSON 环自动首尾闭合）
//! - 面积计算（鞋带公式，经 geo crate；用于提示用户绘制范围大小）
//! - 自相交检测（T24/ADR-0029：相邻边除外的线段相交检查，O(n²)）
//!
//! 注：早期注释曾把自相交划归规划中的 B14 geometry-validator；
//! ADR-0029（2026-07-28）明确"最终边界校验（自相交/点数不足）"属 B5，
//! 以该 ADR 为准。

use crate::boundary_ui::Vertex;
use geo::{Area, Coord, Intersects, Line, LineString, Polygon};

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

    /// 多边形自相交（T24/ADR-0029：非法边界，必须修正）
    #[error("边界自相交：第 {edge_a} 条边与第 {edge_b} 条边相交")]
    SelfIntersecting {
        /// 第一条相交边索引
        edge_a: usize,
        /// 第二条相交边索引
        edge_b: usize,
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

    // 检查 3：自相交检测（T24/ADR-0029）
    if let Some((edge_a, edge_b)) = detect_self_intersection(vertices) {
        errors.push(BoundaryValidationError::SelfIntersecting { edge_a, edge_b });
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        area: Some(area),
    }
}

/// 自相交检测：返回第一对相交的非相邻边索引（无则 None）
///
/// 算法：枚举所有线段对 (i, j)，跳过相邻边（|i-j|<=1 以及首尾相接），
/// 用 geo `Intersects` 判定。复杂度 O(n²)，校园边界顶点数通常 < 1000，可接受。
///
/// 输入先规范化：去除连续重复点与首尾闭合重复点（预闭合成环的输入
/// 不应被误判——共享端点不算自相交）。
pub fn detect_self_intersection(vertices: &[Vertex]) -> Option<(usize, usize)> {
    // 规范化：去除连续重复点（含首尾闭合重复）
    let mut dedup: Vec<&Vertex> = Vec::with_capacity(vertices.len());
    for v in vertices {
        if let Some(last) = dedup.last() {
            if (last.x - v.x).abs() < f64::EPSILON && (last.y - v.y).abs() < f64::EPSILON {
                continue;
            }
        }
        dedup.push(v);
    }
    if dedup.len() >= 2 {
        let first = dedup[0];
        let last = dedup[dedup.len() - 1];
        if (first.x - last.x).abs() < f64::EPSILON && (first.y - last.y).abs() < f64::EPSILON {
            dedup.pop();
        }
    }
    if dedup.len() < 4 {
        // 少于 4 个有效点不可能自相交
        return None;
    }
    let n = dedup.len();
    let segments: Vec<Line<f64>> = (0..n)
        .map(|i| {
            let a = dedup[i];
            let b = dedup[(i + 1) % n];
            Line::new(Coord { x: a.x, y: a.y }, Coord { x: b.x, y: b.y })
        })
        .collect();

    for i in 0..n {
        for j in (i + 1)..n {
            // 跳过相邻边（共享端点不算自相交）
            if j == i + 1 {
                continue;
            }
            // 跳过首尾相接（边 0 与边 n-1 共享顶点）
            if i == 0 && j == n - 1 {
                continue;
            }
            // 共享端点不算自相交：环可能在中途回到首点闭合后仍带尾点
            // （如 OSM 几何的闭合节点不在末尾），此时尾边与首边在重复
            // 顶点处相接，索引不相邻但共享端点——同样不是自相交。
            let si = &segments[i];
            let sj = &segments[j];
            if si.start == sj.start || si.start == sj.end || si.end == sj.start || si.end == sj.end
            {
                continue;
            }
            if segments[i].intersects(&segments[j]) {
                return Some((i, j));
            }
        }
    }
    None
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

    // ── T24: 自相交检测测试 ──────────────────────────────────────

    #[test]
    fn bowtie_polygon_is_self_intersecting() {
        // 蝴蝶结形（8 字形）：边 0 与边 2 相交
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 20.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(0.0, 20.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(!result.is_valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, BoundaryValidationError::SelfIntersecting { .. })));
    }

    #[test]
    fn simple_rectangle_is_not_self_intersecting() {
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(20.0, 20.0),
            Vertex::new(0.0, 20.0),
        ];

        assert!(detect_self_intersection(&vertices).is_none());
    }

    #[test]
    fn triangle_cannot_self_intersect() {
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(10.0, 20.0),
        ];

        assert!(detect_self_intersection(&vertices).is_none());
    }

    #[test]
    fn adjacent_edges_sharing_vertex_are_not_intersection() {
        // L 形折线闭合后，相邻边共享端点不应误判为自相交
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(20.0, 10.0),
            Vertex::new(10.0, 10.0),
            Vertex::new(10.0, 20.0),
            Vertex::new(0.0, 20.0),
        ];

        assert!(detect_self_intersection(&vertices).is_none());
    }

    #[test]
    fn ring_closing_midway_with_tail_is_not_self_intersecting() {
        // T33 回归：真实 OSM 校区边界（如华东师大普陀校区）的闭合节点
        // 不在末尾——环在中途回到首点（v4 == v0）后仍带 2 个尾点。
        // 尾边（v3→v0、v6→v0）与首边（v0→v1）在 v0 处共享端点，
        // 索引不相邻但共享端点，不得判为自相交。
        let vertices = vec![
            Vertex::new(0.0, 0.0),   // v0
            Vertex::new(20.0, 0.0),  // v1
            Vertex::new(20.0, 20.0), // v2
            Vertex::new(0.0, 20.0),  // v3
            Vertex::new(0.0, 0.0),   // v4 == v0（中途闭合）
            Vertex::new(-1.0, -1.0), // v5 尾点（环外）
            Vertex::new(-2.0, 0.0),  // v6 尾点（环外）
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(
            !result
                .errors
                .iter()
                .any(|e| matches!(e, BoundaryValidationError::SelfIntersecting { .. })),
            "中途闭合环的共享端点不得被误判为自相交：{result:?}"
        );
        assert!(result.area.unwrap() > 100.0);
    }

    #[test]
    fn genuine_bowtie_is_still_self_intersecting() {
        // 真正的自相交（两条边在中点处交叉、不共享端点）仍必须被检出。
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(10.0, 10.0),
            Vertex::new(10.0, 0.0),
            Vertex::new(0.0, 10.0),
        ];

        let result = validate_polygon_closure(&vertices);

        assert!(result
            .errors
            .iter()
            .any(|e| { matches!(e, BoundaryValidationError::SelfIntersecting { .. }) }));
    }

    #[test]
    fn detect_returns_edge_indices() {
        // 蝴蝶结形：边 0 (0,0)-(20,20) 与边 2 (20,0)-(0,20) 相交
        let vertices = vec![
            Vertex::new(0.0, 0.0),
            Vertex::new(20.0, 20.0),
            Vertex::new(20.0, 0.0),
            Vertex::new(0.0, 20.0),
        ];

        let (a, b) = detect_self_intersection(&vertices).expect("应检测到自相交");
        assert_eq!((a, b), (0, 2));
    }
}
