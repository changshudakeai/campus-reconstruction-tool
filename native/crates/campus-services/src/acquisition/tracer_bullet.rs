use crate::acquisition::{
    AcquisitionClient, AcquisitionTransport, FoundationCategory as ServiceCategory,
    ProviderOutcomeStatus, SourceGeometry,
};
use campus_export::{write_schematic, VoxelModel};
use campus_state::{
    CampusProjectLibrary, CampusScope, FoundationCategory, FoundationResumePoint,
    FoundationReviewDisposition, InstallationId, PinnedAcquisitionEvidence,
    PinnedBoundaryCandidate, PinnedBoundaryEvidence, PinnedCoverageOutcome,
    PinnedFoundationObservation, ProjectId, ReviewedFoundationProjection,
    V11ConstructionCapability,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct FixedDatasetTracerRequest<'a, T> {
    pub library_root: &'a Path,
    pub output_root: &'a Path,
    pub campus_scope: CampusScope,
    pub project_name: &'a str,
    pub actor: InstallationId,
    pub construction_capability: &'a V11ConstructionCapability,
    pub acquisition_client: &'a AcquisitionClient<T>,
}

#[derive(Debug)]
pub struct FixedDatasetTracerReport {
    pub project_id: ProjectId,
    pub resume_after_reopen: FoundationResumePoint,
    pub schematic_path: PathBuf,
    pub schematic_bytes: u64,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FoundationManifest {
    project_id: String,
    project_revision: u64,
    compatibility_profile_id: String,
    dataset_bundle_id: String,
    schematic: FoundationManifestSchematic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FoundationManifestSchematic {
    file_name: String,
    bytes: u64,
    sha256: String,
    width: usize,
    height: usize,
    length: usize,
}

fn write_foundation_manifest(path: &Path, manifest: &FoundationManifest) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/// Compiles only the durable reviewed projection. Pinned provider records and
/// candidate state are intentionally unavailable at this generation seam.
fn foundation_model_from_schema2_projection(
    projection: &ReviewedFoundationProjection,
) -> Result<VoxelModel, String> {
    if projection.boundary.len() < 4 {
        return Err("Reviewed Campus Boundary is incomplete".into());
    }
    let origin = projection.boundary[0];
    let longitude_scale = 111_320.0 * origin[1].to_radians().cos();
    let latitude_scale = 111_320.0;
    let to_local = |point: [f64; 2]| {
        (
            (point[0] - origin[0]) * longitude_scale * 0.02,
            (point[1] - origin[1]) * latitude_scale * 0.02,
        )
    };
    let boundary_local = projection
        .boundary
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
    let width = ((max_x - min_x).ceil() as usize + 1).clamp(2, 256);
    let length = ((max_z - min_z).ceil() as usize + 1).clamp(2, 256);
    let height = 4;
    let mut blocks = vec![0; width * height * length];
    for z in 0..length {
        for x in 0..width {
            blocks[z * width + x] = 1;
        }
    }
    for feature in &projection.selected_features {
        if feature.review_geometry.is_empty() {
            continue;
        }
        let local = feature
            .review_geometry
            .iter()
            .copied()
            .map(to_local)
            .collect::<Vec<_>>();
        let feature_min_x = local
            .iter()
            .map(|point| point.0)
            .fold(f64::INFINITY, f64::min);
        let feature_max_x = local
            .iter()
            .map(|point| point.0)
            .fold(f64::NEG_INFINITY, f64::max);
        let feature_min_z = local
            .iter()
            .map(|point| point.1)
            .fold(f64::INFINITY, f64::min);
        let feature_max_z = local
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let start_x = ((feature_min_x - min_x).floor().max(0.0) as usize).min(width - 1);
        let end_x = ((feature_max_x - min_x).ceil().max(0.0) as usize).min(width - 1);
        let start_z = ((feature_min_z - min_z).floor().max(0.0) as usize).min(length - 1);
        let end_z = ((feature_max_z - min_z).ceil().max(0.0) as usize).min(length - 1);
        for y in 1..height {
            for z in start_z..=end_z {
                for x in start_x..=end_x {
                    blocks[y * width * length + z * width + x] = 2;
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
            "minecraft:grass_block".into(),
            "minecraft:stone_bricks".into(),
        ],
        blocks,
    })
}

pub fn run_fixed_dataset_tracer_bullet<T: AcquisitionTransport>(
    request: FixedDatasetTracerRequest<'_, T>,
) -> Result<FixedDatasetTracerReport, String> {
    let campus_target_id = request.campus_scope.target_id().to_string();
    let mut library = CampusProjectLibrary::open_for_construction(
        request.library_root,
        campus_target_id.clone(),
        request.construction_capability,
    )?;
    let mut project = library.create_project(
        request.campus_scope,
        request.project_name,
        request.actor.clone(),
    )?;

    let boundary = request
        .acquisition_client
        .load_boundary_discovery("fixed-dataset-boundary")
        .map_err(|error| error.to_string())?;
    let selected_boundary = boundary
        .candidates
        .iter()
        .min_by_key(|candidate| candidate.rank)
        .ok_or("The fixed boundary result contains no candidate")?;
    project.confirm_boundary(
        PinnedBoundaryEvidence {
            bundle_id: boundary.manifest.bundle.id.clone(),
            result_sha256: boundary.manifest.result_sha256,
            candidate_id: selected_boundary.id.clone(),
            geometry: largest_ring(&selected_boundary.geometry)
                .ok_or("The selected fixture boundary has no polygon ring")?,
            candidates: boundary
                .candidates
                .iter()
                .map(|candidate| {
                    Ok(PinnedBoundaryCandidate {
                        id: candidate.id.clone(),
                        rank: candidate.rank,
                        geometry: largest_ring(&candidate.geometry)
                            .ok_or("A fixture boundary candidate has no polygon ring")?,
                        provider: candidate.lineage.provider.clone(),
                        dataset_release: candidate.lineage.dataset_release.clone(),
                        source_record_id: candidate.lineage.source_record_id.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        },
        request.actor.clone(),
    )?;

    let acquisition = request
        .acquisition_client
        .load_acquisition_result("fixed-dataset-acquisition")
        .map_err(|error| error.to_string())?;
    let mut coverage_gaps = BTreeMap::<FoundationCategory, Vec<String>>::new();
    let mut coverage_outcomes = Vec::new();
    for outcome in &acquisition.manifest.coverage_report.outcomes {
        let category = map_category(outcome.category);
        let gaps = coverage_gaps.entry(category).or_default();
        gaps.extend(outcome.gaps.iter().cloned());
        if !matches!(
            outcome.status,
            ProviderOutcomeStatus::Complete | ProviderOutcomeStatus::CompleteEmpty
        ) && gaps.is_empty()
        {
            gaps.push(format!(
                "{} did not complete for tile {}",
                outcome.provider, outcome.tile_id
            ));
        }
        coverage_outcomes.push(PinnedCoverageOutcome {
            provider: outcome.provider.clone(),
            category,
            tile_id: outcome.tile_id.clone(),
            status: match outcome.status {
                ProviderOutcomeStatus::Complete => "complete",
                ProviderOutcomeStatus::CompleteEmpty => "complete-empty",
                ProviderOutcomeStatus::Partial => "partial",
                ProviderOutcomeStatus::Failed => "failed",
                ProviderOutcomeStatus::Cancelled => "cancelled",
            }
            .into(),
            gaps: outcome.gaps.clone(),
        });
    }
    let observations = acquisition
        .observations
        .iter()
        .map(|observation| {
            Ok(PinnedFoundationObservation {
                id: observation.id.clone(),
                category: map_category(observation.category),
                review_geometry: largest_ring(&observation.review_geometry_proposal)
                    .unwrap_or_default(),
                source_record_id: observation.lineage.source_record_id.clone(),
                dataset_release: observation.lineage.dataset_release.clone(),
                geometry_sha256: observation.geometry_sha256.clone(),
                licence_identifier: observation.licence.identifier.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    project.pin_acquisition(
        PinnedAcquisitionEvidence {
            bundle_id: acquisition.manifest.bundle.id,
            result_sha256: acquisition.manifest.result_sha256,
            observation_count: observations.len(),
            observations,
            coverage_outcomes,
            coverage_gaps,
        },
        request.actor.clone(),
    )?;
    library.save_project(&project)?;

    // Exercise the real close/reopen boundary before any review decision. The
    // reopened project must resume from its pinned state without using the client.
    let project_id = project.id().clone();
    drop(project);
    drop(library);
    let mut library = CampusProjectLibrary::open_for_construction(
        request.library_root,
        campus_target_id,
        request.construction_capability,
    )?;
    let mut project = library.open_project(&project_id)?;
    let resume_after_reopen = project.resume_point();
    if resume_after_reopen != FoundationResumePoint::Review(FoundationCategory::Building) {
        return Err(
            "Reopened tracer project did not resume at the earliest incomplete review".into(),
        );
    }

    for category in FoundationCategory::ALL {
        let evidence = project
            .pinned_evidence()
            .ok_or("Reopened project lost pinned evidence")?;
        let evidence_ids = evidence
            .acquisition
            .observations
            .iter()
            .filter(|observation| observation.category == category)
            .map(|observation| observation.id.clone())
            .collect::<Vec<_>>();
        let disposition = if !evidence_ids.is_empty() {
            FoundationReviewDisposition::SelectedEvidence { evidence_ids }
        } else {
            let reasons = evidence
                .acquisition
                .coverage_gaps
                .get(&category)
                .cloned()
                .unwrap_or_default();
            if reasons.is_empty() {
                FoundationReviewDisposition::CompleteEmpty
            } else {
                FoundationReviewDisposition::KnownGap { reasons }
            }
        };
        project.complete_foundation_review(category, disposition, request.actor.clone())?;
        library.save_project(&project)?;
    }

    let model = foundation_model_from_schema2_projection(&project.reviewed_projection()?)?;
    let non_air_blocks = model.blocks.iter().filter(|block| **block != 0).count();
    project.record_generation(
        model.width,
        model.height,
        model.length,
        non_air_blocks,
        request.actor,
    )?;
    std::fs::create_dir_all(request.output_root).map_err(|error| error.to_string())?;
    let schematic_path = request
        .output_root
        .join(format!("{}.schem", project.id().as_str()));
    write_schematic(&schematic_path, project.name(), &model)?;
    let schematic_bytes = std::fs::metadata(&schematic_path)
        .map_err(|error| error.to_string())?
        .len();
    let schematic_sha256 = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&schematic_path).map_err(|error| error.to_string())?)
    );
    let manifest_path = request.output_root.join(format!(
        "{}.foundation-manifest.json",
        project.id().as_str()
    ));
    let manifest = FoundationManifest {
        project_id: project.id().as_str().into(),
        project_revision: project.workflow().project_revision(),
        compatibility_profile_id: project.compatibility_profile().profile_id().into(),
        dataset_bundle_id: project
            .pinned_evidence()
            .ok_or("Pinned evidence disappeared before export")?
            .acquisition
            .bundle_id
            .clone(),
        schematic: FoundationManifestSchematic {
            file_name: schematic_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Schematic output has no valid file name")?
                .into(),
            bytes: schematic_bytes,
            sha256: schematic_sha256.clone(),
            width: model.width,
            height: model.height,
            length: model.length,
        },
    };
    write_foundation_manifest(&manifest_path, &manifest)?;
    project.record_export(
        schematic_sha256,
        schematic_bytes,
        manifest_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Foundation Manifest output has no valid file name")?
            .into(),
    )?;
    library.save_project(&project)?;

    Ok(FixedDatasetTracerReport {
        project_id,
        resume_after_reopen,
        schematic_path,
        schematic_bytes,
        manifest_path,
    })
}

fn map_category(category: ServiceCategory) -> FoundationCategory {
    match category {
        ServiceCategory::Building => FoundationCategory::Building,
        ServiceCategory::Circulation => FoundationCategory::Circulation,
        ServiceCategory::Water => FoundationCategory::Water,
        ServiceCategory::Vegetation => FoundationCategory::Vegetation,
        ServiceCategory::Sports => FoundationCategory::Sports,
    }
}

fn largest_ring(geometry: &SourceGeometry) -> Option<Vec<[f64; 2]>> {
    match geometry {
        SourceGeometry::Polygon(rings) => rings.iter().max_by_key(|ring| ring.len()).cloned(),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter())
            .max_by_key(|ring| ring.len())
            .cloned(),
        _ => None,
    }
}
