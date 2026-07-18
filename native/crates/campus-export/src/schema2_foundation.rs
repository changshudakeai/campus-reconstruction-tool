use campus_state::{ReviewedFoundationProjection, SourceGeometry};
use serde::Serialize;
use std::path::Path;

use crate::VoxelModel;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationManifest {
    pub project_id: String,
    pub project_revision: u64,
    pub compatibility_profile_id: String,
    pub dataset_bundle_id: String,
    pub schematic: FoundationManifestSchematic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationManifestSchematic {
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
    pub width: usize,
    pub height: usize,
    pub length: usize,
}

pub fn write_foundation_manifest(path: &Path, manifest: &FoundationManifest) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/// The compiler can see only the deterministic Reviewed Campus Model and its
/// project-owned orientation, scale, and style settings.
pub fn foundation_model_from_schema2_reviewed(
    reviewed: &ReviewedFoundationProjection,
) -> Result<VoxelModel, String> {
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
    for feature in &reviewed.selected_features {
        for polygon in polygons(&feature.review_geometry_proposal) {
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
    Ok(VoxelModel {
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

fn polygons(geometry: &SourceGeometry) -> Vec<&Vec<Vec<[f64; 2]>>> {
    match geometry {
        SourceGeometry::Polygon(rings) => vec![rings],
        SourceGeometry::MultiPolygon(polygons) => polygons.iter().collect(),
        _ => Vec::new(),
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
