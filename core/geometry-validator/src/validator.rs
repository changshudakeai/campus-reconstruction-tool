//! B14 的形状校验实现：只规范化可唯一修复的候选几何，并逐项隔离不可靠对象。

use std::collections::HashSet;

use thiserror::Error;

/// 采集候选的单一闭合面环。坐标为经度、纬度；输入可以暂未闭合。
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateGeometry {
    /// 上层用来关联原始观测或候选的稳定标识。
    pub candidate_id: String,
    /// 数据源明确给出的形状；B14 从不按标签或原始 JSON 猜测几何。
    pub shape: GeometryShape,
    /// 环的坐标点；验证成功后恰有一个闭合终点。
    pub coordinates: Vec<(f64, f64)>,
}

/// 来源声明的候选形状。
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GeometryShape {
    /// 单一坐标点，例如高德 POI 的位置证据。
    Point((f64, f64)),
    /// 有序线段，例如道路中心线。
    LineString(Vec<(f64, f64)>),
    /// 面环；未闭合输入可由 B14 安全补齐。
    Polygon,
}

impl CandidateGeometry {
    /// 建立一个尚未验证的候选几何。
    pub fn new(candidate_id: impl Into<String>, coordinates: Vec<(f64, f64)>) -> Self {
        Self {
            candidate_id: candidate_id.into(),
            shape: GeometryShape::Polygon,
            coordinates,
        }
    }

    /// 以来源声明的形状建立候选，Point 不会被伪造为面环。
    pub fn with_shape(candidate_id: impl Into<String>, shape: GeometryShape) -> Self {
        let coordinates = match &shape {
            GeometryShape::Point(point) => vec![*point],
            GeometryShape::LineString(points) => points.clone(),
            GeometryShape::Polygon => Vec::new(),
        };
        Self {
            candidate_id: candidate_id.into(),
            shape,
            coordinates,
        }
    }
}

/// 不能自动猜测时的隔离原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum RejectionReason {
    /// 有效顶点少于三个。
    #[error("有效点不足")]
    InsufficientPoints,
    /// 经纬度非有限值或超出地理坐标范围。
    #[error("坐标明显错误")]
    InvalidCoordinates,
    /// 面积为零。
    #[error("零面积")]
    ZeroArea,
    /// 非相邻边相交。
    #[error("自相交")]
    SelfIntersecting,
    /// 线对象含有零长度连续段。
    #[error("零长度线段")]
    ZeroLengthSegment,
}

/// 一项验证的最后去向。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationDisposition {
    /// 原几何已经合格。
    Retained,
    /// 仅做了不改变外观的唯一修复。
    Repaired,
    /// 不可靠的对象被隔离，且不会进入候选、评审或导出。
    Rejected(RejectionReason),
}

/// 单候选验证结果。被拒绝时 `geometry` 为 `None`，原始观测由调用方继续保存。
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryOutcome {
    pub candidate_id: String,
    pub disposition: ValidationDisposition,
    pub geometry: Option<CandidateGeometry>,
}

/// 批量校验结果与面向报告的结构化统计。
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryValidation {
    pub outcomes: Vec<GeometryOutcome>,
    pub automatically_repaired_count: usize,
    pub rejected_count: usize,
}

impl GeometryValidation {
    /// 可安全进入候选池的几何（包含保留及自动修复）。
    pub fn retained(&self) -> Vec<&CandidateGeometry> {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.geometry.as_ref())
            .collect()
    }
}

/// B14 的无状态入口。
#[derive(Debug, Default, Clone, Copy)]
pub struct GeometryValidator;

impl GeometryValidator {
    pub fn new() -> Self {
        Self
    }

    /// 独立验证一个候选；无效对象不会影响同批其它对象。
    pub fn validate(&self, candidate: CandidateGeometry) -> GeometryOutcome {
        let candidate_id = candidate.candidate_id.clone();
        if let GeometryShape::Point(point) = &candidate.shape {
            return if valid_coordinate(*point) {
                GeometryOutcome {
                    candidate_id,
                    disposition: ValidationDisposition::Retained,
                    geometry: Some(candidate),
                }
            } else {
                rejected(candidate_id, RejectionReason::InvalidCoordinates)
            };
        }
        if let GeometryShape::LineString(points) = &candidate.shape {
            return validate_line(candidate_id, points);
        }
        let original = candidate.coordinates.clone();
        let mut points = candidate.coordinates;
        if points.iter().any(|point| !valid_coordinate(*point)) {
            return rejected(candidate_id, RejectionReason::InvalidCoordinates);
        }

        let mut repaired = false;
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
            repaired = true;
        }
        let before_dedup = points.len();
        points.dedup();
        repaired |= points.len() != before_dedup;

        if points.len() < 3 || unique_point_count(&points) < 3 {
            return rejected(candidate_id, RejectionReason::InsufficientPoints);
        }
        if self_intersects(&points) {
            return rejected(candidate_id, RejectionReason::SelfIntersecting);
        }
        if signed_area(&points).abs() <= f64::EPSILON {
            return rejected(candidate_id, RejectionReason::ZeroArea);
        }

        points.push(points[0]);
        repaired |= points != original;
        let disposition = if repaired {
            ValidationDisposition::Repaired
        } else {
            ValidationDisposition::Retained
        };
        GeometryOutcome {
            candidate_id: candidate_id.clone(),
            disposition,
            geometry: Some(CandidateGeometry {
                candidate_id,
                shape: GeometryShape::Polygon,
                coordinates: points,
            }),
        }
    }

    /// 逐项验证一批候选，并返回修复/隔离统计供采集报告呈现。
    pub fn validate_batch(&self, candidates: Vec<CandidateGeometry>) -> GeometryValidation {
        let outcomes: Vec<_> = candidates
            .into_iter()
            .map(|candidate| self.validate(candidate))
            .collect();
        let automatically_repaired_count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.disposition, ValidationDisposition::Repaired))
            .count();
        let rejected_count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome.disposition, ValidationDisposition::Rejected(_)))
            .count();
        GeometryValidation {
            outcomes,
            automatically_repaired_count,
            rejected_count,
        }
    }
}

fn validate_line(candidate_id: String, points: &[(f64, f64)]) -> GeometryOutcome {
    if points.iter().any(|point| !valid_coordinate(*point)) {
        return rejected(candidate_id, RejectionReason::InvalidCoordinates);
    }
    if points.len() < 2 || unique_point_count(points) < 2 {
        return rejected(candidate_id, RejectionReason::InsufficientPoints);
    }
    if points.windows(2).any(|segment| segment[0] == segment[1]) {
        return rejected(candidate_id, RejectionReason::ZeroLengthSegment);
    }
    GeometryOutcome {
        candidate_id: candidate_id.clone(),
        disposition: ValidationDisposition::Retained,
        geometry: Some(CandidateGeometry {
            candidate_id,
            shape: GeometryShape::LineString(points.to_vec()),
            coordinates: points.to_vec(),
        }),
    }
}

fn rejected(candidate_id: String, reason: RejectionReason) -> GeometryOutcome {
    GeometryOutcome {
        candidate_id,
        disposition: ValidationDisposition::Rejected(reason),
        geometry: None,
    }
}

fn valid_coordinate((longitude, latitude): (f64, f64)) -> bool {
    longitude.is_finite()
        && latitude.is_finite()
        && longitude.abs() <= 180.0
        && latitude.abs() <= 90.0
}

fn unique_point_count(points: &[(f64, f64)]) -> usize {
    points
        .iter()
        .map(|point| (point.0.to_bits(), point.1.to_bits()))
        .collect::<HashSet<_>>()
        .len()
}

fn signed_area(points: &[(f64, f64)]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(left, right)| left.0 * right.1 - right.0 * left.1)
        .sum::<f64>()
        / 2.0
}

fn self_intersects(points: &[(f64, f64)]) -> bool {
    let length = points.len();
    (0..length).any(|first| {
        let first_next = (first + 1) % length;
        ((first + 1)..length).any(|second| {
            let second_next = (second + 1) % length;
            if first == second || first_next == second || second_next == first {
                return false;
            }
            segments_intersect(
                points[first],
                points[first_next],
                points[second],
                points[second_next],
            )
        })
    })
}

fn segments_intersect(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    ab_c * ab_d < 0.0 && cd_a * cd_b < 0.0
}

fn orientation(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(points: &[(f64, f64)]) -> CandidateGeometry {
        CandidateGeometry::new("candidate", points.to_vec())
    }

    #[test]
    fn safely_repairs_only_unambiguous_ring_defects() {
        let outcome = GeometryValidator::new().validate(ring(&[
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]));
        assert!(matches!(
            outcome.disposition,
            ValidationDisposition::Repaired
        ));
        assert_eq!(
            outcome.geometry.unwrap().coordinates,
            vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 0.0)]
        );
    }

    #[test]
    fn isolates_ambiguous_geometry_without_stopping_the_batch() {
        let result = GeometryValidator::new().validate_batch(vec![
            ring(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
            ring(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)]),
            ring(&[(0.0, 0.0), (1.0, 1.0)]),
        ]);
        assert_eq!(result.automatically_repaired_count, 1);
        assert_eq!(result.rejected_count, 2);
        assert_eq!(result.retained().len(), 1);
        assert!(matches!(
            result.outcomes[1].disposition,
            ValidationDisposition::Rejected(RejectionReason::SelfIntersecting)
        ));
    }
}
