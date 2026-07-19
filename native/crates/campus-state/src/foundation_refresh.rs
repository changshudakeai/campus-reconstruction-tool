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
    pub stable_identity: String,
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
    pub stable_identity: String,
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
    pub conflation_rules: String,
    pub derivation_rules: String,
    pub subjects: BTreeMap<String, ReviewSubjectDependencyBasis>,
}

impl ReviewDependencyBasis {
    pub fn content_equivalent(&self, other: &Self) -> bool {
        self.boundary_digest == other.boundary_digest
            && self.coverage_digest == other.coverage_digest
            && self.classification_rules == other.classification_rules
            && self.conflation_rules == other.conflation_rules
            && self.derivation_rules == other.derivation_rules
            && self.subjects.len() == other.subjects.len()
            && self.subjects.iter().all(|(stable_identity, subject)| {
                other.subjects.get(stable_identity).is_some_and(|other| {
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

pub fn stable_observation_identity(observation: &SourceObservation) -> String {
    format!(
        "{:?}:{}:{}",
        observation.category, observation.lineage.provider, observation.lineage.source_record_id
    )
    .to_ascii_lowercase()
}

pub(crate) fn observation_dependency_snapshot(
    observation: &SourceObservation,
) -> ObservationDependencySnapshot {
    let stable_identity = stable_observation_identity(observation);
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
    let attribute_digest = stable_digest(&(other_attributes, &observation.raw_spatial_measures));
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
            stable_identity,
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
                stable_observation_identity(observation),
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
                stable_observation_identity(observation),
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
        .map(|stable_identity| {
            let previous = previous_by_identity.get(&stable_identity);
            let current = current_by_identity.get(&stable_identity);
            let (category, classification, changed_dependencies) = match (previous, current) {
                (Some((observation, before)), Some((_, after))) => {
                    let changes = dependency_changes(before, after);
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
                stable_identity,
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
    let previous_area = approximate_area(previous);
    let current_area = approximate_area(current);
    if current_area > previous_area {
        BoundaryRefreshClassification::Expanded
    } else if current_area < previous_area {
        BoundaryRefreshClassification::Shrunk
    } else {
        BoundaryRefreshClassification::RelationshipChanged
    }
}

fn approximate_area(geometry: &SourceGeometry) -> f64 {
    match geometry {
        SourceGeometry::Polygon(rings) => rings.first().map_or(0.0, |ring| ring_area(ring)),
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .filter_map(|polygon| polygon.first())
            .map(|ring| ring_area(ring))
            .sum(),
        _ => 0.0,
    }
}

fn ring_area(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(left, right)| left[0] * right[1] - right[0] * left[1])
        .sum::<f64>()
        .abs()
        / 2.0
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
