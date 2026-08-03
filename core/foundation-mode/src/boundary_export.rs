//! 已确认边界的最小导出范围计算。
//!
//! 这是 B5 对导出用例提供的边界几何能力：把已确认的 GeoJSON Polygon
//! 转成覆盖其外接范围所需的 Minecraft 块尺寸。这里不修复几何、不推导
//! 候选，也不决定朝向；导出资格与朝向选择仍由 F9 完整用例负责。

use shared_domain_types::Boundary;

use crate::coordinate::{CoordinateConverter, MercatorCoord};
use crate::validation::validate_polygon_closure;
use crate::Vertex;

/// 边界覆盖范围的最小平整场地尺寸（单位：Minecraft 块）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryFootprint {
    /// 东西方向尺寸（X）。
    pub width_blocks: usize,
    /// 南北方向尺寸（Z）。
    pub length_blocks: usize,
}

/// 从边界计算最小覆盖范围时的结构化错误。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum BoundaryFootprintError {
    /// 边界没有任何坐标。
    #[error("边界没有坐标")]
    Empty,
    /// 当前导出路径只接受 Polygon/MultiPolygon 外环。
    #[error("边界类型不支持：{0}")]
    UnsupportedType(String),
    /// GeoJSON 坐标结构不符合经度/纬度点数组。
    #[error("边界坐标结构无效")]
    MalformedCoordinates,
    /// B5 几何校验拒绝该边界。
    #[error("边界几何无效：{0}")]
    InvalidGeometry(String),
    /// 投影结果无法表示为块尺寸。
    #[error("边界尺寸超出可导出范围")]
    SizeOverflow,
}

/// 计算 GeoJSON 边界的最小矩形块范围。
pub fn boundary_footprint(
    boundary: &Boundary,
) -> Result<BoundaryFootprint, BoundaryFootprintError> {
    if boundary.is_empty() {
        return Err(BoundaryFootprintError::Empty);
    }

    let ring = outer_ring(boundary)?;
    let points = ring
        .iter()
        .map(parse_point)
        .collect::<Result<Vec<_>, _>>()?;
    if points.len() < 3 {
        return Err(BoundaryFootprintError::InvalidGeometry(
            "至少需要 3 个边界点".to_owned(),
        ));
    }

    let (center_lon, center_lat) = points.iter().fold((0.0, 0.0), |(lon, lat), point| {
        (lon + point[0], lat + point[1])
    });
    let count = points.len() as f64;
    let center_lon = center_lon / count;
    let center_lat = center_lat / count;
    if !(-85.0..=85.0).contains(&center_lat) {
        return Err(BoundaryFootprintError::InvalidGeometry(
            "中心纬度不在可投影范围内".to_owned(),
        ));
    }

    let mut converter = CoordinateConverter::default();
    converter.set_center(MercatorCoord::from_lat_lon(center_lat, center_lon));
    let vertices = points
        .iter()
        .map(|[lon, lat]| {
            let mercator = MercatorCoord::from_lat_lon(*lat, *lon);
            converter
                .mercator_to_plane(mercator)
                .map(|plane| Vertex::new(plane.x, plane.y))
                .ok_or(BoundaryFootprintError::InvalidGeometry(
                    "边界坐标无法转换".to_owned(),
                ))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let validation = validate_polygon_closure(&vertices);
    if !validation.is_valid {
        let detail = validation
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(BoundaryFootprintError::InvalidGeometry(detail));
    }

    let (min_x, max_x, min_y, max_y) = vertices.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), vertex| {
            (
                min_x.min(vertex.x),
                max_x.max(vertex.x),
                min_y.min(vertex.y),
                max_y.max(vertex.y),
            )
        },
    );

    Ok(BoundaryFootprint {
        width_blocks: span_to_blocks(max_x - min_x)?,
        length_blocks: span_to_blocks(max_y - min_y)?,
    })
}

fn outer_ring(boundary: &Boundary) -> Result<&Vec<serde_json::Value>, BoundaryFootprintError> {
    let coordinates = boundary
        .coordinates
        .as_array()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    match boundary.r#type.as_str() {
        "Polygon" => coordinates
            .first()
            .and_then(serde_json::Value::as_array)
            .ok_or(BoundaryFootprintError::MalformedCoordinates),
        "MultiPolygon" => coordinates
            .first()
            .and_then(serde_json::Value::as_array)
            .and_then(|polygon| polygon.first())
            .and_then(serde_json::Value::as_array)
            .ok_or(BoundaryFootprintError::MalformedCoordinates),
        other => Err(BoundaryFootprintError::UnsupportedType(other.to_owned())),
    }
}

fn parse_point(value: &serde_json::Value) -> Result<[f64; 2], BoundaryFootprintError> {
    let point = value
        .as_array()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    if point.len() != 2 {
        return Err(BoundaryFootprintError::MalformedCoordinates);
    }
    let lon = point[0]
        .as_f64()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    let lat = point[1]
        .as_f64()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    if !lon.is_finite() || !lat.is_finite() {
        return Err(BoundaryFootprintError::MalformedCoordinates);
    }
    Ok([lon, lat])
}

fn span_to_blocks(span_meters: f64) -> Result<usize, BoundaryFootprintError> {
    if !span_meters.is_finite() || span_meters < 0.0 {
        return Err(BoundaryFootprintError::SizeOverflow);
    }
    let blocks = span_meters.ceil().max(1.0);
    if blocks > usize::MAX as f64 {
        return Err(BoundaryFootprintError::SizeOverflow);
    }
    Ok(blocks as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Boundary {
        Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [116.0, 39.0],
                [116.001, 39.0],
                [116.001, 39.001],
                [116.0, 39.001],
                [116.0, 39.0]
            ]]),
        }
    }

    #[test]
    fn computes_positive_minimum_footprint_for_valid_polygon() {
        let footprint = boundary_footprint(&square()).unwrap();
        assert!(footprint.width_blocks > 1);
        assert!(footprint.length_blocks > 1);
    }

    #[test]
    fn rejects_empty_or_unsupported_boundary() {
        assert!(matches!(
            boundary_footprint(&Boundary::empty()),
            Err(BoundaryFootprintError::Empty)
        ));
        let unsupported = Boundary {
            r#type: "LineString".to_owned(),
            coordinates: serde_json::json!([[116.0, 39.0], [116.1, 39.1]]),
        };
        assert!(matches!(
            boundary_footprint(&unsupported),
            Err(BoundaryFootprintError::UnsupportedType(_))
        ));
    }
}
