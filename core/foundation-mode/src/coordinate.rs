//! 坐标系转换子模块
//!
//! 提供三种坐标系统的互转（B5 内部子模块，不独立成 crate）：
//! - **高德 Mercator 投影**：经纬度 ⇄ Web 墨卡托（EPSG:3857）
//! - **平面米单位**：以边界中心为原点、纠正纬度畸变后的米制坐标
//! - **Minecraft 块坐标**：1 块 = 1 米的 MC 世界坐标（X 东向、Z 南向）

use std::f64::consts::PI;

/// 地球赤道半径（米），Web Mercator 使用的球体近似
const EARTH_RADIUS_M: f64 = 6378137.0;

/// 高德地图 Mercator 投影坐标（EPSG:3857）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MercatorCoord {
    /// 经度方向墨卡托投影（米）
    pub x: f64,
    /// 纬度方向墨卡托投影（米）
    pub y: f64,
}

impl MercatorCoord {
    /// 从经纬度创建 Mercator 坐标（高德地图使用）
    ///
    /// 公式：x = R·λ，y = R·ln(tan(π/4 + φ/2))，λ/φ 为弧度
    pub fn from_lat_lon(lat: f64, lon: f64) -> Self {
        let lon_rad = lon.to_radians();
        let lat_rad = lat.to_radians();

        let x = EARTH_RADIUS_M * lon_rad;
        let y = EARTH_RADIUS_M * (PI / 4.0 + lat_rad / 2.0).tan().ln();

        Self { x, y }
    }

    /// 反向：从 Mercator 坐标转回经纬度（度）
    pub fn to_lat_lon(&self) -> (f64, f64) {
        let lon = self.x / EARTH_RADIUS_M;
        let lat = 2.0 * (self.y / EARTH_RADIUS_M).exp().atan() - PI / 2.0;
        (lat.to_degrees(), lon.to_degrees())
    }
}

/// 平面米单位坐标（以边界中心为原点）
/// X：东向为正；Y：北向为正（与 orientation::Point2D 保持一致）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneMileUnit {
    /// 东向偏移（米，正东为正）
    pub x: f64,
    /// 北向偏移（米，正北为正）
    pub y: f64,
}

impl PlaneMileUnit {
    /// 新建平面坐标
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// 计算两点间距离（米）
    pub fn distance_to(&self, other: &Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Minecraft 块坐标（1 个方块 = 1 立方米）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McBlockCoord {
    /// X 轴块位置（东向为正）
    pub block_x: i32,
    /// Z 轴块位置（南向为正，北向为负）
    pub block_z: i32,
}

impl McBlockCoord {
    /// 新建 MC 块坐标
    pub fn new(block_x: i32, block_z: i32) -> Self {
        Self { block_x, block_z }
    }

    /// 从平面米单位坐标转换
    ///
    /// 轴向映射：
    /// - plane.x（东向）→ MC.X 正方向
    /// - plane.y（北向）→ MC.Z 负方向（MC 中 Z 轴指向南）
    pub fn from_plane(plane: &PlaneMileUnit, blocks_per_meter: f64) -> Self {
        // x 对应东向 → MC.X 正
        let mc_x = (plane.x * blocks_per_meter) as i32;
        // y 对应北向 → MC.Z 负
        let mc_z = -(plane.y * blocks_per_meter) as i32;
        Self {
            block_x: mc_x,
            block_z: mc_z,
        }
    }
}

/// 坐标转换器 —— 串联三个坐标系统
///
/// 内部状态维护当前边界的中心点和比例尺（由用户设定）。
#[derive(Debug)]
pub struct CoordinateConverter {
    /// 边界中心的 Mercator 坐标（用户圈画多边形后取重心）
    center_mercator: Option<MercatorCoord>,
    /// 当前比例尺：1 米 = ? MC 块（默认 1）
    blocks_per_meter: f64,
}

impl Default for CoordinateConverter {
    fn default() -> Self {
        Self {
            center_mercator: None,
            blocks_per_meter: 1.0,
        }
    }
}

impl CoordinateConverter {
    /// 设置边界中心点（来自用户绘制的多边形重心）
    pub fn set_center(&mut self, mercator: MercatorCoord) {
        self.center_mercator = Some(mercator);
    }

    /// 获取当前比例尺
    pub fn blocks_per_meter(&self) -> f64 {
        self.blocks_per_meter
    }

    /// 设置比例尺（非正数被忽略）
    pub fn set_blocks_per_meter(&mut self, ratio: f64) {
        if ratio > 0.0 {
            self.blocks_per_meter = ratio;
        }
    }

    /// 将 Mercator 坐标转换为平面米单位（相对于中心）
    ///
    /// Web Mercator 的长度在纬度 φ 处被放大 1/cos(φ)，
    /// 校园尺度（数公里内）用中心纬度的 cos 值统一纠正即可。
    /// 返回 (x=East, y=North) —— 与 Point2D 一致
    pub fn mercator_to_plane(&self, mercator: MercatorCoord) -> Option<PlaneMileUnit> {
        let center = self.center_mercator?;
        let (center_lat, _) = center.to_lat_lon();
        let scale = center_lat.to_radians().cos();

        let east_m = (mercator.x - center.x) * scale;
        let north_m = (mercator.y - center.y) * scale;

        // x=East, y=North（与 Point2D 一致）
        Some(PlaneMileUnit { x: east_m, y: north_m })
    }

    /// 将平面米单位转为 MC 块坐标
    pub fn plane_to_mc(&self, plane: &PlaneMileUnit) -> McBlockCoord {
        McBlockCoord::from_plane(plane, self.blocks_per_meter)
    }

    /// 完整链式转换：Mercator → 平面米单位 → MC 块坐标
    pub fn convert(&self, mercator: MercatorCoord) -> Option<(PlaneMileUnit, McBlockCoord)> {
        let plane = self.mercator_to_plane(mercator)?;
        let mc = self.plane_to_mc(&plane);
        Some((plane, mc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mercator_roundtrip_is_consistent() {
        // 经纬度 → Mercator → 经纬度双向一致
        let (lat, lon) = (39.9042, 116.4074); // 北京故宫
        let mercator = MercatorCoord::from_lat_lon(lat, lon);
        let (lat_back, lon_back) = mercator.to_lat_lon();

        assert!((lat - lat_back).abs() < 1e-6);
        assert!((lon - lon_back).abs() < 1e-6);
    }

    #[test]
    fn plane_distance_is_euclidean() {
        let a = PlaneMileUnit::new(0.0, 0.0);
        let b = PlaneMileUnit::new(3.0, 4.0);

        assert_eq!(a.distance_to(&b), 5.0);
    }

    #[test]
    fn mc_block_axes_follow_minecraft_convention() {
        let converter = CoordinateConverter::default();

        let plane = PlaneMileUnit::new(20.0, 10.0); // 东 20 米，北 10 米
        let mc = converter.plane_to_mc(&plane);

        assert_eq!(mc.block_x, 20); // 东向 → X 正
        assert_eq!(mc.block_z, -10); // 北向 → Z 负
    }

    #[test]
    fn mercator_to_plane_corrects_latitude_distortion() {
        let mut converter = CoordinateConverter::default();
        let center = MercatorCoord::from_lat_lon(39.9042, 116.4074);
        converter.set_center(center);

        // 向东偏 0.001 度经度 ≈ 85 米（北纬 39.9°）→ x（East）
        let east_point = MercatorCoord::from_lat_lon(39.9042, 116.4084);
        let plane = converter.mercator_to_plane(east_point).unwrap();

        assert!((plane.x - 85.4).abs() < 1.0, "东向 {} 米", plane.x);
        assert!(plane.y.abs() < 0.5, "北向偏移应接近 0，实际 {} 米", plane.y);
    }

    #[test]
    fn missing_center_returns_none() {
        let converter = CoordinateConverter::default();
        let point = MercatorCoord::from_lat_lon(39.9, 116.4);

        assert!(converter.mercator_to_plane(point).is_none());
        assert!(converter.convert(point).is_none());
    }
}
