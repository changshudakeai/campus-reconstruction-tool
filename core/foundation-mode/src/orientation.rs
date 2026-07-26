//! 朝向角度计算子模块
//!
//! **核心功能**：通过高德地图上两点连线确定朝向，计算与正北方向的夹角。
//!
//! ## 业务规则（ADR-0012）
//! - **必选步骤**：无默认值、不可跳过——因此计算结果直接产出
//!   [shared_domain_types::Orientation]（复用 T02 类型，不重新定义）
//! - **输入方式**：在地图上画两点参考线（鼠标点击/触控点按）
//! - **输出范围**：[0°, 360°]，越界输入拒绝（返回 None）或经 normalize 修正

use serde::{Deserialize, Serialize};
use shared_domain_types::Orientation;

/// 平面上的二维点（平面米单位，用于表示地图上的两点参考线）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2D {
    /// X 坐标（东向为正）
    pub x: f64,
    /// Y 坐标（北向为正）
    pub y: f64,
}

impl Point2D {
    /// 新建二维点
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// 朝向参考线 —— 用户在高德地图上选择的两点
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OrientationLine {
    /// 起点
    pub start_point: Point2D,
    /// 终点（指向目标朝向方向）
    pub end_point: Point2D,
}

impl OrientationLine {
    /// 新建朝向参考线；起点终点重合（无法确定方向）时返回 None
    pub fn new(start: Point2D, end: Point2D) -> Option<Self> {
        let dist_sq = (start.x - end.x).powi(2) + (start.y - end.y).powi(2);
        if dist_sq < 1e-12 {
            return None;
        }
        Some(Self {
            start_point: start,
            end_point: end,
        })
    }

    /// 参考线长度（米）
    pub fn length(&self) -> f64 {
        ((self.start_point.x - self.end_point.x).powi(2)
            + (self.start_point.y - self.end_point.y).powi(2))
        .sqrt()
    }
}

/// 朝向计算器 —— 从参考线推导方位角
pub struct OrientationCalculator;

impl OrientationCalculator {
    /// 从参考线计算朝向，直接产出 B1 共享类型 [Orientation]
    ///
    /// 公式：angle = atan2(dx, dy)，正北为 0°、顺时针增加：
    /// - 正北（dy > 0, dx = 0）→ 0°
    /// - 正东（dy = 0, dx > 0）→ 90°
    /// - 正南（dy < 0, dx = 0）→ 180°
    /// - 正西（dy = 0, dx < 0）→ 270°
    pub fn calculate(line: &OrientationLine) -> Option<Orientation> {
        let dx = line.end_point.x - line.start_point.x;
        let dy = line.end_point.y - line.start_point.y;

        // atan2(dx, dy)：以正北（Y 轴正向）为 0°、顺时针为正
        let angle_deg = dx.atan2(dy).to_degrees();

        // atan2 结果落在 (-180, 180]，负角折回 [0, 360)
        let normalized = if angle_deg < 0.0 {
            angle_deg + 360.0
        } else {
            angle_deg
        };

        // 复用 T02 的范围校验（0~360 之外返回 None）
        Orientation::new(normalized as f32)
    }

    /// 把任意角度修正到 [0, 360)；无效输入（NaN/∞）返回 None
    ///
    /// 例：-90° → 270°，400° → 40°，360° → 0°
    pub fn normalize_angle(angle: f32) -> Option<Orientation> {
        if !angle.is_finite() {
            return None;
        }

        let mut normalized = angle % 360.0;
        if normalized < 0.0 {
            normalized += 360.0;
        }
        if normalized >= 360.0 {
            normalized = 0.0;
        }

        Orientation::new(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: f64, y: f64) -> OrientationLine {
        OrientationLine::new(Point2D::new(0.0, 0.0), Point2D::new(x, y)).expect("非重合两点")
    }

    #[test]
    fn cardinal_directions_map_to_expected_degrees() {
        let cases = [
            (0.0, 100.0, 0.0),    // 正北
            (100.0, 0.0, 90.0),   // 正东
            (0.0, -100.0, 180.0), // 正南
            (-100.0, 0.0, 270.0), // 正西
            (100.0, 100.0, 45.0), // 东北
        ];
        for (x, y, expected) in cases {
            let orientation = OrientationCalculator::calculate(&line(x, y)).unwrap();
            assert!(
                (orientation.degree() - expected).abs() < 0.01,
                "({x}, {y}) 应为 {expected}°，实际 {}°",
                orientation.degree()
            );
        }
    }

    #[test]
    fn out_of_range_angles_are_normalized() {
        // -90° → 270°
        assert_eq!(
            OrientationCalculator::normalize_angle(-90.0)
                .unwrap()
                .degree(),
            270.0
        );
        // 400° → 40°
        assert_eq!(
            OrientationCalculator::normalize_angle(400.0)
                .unwrap()
                .degree(),
            40.0
        );
        // 360° → 0°
        assert_eq!(
            OrientationCalculator::normalize_angle(360.0)
                .unwrap()
                .degree(),
            0.0
        );
    }

    #[test]
    fn non_finite_angles_are_rejected() {
        assert!(OrientationCalculator::normalize_angle(f32::NAN).is_none());
        assert!(OrientationCalculator::normalize_angle(f32::INFINITY).is_none());
        assert!(OrientationCalculator::normalize_angle(f32::NEG_INFINITY).is_none());
    }

    #[test]
    fn coincident_points_cannot_form_a_line() {
        let p = Point2D::new(1.0, 1.0);
        assert!(OrientationLine::new(p, p).is_none());
    }

    #[test]
    fn line_length_is_euclidean() {
        assert_eq!(line(3.0, 4.0).length(), 5.0);
    }
}
