use crate::{
    validate_boundary_geometry, AcquisitionRequestIdentity, CandidateEvidenceAssessment,
    DatasetBundle, FoundationCategory, LicenceRecord, ResultManifest, ServiceFailure,
    SourceGeometry, SourceObservation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const COARSE_RASTER_APPROXIMATE_COVERAGE_WARNING: &str =
    "Approximate coverage only; this is not a precise feature boundary.";
pub const COARSE_RASTER_PIXEL_EDGE_WARNING: &str =
    "The displayed edge may differ by at least one source pixel.";
pub const COARSE_RASTER_FINISHING_WARNING: &str = "Minecraft/Axiom finishing is still expected.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterAlgorithmProfile {
    pub algorithm_version: String,
    pub vectorization_version: String,
    pub thresholds: BTreeMap<String, f64>,
    pub minimum_component_pixels: u64,
    pub simplification_tolerance_metres: f64,
}

impl Eq for CoarseRasterAlgorithmProfile {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterGrid {
    pub crs: String,
    pub affine_transform: [f64; 6],
    pub cloud_handling: String,
    pub nodata_handling: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterSource {
    pub provider: String,
    pub dataset_version: String,
    pub observed_at: String,
    pub native_resolution_metres: f64,
    pub class_label: String,
    pub source_chunk_id: String,
    pub source_sha256: String,
    pub licence: LicenceRecord,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoarseRasterSubject {
    Water,
    Vegetation,
    LandCover,
    Building,
    Circulation,
    SportsFacility,
    IndividualTree,
    NarrowBank,
    SubResolutionDetail,
}

impl CoarseRasterSubject {
    fn is_allowed(self) -> bool {
        matches!(self, Self::Water | Self::Vegetation | Self::LandCover)
    }

    fn matches_category(self, category: FoundationCategory) -> bool {
        matches!(
            (self, category),
            (Self::Water, FoundationCategory::Water)
                | (
                    Self::Vegetation | Self::LandCover,
                    FoundationCategory::Vegetation
                )
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoarseRasterExclusionReason {
    StructuredGeometryPriority,
    OutsideBoundaryOrGap,
    Cloud,
    NoData,
    BelowThreshold,
    BelowMinimumComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterExclusion {
    pub reason: CoarseRasterExclusionReason,
    pub excluded_cell_count: u64,
    #[serde(default)]
    pub structured_observation_ids: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterClip {
    pub boundary_result_sha256: String,
    pub linked_gap_id: String,
    pub gap_tile_id: String,
    pub gap_geometry: SourceGeometry,
    pub clipped_to_boundary_and_gap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum CoarseRasterDecision {
    Unresolved,
    Accepted,
    Rejected { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterObservation {
    pub id: String,
    pub category: FoundationCategory,
    pub subject: CoarseRasterSubject,
    pub linked_gap_id: String,
    pub dataset_bundle_id: String,
    pub source: CoarseRasterSource,
    pub grid: CoarseRasterGrid,
    pub algorithm: CoarseRasterAlgorithmProfile,
    pub clip: CoarseRasterClip,
    pub pre_clip_geometry: SourceGeometry,
    pub approximate_geometry: SourceGeometry,
    pub input_cell_count: u64,
    pub retained_cell_count: u64,
    pub component_cell_counts: Vec<u64>,
    #[serde(default)]
    pub structured_conflict_observation_ids: Vec<String>,
    pub exclusions: Vec<CoarseRasterExclusion>,
    pub assessment: CandidateEvidenceAssessment,
    pub derived_sha256: String,
}

impl CoarseRasterObservation {
    pub fn warnings(&self) -> [&'static str; 3] {
        [
            COARSE_RASTER_APPROXIMATE_COVERAGE_WARNING,
            COARSE_RASTER_PIXEL_EDGE_WARNING,
            COARSE_RASTER_FINISHING_WARNING,
        ]
    }

    fn validate_intrinsic(&self) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.linked_gap_id.trim().is_empty()
            || self.dataset_bundle_id.trim().is_empty()
            || self.source.provider.trim().is_empty()
            || self.source.dataset_version.trim().is_empty()
            || self.source.observed_at.trim().is_empty()
            || self.source.class_label.trim().is_empty()
            || self.source.source_chunk_id.trim().is_empty()
            || self.source.licence.identifier.trim().is_empty()
            || self.source.licence.url.trim().is_empty()
            || self.source.licence.attribution.trim().is_empty()
            || self.source.licence.dataset_release.trim().is_empty()
            || self.source.licence.acquired_at.trim().is_empty()
        {
            return Err("Coarse raster evidence has incomplete reproducibility metadata".into());
        }
        let provider = self.source.provider.to_ascii_lowercase();
        if !provider.contains("sentinel") && !provider.contains("worldcover") {
            return Err("Coarse raster evidence must use Sentinel-2 or WorldCover".into());
        }
        if !self.subject.is_allowed() || !self.subject.matches_category(self.category) {
            return Err(
                "Coarse raster proposals are limited to large contiguous water, vegetation, or land-cover"
                    .into(),
            );
        }
        if !validate_boundary_geometry(&self.pre_clip_geometry).valid
            || !validate_boundary_geometry(&self.approximate_geometry).valid
        {
            return Err(
                "Coarse raster evidence requires non-empty, valid approximate area topology".into(),
            );
        }
        if [
            &self.assessment.geometry,
            &self.assessment.semantics,
            &self.assessment.entity_match,
            &self.assessment.name_match,
        ]
        .into_iter()
        .any(|dimension| dimension.status.trim().is_empty() || dimension.reason.trim().is_empty())
            || self.assessment.priority.trim().is_empty()
        {
            return Err(
                "Coarse raster Map Candidate requires a complete separated evidence assessment"
                    .into(),
            );
        }
        if !self.source.native_resolution_metres.is_finite()
            || self.source.native_resolution_metres < 5.0
            || self.grid.crs.trim().is_empty()
            || self.grid.cloud_handling.trim().is_empty()
            || self.grid.nodata_handling.trim().is_empty()
            || self
                .grid
                .affine_transform
                .iter()
                .any(|value| !value.is_finite())
            || (self.grid.affine_transform[0] * self.grid.affine_transform[4]
                - self.grid.affine_transform[1] * self.grid.affine_transform[3])
                .abs()
                <= f64::EPSILON
        {
            return Err(
                "Coarse raster resolution, CRS, transform, cloud, or nodata metadata is invalid"
                    .into(),
            );
        }
        if !valid_sha256(&self.source.source_sha256) || !valid_sha256(&self.derived_sha256) {
            return Err("Coarse raster source and derivation digests must be SHA-256".into());
        }
        if self.algorithm.algorithm_version.trim().is_empty()
            || self.algorithm.vectorization_version.trim().is_empty()
            || self.algorithm.thresholds.is_empty()
            || self
                .algorithm
                .thresholds
                .iter()
                .any(|(name, value)| name.trim().is_empty() || !value.is_finite())
            || self.algorithm.minimum_component_pixels == 0
            || !self.algorithm.simplification_tolerance_metres.is_finite()
            || self.algorithm.simplification_tolerance_metres < 0.0
        {
            return Err("Coarse raster algorithm parameters are incomplete or invalid".into());
        }
        let component_count = polygon_count(&self.approximate_geometry);
        if self.input_cell_count == 0
            || self.retained_cell_count > self.input_cell_count
            || self.component_cell_counts.len() != component_count
            || self
                .component_cell_counts
                .iter()
                .any(|count| *count < self.algorithm.minimum_component_pixels)
            || self.component_cell_counts.iter().sum::<u64>() != self.retained_cell_count
        {
            return Err(
                "Every coarse raster polygon must be a retained large contiguous component".into(),
            );
        }
        if self.clip.linked_gap_id != self.linked_gap_id
            || self.clip.gap_tile_id.trim().is_empty()
            || !validate_boundary_geometry(&self.clip.gap_geometry).valid
            || !geometry_within(&self.approximate_geometry, &self.clip.gap_geometry)
            || !self.clip.clipped_to_boundary_and_gap
            || !valid_sha256(&self.clip.boundary_result_sha256)
        {
            return Err(
                "Coarse raster evidence must be clipped to the boundary/gap intersection".into(),
            );
        }
        let excluded_cells = self.exclusions.iter().try_fold(0_u64, |total, exclusion| {
            if exclusion.excluded_cell_count == 0 || exclusion.explanation.trim().is_empty() {
                return Err(
                    "Coarse raster exclusions require cell counts and persisted reasons"
                        .to_string(),
                );
            }
            total
                .checked_add(exclusion.excluded_cell_count)
                .ok_or_else(|| "Coarse raster exclusion cell count overflowed".to_string())
        })?;
        if excluded_cells != self.input_cell_count - self.retained_cell_count {
            return Err("Coarse raster exclusions do not reproduce the retained cell count".into());
        }
        let conflicts = self
            .structured_conflict_observation_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if conflicts.len() != self.structured_conflict_observation_ids.len() {
            return Err("Coarse raster structured conflicts contain duplicate observations".into());
        }
        let priority_exclusions = self
            .exclusions
            .iter()
            .filter(|exclusion| {
                exclusion.reason == CoarseRasterExclusionReason::StructuredGeometryPriority
            })
            .flat_map(|exclusion| {
                exclusion
                    .structured_observation_ids
                    .iter()
                    .map(String::as_str)
            })
            .collect::<BTreeSet<_>>();
        if conflicts != priority_exclusions {
            return Err(
                "Every raster conflict must preserve structured geometry priority with a traceable exclusion"
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CoarseRasterRunOutcome {
    Proposals {
        observations: Vec<CoarseRasterObservation>,
    },
    ProviderFailure {
        failure: ServiceFailure,
    },
    UnusableCoverage {
        failure: ServiceFailure,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseRasterSupplementRun {
    pub id: String,
    pub category: FoundationCategory,
    pub linked_gap_id: String,
    pub dataset_bundle_id: String,
    pub requested_at: String,
    pub job_id: String,
    pub contract_version: String,
    pub request_identity: AcquisitionRequestIdentity,
    pub retention_days: u64,
    pub manifest: Option<ResultManifest>,
    pub outcome: CoarseRasterRunOutcome,
}

impl CoarseRasterSupplementRun {
    pub fn observations(&self) -> impl Iterator<Item = &CoarseRasterObservation> {
        match &self.outcome {
            CoarseRasterRunOutcome::Proposals { observations } => observations.iter(),
            CoarseRasterRunOutcome::ProviderFailure { .. }
            | CoarseRasterRunOutcome::UnusableCoverage { .. } => [].iter(),
        }
    }

    fn validate_intrinsic(&self) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.linked_gap_id.trim().is_empty()
            || self.dataset_bundle_id.trim().is_empty()
            || self.requested_at.trim().is_empty()
            || self.job_id.trim().is_empty()
            || self.contract_version.trim().is_empty()
            || self.request_identity.idempotency_key.trim().is_empty()
            || !valid_sha256(&self.request_identity.content_sha256)
            || self.retention_days < 30
        {
            return Err(
                "Coarse raster controlled-job identity, replay key, or retention is incomplete"
                    .into(),
            );
        }
        if let Some(manifest) = &self.manifest {
            if manifest.contract_version != self.contract_version
                || manifest.bundle.id != self.dataset_bundle_id
                || !valid_sha256(&manifest.result_sha256)
                || manifest.chunks.is_empty()
                || manifest.chunks.iter().any(|chunk| {
                    chunk.id.trim().is_empty()
                        || chunk.stable_cursor.trim().is_empty()
                        || chunk.content_type != "application/x-ndjson"
                        || chunk.content_encoding != "gzip"
                        || !valid_sha256(&chunk.sha256)
                        || chunk.uncompressed_bytes == 0
                })
            {
                return Err(
                    "Coarse raster manifest does not preserve the controlled acquisition contract"
                        .into(),
                );
            }
        }
        if !matches!(
            self.category,
            FoundationCategory::Water | FoundationCategory::Vegetation
        ) {
            return Err(
                "Coarse raster supplementation is available only for water, vegetation, or land-cover gaps"
                    .into(),
            );
        }
        match &self.outcome {
            CoarseRasterRunOutcome::Proposals { observations } => {
                if observations.is_empty() {
                    return Err("Coarse raster proposal success cannot be empty".into());
                }
                let manifest = self.manifest.as_ref().ok_or(
                    "Coarse raster proposal success requires its verified result manifest",
                )?;
                let mut ids = BTreeSet::new();
                for observation in observations {
                    observation.validate_intrinsic()?;

                    let declared_licence = manifest
                        .licences
                        .iter()
                        .any(|licence| licence == &observation.source.licence);
                    if !ids.insert(observation.id.as_str())
                        || observation.category != self.category
                        || observation.linked_gap_id != self.linked_gap_id
                        || observation.dataset_bundle_id != self.dataset_bundle_id
                        || !declared_licence
                    {
                        return Err("Coarse raster proposal is not reproduced by its manifest licence and run identity".into());
                    }
                }
            }
            CoarseRasterRunOutcome::ProviderFailure { failure }
            | CoarseRasterRunOutcome::UnusableCoverage { failure } => {
                validate_failure(failure)?;
            }
        }
        Ok(())
    }
}

pub(crate) struct CoarseRasterValidationContext<'a> {
    pub dataset_bundle: &'a DatasetBundle,
    pub contract_version: &'a str,
    pub boundary_result_sha256: &'a str,
    pub gaps: &'a BTreeMap<String, (String, Option<SourceGeometry>)>,
    pub boundary_geometry: &'a SourceGeometry,
    pub structured_observations: &'a [SourceObservation],
}

pub(crate) fn validate_new_coarse_raster_run(
    run: &CoarseRasterSupplementRun,
    existing_runs: &[CoarseRasterSupplementRun],
    context: CoarseRasterValidationContext<'_>,
) -> Result<(), String> {
    run.validate_intrinsic()?;
    if existing_runs.iter().any(|existing| existing.id == run.id) {
        return Err("Coarse raster supplement run ID is already persisted".into());
    }
    if run.dataset_bundle_id != context.dataset_bundle.id
        || run.contract_version != context.contract_version
        || run
            .manifest
            .as_ref()
            .is_some_and(|manifest| manifest.bundle != *context.dataset_bundle)
    {
        return Err("Coarse raster supplement must use the pinned Dataset Bundle".into());
    }
    if !context.gaps.contains_key(&run.linked_gap_id) {
        return Err(
            "Coarse raster supplementation requires a relevant current Known Feature Gap from the structured Coverage Report"
                .into(),
        );
    }
    let existing_observation_ids = existing_runs
        .iter()
        .flat_map(CoarseRasterSupplementRun::observations)
        .map(|observation| observation.id.as_str())
        .collect::<BTreeSet<_>>();
    for observation in run.observations() {
        if existing_observation_ids.contains(observation.id.as_str()) {
            return Err("Coarse raster observation ID is already persisted".into());
        }
        if observation.clip.boundary_result_sha256 != context.boundary_result_sha256 {
            return Err("Coarse raster clip does not match the confirmed Campus Boundary".into());
        }
        if !geometry_within(&observation.approximate_geometry, context.boundary_geometry) {
            return Err(
                "Coarse raster output geometry leaves the confirmed Campus Boundary".into(),
            );
        }
        let (gap_tile_id, structured_gap_geometry) = context
            .gaps
            .get(&run.linked_gap_id)
            .ok_or("The current Known Feature Gap scope is unavailable")?;
        let structured_gap_geometry = structured_gap_geometry.as_ref().ok_or(
            "Coarse raster supplementation requires controlled Known Feature Gap geometry",
        )?;
        if observation.clip.gap_tile_id != *gap_tile_id
            || structured_gap_geometry != &observation.clip.gap_geometry
        {
            return Err(
                "Coarse raster clip does not match the current Known Feature Gap location".into(),
            );
        }
        let actual_conflicts = context
            .structured_observations
            .iter()
            .filter(|structured| {
                geometries_intersect(&observation.pre_clip_geometry, &structured.geometry)
            })
            .map(|structured| structured.id.as_str())
            .collect::<BTreeSet<_>>();
        let declared_conflicts = observation
            .structured_conflict_observation_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if actual_conflicts != declared_conflicts {
            return Err(
                "Raster/structured intersections must be detected, declared, and excluded exactly"
                    .into(),
            );
        }
        if context.structured_observations.iter().any(|structured| {
            actual_conflicts.contains(structured.id.as_str())
                && geometries_intersect(&observation.approximate_geometry, &structured.geometry)
        }) {
            return Err("Structured geometry priority requires conflicting raster cells to be absent from the retained surface".into());
        }
        let pinned_profile = context
            .dataset_bundle
            .coarse_raster_profiles
            .get(&observation.algorithm.algorithm_version)
            .ok_or("The Dataset Bundle does not pin this coarse raster algorithm version")?;
        if pinned_profile != &observation.algorithm {
            return Err(
                "Raster thresholds must exactly match the profile pinned by the Dataset Bundle"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_persisted_coarse_raster_runs(
    runs: &[CoarseRasterSupplementRun],
) -> Result<(), String> {
    let mut run_ids = BTreeSet::new();
    let mut observation_ids = BTreeSet::new();
    let mut profiles = BTreeMap::<(&str, &str), &CoarseRasterAlgorithmProfile>::new();
    for run in runs {
        run.validate_intrinsic()?;
        if !run_ids.insert(run.id.as_str()) {
            return Err("Persisted coarse raster run IDs are not unique".into());
        }
        for observation in run.observations() {
            if !observation_ids.insert(observation.id.as_str()) {
                return Err("Persisted coarse raster observation IDs are not unique".into());
            }
            let key = (
                observation.dataset_bundle_id.as_str(),
                observation.algorithm.algorithm_version.as_str(),
            );
            if let Some(profile) = profiles.insert(key, &observation.algorithm) {
                if profile != &observation.algorithm {
                    return Err(
                        "Persisted raster thresholds changed without an explicit algorithm-version update"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn polygon_count(geometry: &SourceGeometry) -> usize {
    match geometry {
        SourceGeometry::Polygon(_) => 1,
        SourceGeometry::MultiPolygon(polygons) => polygons.len(),
        _ => 0,
    }
}

fn geometry_within(geometry: &SourceGeometry, container: &SourceGeometry) -> bool {
    let points = geometry.all_points();
    if points.is_empty()
        || !points
            .into_iter()
            .all(|point| point_in_area(point, container))
    {
        return false;
    }
    let container_boundaries = geometry_paths(container);
    let every_edge_interval_is_inside = geometry_paths(geometry).iter().all(|path| {
        path.windows(2).all(|edge| {
            let mut breakpoints = vec![0.0, 1.0];
            for boundary in &container_boundaries {
                for boundary_edge in boundary.windows(2) {
                    append_segment_intersection_parameters(
                        edge[0],
                        edge[1],
                        boundary_edge[0],
                        boundary_edge[1],
                        &mut breakpoints,
                    );
                }
            }
            breakpoints.sort_by(f64::total_cmp);
            breakpoints.dedup_by(|left, right| (*left - *right).abs() <= 1e-10);
            breakpoints.windows(2).all(|interval| {
                let t = (interval[0] + interval[1]) / 2.0;
                point_in_area(
                    [
                        edge[0][0] + (edge[1][0] - edge[0][0]) * t,
                        edge[0][1] + (edge[1][1] - edge[0][1]) * t,
                    ],
                    container,
                )
            })
        })
    });
    every_edge_interval_is_inside
        && container_hole_points(container)
            .into_iter()
            .all(|hole_point| !point_in_area(hole_point, geometry))
}

fn append_segment_intersection_parameters(
    start: [f64; 2],
    end: [f64; 2],
    boundary_start: [f64; 2],
    boundary_end: [f64; 2],
    parameters: &mut Vec<f64>,
) {
    let direction = [end[0] - start[0], end[1] - start[1]];
    let boundary_direction = [
        boundary_end[0] - boundary_start[0],
        boundary_end[1] - boundary_start[1],
    ];
    let offset = [boundary_start[0] - start[0], boundary_start[1] - start[1]];
    let cross = |left: [f64; 2], right: [f64; 2]| left[0] * right[1] - left[1] * right[0];
    let denominator = cross(direction, boundary_direction);
    if denominator.abs() > 1e-10 {
        let t = cross(offset, boundary_direction) / denominator;
        let u = cross(offset, direction) / denominator;
        if (-1e-10..=1.0 + 1e-10).contains(&t) && (-1e-10..=1.0 + 1e-10).contains(&u) {
            parameters.push(t.clamp(0.0, 1.0));
        }
        return;
    }
    if cross(offset, direction).abs() > 1e-10 {
        return;
    }
    let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
    if length_squared <= f64::EPSILON {
        return;
    }
    for point in [boundary_start, boundary_end] {
        let t = ((point[0] - start[0]) * direction[0] + (point[1] - start[1]) * direction[1])
            / length_squared;
        if (-1e-10..=1.0 + 1e-10).contains(&t) {
            parameters.push(t.clamp(0.0, 1.0));
        }
    }
}

fn container_hole_points(container: &SourceGeometry) -> Vec<[f64; 2]> {
    let polygons = match container {
        SourceGeometry::Polygon(rings) => vec![rings],
        SourceGeometry::MultiPolygon(polygons) => polygons.iter().collect(),
        _ => Vec::new(),
    };
    polygons
        .into_iter()
        .flat_map(|rings| rings.iter().skip(1))
        .filter_map(|hole| hole.first().copied())
        .collect()
}

fn geometries_intersect(left: &SourceGeometry, right: &SourceGeometry) -> bool {
    left.all_points()
        .into_iter()
        .any(|point| point_in_area(point, right))
        || right
            .all_points()
            .into_iter()
            .any(|point| point_in_area(point, left))
        || geometry_paths(left).iter().any(|left_path| {
            geometry_paths(right).iter().any(|right_path| {
                left_path.windows(2).any(|left_edge| {
                    right_path.windows(2).any(|right_edge| {
                        segments_intersect(left_edge[0], left_edge[1], right_edge[0], right_edge[1])
                    })
                })
            })
        })
}

fn geometry_paths(geometry: &SourceGeometry) -> Vec<Vec<[f64; 2]>> {
    match geometry {
        SourceGeometry::Point(point) => vec![vec![*point]],
        SourceGeometry::MultiPoint(points) | SourceGeometry::LineString(points) => {
            vec![points.clone()]
        }
        SourceGeometry::MultiLineString(lines) | SourceGeometry::Polygon(lines) => lines.clone(),
        SourceGeometry::MultiPolygon(polygons) => polygons.iter().flatten().cloned().collect(),
    }
}

fn point_in_area(point: [f64; 2], geometry: &SourceGeometry) -> bool {
    match geometry {
        SourceGeometry::Polygon(rings) => point_in_polygon(point, rings),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .any(|polygon| point_in_polygon(point, polygon)),
        _ => false,
    }
}

fn point_in_polygon(point: [f64; 2], rings: &[Vec<[f64; 2]>]) -> bool {
    let Some(outer) = rings.first() else {
        return false;
    };
    point_in_ring(point, outer) && !rings.iter().skip(1).any(|hole| point_in_ring(point, hole))
}

fn point_in_ring(point: [f64; 2], ring: &[[f64; 2]]) -> bool {
    if ring
        .windows(2)
        .any(|edge| point_on_segment(point, edge[0], edge[1]))
    {
        return true;
    }
    let mut inside = false;
    for edge in ring.windows(2) {
        let [a, b] = [edge[0], edge[1]];
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    let cross =
        (point[1] - start[1]) * (end[0] - start[0]) - (point[0] - start[0]) * (end[1] - start[1]);
    cross.abs() <= 1e-10
        && point[0] >= start[0].min(end[0]) - 1e-10
        && point[0] <= start[0].max(end[0]) + 1e-10
        && point[1] >= start[1].min(end[1]) - 1e-10
        && point[1] <= start[1].max(end[1]) + 1e-10
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    (o1 * o2 < 0.0 && o3 * o4 < 0.0)
        || (o1.abs() <= 1e-10 && point_on_segment(c, a, b))
        || (o2.abs() <= 1e-10 && point_on_segment(d, a, b))
        || (o3.abs() <= 1e-10 && point_on_segment(a, c, d))
        || (o4.abs() <= 1e-10 && point_on_segment(b, c, d))
}
fn validate_failure(failure: &ServiceFailure) -> Result<(), String> {
    if failure.code.trim().is_empty()
        || failure.scope.trim().is_empty()
        || failure.explanation.trim().is_empty()
        || failure.suggested_action.trim().is_empty()
    {
        Err("Coarse raster failure must remain an explicit retryable or terminal outcome".into())
    } else {
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_checks_every_boundary_crossing_interval() {
        let concave_container = SourceGeometry::Polygon(vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 6.2],
            [3.0, 6.2],
            [3.0, 6.0],
            [0.0, 6.0],
            [0.0, 0.0],
        ]]);
        let crossing_candidate = SourceGeometry::Polygon(vec![vec![
            [1.0, 5.0],
            [9.0, 5.0],
            [9.0, 8.0],
            [1.0, 8.0],
            [1.0, 5.0],
        ]]);

        assert!(!geometry_within(&crossing_candidate, &concave_container));
    }

    #[test]
    fn containment_rejects_a_candidate_that_covers_a_container_hole() {
        let container_with_hole = SourceGeometry::Polygon(vec![
            vec![
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 10.0],
                [0.0, 10.0],
                [0.0, 0.0],
            ],
            vec![[2.0, 4.0], [3.0, 4.0], [3.0, 6.0], [2.0, 6.0], [2.0, 4.0]],
        ]);
        let covering_candidate = SourceGeometry::Polygon(vec![vec![
            [1.0, 3.0],
            [9.0, 3.0],
            [9.0, 7.0],
            [1.0, 7.0],
            [1.0, 3.0],
        ]]);

        assert!(!geometry_within(&covering_candidate, &container_with_hole));
    }
}
