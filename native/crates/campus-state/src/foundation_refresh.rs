use crate::{
    AttributeProvenance, FoundationCategory, PinnedAcquisitionEvidence, ProviderOutcome,
    SourceGeometry, SourceObservation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationRefreshClassification {
    Unchanged,
    Added,
    Changed,
    Withdrawn,
    CoverageChanged,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedReviewDependencies {
    pub geometry: bool,
    pub grouping: bool,
    pub naming: bool,
    pub attribute: bool,
    pub containment: bool,
    pub licence: bool,
    pub coverage: bool,
    pub rule_version: bool,
    pub boundary: bool,
}

impl ChangedReviewDependencies {
    pub fn any(&self) -> bool {
        self.geometry
            || self.grouping
            || self.naming
            || self.attribute
            || self.containment
            || self.licence
            || self.coverage
            || self.rule_version
            || self.boundary
    }

    pub(crate) fn merge(&mut self, other: &Self) {
        self.geometry |= other.geometry;
        self.grouping |= other.grouping;
        self.naming |= other.naming;
        self.attribute |= other.attribute;
        self.containment |= other.containment;
        self.licence |= other.licence;
        self.coverage |= other.coverage;
        self.rule_version |= other.rule_version;
        self.boundary |= other.boundary;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ObservationRefreshDifference {
    pub upstream_source_record_identity: String,
    pub category: FoundationCategory,
    pub previous_observation_id: Option<String>,
    pub current_observation_id: Option<String>,
    pub previous_content_digest: Option<String>,
    pub current_content_digest: Option<String>,
    pub classification: ObservationRefreshClassification,
    pub changed_dependencies: ChangedReviewDependencies,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRefreshDifference {
    pub provider: String,
    pub category: FoundationCategory,
    pub tile_id: String,
    pub previous_digest: Option<String>,
    pub current_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryRefreshClassification {
    Unchanged,
    Expanded,
    Shrunk,
    RelationshipChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationSourceRefreshDifference {
    pub previous_bundle_id: String,
    pub current_bundle_id: String,
    pub previous_result_sha256: String,
    pub current_result_sha256: String,
    pub boundary: BoundaryRefreshClassification,
    pub observations: Vec<ObservationRefreshDifference>,
    pub coverage: Vec<CoverageRefreshDifference>,
}

impl FoundationSourceRefreshDifference {
    pub fn changed_categories(&self) -> BTreeSet<FoundationCategory> {
        self.observations
            .iter()
            .filter(|change| change.classification != ObservationRefreshClassification::Unchanged)
            .map(|change| change.category)
            .chain(self.coverage.iter().map(|change| change.category))
            .collect()
    }

    pub(crate) fn dependency_changes_for(
        &self,
        category: FoundationCategory,
    ) -> ChangedReviewDependencies {
        let mut changes = ChangedReviewDependencies::default();
        for observation in self
            .observations
            .iter()
            .filter(|change| change.category == category)
        {
            changes.merge(&observation.changed_dependencies);
            if matches!(
                observation.classification,
                ObservationRefreshClassification::Added
                    | ObservationRefreshClassification::Withdrawn
            ) {
                changes.geometry = true;
            }
        }
        if self
            .coverage
            .iter()
            .any(|coverage| coverage.category == category)
        {
            changes.coverage = true;
        }
        changes
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSubjectDependencyBasis {
    pub observation_id: String,
    pub upstream_source_record_identity: String,
    pub geometry_digest: String,
    pub grouping_digest: String,
    pub naming_digest: String,
    pub attribute_digest: String,
    pub containment_digest: String,
    pub licence_digest: String,
    pub rule_version_digest: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDependencyBasis {
    pub boundary_digest: String,
    pub coverage_digest: String,
    pub classification_rules: String,
    #[serde(default)]
    pub assembly_rules: String,
    pub conflation_rules: String,
    pub derivation_rules: String,
    pub subjects: BTreeMap<String, ReviewSubjectDependencyBasis>,
}

impl ReviewDependencyBasis {
    pub fn content_equivalent(&self, other: &Self) -> bool {
        self.boundary_digest == other.boundary_digest
            && self.coverage_digest == other.coverage_digest
            && self.classification_rules == other.classification_rules
            && self.assembly_rules == other.assembly_rules
            && self.conflation_rules == other.conflation_rules
            && self.derivation_rules == other.derivation_rules
            && self.subjects.len() == other.subjects.len()
            && self.subjects.iter().all(|(upstream_identity, subject)| {
                other.subjects.get(upstream_identity).is_some_and(|other| {
                    subject.geometry_digest == other.geometry_digest
                        && subject.grouping_digest == other.grouping_digest
                        && subject.naming_digest == other.naming_digest
                        && subject.attribute_digest == other.attribute_digest
                        && subject.containment_digest == other.containment_digest
                        && subject.licence_digest == other.licence_digest
                        && subject.rule_version_digest == other.rule_version_digest
                        && subject.content_digest == other.content_digest
                })
            })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ObservationDependencySnapshot {
    pub basis: ReviewSubjectDependencyBasis,
}

pub fn upstream_source_record_identity(observation: &SourceObservation) -> String {
    format!(
        "{:?}:{}:{}",
        observation.category, observation.lineage.provider, observation.lineage.source_record_id
    )
    .to_ascii_lowercase()
}

pub(crate) fn observation_dependency_snapshot(
    observation: &SourceObservation,
) -> ObservationDependencySnapshot {
    let upstream_source_record_identity = upstream_source_record_identity(observation);
    let grouping = observation
        .suggestions
        .iter()
        .filter(|suggestion| {
            suggestion.building_entity_id.is_some()
                || suggestion.building_role.is_some()
                || suggestion.overlap_group.is_some()
                || suggestion.kind.to_ascii_lowercase().contains("group")
                || suggestion.kind.to_ascii_lowercase().contains("overlap")
                || suggestion.kind.to_ascii_lowercase().contains("entity")
        })
        .collect::<Vec<_>>();
    let containment = observation
        .suggestions
        .iter()
        .filter(|suggestion| {
            suggestion.boundary_relationship.is_some()
                || suggestion.kind.to_ascii_lowercase().contains("contain")
                || suggestion.kind.to_ascii_lowercase().contains("boundary")
        })
        .collect::<Vec<_>>();
    let naming_attributes = observation
        .attribute_provenance
        .iter()
        .filter(|attribute| matches!(attribute, AttributeProvenance::Name { .. }))
        .collect::<Vec<_>>();
    let other_attributes = observation
        .attribute_provenance
        .iter()
        .filter(|attribute| !matches!(attribute, AttributeProvenance::Name { .. }))
        .collect::<Vec<_>>();
    let naming_properties = observation
        .original_properties
        .iter()
        .filter(|(key, _)| key.to_ascii_lowercase().contains("name"))
        .collect::<BTreeMap<_, _>>();
    let other_properties = observation
        .original_properties
        .iter()
        .filter(|(key, _)| !key.to_ascii_lowercase().contains("name"))
        .collect::<BTreeMap<_, _>>();
    let rule_versions = (
        &observation.derivation.rule_version,
        observation
            .suggestions
            .iter()
            .map(|suggestion| suggestion.rule_version.as_str())
            .collect::<Vec<_>>(),
        observation
            .attribute_provenance
            .iter()
            .map(attribute_rule_version)
            .collect::<Vec<_>>(),
    );
    let geometry_digest = stable_digest(&(
        &observation.geometry_sha256,
        &observation.derivation.source_geometry_sha256,
        &observation.derivation.review_geometry_sha256,
        &observation.geometry,
        &observation.review_geometry_proposal,
    ));
    let grouping_digest = stable_digest(&grouping);
    let naming_digest = stable_digest(&(naming_properties, naming_attributes));
    let attribute_digest = stable_digest(&(
        other_properties,
        other_attributes,
        &observation.raw_spatial_measures,
        &observation.coordinate_semantics,
        &observation.unit_semantics,
        &observation.time_semantics,
    ));
    let containment_digest = stable_digest(&containment);
    let licence_digest = stable_digest(&observation.licence);
    let rule_version_digest = stable_digest(&rule_versions);
    let content_digest = stable_digest(&(
        &geometry_digest,
        &grouping_digest,
        &naming_digest,
        &attribute_digest,
        &containment_digest,
        &licence_digest,
        &rule_version_digest,
        observation.category,
        &observation.lineage.original_classification,
    ));
    ObservationDependencySnapshot {
        basis: ReviewSubjectDependencyBasis {
            observation_id: observation.id.clone(),
            upstream_source_record_identity,
            geometry_digest,
            grouping_digest,
            naming_digest,
            attribute_digest,
            containment_digest,
            licence_digest,
            rule_version_digest,
            content_digest,
        },
    }
}

pub(crate) fn compare_foundation_evidence(
    previous: &PinnedAcquisitionEvidence,
    current: &PinnedAcquisitionEvidence,
    previous_boundary: &SourceGeometry,
    current_boundary: &SourceGeometry,
) -> FoundationSourceRefreshDifference {
    let coverage = compare_coverage(
        &previous.manifest.coverage_report.outcomes,
        &current.manifest.coverage_report.outcomes,
    );
    let coverage_changed_categories = coverage
        .iter()
        .map(|change| change.category)
        .collect::<BTreeSet<_>>();
    let previous_by_identity = previous
        .observations
        .iter()
        .map(|observation| {
            (
                upstream_source_record_identity(observation),
                (
                    observation,
                    observation_dependency_snapshot(observation).basis,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let current_by_identity = current
        .observations
        .iter()
        .map(|observation| {
            (
                upstream_source_record_identity(observation),
                (
                    observation,
                    observation_dependency_snapshot(observation).basis,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let identities = previous_by_identity
        .keys()
        .chain(current_by_identity.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let observations = identities
        .into_iter()
        .map(|upstream_source_record_identity| {
            let previous = previous_by_identity.get(&upstream_source_record_identity);
            let current = current_by_identity.get(&upstream_source_record_identity);
            let (category, classification, changed_dependencies) = match (previous, current) {
                (Some((observation, before)), Some((current_observation, after))) => {
                    let mut changes = dependency_changes(before, after);
                    if previous_boundary != current_boundary
                        && geometry_boundary_relationship(
                            &observation.review_geometry_proposal,
                            previous_boundary,
                        ) != geometry_boundary_relationship(
                            &current_observation.review_geometry_proposal,
                            current_boundary,
                        )
                    {
                        changes.boundary = true;
                        changes.containment = true;
                    }
                    let coverage_changed =
                        coverage_changed_categories.contains(&observation.category);
                    (
                        observation.category,
                        if changes.any() {
                            ObservationRefreshClassification::Changed
                        } else if coverage_changed {
                            ObservationRefreshClassification::CoverageChanged
                        } else {
                            ObservationRefreshClassification::Unchanged
                        },
                        ChangedReviewDependencies {
                            coverage: coverage_changed,
                            ..changes
                        },
                    )
                }
                (None, Some((observation, _))) => (
                    observation.category,
                    ObservationRefreshClassification::Added,
                    all_subject_dependencies_changed(),
                ),
                (Some((observation, _)), None) => (
                    observation.category,
                    ObservationRefreshClassification::Withdrawn,
                    all_subject_dependencies_changed(),
                ),
                (None, None) => unreachable!("identity came from one side"),
            };
            ObservationRefreshDifference {
                upstream_source_record_identity,
                category,
                previous_observation_id: previous.map(|(observation, _)| observation.id.clone()),
                current_observation_id: current.map(|(observation, _)| observation.id.clone()),
                previous_content_digest: previous
                    .map(|(_, dependency)| dependency.content_digest.clone()),
                current_content_digest: current
                    .map(|(_, dependency)| dependency.content_digest.clone()),
                classification,
                changed_dependencies,
            }
        })
        .collect();
    FoundationSourceRefreshDifference {
        previous_bundle_id: previous.manifest.bundle.id.clone(),
        current_bundle_id: current.manifest.bundle.id.clone(),
        previous_result_sha256: previous.manifest.result_sha256.clone(),
        current_result_sha256: current.manifest.result_sha256.clone(),
        boundary: compare_boundary(previous_boundary, current_boundary),
        coverage,
        observations,
    }
}

pub(crate) fn coverage_digest_for(
    outcomes: &[ProviderOutcome],
    category: FoundationCategory,
) -> String {
    let relevant = outcomes
        .iter()
        .filter(|outcome| outcome.category == category)
        .collect::<Vec<_>>();
    stable_digest(&relevant)
}

pub(crate) fn geometry_digest(geometry: &SourceGeometry) -> String {
    stable_digest(geometry)
}

fn dependency_changes(
    before: &ReviewSubjectDependencyBasis,
    after: &ReviewSubjectDependencyBasis,
) -> ChangedReviewDependencies {
    ChangedReviewDependencies {
        geometry: before.geometry_digest != after.geometry_digest,
        grouping: before.grouping_digest != after.grouping_digest,
        naming: before.naming_digest != after.naming_digest,
        attribute: before.attribute_digest != after.attribute_digest,
        containment: before.containment_digest != after.containment_digest,
        licence: before.licence_digest != after.licence_digest,
        rule_version: before.rule_version_digest != after.rule_version_digest,
        boundary: before.containment_digest != after.containment_digest,
        ..ChangedReviewDependencies::default()
    }
}

fn all_subject_dependencies_changed() -> ChangedReviewDependencies {
    ChangedReviewDependencies {
        geometry: true,
        grouping: true,
        naming: true,
        attribute: true,
        containment: true,
        licence: true,
        rule_version: true,
        boundary: true,
        ..ChangedReviewDependencies::default()
    }
}

fn compare_coverage(
    previous: &[ProviderOutcome],
    current: &[ProviderOutcome],
) -> Vec<CoverageRefreshDifference> {
    let key = |outcome: &ProviderOutcome| {
        (
            outcome.provider.clone(),
            outcome.category,
            outcome.tile_id.clone(),
        )
    };
    let previous = previous
        .iter()
        .map(|outcome| (key(outcome), stable_digest(outcome)))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|outcome| (key(outcome), stable_digest(outcome)))
        .collect::<BTreeMap<_, _>>();
    previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|(provider, category, tile_id)| {
            let key = (provider.clone(), category, tile_id.clone());
            let before = previous.get(&key);
            let after = current.get(&key);
            (before != after).then(|| CoverageRefreshDifference {
                provider,
                category,
                tile_id,
                previous_digest: before.cloned(),
                current_digest: after.cloned(),
            })
        })
        .collect()
}

fn compare_boundary(
    previous: &SourceGeometry,
    current: &SourceGeometry,
) -> BoundaryRefreshClassification {
    if previous == current {
        return BoundaryRefreshClassification::Unchanged;
    }
    let current_contains_previous = geometry_contains_geometry(current, previous);
    let previous_contains_current = geometry_contains_geometry(previous, current);
    if current_contains_previous && !previous_contains_current {
        BoundaryRefreshClassification::Expanded
    } else if previous_contains_current && !current_contains_previous {
        BoundaryRefreshClassification::Shrunk
    } else {
        BoundaryRefreshClassification::RelationshipChanged
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryBoundaryRelationship {
    Inside,
    Outside,
    Straddling,
}

fn geometry_boundary_relationship(
    geometry: &SourceGeometry,
    boundary: &SourceGeometry,
) -> GeometryBoundaryRelationship {
    let points = geometry.all_points();
    if points.is_empty() {
        return GeometryBoundaryRelationship::Outside;
    }
    let crosses_boundary = geometry_segments(geometry).iter().any(|candidate| {
        geometry_segments(boundary)
            .iter()
            .any(|boundary_segment| segments_intersect(*candidate, *boundary_segment))
    });
    let contains_boundary = boundary
        .all_points()
        .iter()
        .any(|point| geometry_contains_point(geometry, *point));
    if crosses_boundary || contains_boundary {
        return GeometryBoundaryRelationship::Straddling;
    }
    let inside = points
        .iter()
        .filter(|point| geometry_contains_point(boundary, **point))
        .count();
    if inside == points.len() {
        GeometryBoundaryRelationship::Inside
    } else if inside == 0 {
        GeometryBoundaryRelationship::Outside
    } else {
        GeometryBoundaryRelationship::Straddling
    }
}

fn geometry_segments(geometry: &SourceGeometry) -> Vec<([f64; 2], [f64; 2])> {
    let line_segments = |line: &[[f64; 2]]| {
        line.windows(2)
            .map(|points| (points[0], points[1]))
            .collect::<Vec<_>>()
    };
    match geometry {
        SourceGeometry::Point(_) | SourceGeometry::MultiPoint(_) => Vec::new(),
        SourceGeometry::LineString(line) => line_segments(line),
        SourceGeometry::MultiLineString(lines) | SourceGeometry::Polygon(lines) => {
            lines.iter().flat_map(|line| line_segments(line)).collect()
        }
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter())
            .flat_map(|ring| line_segments(ring))
            .collect(),
    }
}

fn segments_intersect(left: ([f64; 2], [f64; 2]), right: ([f64; 2], [f64; 2])) -> bool {
    let orientation = |start: [f64; 2], end: [f64; 2], point: [f64; 2]| {
        (end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0])
    };
    let left_start = orientation(left.0, left.1, right.0);
    let left_end = orientation(left.0, left.1, right.1);
    let right_start = orientation(right.0, right.1, left.0);
    let right_end = orientation(right.0, right.1, left.1);
    (left_start.signum() != left_end.signum() && right_start.signum() != right_end.signum())
        || point_on_segment(right.0, left.0, left.1)
        || point_on_segment(right.1, left.0, left.1)
        || point_on_segment(left.0, right.0, right.1)
        || point_on_segment(left.1, right.0, right.1)
}

fn geometry_contains_geometry(container: &SourceGeometry, candidate: &SourceGeometry) -> bool {
    let points = candidate.all_points();
    !points.is_empty()
        && points
            .iter()
            .all(|point| geometry_contains_point(container, *point))
        && !geometry_segments(candidate)
            .iter()
            .any(|candidate_segment| {
                geometry_segments(container)
                    .iter()
                    .any(|container_segment| {
                        segments_properly_intersect(*candidate_segment, *container_segment)
                    })
            })
        && !geometry_hole_points(container)
            .iter()
            .any(|hole_point| geometry_contains_point(candidate, *hole_point))
}

fn geometry_hole_points(geometry: &SourceGeometry) -> Vec<[f64; 2]> {
    match geometry {
        SourceGeometry::Polygon(rings) => rings
            .iter()
            .skip(1)
            .filter_map(|ring| ring.first().copied())
            .collect(),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter().skip(1))
            .filter_map(|ring| ring.first().copied())
            .collect(),
        _ => Vec::new(),
    }
}

fn segments_properly_intersect(left: ([f64; 2], [f64; 2]), right: ([f64; 2], [f64; 2])) -> bool {
    const EPSILON: f64 = 1e-10;
    let orientation = |start: [f64; 2], end: [f64; 2], point: [f64; 2]| {
        (end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0])
    };
    let left_start = orientation(left.0, left.1, right.0);
    let left_end = orientation(left.0, left.1, right.1);
    let right_start = orientation(right.0, right.1, left.0);
    let right_end = orientation(right.0, right.1, left.1);
    left_start * left_end < -EPSILON && right_start * right_end < -EPSILON
}

fn geometry_contains_point(geometry: &SourceGeometry, point: [f64; 2]) -> bool {
    match geometry {
        SourceGeometry::Polygon(rings) => polygon_contains_point(rings, point),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .any(|polygon| polygon_contains_point(polygon, point)),
        _ => false,
    }
}

fn polygon_contains_point(rings: &[Vec<[f64; 2]>], point: [f64; 2]) -> bool {
    let Some(exterior) = rings.first() else {
        return false;
    };
    point_in_ring(exterior, point) && rings.iter().skip(1).all(|hole| !point_in_ring(hole, point))
}

fn point_in_ring(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    for (left, right) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if point_on_segment(point, *left, *right) {
            return true;
        }
        let crosses = (left[1] > point[1]) != (right[1] > point[1])
            && point[0]
                < (right[0] - left[0]) * (point[1] - left[1]) / (right[1] - left[1]) + left[0];
        if crosses {
            inside = !inside;
        }
    }
    inside
}

fn point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    const EPSILON: f64 = 1e-10;
    let cross =
        (point[1] - start[1]) * (end[0] - start[0]) - (point[0] - start[0]) * (end[1] - start[1]);
    if cross.abs() > EPSILON {
        return false;
    }
    point[0] >= start[0].min(end[0]) - EPSILON
        && point[0] <= start[0].max(end[0]) + EPSILON
        && point[1] >= start[1].min(end[1]) - EPSILON
        && point[1] <= start[1].max(end[1]) + EPSILON
}

fn attribute_rule_version(attribute: &AttributeProvenance) -> &str {
    match attribute {
        AttributeProvenance::HeightMetres { rule_version, .. }
        | AttributeProvenance::Levels { rule_version, .. }
        | AttributeProvenance::WidthMetres { rule_version, .. }
        | AttributeProvenance::Subtype { rule_version, .. }
        | AttributeProvenance::Name { rule_version, .. } => rule_version,
    }
}

fn stable_digest<T: Serialize + ?Sized>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("domain evidence is serializable");
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}
