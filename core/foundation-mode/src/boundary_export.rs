//! 已确认边界的最小导出范围计算。
//!
//! 这是 B5 对导出用例提供的边界几何能力：把已确认的 GeoJSON Polygon
//! 转成覆盖其外接范围所需的 Minecraft 块尺寸。这里不修复几何、不推导
//! 候选，也不决定朝向；导出资格与朝向选择仍由 F9 完整用例负责。

use shared_domain_types::{Boundary, Orientation};

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

/// 边界投影到块坐标系后的外接范围视图（B5 丈量员）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryProjection {
    /// 东西方向尺寸（X）。
    pub width_blocks: usize,
    /// 南北方向尺寸（Z）。
    pub length_blocks: usize,
    /// 外接范围最小块坐标（X，东向为正）。
    pub min_block_x: i32,
    /// 外接范围最小块坐标（Z，南向为正）。
    pub min_block_z: i32,
}

impl BoundaryProjection {
    /// 尺寸视图（与既有 [`BoundaryFootprint`] 一致）。
    pub fn footprint(&self) -> BoundaryFootprint {
        BoundaryFootprint {
            width_blocks: self.width_blocks,
            length_blocks: self.length_blocks,
        }
    }
}

/// 单个候选多边形在边界投影块坐标系中的外接范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateBlockBounds {
    /// 最小块坐标（X）。
    pub min_x: i32,
    /// 最小块坐标（Z）。
    pub min_z: i32,
    /// X 方向尺寸（格）。
    pub width_blocks: usize,
    /// Z 方向尺寸（格）。
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

/// 计算 GeoJSON 边界按地图正北坐标的最小矩形块范围。
pub fn boundary_footprint(
    boundary: &Boundary,
) -> Result<BoundaryFootprint, BoundaryFootprintError> {
    BoundaryProjector::for_boundary_with_orientation(
        boundary,
        shared_domain_types::Orientation::new(0.0).expect("地图正北是合法朝向"),
    )
    .map(|projector| projector.footprint())
}

/// 计算 GeoJSON 边界按实际导出朝向旋转后的最小矩形块范围。
///
/// F9 在完整导出用例中先决定地图正北或用户朝向，再把该决定传入 B5；
/// B18 随后以这里的尺寸生成实际 `.schem` 坐标，而不是只把朝向写进 manifest。
pub fn boundary_footprint_with_orientation(
    boundary: &Boundary,
    orientation: Orientation,
) -> Result<BoundaryFootprint, BoundaryFootprintError> {
    BoundaryProjector::for_boundary_with_orientation(boundary, orientation)
        .map(|projector| projector.footprint())
}

/// 已确认边界 + 实际朝向的投影上下文。
///
/// F9 增强导出用它把边界外接范围与后续候选多边形投影到**同一块坐标系**，
/// 保证候选落位与场地尺寸来自同一中心与朝向（ADR-0043）。
#[derive(Debug, Clone)]
pub struct BoundaryProjector {
    center: MercatorCoord,
    sin: f64,
    cos: f64,
    projection: BoundaryProjection,
    /// 边界外环投影到块坐标系后的多边形（模型本地坐标，原点为外接范围最小块）。
    polygons: Vec<Vec<(i32, i32)>>,
}

impl BoundaryProjector {
    /// 用边界与完整导出实际朝向建立投影上下文。
    pub fn for_boundary_with_orientation(
        boundary: &Boundary,
        orientation: Orientation,
    ) -> Result<Self, BoundaryFootprintError> {
        if boundary.is_empty() {
            return Err(BoundaryFootprintError::Empty);
        }

        let rings = outer_rings(boundary)?;
        if rings.is_empty() {
            return Err(BoundaryFootprintError::Empty);
        }
        let points = rings
            .iter()
            .map(|ring| {
                let points = ring
                    .iter()
                    .map(parse_point)
                    .collect::<Result<Vec<_>, _>>()?;
                if points.len() < 3 {
                    return Err(BoundaryFootprintError::InvalidGeometry(
                        "每个边界分片至少需要 3 个边界点".to_owned(),
                    ));
                }
                Ok(points)
            })
            .collect::<Result<Vec<Vec<[f64; 2]>>, BoundaryFootprintError>>()?;

        let (center_lon, center_lat, count) = points
            .iter()
            .flatten()
            .fold((0.0, 0.0, 0_usize), |(lon, lat, count), point| {
                (lon + point[0], lat + point[1], count + 1)
            });
        let count = count as f64;
        let center_lon = center_lon / count;
        let center_lat = center_lat / count;
        if !(-85.0..=85.0).contains(&center_lat) {
            return Err(BoundaryFootprintError::InvalidGeometry(
                "中心纬度不在可投影范围内".to_owned(),
            ));
        }
        let center = MercatorCoord::from_lat_lon(center_lat, center_lon);
        let radians = f64::from(orientation.degree()).to_radians();
        let (sin, cos) = radians.sin_cos();

        let mut converter = CoordinateConverter::default();
        converter.set_center(center);
        let mut projected = Vec::new();
        let mut rings = Vec::with_capacity(points.len());
        for points in &points {
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
            let rotated = vertices
                .into_iter()
                .map(|vertex| {
                    Vertex::new(
                        vertex.x * cos - vertex.y * sin,
                        vertex.x * sin + vertex.y * cos,
                    )
                })
                .collect::<Vec<_>>();
            projected.extend(rotated.iter().copied());
            rings.push(rotated);
        }

        let projection = projected_bounds_to_blocks(&projected)?;
        let width = projection.width_blocks as i64;
        let length = projection.length_blocks as i64;
        let min_x = i64::from(projection.min_block_x);
        let min_z = i64::from(projection.min_block_z);
        let polygons = rings
            .into_iter()
            .map(|ring| {
                let ring_min_x = ring.iter().map(|v| v.x).fold(f64::INFINITY, f64::min);
                let ring_max_x = ring.iter().map(|v| v.x).fold(f64::NEG_INFINITY, f64::max);
                let ring_min_neg_y = ring.iter().map(|v| -v.y).fold(f64::INFINITY, f64::min);
                let ring_max_neg_y = ring.iter().map(|v| -v.y).fold(f64::NEG_INFINITY, f64::max);
                ring.into_iter()
                    .map(|vertex| {
                        let x_block = if (vertex.x - ring_min_x).abs() < 1e-9 {
                            vertex.x.floor()
                        } else if (vertex.x - ring_max_x).abs() < 1e-9 {
                            vertex.x.ceil()
                        } else {
                            (vertex.x + 0.5).floor()
                        };
                        let neg_y = -vertex.y;
                        let z_block = if (neg_y - ring_min_neg_y).abs() < 1e-9 {
                            neg_y.floor()
                        } else if (neg_y - ring_max_neg_y).abs() < 1e-9 {
                            neg_y.ceil()
                        } else {
                            (neg_y + 0.5).floor()
                        };
                        (
                            // 环最小边 floor、最大边 ceil、中间顶点四舍五入：
                            // 既包含最外圈边界格，又不把最小边裁掉一格。
                            (x_block as i64 - min_x).clamp(0, width) as i32,
                            (z_block as i64 - min_z).clamp(0, length) as i32,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        Ok(Self {
            center,
            sin,
            cos,
            projection,
            polygons,
        })
    }

    /// 边界覆盖范围尺寸（与既有 footprint API 一致）。
    pub fn footprint(&self) -> BoundaryFootprint {
        self.projection.footprint()
    }

    /// 边界在块坐标系中的外接范围视图。
    pub fn bounds(&self) -> BoundaryProjection {
        self.projection
    }

    /// 边界外环在块坐标系中的多边形（模型本地坐标：原点 = 外接范围最小块）。
    ///
    /// B18 用它把平整场地裁剪成真实边界形状，而不是铺满外接矩形；
    /// 坐标与 `bounds()` 的 min_block 同源，可直接用于 0..w / 0..l 网格。
    pub fn boundary_polygons_local(&self) -> &[Vec<(i32, i32)>] {
        &self.polygons
    }

    /// 把候选多边形（GeoJSON coordinates）投影到与边界同一块坐标系。
    ///
    /// 只做几何换算，不判断候选资格；F9 在调用前已完成 Reviewable/保留筛选。
    pub fn candidate_bounds(
        &self,
        coordinates: &serde_json::Value,
    ) -> Result<CandidateBlockBounds, BoundaryFootprintError> {
        let points = candidate_points(coordinates)?;
        let mut converter = CoordinateConverter::default();
        converter.set_center(self.center);
        let mut projected = Vec::with_capacity(points.len());
        for [lon, lat] in points {
            let mercator = MercatorCoord::from_lat_lon(lat, lon);
            let plane = converter.mercator_to_plane(mercator).ok_or(
                BoundaryFootprintError::InvalidGeometry("候选坐标无法转换".to_owned()),
            )?;
            projected.push(Vertex::new(
                plane.x * self.cos - plane.y * self.sin,
                plane.x * self.sin + plane.y * self.cos,
            ));
        }
        let bounds = projected_bounds_to_blocks(&projected)?;
        Ok(CandidateBlockBounds {
            min_x: bounds.min_block_x,
            min_z: bounds.min_block_z,
            width_blocks: bounds.width_blocks,
            length_blocks: bounds.length_blocks,
        })
    }
}

/// 候选坐标数组 → 点序列：支持单点、线串与面环（取外环）。
fn candidate_points(
    coordinates: &serde_json::Value,
) -> Result<Vec<[f64; 2]>, BoundaryFootprintError> {
    let array = coordinates
        .as_array()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    if array.is_empty() {
        return Err(BoundaryFootprintError::MalformedCoordinates);
    }
    // 单点：[lon, lat]
    if array.len() == 2 {
        if let (Some(lon), Some(lat)) = (array[0].as_f64(), array[1].as_f64()) {
            if lon.is_finite() && lat.is_finite() {
                return Ok(vec![[lon, lat]]);
            }
        }
    }
    // 面环：[[[lon, lat], ...], ...]（取第一个外环）
    if array[0]
        .as_array()
        .is_some_and(|ring| ring.first().and_then(serde_json::Value::as_array).is_some())
    {
        let ring = array
            .first()
            .and_then(serde_json::Value::as_array)
            .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
        return ring
            .iter()
            .map(parse_point_value)
            .collect::<Result<Vec<_>, _>>();
    }
    // 线串：[[lon, lat], ...]
    array
        .iter()
        .map(parse_point_value)
        .collect::<Result<Vec<_>, _>>()
}

fn parse_point_value(value: &serde_json::Value) -> Result<[f64; 2], BoundaryFootprintError> {
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

/// 旋转后平面点集 → 块坐标外接范围（X 东向、Z 南向）。
fn projected_bounds_to_blocks(
    projected: &[Vertex],
) -> Result<BoundaryProjection, BoundaryFootprintError> {
    let (min_x, max_x, min_y, max_y) = projected.iter().fold(
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
    Ok(BoundaryProjection {
        width_blocks: span_to_blocks(max_x - min_x)?,
        length_blocks: span_to_blocks(max_y - min_y)?,
        min_block_x: min_x.floor() as i32,
        min_block_z: (-max_y).floor() as i32,
    })
}

fn outer_rings(boundary: &Boundary) -> Result<Vec<&[serde_json::Value]>, BoundaryFootprintError> {
    let coordinates = boundary
        .coordinates
        .as_array()
        .ok_or(BoundaryFootprintError::MalformedCoordinates)?;
    match boundary.r#type.as_str() {
        "Polygon" => Ok(vec![coordinates
            .first()
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .ok_or(BoundaryFootprintError::MalformedCoordinates)?]),
        "MultiPolygon" => coordinates
            .iter()
            .map(|polygon| {
                polygon
                    .as_array()
                    .and_then(|polygon| polygon.first())
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::as_slice)
                    .ok_or(BoundaryFootprintError::MalformedCoordinates)
            })
            .collect(),
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
    fn local_polygons_match_the_projection_footprint() {
        let projector = BoundaryProjector::for_boundary_with_orientation(
            &square(),
            Orientation::new(0.0).unwrap(),
        )
        .unwrap();
        let polygons = projector.boundary_polygons_local();
        assert_eq!(polygons.len(), 1, "单外环边界只有一个多边形");
        let ring = &polygons[0];
        assert!(ring.len() >= 4, "矩形外环至少四个顶点");
        let min_x = ring.iter().map(|p| p.0).min().unwrap();
        let max_x = ring.iter().map(|p| p.0).max().unwrap();
        let min_z = ring.iter().map(|p| p.1).min().unwrap();
        let max_z = ring.iter().map(|p| p.1).max().unwrap();
        let bounds = projector.bounds();
        assert_eq!(min_x, 0);
        assert_eq!(min_z, 0);
        assert!(max_x + 1 >= bounds.width_blocks as i32 - 1);
        assert!(max_z + 1 >= bounds.length_blocks as i32 - 1);
        assert!(
            max_x as usize <= bounds.width_blocks && max_z as usize <= bounds.length_blocks,
            "多边形顶点允许到网格外缘（=width/length），不得越界"
        );
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

    #[test]
    fn multipolygon_uses_all_outer_rings() {
        let first = square();
        let multi = Boundary {
            r#type: "MultiPolygon".to_owned(),
            coordinates: serde_json::json!([
                [first.coordinates[0].clone()],
                [[
                    [116.0200, 39.0200],
                    [116.0210, 39.0200],
                    [116.0210, 39.0210],
                    [116.0200, 39.0210],
                    [116.0200, 39.0200]
                ]]
            ]),
        };

        let single = boundary_footprint(&first).unwrap();
        let combined = boundary_footprint(&multi).unwrap();
        assert!(combined.width_blocks > single.width_blocks);
        assert!(combined.length_blocks > single.length_blocks);
    }

    #[test]
    fn rejects_a_malformed_multipolygon_fragment() {
        let malformed = Boundary {
            r#type: "MultiPolygon".to_owned(),
            coordinates: serde_json::json!([[square().coordinates[0].clone()], []]),
        };

        assert!(matches!(
            boundary_footprint(&malformed),
            Err(BoundaryFootprintError::MalformedCoordinates)
        ));
    }

    #[test]
    fn orientation_changes_non_square_footprint_dimensions() {
        let non_square = Boundary {
            r#type: "Polygon".to_owned(),
            coordinates: serde_json::json!([[
                [116.0000, 39.0000],
                [116.0100, 39.0000],
                [116.0100, 39.0010],
                [116.0000, 39.0010],
                [116.0000, 39.0000]
            ]]),
        };
        let north = boundary_footprint(&non_square).unwrap();
        let rotated = boundary_footprint_with_orientation(
            &non_square,
            shared_domain_types::Orientation::new(90.0).unwrap(),
        )
        .unwrap();
        assert_ne!(north, rotated);
    }

    #[test]
    fn projector_projects_candidate_inside_footprint_bounds() {
        let boundary = square();
        let projector = BoundaryProjector::for_boundary_with_orientation(
            &boundary,
            Orientation::new(0.0).unwrap(),
        )
        .unwrap();
        let bounds = projector.bounds();
        assert_eq!(bounds.footprint(), boundary_footprint(&boundary).unwrap());

        // 边界内部的候选多边形必须落在外接范围内。
        let candidate = serde_json::json!([
            [116.0002, 39.0002],
            [116.0006, 39.0002],
            [116.0006, 39.0006],
            [116.0002, 39.0002]
        ]);
        let candidate_bounds = projector.candidate_bounds(&candidate).unwrap();
        assert!(candidate_bounds.min_x >= bounds.min_block_x);
        assert!(candidate_bounds.min_z >= bounds.min_block_z);
        assert!(
            candidate_bounds.min_x + candidate_bounds.width_blocks as i32
                <= bounds.min_block_x + bounds.width_blocks as i32
        );
        assert!(
            candidate_bounds.min_z + candidate_bounds.length_blocks as i32
                <= bounds.min_block_z + bounds.length_blocks as i32
        );
        assert!(candidate_bounds.width_blocks > 0);
        assert!(candidate_bounds.length_blocks > 0);
    }

    #[test]
    fn candidate_bounds_accepts_point_line_and_polygon_coordinates() {
        let projector = BoundaryProjector::for_boundary_with_orientation(
            &square(),
            Orientation::new(0.0).unwrap(),
        )
        .unwrap();
        let point = projector
            .candidate_bounds(&serde_json::json!([116.0005, 39.0005]))
            .unwrap();
        assert_eq!((point.width_blocks, point.length_blocks), (1, 1));
        let line = projector
            .candidate_bounds(&serde_json::json!([
                [116.0002, 39.0002],
                [116.0008, 39.0002]
            ]))
            .unwrap();
        assert!(line.width_blocks >= 1);
        let malformed = projector.candidate_bounds(&serde_json::json!([]));
        assert!(matches!(
            malformed,
            Err(BoundaryFootprintError::MalformedCoordinates)
        ));
    }
}
