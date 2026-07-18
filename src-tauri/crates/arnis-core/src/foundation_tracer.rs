use campus_state::{ReviewedFoundationProjection, SourceGeometry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundationVoxelModel {
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub palette: Vec<String>,
    pub blocks: Vec<u16>,
}

/// Generates Foundation voxels from the deterministic Reviewed Campus Model.
/// Source snapshots, review UI state, and export concerns are unavailable here.
pub fn generate_foundation(
    reviewed: &ReviewedFoundationProjection,
) -> Result<FoundationVoxelModel, String> {
    let boundary_points = reviewed.boundary.all_points();
    if boundary_points.len() < 4 {
        return Err("Reviewed Campus Boundary is incomplete".into());
    }
    let settings = &reviewed.generation_settings;
    if !settings.orientation_degrees.is_finite()
        || !settings.blocks_per_meter.is_finite()
        || settings.blocks_per_meter <= 0.0
        || settings.surface_block.trim().is_empty()
        || settings.building_block.trim().is_empty()
    {
        return Err("Reviewed Campus Model has invalid generation settings".into());
    }
    let origin = boundary_points[0];
    let longitude_scale = 111_320.0 * origin[1].to_radians().cos();
    let latitude_scale = 111_320.0;
    let angle = -settings.orientation_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    let to_local = |point: [f64; 2]| {
        let east = (point[0] - origin[0]) * longitude_scale * settings.blocks_per_meter;
        let north = (point[1] - origin[1]) * latitude_scale * settings.blocks_per_meter;
        (east * cos - north * sin, east * sin + north * cos)
    };
    let boundary_local = boundary_points
        .iter()
        .copied()
        .map(to_local)
        .collect::<Vec<_>>();
    let min_x = boundary_local
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let max_x = boundary_local
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_z = boundary_local
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_z = boundary_local
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    if ![min_x, max_x, min_z, max_z]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err("Reviewed projection contains invalid coordinates".into());
    }
    let width = ((max_x - min_x).ceil() as usize + 1).clamp(2, 2048);
    let length = ((max_z - min_z).ceil() as usize + 1).clamp(2, 2048);
    let height = 4;
    let mut blocks = vec![0; width * height * length];
    for z in 0..length {
        for x in 0..width {
            blocks[z * width + x] = 1;
        }
    }
    let bounds = (min_x, min_z, width, length, height);
    for feature in &reviewed.selected_features {
        match &feature.review_geometry_proposal {
            SourceGeometry::Point(point) => {
                let (x, z) = to_local(*point);
                set_feature(&mut blocks, bounds, x, z, 2);
            }
            SourceGeometry::MultiPoint(points) => {
                for point in points {
                    let (x, z) = to_local(*point);
                    set_feature(&mut blocks, bounds, x, z, 2);
                }
            }
            SourceGeometry::LineString(line) => {
                render_lines(std::slice::from_ref(line), &to_local, bounds, &mut blocks);
            }
            SourceGeometry::MultiLineString(lines) => {
                render_lines(lines, &to_local, bounds, &mut blocks);
            }
            SourceGeometry::Polygon(rings) => {
                render_polygons(std::slice::from_ref(rings), &to_local, bounds, &mut blocks);
            }
            SourceGeometry::MultiPolygon(polygons) => {
                render_polygons(polygons, &to_local, bounds, &mut blocks);
            }
        }
    }
    Ok(FoundationVoxelModel {
        width,
        height,
        length,
        palette: vec![
            "minecraft:air".into(),
            settings.surface_block.clone(),
            settings.building_block.clone(),
        ],
        blocks,
    })
}

fn render_lines(
    lines: &[Vec<[f64; 2]>],
    to_local: &impl Fn([f64; 2]) -> (f64, f64),
    bounds: (f64, f64, usize, usize, usize),
    blocks: &mut [u16],
) {
    for line in lines {
        for segment in line.windows(2) {
            let start = to_local(segment[0]);
            let end = to_local(segment[1]);
            let steps = (end.0 - start.0)
                .abs()
                .max((end.1 - start.1).abs())
                .ceil()
                .max(1.0) as usize;
            for step in 0..=steps {
                let progress = step as f64 / steps as f64;
                set_feature(
                    blocks,
                    bounds,
                    start.0 + (end.0 - start.0) * progress,
                    start.1 + (end.1 - start.1) * progress,
                    2,
                );
            }
        }
    }
}

fn set_feature(
    blocks: &mut [u16],
    bounds: (f64, f64, usize, usize, usize),
    x: f64,
    z: f64,
    feature_height: usize,
) {
    let (min_x, min_z, width, length, height) = bounds;
    let x = ((x - min_x).round() as isize).clamp(0, width as isize - 1) as usize;
    let z = ((z - min_z).round() as isize).clamp(0, length as isize - 1) as usize;
    for y in 1..feature_height.min(height) {
        blocks[y * width * length + z * width + x] = 2;
    }
}

fn render_polygons(
    polygons: &[Vec<Vec<[f64; 2]>>],
    to_local: &impl Fn([f64; 2]) -> (f64, f64),
    bounds: (f64, f64, usize, usize, usize),
    blocks: &mut [u16],
) {
    let (min_x, min_z, width, length, height) = bounds;
    for polygon in polygons {
        let rings = polygon
            .iter()
            .map(|ring| ring.iter().copied().map(to_local).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let Some(outer) = rings.first() else {
            continue;
        };
        for z in 0..length {
            for x in 0..width {
                let point = (x as f64 + min_x + 0.5, z as f64 + min_z + 0.5);
                if point_in_ring(point, outer)
                    && !rings[1..].iter().any(|hole| point_in_ring(point, hole))
                {
                    for y in 1..height {
                        blocks[y * width * length + z * width + x] = 2;
                    }
                }
            }
        }
    }
}

fn point_in_ring(point: (f64, f64), ring: &[(f64, f64)]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = ring.len() - 1;
    for current in 0..ring.len() {
        let (x1, z1) = ring[current];
        let (x2, z2) = ring[previous];
        if ((z1 > point.1) != (z2 > point.1))
            && point.0 < (x2 - x1) * (point.1 - z1) / (z2 - z1) + x1
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}
