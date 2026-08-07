//! WGS-84 → GCJ-02 坐标转换（T31，ADR-0029 配套）。
//!
//! 应用工作坐标系为 GCJ-02（已入库边界即 GCJ-02）；OSM/Overpass 返回
//! WGS-84，必须在采集入口转 GCJ-02 后才参与排序/落库/上屏，同时保留
//! 原始 WGS-84 载荷备查。转换采用开源批量实现（精度约 1 米，校区尺度
//! 误差可忽略；调研报告 §7.2 明确允许，见 `docs/research/gcj02-conversion-practice.md`）。
//! 本模块只做 WGS→GCJ，不做 GCJ→WGS 反向（工单红线）。

use std::f64::consts::PI;

/// 克拉索夫斯基椭球长半轴（米）
const KRASOVSKY_A: f64 = 6378245.0;
/// 克拉索夫斯基椭球第一偏心率平方
const KRASOVSKY_EE: f64 = 0.006693421622965943;

/// 中国境外不做偏移（GCJ-02 只作用于中国境内）
fn out_of_china(lon: f64, lat: f64) -> bool {
    !(72.004..=137.8347).contains(&lon) || !(0.8293..=55.8271).contains(&lat)
}

fn transform_lat(x: f64, y: f64) -> f64 {
    let mut ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    ret += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * 2.0 / 3.0;
    ret += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y * PI / 30.0).sin()) * 2.0 / 3.0;
    ret
}

fn transform_lon(x: f64, y: f64) -> f64 {
    let mut ret = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * 2.0 / 3.0;
    ret += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * 2.0 / 3.0;
    ret += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * 2.0 / 3.0;
    ret
}

/// WGS-84 经纬度 → GCJ-02（中国境外原样返回）
pub fn wgs84_to_gcj02(lon: f64, lat: f64) -> (f64, f64) {
    if out_of_china(lon, lat) || !lon.is_finite() || !lat.is_finite() {
        return (lon, lat);
    }
    let d_lat = transform_lat(lon - 105.0, lat - 35.0);
    let d_lon = transform_lon(lon - 105.0, lat - 35.0);
    let rad_lat = lat / 180.0 * PI;
    let magic = (rad_lat).sin();
    let magic = 1.0 - KRASOVSKY_EE * magic * magic;
    let sqrt_magic = magic.sqrt();
    let d_lat =
        (d_lat * 180.0) / ((KRASOVSKY_A * (1.0 - KRASOVSKY_EE)) / (magic * sqrt_magic) * PI);
    let d_lon = (d_lon * 180.0) / (KRASOVSKY_A / sqrt_magic * rad_lat.cos() * PI);
    (lon + d_lon, lat + d_lat)
}

/// 就地批量转换 `[lon, lat]` 坐标数组（WGS-84 → GCJ-02）
pub fn convert_coords_wgs84_to_gcj02(coords: &mut [[f64; 2]]) {
    for pair in coords {
        let (lon, lat) = wgs84_to_gcj02(pair[0], pair[1]);
        pair[0] = lon;
        pair[1] = lat;
    }
}

/// 就地批量转换 `(lon, lat)` 坐标元组（WGS-84 → GCJ-02；SourceGeometry 使用元组）
pub fn convert_pairs_wgs84_to_gcj02(coords: &mut [(f64, f64)]) {
    for pair in coords {
        let (lon, lat) = wgs84_to_gcj02(pair.0, pair.1);
        *pair = (lon, lat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beijing_wgs84_moves_into_gcj02() {
        // 公开测试向量：北京 WGS-84 (116.391, 39.907) → GCJ-02 ≈ (116.397, 39.909)
        let (lon, lat) = wgs84_to_gcj02(116.391, 39.907);
        assert!((lon - 116.397).abs() < 0.001, "lon={lon}");
        assert!((lat - 39.909).abs() < 0.001, "lat={lat}");
    }

    #[test]
    fn shanghai_wgs84_shift_is_bounded() {
        // 上海（交大闵行 WGS 约 121.44, 31.03）：偏移应在一百~几百米量级
        let (lon, lat) = wgs84_to_gcj02(121.44, 31.03);
        assert!((lon - 121.44).abs() < 0.01);
        assert!((lat - 31.03).abs() < 0.01);
        assert!(lon.is_finite() && lat.is_finite());
    }

    #[test]
    fn outside_china_is_unchanged() {
        let (lon, lat) = wgs84_to_gcj02(-74.006, 40.7128); // New York
        assert_eq!((lon, lat), (-74.006, 40.7128));
    }

    #[test]
    fn batch_conversion_preserves_length_and_updates_points() {
        let mut coords = vec![[116.391, 39.907], [121.44, 31.03]];
        convert_coords_wgs84_to_gcj02(&mut coords);
        assert_eq!(coords.len(), 2);
        assert!((coords[0][0] - 116.397).abs() < 0.001);
        assert!((coords[0][1] - 39.909).abs() < 0.001);
    }

    #[test]
    fn tuple_pairs_are_converted_in_place() {
        let mut pairs = vec![(116.391, 39.907), (121.44, 31.03)];
        convert_pairs_wgs84_to_gcj02(&mut pairs);
        assert!((pairs[0].0 - 116.397).abs() < 0.001);
        assert!((pairs[0].1 - 39.909).abs() < 0.001);
    }
}
