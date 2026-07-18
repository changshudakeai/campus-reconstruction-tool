use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetBundle {
    pub id: String,
    pub osm_snapshot: String,
    pub overture_release: String,
    pub output_schema: String,
    pub classification_rules: String,
    pub assembly_rules: String,
    pub conflation_rules: String,
    pub derivation_rules: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOutcomeStatus {
    Complete,
    CompleteEmpty,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FoundationCategory {
    Building,
    Circulation,
    Water,
    Vegetation,
    Sports,
}

impl FoundationCategory {
    pub const ALL: [Self; 5] = [
        Self::Building,
        Self::Circulation,
        Self::Water,
        Self::Vegetation,
        Self::Sports,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceFailure {
    pub code: String,
    pub scope: String,
    pub retryable: bool,
    pub explanation: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderOutcome {
    pub provider: String,
    pub category: FoundationCategory,
    pub tile_id: String,
    pub status: ProviderOutcomeStatus,
    pub pagination_exhausted: bool,
    pub raw_count: u64,
    pub deduplicated_count: u64,
    pub relation_members_complete: bool,
    pub gaps: Vec<String>,
    #[serde(default)]
    pub failure: Option<ServiceFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageReport {
    pub outcomes: Vec<ProviderOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LicenceRecord {
    pub identifier: String,
    pub url: String,
    pub attribution: String,
    pub dataset_release: String,
    pub acquired_at: String,
    #[serde(default)]
    pub upstream_obligations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultChunk {
    pub id: String,
    pub stable_cursor: String,
    pub content_type: String,
    pub content_encoding: String,
    pub sha256: String,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultManifest {
    pub contract_version: String,
    pub bundle: DatasetBundle,
    pub coverage_report: CoverageReport,
    pub licences: Vec<LicenceRecord>,
    pub chunks: Vec<ResultChunk>,
    pub result_sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcquisitionJobState {
    Queued,
    Running,
    Partial,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcquisitionRequestIdentity {
    pub idempotency_key: String,
    pub content_sha256: String,
}

impl AcquisitionRequestIdentity {
    pub fn new(
        idempotency_key: impl Into<String>,
        content_sha256: impl Into<String>,
    ) -> Result<Self, String> {
        let idempotency_key = idempotency_key.into();
        let content_sha256 = content_sha256.into();
        let identity = Self {
            idempotency_key,
            content_sha256,
        };
        identity.validate()?;
        Ok(identity)
    }

    fn validate(&self) -> Result<(), String> {
        if self.idempotency_key.trim().is_empty() {
            return Err("Foundation acquisition requires a stable idempotency key".into());
        }
        if self.content_sha256.len() != 64
            || !self
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Foundation acquisition content identity must be a SHA-256 digest".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifiedAcquisitionChunk {
    pub descriptor: ResultChunk,
    pub canonical_ndjson: String,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FoundationAcquisitionCheckpoint {
    pub job_id: String,
    pub contract_version: String,
    pub bundle: DatasetBundle,
    pub boundary_revision: String,
    pub request_identity: AcquisitionRequestIdentity,
    pub state: AcquisitionJobState,
    pub outcomes: Vec<ProviderOutcome>,
    pub failure: Option<ServiceFailure>,
    pub retention_days: u64,
    pub manifest: Option<ResultManifest>,
    pub verified_chunks: Vec<VerifiedAcquisitionChunk>,
}

impl FoundationAcquisitionCheckpoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        job_id: impl Into<String>,
        contract_version: impl Into<String>,
        bundle: DatasetBundle,
        boundary_revision: impl Into<String>,
        request_identity: AcquisitionRequestIdentity,
        state: AcquisitionJobState,
        outcomes: Vec<ProviderOutcome>,
        failure: Option<ServiceFailure>,
        retention_days: u64,
    ) -> Result<Self, String> {
        let checkpoint = Self {
            job_id: job_id.into(),
            contract_version: contract_version.into(),
            bundle,
            boundary_revision: boundary_revision.into(),
            request_identity,
            state,
            outcomes,
            failure,
            retention_days,
            manifest: None,
            verified_chunks: Vec::new(),
        };
        checkpoint.validate_identity()?;
        validate_coverage_outcomes(&checkpoint.outcomes)?;
        Ok(checkpoint)
    }

    pub fn record_manifest(&mut self, manifest: ResultManifest) -> Result<(), String> {
        if manifest.contract_version != self.contract_version || manifest.bundle != self.bundle {
            return Err("Acquisition manifest does not match the pinned job or bundle".into());
        }
        validate_coverage_outcomes(&manifest.coverage_report.outcomes)?;
        if !FoundationCategory::ALL.into_iter().all(|category| {
            manifest
                .coverage_report
                .outcomes
                .iter()
                .any(|outcome| outcome.category == category)
        }) {
            return Err("Acquisition manifest must report all five Foundation categories".into());
        }
        if manifest.result_sha256.len() != 64
            || manifest.chunks.iter().any(|chunk| {
                chunk.id.trim().is_empty()
                    || chunk.stable_cursor.trim().is_empty()
                    || chunk.content_type != "application/x-ndjson"
                    || chunk.content_encoding != "gzip"
            })
        {
            return Err("Acquisition manifest integrity metadata is incomplete".into());
        }
        if self.verified_chunks.iter().any(|verified| {
            !manifest
                .chunks
                .iter()
                .any(|expected| expected == &verified.descriptor)
        }) {
            return Err(
                "A resumed acquisition manifest discarded a previously verified chunk".into(),
            );
        }
        self.manifest = Some(manifest);
        Ok(())
    }

    pub fn record_verified_chunk(&mut self, chunk: VerifiedAcquisitionChunk) -> Result<(), String> {
        let manifest = self
            .manifest
            .as_ref()
            .ok_or("Record the acquisition manifest before a verified chunk")?;
        if !manifest
            .chunks
            .iter()
            .any(|expected| expected == &chunk.descriptor)
        {
            return Err("Verified acquisition chunk is not declared by the pinned manifest".into());
        }
        if self
            .verified_chunks
            .iter()
            .any(|existing| existing.descriptor.id == chunk.descriptor.id)
        {
            return Err("Verified acquisition chunk was already persisted".into());
        }
        if chunk.canonical_ndjson.len() as u64 != chunk.descriptor.uncompressed_bytes {
            return Err("Verified acquisition chunk has an unexpected decoded size".into());
        }
        let decoded = chunk
            .canonical_ndjson
            .lines()
            .filter(|line| !line.is_empty())
            .map(serde_json::from_str::<SourceObservation>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Verified acquisition chunk is invalid NDJSON: {error}"))?;
        if decoded != chunk.observations {
            return Err("Verified acquisition chunk observations are not self-contained".into());
        }
        self.verified_chunks.push(chunk);
        self.verified_chunks.sort_by_key(|verified| {
            manifest
                .chunks
                .iter()
                .position(|expected| expected.id == verified.descriptor.id)
                .unwrap_or(usize::MAX)
        });
        Ok(())
    }

    pub(crate) fn validate_identity(&self) -> Result<(), String> {
        if self.job_id.trim().is_empty()
            || self.contract_version.trim().is_empty()
            || self.bundle.id.trim().is_empty()
            || self.boundary_revision.trim().is_empty()
            || self.retention_days < 30
        {
            return Err(
                "Foundation acquisition checkpoint identity or retention is invalid".into(),
            );
        }
        self.request_identity.validate()?;
        if self.state == AcquisitionJobState::Failed && self.failure.is_none() {
            return Err(
                "A failed Foundation acquisition job must retain its explicit failure".into(),
            );
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        self.validate_identity()?;
        validate_coverage_outcomes(&self.outcomes)?;
        let mut rebuilt = Self {
            job_id: self.job_id.clone(),
            contract_version: self.contract_version.clone(),
            bundle: self.bundle.clone(),
            boundary_revision: self.boundary_revision.clone(),
            request_identity: self.request_identity.clone(),
            state: self.state,
            outcomes: self.outcomes.clone(),
            failure: self.failure.clone(),
            retention_days: self.retention_days,
            manifest: None,
            verified_chunks: Vec::new(),
        };
        if let Some(manifest) = &self.manifest {
            rebuilt.record_manifest(manifest.clone())?;
        } else if !self.verified_chunks.is_empty() {
            return Err("Verified acquisition chunks require their pinned manifest".into());
        }
        for chunk in &self.verified_chunks {
            rebuilt.record_verified_chunk(chunk.clone())?;
        }
        Ok(())
    }
}

fn validate_coverage_outcomes(outcomes: &[ProviderOutcome]) -> Result<(), String> {
    let mut scopes = std::collections::BTreeSet::new();
    for outcome in outcomes {
        if outcome.provider.trim().is_empty()
            || outcome.tile_id.trim().is_empty()
            || !scopes.insert((
                outcome.provider.as_str(),
                outcome.category,
                outcome.tile_id.as_str(),
            ))
        {
            return Err(
                "Coverage contains a missing or duplicate provider/category/tile scope".into(),
            );
        }
        if matches!(
            outcome.status,
            ProviderOutcomeStatus::Complete | ProviderOutcomeStatus::CompleteEmpty
        ) && (!outcome.pagination_exhausted || !outcome.relation_members_complete)
        {
            return Err(
                "Complete coverage requires page exhaustion and complete relation membership"
                    .into(),
            );
        }
        if outcome.status == ProviderOutcomeStatus::CompleteEmpty
            && (outcome.raw_count != 0 || outcome.deduplicated_count != 0)
        {
            return Err("Complete-empty coverage must contain no observations".into());
        }
        if outcome.deduplicated_count > outcome.raw_count {
            return Err("Coverage deduplicated count exceeds its raw count".into());
        }
        if matches!(
            outcome.status,
            ProviderOutcomeStatus::Partial
                | ProviderOutcomeStatus::Failed
                | ProviderOutcomeStatus::Cancelled
        ) && outcome.failure.is_none()
        {
            return Err(
                "Incomplete coverage must retain its structured failure and recovery action".into(),
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "coordinates")]
pub enum SourceGeometry {
    Point([f64; 2]),
    MultiPoint(Vec<[f64; 2]>),
    LineString(Vec<[f64; 2]>),
    MultiLineString(Vec<Vec<[f64; 2]>>),
    Polygon(Vec<Vec<[f64; 2]>>),
    MultiPolygon(Vec<Vec<Vec<[f64; 2]>>>),
}

impl SourceGeometry {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::MultiPoint(_) => "MultiPoint",
            Self::LineString(_) => "LineString",
            Self::MultiLineString(_) => "MultiLineString",
            Self::Polygon(_) => "Polygon",
            Self::MultiPolygon(_) => "MultiPolygon",
        }
    }

    pub fn all_points(&self) -> Vec<[f64; 2]> {
        match self {
            Self::Point(point) => vec![*point],
            Self::MultiPoint(points) | Self::LineString(points) => points.clone(),
            Self::MultiLineString(lines) | Self::Polygon(lines) => {
                lines.iter().flatten().copied().collect()
            }
            Self::MultiPolygon(polygons) => polygons.iter().flatten().flatten().copied().collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompleteRelation {
    pub relation_id: String,
    pub assembly_status: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLineage {
    pub provider: String,
    pub dataset_release: String,
    pub source_record_id: String,
    pub source_record_version: String,
    pub upstream_records: Vec<String>,
    pub acquired_at: String,
    pub original_classification: String,
    #[serde(default)]
    pub relation: Option<CompleteRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinateSemantics {
    pub crs: String,
    pub axis_order: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeSemantics {
    pub dataset_release: String,
    pub acquired_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeometryDerivationRecord {
    pub rule_version: String,
    pub steps: Vec<String>,
    pub source_geometry_sha256: String,
    pub review_geometry_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcquisitionSuggestion {
    pub kind: String,
    pub rule_version: String,
    pub reason: String,
    #[serde(default)]
    pub building_entity_id: Option<String>,
    #[serde(default)]
    pub building_role: Option<BuildingEvidenceRole>,
    #[serde(default)]
    pub boundary_relationship: Option<CampusBoundaryRelationship>,
    #[serde(default)]
    pub overlap_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AttributeDerivation {
    Direct,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "attribute")]
pub enum AttributeProvenance {
    #[serde(rename = "height_m")]
    HeightMetres {
        value: f64,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: MetreUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "levels")]
    Levels {
        value: u32,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: LevelUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "width_m")]
    WidthMetres {
        value: f64,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: MetreUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "subtype")]
    Subtype {
        value: String,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: NoUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
    #[serde(rename = "name")]
    Name {
        value: String,
        source_observation_id: String,
        original_value: serde_json::Value,
        unit: NoUnit,
        derivation: AttributeDerivation,
        rule_version: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetreUnit {
    #[serde(rename = "m")]
    Metres,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LevelUnit {
    #[serde(rename = "levels")]
    Levels,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NoUnit {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceObservation {
    pub id: String,
    pub category: FoundationCategory,
    pub geometry: SourceGeometry,
    pub original_properties: BTreeMap<String, serde_json::Value>,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub coordinate_semantics: CoordinateSemantics,
    pub unit_semantics: BTreeMap<String, String>,
    pub time_semantics: TimeSemantics,
    pub geometry_sha256: String,
    pub derivation: GeometryDerivationRecord,
    pub review_geometry_proposal: SourceGeometry,
    pub raw_spatial_measures: BTreeMap<String, f64>,
    pub suggestions: Vec<AcquisitionSuggestion>,
    pub attribute_provenance: Vec<AttributeProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundaryRankingEvidence {
    pub name_match: f64,
    pub distance_m: f64,
    pub contains_anchor: bool,
    pub area_m2: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundaryCandidate {
    pub id: String,
    pub rank: u32,
    pub geometry: SourceGeometry,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub ranking_evidence: BoundaryRankingEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingEvidenceRole {
    Whole,
    Part,
    Unclassified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CampusBoundaryRelationship {
    Inside,
    Outside,
    Straddling,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEvidenceDescriptor {
    pub observation_id: String,
    pub entity_id: String,
    pub role: BuildingEvidenceRole,
    pub boundary_relationship: CampusBoundaryRelationship,
    #[serde(default)]
    pub overlap_group: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingNameEvidenceScope {
    Building,
    Campus,
    Address,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingNameEvidence {
    pub id: String,
    pub poi_id: String,
    pub name: String,
    pub scope: BuildingNameEvidenceScope,
    pub candidate_entity_ids: Vec<String>,
    pub source_observation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingBoundaryDecision {
    Pending,
    RetainWhole,
    Exclude,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingNameAssignmentMode {
    Automatic,
    Human,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildingNameResolution {
    #[default]
    Pending,
    Named,
    Unnamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEntitySplit {
    pub entity_id: String,
    pub evidence_ids: Vec<String>,
    pub primary_observation_id: String,
    pub part_observation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BuildingEntityDecision {
    SetPrimary {
        entity_id: String,
        observation_id: String,
    },
    Merge {
        entity_ids: Vec<String>,
        merged_entity_id: String,
        primary_observation_id: String,
        part_observation_ids: Vec<String>,
    },
    Split {
        entity_id: String,
        outputs: Vec<BuildingEntitySplit>,
    },
    KeepSeparate {
        entity_ids: Vec<String>,
    },
    SetBoundary {
        entity_id: String,
        decision: BuildingBoundaryDecision,
    },
    AssignName {
        entity_id: String,
        name_evidence_id: String,
        mode: BuildingNameAssignmentMode,
    },
    LeaveUnnamed {
        entity_id: String,
        reason: String,
    },
    RefreshEvidence {
        added_observation_ids: Vec<String>,
    },
    Revoke {
        target_sequence: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEntity {
    pub id: String,
    pub evidence_ids: Vec<String>,
    pub primary_observation_id: String,
    pub part_observation_ids: Vec<String>,
    pub boundary_relationship: CampusBoundaryRelationship,
    pub boundary_decision: BuildingBoundaryDecision,
    pub display_name: Option<String>,
    pub name_evidence_ids: Vec<String>,
    pub split_from: Option<String>,
    pub merged_from: Vec<String>,
    pub automatic_name_poi_id: Option<String>,
    pub unresolved_overlap_groups: Vec<String>,
    #[serde(default)]
    pub name_resolution: BuildingNameResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEntityReviewBasis {
    pub observation_geometry_sha256: BTreeMap<String, String>,
    pub overlap_groups: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEntityReviewEntry {
    pub sequence: u64,
    pub subjects: Vec<String>,
    pub decision: BuildingEntityDecision,
    pub before: Vec<BuildingEntity>,
    pub after: Vec<BuildingEntity>,
    pub basis: BuildingEntityReviewBasis,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingGenerationBasis {
    pub origin_wgs84: [f64; 2],
    pub orientation_degrees: f64,
    pub blocks_per_meter: f64,
    pub rule_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingGenerationGeometry {
    pub source_entity_id: String,
    pub geometry: SourceGeometry,
    pub part_geometries: Vec<SourceGeometry>,
    pub basis: BuildingGenerationBasis,
    pub primary_review_geometry_sha256: String,
}

impl BuildingGenerationGeometry {
    pub fn wgs84_generator_geometry(&self) -> SourceGeometry {
        restore_wgs84_geometry(&self.geometry, &self.basis)
    }

    pub fn wgs84_part_generator_geometries(&self) -> Vec<SourceGeometry> {
        self.part_geometries
            .iter()
            .map(|geometry| restore_wgs84_geometry(geometry, &self.basis))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewedBuildingEntity {
    pub id: String,
    pub display_name: Option<String>,
    pub evidence_ids: Vec<String>,
    pub primary_observation_id: String,
    pub part_observation_ids: Vec<String>,
    pub source_observations: Vec<SourceObservation>,
    pub review_geometry: SourceGeometry,
    pub generation_geometry: BuildingGenerationGeometry,
    pub split_from: Option<String>,
    pub merged_from: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BuildingEntityReviewLedger {
    #[serde(default)]
    initial_evidence: Vec<BuildingEvidenceDescriptor>,
    evidence: Vec<BuildingEvidenceDescriptor>,
    name_evidence: Vec<BuildingNameEvidence>,
    duplicate_deliveries: BTreeMap<String, Vec<String>>,
    initial_entities: Vec<BuildingEntity>,
    entities: Vec<BuildingEntity>,
    entries: Vec<BuildingEntityReviewEntry>,
}

impl BuildingEntityReviewLedger {
    pub fn new(
        observations: &[SourceObservation],
        evidence: Vec<BuildingEvidenceDescriptor>,
        name_evidence: Vec<BuildingNameEvidence>,
    ) -> Result<Self, String> {
        let observation_by_id = observations
            .iter()
            .map(|observation| (observation.id.as_str(), observation))
            .collect::<BTreeMap<_, _>>();
        let mut canonical_by_delivery = BTreeMap::<String, String>::new();
        let mut duplicate_deliveries = BTreeMap::<String, Vec<String>>::new();
        let mut canonical_evidence = Vec::new();

        for descriptor in evidence {
            if descriptor.observation_id.trim().is_empty() || descriptor.entity_id.trim().is_empty()
            {
                return Err("Building evidence requires observation and entity identities".into());
            }
            let observation = observation_by_id
                .get(descriptor.observation_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "Building evidence observation is not pinned: {}",
                        descriptor.observation_id
                    )
                })?;
            if observation.category != FoundationCategory::Building {
                return Err(format!(
                    "Building evidence has a non-building category: {}",
                    descriptor.observation_id
                ));
            }
            let delivery_identity = observation.delivery_identity()?;
            if let Some(canonical_id) = canonical_by_delivery.get(&delivery_identity) {
                duplicate_deliveries
                    .entry(canonical_id.clone())
                    .or_default()
                    .push(descriptor.observation_id);
                continue;
            }
            canonical_by_delivery.insert(delivery_identity, descriptor.observation_id.clone());
            canonical_evidence.push(descriptor);
        }

        let mut overlap_shapes = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
        for descriptor in canonical_evidence
            .iter()
            .filter(|descriptor| descriptor.role != BuildingEvidenceRole::Part)
        {
            if let Some(group) = &descriptor.overlap_group {
                let observation = observation_by_id[descriptor.observation_id.as_str()];
                overlap_shapes
                    .entry(group.clone())
                    .or_default()
                    .insert(observation.geometry_sha256.clone());
            }
        }
        let conflicting_groups = overlap_shapes
            .into_iter()
            .filter_map(|(group, shapes)| (shapes.len() > 1).then_some(group))
            .collect::<std::collections::BTreeSet<_>>();
        let mut entities = Vec::new();
        for descriptor in &canonical_evidence {
            let entity_index = entities
                .iter()
                .position(|entity: &BuildingEntity| entity.id == descriptor.entity_id);
            if let Some(index) = entity_index {
                let entity = &mut entities[index];
                entity.evidence_ids.push(descriptor.observation_id.clone());
                if descriptor.role == BuildingEvidenceRole::Part {
                    entity
                        .part_observation_ids
                        .push(descriptor.observation_id.clone());
                }
                entity.boundary_relationship = combine_boundary_relationship(
                    entity.boundary_relationship,
                    descriptor.boundary_relationship,
                );
                entity.boundary_decision = initial_boundary_decision(entity.boundary_relationship);
            } else {
                entities.push(BuildingEntity {
                    id: descriptor.entity_id.clone(),
                    evidence_ids: vec![descriptor.observation_id.clone()],
                    primary_observation_id: descriptor.observation_id.clone(),
                    part_observation_ids: if descriptor.role == BuildingEvidenceRole::Part {
                        vec![descriptor.observation_id.clone()]
                    } else {
                        Vec::new()
                    },
                    boundary_relationship: descriptor.boundary_relationship,
                    boundary_decision: initial_boundary_decision(descriptor.boundary_relationship),
                    display_name: None,
                    name_evidence_ids: Vec::new(),
                    split_from: None,
                    merged_from: Vec::new(),
                    automatic_name_poi_id: None,
                    unresolved_overlap_groups: Vec::new(),
                    name_resolution: BuildingNameResolution::Pending,
                });
            }
        }
        for entity in &mut entities {
            if let Some(whole) = entity.evidence_ids.iter().find(|observation_id| {
                canonical_evidence.iter().any(|descriptor| {
                    descriptor.observation_id == **observation_id
                        && descriptor.role == BuildingEvidenceRole::Whole
                })
            }) {
                entity.primary_observation_id = whole.clone();
            }
            entity.unresolved_overlap_groups = canonical_evidence
                .iter()
                .filter(|descriptor| descriptor.entity_id == entity.id)
                .filter_map(|descriptor| descriptor.overlap_group.as_ref())
                .filter(|group| conflicting_groups.contains(*group))
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        validate_name_evidence(&name_evidence)?;
        let initial_entities = entities.clone();
        Ok(Self {
            initial_evidence: canonical_evidence.clone(),
            evidence: canonical_evidence,
            name_evidence,
            duplicate_deliveries,
            initial_entities,
            entities,
            entries: Vec::new(),
        })
    }

    pub fn from_observations(
        observations: &[SourceObservation],
        name_evidence: Vec<BuildingNameEvidence>,
    ) -> Result<Self, String> {
        let evidence = observations
            .iter()
            .filter(|observation| observation.category == FoundationCategory::Building)
            .map(|observation| {
                let suggestion = observation.suggestions.iter().find(|suggestion| {
                    suggestion.building_entity_id.is_some()
                        || suggestion.building_role.is_some()
                        || suggestion.overlap_group.is_some()
                });
                let role = suggestion
                    .and_then(|suggestion| suggestion.building_role)
                    .unwrap_or_else(|| {
                        if observation
                            .original_properties
                            .contains_key("building:part")
                        {
                            BuildingEvidenceRole::Part
                        } else {
                            BuildingEvidenceRole::Whole
                        }
                    });
                BuildingEvidenceDescriptor {
                    observation_id: observation.id.clone(),
                    entity_id: suggestion
                        .and_then(|suggestion| suggestion.building_entity_id.clone())
                        .unwrap_or_else(|| {
                            format!(
                                "building:{}:{}",
                                observation.lineage.provider, observation.lineage.source_record_id
                            )
                        }),
                    role,
                    boundary_relationship: suggestion
                        .and_then(|suggestion| suggestion.boundary_relationship)
                        .unwrap_or(CampusBoundaryRelationship::Unknown),
                    overlap_group: suggestion
                        .and_then(|suggestion| suggestion.overlap_group.clone()),
                }
            })
            .collect();
        Self::new(observations, evidence, name_evidence)
    }

    pub fn is_empty(&self) -> bool {
        self.initial_entities.is_empty()
    }

    pub fn entities(&self) -> &[BuildingEntity] {
        &self.entities
    }

    pub fn entries(&self) -> &[BuildingEntityReviewEntry] {
        &self.entries
    }

    pub fn duplicate_deliveries(&self) -> &BTreeMap<String, Vec<String>> {
        &self.duplicate_deliveries
    }

    pub fn record(
        &mut self,
        decision: BuildingEntityDecision,
        observations: &[SourceObservation],
    ) -> Result<u64, String> {
        self.validate_against(observations)?;
        if matches!(
            decision,
            BuildingEntityDecision::Revoke { .. } | BuildingEntityDecision::RefreshEvidence { .. }
        ) {
            return Err(
                "Ledger-managed Building Entity decisions cannot be recorded directly".into(),
            );
        }
        let before = self.entities.clone();
        let mut after = before.clone();
        apply_building_decision(
            &mut after,
            &decision,
            &self.evidence,
            &self.name_evidence,
            observations,
        )?;
        let sequence = self.entries.len() as u64 + 1;
        self.entries.push(BuildingEntityReviewEntry {
            sequence,
            subjects: decision_subjects(&decision),
            decision,
            before,
            after: after.clone(),
            basis: self.review_basis(observations)?,
            recorded_at_unix_ms: now_unix_ms(),
        });
        self.entities = after;
        Ok(sequence)
    }

    pub fn refresh_from_observations(
        &mut self,
        observations: &[SourceObservation],
    ) -> Result<Option<u64>, String> {
        self.validate_against(observations)?;
        let refreshed = Self::from_observations(observations, self.name_evidence.clone())?;
        let known_ids = self
            .evidence
            .iter()
            .map(|descriptor| descriptor.observation_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let added = refreshed
            .evidence
            .iter()
            .filter(|descriptor| !known_ids.contains(descriptor.observation_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        self.duplicate_deliveries = refreshed.duplicate_deliveries;
        if added.is_empty() {
            return Ok(None);
        }
        let before = self.entities.clone();
        let mut after = before.clone();
        apply_evidence_refresh(&mut after, &added, &refreshed.evidence, observations)?;
        self.evidence.extend(added.iter().cloned());
        let sequence = self.entries.len() as u64 + 1;
        let decision = BuildingEntityDecision::RefreshEvidence {
            added_observation_ids: added
                .iter()
                .map(|descriptor| descriptor.observation_id.clone())
                .collect(),
        };
        self.entries.push(BuildingEntityReviewEntry {
            sequence,
            subjects: decision_subjects(&decision),
            decision,
            before,
            after: after.clone(),
            basis: self.review_basis(observations)?,
            recorded_at_unix_ms: now_unix_ms(),
        });
        self.entities = after;
        Ok(Some(sequence))
    }

    pub fn revoke_last(&mut self, observations: &[SourceObservation]) -> Result<u64, String> {
        let revoked = self
            .entries
            .iter()
            .filter_map(|entry| match entry.decision {
                BuildingEntityDecision::Revoke { target_sequence } => Some(target_sequence),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let target = self
            .entries
            .iter()
            .rev()
            .find(|entry| {
                !matches!(
                    entry.decision,
                    BuildingEntityDecision::Revoke { .. }
                        | BuildingEntityDecision::RefreshEvidence { .. }
                ) && !revoked.contains(&entry.sequence)
            })
            .ok_or("There is no Building Entity decision to revoke")?
            .clone();
        let (target_after_refreshes, _) = self.replay_refreshes_after(
            target.after.clone(),
            target.sequence,
            u64::MAX,
            observations,
        )?;
        if self.entities != target_after_refreshes {
            return Err("Only the latest active Building Entity decision can be revoked".into());
        }
        let before = self.entities.clone();
        let (after, _) =
            self.replay_refreshes_after(target.before, target.sequence, u64::MAX, observations)?;
        let sequence = self.entries.len() as u64 + 1;
        self.entries.push(BuildingEntityReviewEntry {
            sequence,
            subjects: target.subjects,
            decision: BuildingEntityDecision::Revoke {
                target_sequence: target.sequence,
            },
            before,
            after: after.clone(),
            basis: self.review_basis(observations)?,
            recorded_at_unix_ms: now_unix_ms(),
        });
        self.entities = after;
        Ok(sequence)
    }

    fn replay_refreshes_after(
        &self,
        mut state: Vec<BuildingEntity>,
        target_sequence: u64,
        before_sequence: u64,
        observations: &[SourceObservation],
    ) -> Result<(Vec<BuildingEntity>, Vec<BuildingEvidenceDescriptor>), String> {
        let target = self
            .entries
            .iter()
            .find(|entry| entry.sequence == target_sequence)
            .ok_or("Building Entity decision target is missing")?;
        let mut evidence = self
            .evidence
            .iter()
            .filter(|descriptor| {
                target
                    .basis
                    .observation_geometry_sha256
                    .contains_key(&descriptor.observation_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.sequence > target_sequence && entry.sequence < before_sequence)
        {
            if let BuildingEntityDecision::RefreshEvidence {
                added_observation_ids,
            } = &entry.decision
            {
                let added = added_observation_ids
                    .iter()
                    .map(|id| {
                        self.evidence
                            .iter()
                            .find(|descriptor| descriptor.observation_id == *id)
                            .cloned()
                            .ok_or_else(|| {
                                "Building Entity refresh evidence is missing".to_string()
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                evidence.extend(added.iter().cloned());
                apply_evidence_refresh(&mut state, &added, &evidence, observations)?;
            }
        }
        Ok((state, evidence))
    }

    pub fn validate_against(&self, observations: &[SourceObservation]) -> Result<(), String> {
        let initial_evidence = if self.initial_evidence.is_empty() && !self.evidence.is_empty() {
            &self.evidence
        } else {
            &self.initial_evidence
        };
        let rebuilt = Self::new(
            observations,
            initial_evidence.clone(),
            self.name_evidence.clone(),
        )?;
        if rebuilt.initial_entities != self.initial_entities {
            return Err("Project has a non-deterministic Building Entity projection".into());
        }
        let observation_by_id = observations
            .iter()
            .map(|observation| (observation.id.as_str(), observation))
            .collect::<BTreeMap<_, _>>();
        for (canonical_id, duplicates) in &self.duplicate_deliveries {
            let canonical = observation_by_id
                .get(canonical_id.as_str())
                .ok_or("Building duplicate delivery canonical evidence is missing")?;
            let canonical_identity = canonical.delivery_identity()?;
            for duplicate_id in duplicates {
                let duplicate = observation_by_id
                    .get(duplicate_id.as_str())
                    .ok_or("Building duplicate delivery evidence is missing")?;
                if duplicate.delivery_identity()? != canonical_identity {
                    return Err("Building duplicate delivery identity changed".into());
                }
            }
        }
        let canonical = Self::new(
            observations,
            self.evidence.clone(),
            self.name_evidence.clone(),
        )?;
        if canonical.evidence != self.evidence {
            return Err("Project has a non-deterministic Building Entity projection".into());
        }
        let mut replay_evidence = initial_evidence.clone();
        let mut state = self.initial_entities.clone();
        let mut active = Vec::<u64>::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.sequence != index as u64 + 1
                || entry.recorded_at_unix_ms == 0
                || entry.before != state
            {
                return Err("Project has a non-deterministic Building Entity projection".into());
            }
            match &entry.decision {
                BuildingEntityDecision::Revoke { target_sequence } => {
                    if active.pop() != Some(*target_sequence) {
                        return Err(
                            "Project has a non-deterministic Building Entity projection".into()
                        );
                    }
                    let target = self
                        .entries
                        .iter()
                        .find(|candidate| candidate.sequence == *target_sequence)
                        .ok_or("Building Entity revoke target is missing")?;
                    state = self
                        .replay_refreshes_after(
                            target.before.clone(),
                            target.sequence,
                            entry.sequence,
                            observations,
                        )?
                        .0;
                }
                BuildingEntityDecision::RefreshEvidence {
                    added_observation_ids,
                } => {
                    if added_observation_ids.is_empty()
                        || added_observation_ids.iter().any(|id| {
                            replay_evidence
                                .iter()
                                .any(|descriptor| descriptor.observation_id == *id)
                        })
                    {
                        return Err(
                            "Project has a non-deterministic Building Entity projection".into()
                        );
                    }
                    let added = added_observation_ids
                        .iter()
                        .map(|id| {
                            self.evidence
                                .iter()
                                .find(|descriptor| descriptor.observation_id == *id)
                                .cloned()
                                .ok_or_else(|| {
                                    "Building Entity refresh evidence is missing".to_string()
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut expected = state.clone();
                    let mut expanded = replay_evidence.clone();
                    expanded.extend(added.iter().cloned());
                    apply_evidence_refresh(&mut expected, &added, &expanded, observations)?;
                    replay_evidence = expanded;
                    state = expected;
                }
                decision => {
                    let mut expected = state.clone();
                    apply_building_decision(
                        &mut expected,
                        decision,
                        &replay_evidence,
                        &self.name_evidence,
                        observations,
                    )?;
                    state = expected;
                    active.push(entry.sequence);
                }
            }
            if entry.basis != review_basis_for(&replay_evidence, observations)?
                || entry.after != state
            {
                return Err("Project has a non-deterministic Building Entity projection".into());
            }
        }
        if state != self.entities || replay_evidence != self.evidence {
            return Err("Project has a non-deterministic Building Entity projection".into());
        }
        Ok(())
    }

    fn review_basis(
        &self,
        observations: &[SourceObservation],
    ) -> Result<BuildingEntityReviewBasis, String> {
        review_basis_for(&self.evidence, observations)
    }
}

fn review_basis_for(
    evidence: &[BuildingEvidenceDescriptor],
    observations: &[SourceObservation],
) -> Result<BuildingEntityReviewBasis, String> {
    let observation_by_id = observations
        .iter()
        .map(|observation| (observation.id.as_str(), observation))
        .collect::<BTreeMap<_, _>>();
    let observation_geometry_sha256 = evidence
        .iter()
        .map(|descriptor| {
            observation_by_id
                .get(descriptor.observation_id.as_str())
                .map(|observation| {
                    (
                        descriptor.observation_id.clone(),
                        observation.geometry_sha256.clone(),
                    )
                })
                .ok_or_else(|| {
                    format!(
                        "Building evidence observation is not pinned: {}",
                        descriptor.observation_id
                    )
                })
        })
        .collect::<Result<_, _>>()?;
    let mut overlap_groups = BTreeMap::<String, Vec<String>>::new();
    for descriptor in evidence {
        if let Some(group) = &descriptor.overlap_group {
            overlap_groups
                .entry(group.clone())
                .or_default()
                .push(descriptor.observation_id.clone());
        }
    }
    for ids in overlap_groups.values_mut() {
        ids.sort();
    }
    Ok(BuildingEntityReviewBasis {
        observation_geometry_sha256,
        overlap_groups,
    })
}

impl BuildingEntityReviewLedger {
    pub fn reviewed_entities(
        &self,
        observations: &[SourceObservation],
        basis: BuildingGenerationBasis,
    ) -> Result<Vec<ReviewedBuildingEntity>, String> {
        self.validate_against(observations)?;
        if !basis.origin_wgs84.iter().all(|value| value.is_finite())
            || !basis.orientation_degrees.is_finite()
            || !basis.blocks_per_meter.is_finite()
            || basis.blocks_per_meter <= 0.0
            || basis.rule_version.trim().is_empty()
        {
            return Err("Building Generation Geometry basis is invalid".into());
        }
        let observation_by_id = observations
            .iter()
            .map(|observation| (observation.id.as_str(), observation))
            .collect::<BTreeMap<_, _>>();
        self.entities
            .iter()
            .filter(|entity| entity.boundary_decision != BuildingBoundaryDecision::Exclude)
            .map(|entity| {
                if entity.boundary_decision == BuildingBoundaryDecision::Pending {
                    return Err(format!(
                        "Building with unresolved boundary relationship requires retain or exclude review: {}",
                        entity.id
                    ));
                }
                if !entity.unresolved_overlap_groups.is_empty() {
                    return Err(format!(
                        "Building geometry conflict requires an explicit primary, merge, split, or separate decision: {}",
                        entity.id
                    ));
                }
                if entity.name_resolution != BuildingNameResolution::Named {
                    return Err(format!(
                        "Building needs a resolved display name before becoming a Reviewed Building Slot: {}",
                        entity.id
                    ));
                }
                let display_name = entity.display_name.clone();
                let primary = observation_by_id
                    .get(entity.primary_observation_id.as_str())
                    .ok_or_else(|| {
                        format!(
                            "Building primary observation is missing: {}",
                            entity.primary_observation_id
                        )
                    })?;
                let source_observations = entity
                    .evidence_ids
                    .iter()
                    .map(|id| {
                        observation_by_id
                            .get(id.as_str())
                            .cloned()
                            .cloned()
                            .ok_or_else(|| format!("Building evidence is missing: {id}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let part_geometries = entity
                    .part_observation_ids
                    .iter()
                    .map(|id| {
                        observation_by_id
                            .get(id.as_str())
                            .map(|observation| {
                                derive_generation_geometry(
                                    &observation.review_geometry_proposal,
                                    &basis,
                                )
                            })
                            .ok_or_else(|| format!("Building part evidence is missing: {id}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let review_geometry = primary.review_geometry_proposal.clone();
                let generation_geometry = derive_generation_geometry(&review_geometry, &basis);
                Ok(ReviewedBuildingEntity {
                    id: entity.id.clone(),
                    display_name,
                    evidence_ids: entity.evidence_ids.clone(),
                    primary_observation_id: entity.primary_observation_id.clone(),
                    part_observation_ids: entity.part_observation_ids.clone(),
                    source_observations,
                    review_geometry: review_geometry.clone(),
                    generation_geometry: BuildingGenerationGeometry {
                        source_entity_id: entity.id.clone(),
                        geometry: generation_geometry,
                        part_geometries,
                        basis: basis.clone(),
                        primary_review_geometry_sha256: primary
                            .derivation
                            .review_geometry_sha256
                            .clone(),
                    },
                    split_from: entity.split_from.clone(),
                    merged_from: entity.merged_from.clone(),
                })
            })
            .collect()
    }
}

impl SourceObservation {
    fn delivery_identity(&self) -> Result<String, String> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| format!("Source Observation is not canonical: {error}"))?;
        value
            .as_object_mut()
            .ok_or("Source Observation did not serialize as an object")?
            .remove("id");
        normalize_observation_self_references(&mut value, &self.id);
        normalize_delivery_timestamps(&mut value);
        serde_json::to_string(&value)
            .map_err(|error| format!("Source Observation is not canonical: {error}"))
    }
}

fn normalize_delivery_timestamps(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    for section in ["lineage", "licence", "time_semantics"] {
        if let Some(object) = root
            .get_mut(section)
            .and_then(serde_json::Value::as_object_mut)
        {
            object.remove("acquired_at");
        }
    }
}

fn normalize_observation_self_references(value: &mut serde_json::Value, observation_id: &str) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key == "source_observation_id" && value.as_str() == Some(observation_id) {
                    *value = serde_json::Value::String("$self".into());
                } else {
                    normalize_observation_self_references(value, observation_id);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_observation_self_references(value, observation_id);
            }
        }
        _ => {}
    }
}

fn apply_evidence_refresh(
    entities: &mut Vec<BuildingEntity>,
    added: &[BuildingEvidenceDescriptor],
    all_evidence: &[BuildingEvidenceDescriptor],
    observations: &[SourceObservation],
) -> Result<(), String> {
    for descriptor in added {
        let matching_indices = entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                (entity.id == descriptor.entity_id
                    || entity.merged_from.contains(&descriptor.entity_id)
                    || entity.split_from.as_deref() == Some(descriptor.entity_id.as_str()))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if matching_indices.len() > 1 {
            return Err(format!(
                "Refreshed evidence has ambiguous split lineage and requires an explicit entity mapping: {}",
                descriptor.observation_id
            ));
        }
        if let Some(index) = matching_indices.first().copied() {
            let entity = &mut entities[index];
            if !entity.evidence_ids.contains(&descriptor.observation_id) {
                entity.evidence_ids.push(descriptor.observation_id.clone());
            }
            if descriptor.role == BuildingEvidenceRole::Part
                && !entity
                    .part_observation_ids
                    .contains(&descriptor.observation_id)
            {
                entity
                    .part_observation_ids
                    .push(descriptor.observation_id.clone());
            }
            let relationship = combine_boundary_relationship(
                entity.boundary_relationship,
                descriptor.boundary_relationship,
            );
            entity.boundary_relationship = relationship;
            if matches!(
                relationship,
                CampusBoundaryRelationship::Unknown | CampusBoundaryRelationship::Straddling
            ) {
                entity.boundary_decision = BuildingBoundaryDecision::Pending;
            }
        } else {
            entities.push(BuildingEntity {
                id: descriptor.entity_id.clone(),
                evidence_ids: vec![descriptor.observation_id.clone()],
                primary_observation_id: descriptor.observation_id.clone(),
                part_observation_ids: if descriptor.role == BuildingEvidenceRole::Part {
                    vec![descriptor.observation_id.clone()]
                } else {
                    Vec::new()
                },
                boundary_relationship: descriptor.boundary_relationship,
                boundary_decision: initial_boundary_decision(descriptor.boundary_relationship),
                display_name: None,
                name_evidence_ids: Vec::new(),
                split_from: None,
                merged_from: Vec::new(),
                automatic_name_poi_id: None,
                unresolved_overlap_groups: Vec::new(),
                name_resolution: BuildingNameResolution::Pending,
            });
        }
    }

    for entity in entities.iter_mut() {
        if added
            .iter()
            .any(|descriptor| descriptor.entity_id == entity.id)
        {
            if let Some(whole) = entity.evidence_ids.iter().find(|observation_id| {
                all_evidence.iter().any(|descriptor| {
                    descriptor.observation_id == **observation_id
                        && descriptor.role == BuildingEvidenceRole::Whole
                })
            }) {
                if all_evidence
                    .iter()
                    .find(|descriptor| descriptor.observation_id == entity.primary_observation_id)
                    .is_some_and(|descriptor| descriptor.role == BuildingEvidenceRole::Part)
                {
                    entity.primary_observation_id = whole.clone();
                }
            }
        }
    }

    let observation_by_id = observations
        .iter()
        .map(|observation| (observation.id.as_str(), observation))
        .collect::<BTreeMap<_, _>>();
    let mut group_shapes = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for descriptor in all_evidence
        .iter()
        .filter(|descriptor| descriptor.role != BuildingEvidenceRole::Part)
    {
        if let Some(group) = &descriptor.overlap_group {
            let observation = observation_by_id
                .get(descriptor.observation_id.as_str())
                .ok_or("Building Entity refresh observation is not pinned")?;
            group_shapes
                .entry(group.clone())
                .or_default()
                .insert(observation.geometry_sha256.clone());
        }
    }
    let reopened_groups = added
        .iter()
        .filter_map(|descriptor| descriptor.overlap_group.as_ref())
        .filter(|group| {
            group_shapes
                .get(*group)
                .is_some_and(|shapes| shapes.len() > 1)
        })
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for entity in entities.iter_mut() {
        for group in &reopened_groups {
            let participates = entity.evidence_ids.iter().any(|id| {
                all_evidence.iter().any(|descriptor| {
                    descriptor.observation_id == *id
                        && descriptor.overlap_group.as_ref() == Some(group)
                })
            });
            if participates && !entity.unresolved_overlap_groups.contains(group) {
                entity.unresolved_overlap_groups.push(group.clone());
                entity.unresolved_overlap_groups.sort();
            }
        }
    }
    validate_observation_references(entities, observations)
}

fn apply_building_decision(
    entities: &mut Vec<BuildingEntity>,
    decision: &BuildingEntityDecision,
    evidence: &[BuildingEvidenceDescriptor],
    name_evidence: &[BuildingNameEvidence],
    observations: &[SourceObservation],
) -> Result<(), String> {
    match decision {
        BuildingEntityDecision::SetPrimary {
            entity_id,
            observation_id,
        } => {
            let entity = entity_mut(entities, entity_id)?;
            if !entity.evidence_ids.contains(observation_id) {
                return Err("Primary geometry must come from retained entity evidence".into());
            }
            entity.primary_observation_id = observation_id.clone();
            let selected_groups = evidence
                .iter()
                .filter(|descriptor| descriptor.observation_id == *observation_id)
                .filter_map(|descriptor| descriptor.overlap_group.as_ref())
                .collect::<std::collections::BTreeSet<_>>();
            entity
                .unresolved_overlap_groups
                .retain(|group| !selected_groups.contains(group));
        }
        BuildingEntityDecision::Merge {
            entity_ids,
            merged_entity_id,
            primary_observation_id,
            part_observation_ids,
        } => {
            let distinct_entity_ids = entity_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if distinct_entity_ids.len() < 2
                || merged_entity_id.trim().is_empty()
                || entities.iter().any(|entity| {
                    entity.id == *merged_entity_id && !entity_ids.contains(&entity.id)
                })
            {
                return Err("A merge requires distinct source entities and a new stable ID".into());
            }
            let mut merged_evidence = Vec::new();
            let mut unresolved_counts = BTreeMap::<String, usize>::new();
            let mut merge_lineage = entity_ids.clone();
            for entity_id in entity_ids {
                let entity = entity_ref(entities, entity_id)?;
                for predecessor in &entity.merged_from {
                    if !merge_lineage.contains(predecessor) {
                        merge_lineage.push(predecessor.clone());
                    }
                }
                for group in &entity.unresolved_overlap_groups {
                    *unresolved_counts.entry(group.clone()).or_default() += 1;
                }
                for evidence_id in &entity.evidence_ids {
                    if !merged_evidence.contains(evidence_id) {
                        merged_evidence.push(evidence_id.clone());
                    }
                }
            }
            validate_primary_and_parts(
                &merged_evidence,
                primary_observation_id,
                part_observation_ids,
            )?;
            let boundary_relationship = relationship_for_evidence(&merged_evidence, evidence)?;
            entities.retain(|entity| !entity_ids.contains(&entity.id));
            entities.push(BuildingEntity {
                id: merged_entity_id.clone(),
                evidence_ids: merged_evidence,
                primary_observation_id: primary_observation_id.clone(),
                part_observation_ids: part_observation_ids.clone(),
                boundary_relationship,
                boundary_decision: initial_boundary_decision(boundary_relationship),
                display_name: None,
                name_evidence_ids: Vec::new(),
                split_from: None,
                merged_from: merge_lineage,
                automatic_name_poi_id: None,
                unresolved_overlap_groups: unresolved_counts
                    .into_iter()
                    .filter_map(|(group, count)| (count == 1).then_some(group))
                    .collect(),
                name_resolution: BuildingNameResolution::Pending,
            });
        }
        BuildingEntityDecision::Split { entity_id, outputs } => {
            let source = entity_ref(entities, entity_id)?.clone();
            let external_unresolved = entities
                .iter()
                .filter(|entity| entity.id != *entity_id)
                .flat_map(|entity| entity.unresolved_overlap_groups.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>();
            if outputs.len() < 2 {
                return Err("A Building Entity split requires at least two outputs".into());
            }
            let output_ids = outputs
                .iter()
                .map(|output| output.entity_id.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            if output_ids.len() != outputs.len()
                || outputs
                    .iter()
                    .any(|output| output.entity_id.trim().is_empty())
                || outputs.iter().any(|output| {
                    entities
                        .iter()
                        .any(|entity| entity.id != *entity_id && entity.id == output.entity_id)
                })
            {
                return Err("Split Building Entity IDs must be non-empty and unique".into());
            }
            let partition = outputs
                .iter()
                .flat_map(|output| output.evidence_ids.iter())
                .collect::<Vec<_>>();
            let unique_partition = partition
                .iter()
                .map(|id| id.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            let source_set = source
                .evidence_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if partition.len() != unique_partition.len() || unique_partition != source_set {
                return Err(
                    "A Building Entity split must partition every retained observation once".into(),
                );
            }
            for output in outputs {
                validate_primary_and_parts(
                    &output.evidence_ids,
                    &output.primary_observation_id,
                    &output.part_observation_ids,
                )?;
            }
            entities.retain(|entity| entity.id != *entity_id);
            for output in outputs {
                let boundary_relationship =
                    relationship_for_evidence(&output.evidence_ids, evidence)?;
                let unresolved_overlap_groups = source
                    .unresolved_overlap_groups
                    .iter()
                    .filter(|group| {
                        let participates = output.evidence_ids.iter().any(|id| {
                            evidence.iter().any(|descriptor| {
                                descriptor.observation_id == *id
                                    && descriptor.overlap_group.as_ref() == Some(*group)
                            })
                        });
                        participates
                            && (external_unresolved.contains(*group)
                                || overlap_group_shape_count(
                                    &output.evidence_ids,
                                    group,
                                    evidence,
                                    observations,
                                ) > 1)
                    })
                    .cloned()
                    .collect();
                entities.push(BuildingEntity {
                    id: output.entity_id.clone(),
                    evidence_ids: output.evidence_ids.clone(),
                    primary_observation_id: output.primary_observation_id.clone(),
                    part_observation_ids: output.part_observation_ids.clone(),
                    boundary_relationship,
                    boundary_decision: initial_boundary_decision(boundary_relationship),
                    display_name: None,
                    name_evidence_ids: Vec::new(),
                    split_from: Some(entity_id.clone()),
                    merged_from: source.merged_from.clone(),
                    automatic_name_poi_id: None,
                    unresolved_overlap_groups,
                    name_resolution: BuildingNameResolution::Pending,
                });
            }
        }
        BuildingEntityDecision::KeepSeparate { entity_ids } => {
            let unique = entity_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if unique.len() < 2 {
                return Err("Keep-separate review requires at least two Building Entities".into());
            }
            for entity_id in entity_ids {
                entity_ref(entities, entity_id)?;
            }
            let shared_groups = entity_ids
                .iter()
                .map(|entity_id| {
                    entity_ref(entities, entity_id).map(|entity| {
                        entity
                            .unresolved_overlap_groups
                            .iter()
                            .cloned()
                            .collect::<std::collections::BTreeSet<_>>()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .reduce(|left, right| left.intersection(&right).cloned().collect())
                .unwrap_or_default();
            if shared_groups.is_empty() {
                return Err(
                    "Keep-separate review requires a shared unresolved overlap group".into(),
                );
            }
            for entity_id in entity_ids {
                entity_mut(entities, entity_id)?
                    .unresolved_overlap_groups
                    .retain(|group| !shared_groups.contains(group));
            }
        }
        BuildingEntityDecision::SetBoundary {
            entity_id,
            decision,
        } => {
            if *decision == BuildingBoundaryDecision::Pending {
                return Err("Boundary review must retain the whole Building or exclude it".into());
            }
            let entity = entity_mut(entities, entity_id)?;
            entity.boundary_decision = *decision;
        }
        BuildingEntityDecision::AssignName {
            entity_id,
            name_evidence_id,
            mode,
        } => {
            let name = name_evidence
                .iter()
                .find(|evidence| evidence.id == *name_evidence_id)
                .ok_or("Building Name Evidence is missing")?;
            if *mode == BuildingNameAssignmentMode::Automatic {
                if name.scope != BuildingNameEvidenceScope::Building
                    || name.candidate_entity_ids.as_slice() != [entity_id.as_str()]
                {
                    return Err(
                        "Automatic naming requires exclusive building-level name evidence".into(),
                    );
                }
                if entities.iter().any(|entity| {
                    entity.id != *entity_id
                        && entity.automatic_name_poi_id.as_deref() == Some(name.poi_id.as_str())
                }) {
                    return Err(
                        "One POI ID can automatically name at most one Building Entity".into(),
                    );
                }
            }
            let entity = entity_mut(entities, entity_id)?;
            entity.display_name = Some(name.name.clone());
            if !entity.name_evidence_ids.contains(&name.id) {
                entity.name_evidence_ids.push(name.id.clone());
            }
            entity.automatic_name_poi_id =
                (*mode == BuildingNameAssignmentMode::Automatic).then(|| name.poi_id.clone());
            entity.name_resolution = BuildingNameResolution::Named;
        }
        BuildingEntityDecision::LeaveUnnamed { entity_id, reason } => {
            if reason.trim().is_empty() {
                return Err("Leaving a Building unnamed requires an explicit reason".into());
            }
            let entity = entity_mut(entities, entity_id)?;
            entity.display_name = None;
            entity.automatic_name_poi_id = None;
            entity.name_resolution = BuildingNameResolution::Unnamed;
        }
        BuildingEntityDecision::RefreshEvidence { .. } => {
            return Err("Evidence refresh is recorded only by the ledger".into());
        }
        BuildingEntityDecision::Revoke { .. } => {
            return Err("A revoke is recorded only by the ledger".into());
        }
    }
    validate_observation_references(entities, observations)
}

fn validate_primary_and_parts(
    evidence_ids: &[String],
    primary_observation_id: &str,
    part_observation_ids: &[String],
) -> Result<(), String> {
    if !evidence_ids.iter().any(|id| id == primary_observation_id)
        || part_observation_ids
            .iter()
            .any(|part_id| !evidence_ids.contains(part_id))
    {
        return Err("Primary and part geometry must remain retained entity evidence".into());
    }
    Ok(())
}

fn overlap_group_shape_count(
    evidence_ids: &[String],
    group: &str,
    evidence: &[BuildingEvidenceDescriptor],
    observations: &[SourceObservation],
) -> usize {
    evidence_ids
        .iter()
        .filter_map(|id| {
            evidence
                .iter()
                .find(|descriptor| {
                    descriptor.observation_id == *id
                        && descriptor.overlap_group.as_deref() == Some(group)
                        && descriptor.role != BuildingEvidenceRole::Part
                })
                .and_then(|descriptor| {
                    observations
                        .iter()
                        .find(|observation| observation.id == descriptor.observation_id)
                })
                .map(|observation| observation.geometry_sha256.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn validate_observation_references(
    entities: &[BuildingEntity],
    observations: &[SourceObservation],
) -> Result<(), String> {
    for entity in entities {
        validate_primary_and_parts(
            &entity.evidence_ids,
            &entity.primary_observation_id,
            &entity.part_observation_ids,
        )?;
        if entity
            .evidence_ids
            .iter()
            .any(|id| !observations.iter().any(|observation| observation.id == *id))
        {
            return Err("Building Entity refers to unpinned Source Observation".into());
        }
    }
    Ok(())
}

fn entity_ref<'a>(
    entities: &'a [BuildingEntity],
    entity_id: &str,
) -> Result<&'a BuildingEntity, String> {
    entities
        .iter()
        .find(|entity| entity.id == entity_id)
        .ok_or_else(|| format!("Building Entity is missing: {entity_id}"))
}

fn entity_mut<'a>(
    entities: &'a mut [BuildingEntity],
    entity_id: &str,
) -> Result<&'a mut BuildingEntity, String> {
    entities
        .iter_mut()
        .find(|entity| entity.id == entity_id)
        .ok_or_else(|| format!("Building Entity is missing: {entity_id}"))
}

fn relationship_for_evidence(
    evidence_ids: &[String],
    evidence: &[BuildingEvidenceDescriptor],
) -> Result<CampusBoundaryRelationship, String> {
    let mut relationship = None;
    for evidence_id in evidence_ids {
        let descriptor = evidence
            .iter()
            .find(|descriptor| descriptor.observation_id == *evidence_id)
            .ok_or_else(|| format!("Building evidence descriptor is missing: {evidence_id}"))?;
        relationship = Some(match relationship {
            Some(current) => {
                combine_boundary_relationship(current, descriptor.boundary_relationship)
            }
            None => descriptor.boundary_relationship,
        });
    }
    relationship.ok_or("Building Entity has no retained evidence".into())
}

fn combine_boundary_relationship(
    left: CampusBoundaryRelationship,
    right: CampusBoundaryRelationship,
) -> CampusBoundaryRelationship {
    match (left, right) {
        (CampusBoundaryRelationship::Straddling, _)
        | (_, CampusBoundaryRelationship::Straddling)
        | (CampusBoundaryRelationship::Inside, CampusBoundaryRelationship::Outside)
        | (CampusBoundaryRelationship::Outside, CampusBoundaryRelationship::Inside) => {
            CampusBoundaryRelationship::Straddling
        }
        (CampusBoundaryRelationship::Unknown, _) | (_, CampusBoundaryRelationship::Unknown) => {
            CampusBoundaryRelationship::Unknown
        }
        _ => left,
    }
}

fn initial_boundary_decision(relationship: CampusBoundaryRelationship) -> BuildingBoundaryDecision {
    match relationship {
        CampusBoundaryRelationship::Inside => BuildingBoundaryDecision::RetainWhole,
        CampusBoundaryRelationship::Outside => BuildingBoundaryDecision::Exclude,
        CampusBoundaryRelationship::Straddling | CampusBoundaryRelationship::Unknown => {
            BuildingBoundaryDecision::Pending
        }
    }
}

fn validate_name_evidence(evidence: &[BuildingNameEvidence]) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for item in evidence {
        if item.id.trim().is_empty()
            || item.poi_id.trim().is_empty()
            || item.name.trim().is_empty()
            || item.candidate_entity_ids.is_empty()
            || !ids.insert(item.id.as_str())
        {
            return Err("Building Name Evidence is incomplete or duplicated".into());
        }
    }
    Ok(())
}

fn decision_subjects(decision: &BuildingEntityDecision) -> Vec<String> {
    match decision {
        BuildingEntityDecision::SetPrimary { entity_id, .. }
        | BuildingEntityDecision::Split { entity_id, .. }
        | BuildingEntityDecision::SetBoundary { entity_id, .. }
        | BuildingEntityDecision::AssignName { entity_id, .. }
        | BuildingEntityDecision::LeaveUnnamed { entity_id, .. } => vec![entity_id.clone()],
        BuildingEntityDecision::Merge { entity_ids, .. }
        | BuildingEntityDecision::KeepSeparate { entity_ids } => entity_ids.clone(),
        BuildingEntityDecision::Revoke { target_sequence } => {
            vec![format!("building-review-entry:{target_sequence}")]
        }
        BuildingEntityDecision::RefreshEvidence {
            added_observation_ids,
        } => added_observation_ids.clone(),
    }
}

fn derive_generation_geometry(
    geometry: &SourceGeometry,
    basis: &BuildingGenerationBasis,
) -> SourceGeometry {
    let longitude_scale = 111_320.0 * basis.origin_wgs84[1].to_radians().cos();
    let latitude_scale = 111_320.0;
    let angle = -basis.orientation_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    map_geometry(geometry, &|point| {
        let east = (point[0] - basis.origin_wgs84[0]) * longitude_scale;
        let north = (point[1] - basis.origin_wgs84[1]) * latitude_scale;
        [
            (east * cos - north * sin) * basis.blocks_per_meter,
            (east * sin + north * cos) * basis.blocks_per_meter,
        ]
    })
}

fn restore_wgs84_geometry(
    geometry: &SourceGeometry,
    basis: &BuildingGenerationBasis,
) -> SourceGeometry {
    let longitude_scale = 111_320.0 * basis.origin_wgs84[1].to_radians().cos();
    let latitude_scale = 111_320.0;
    let angle = -basis.orientation_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    map_geometry(geometry, &|point| {
        let local_x = point[0] / basis.blocks_per_meter;
        let local_z = point[1] / basis.blocks_per_meter;
        let east = local_x * cos + local_z * sin;
        let north = -local_x * sin + local_z * cos;
        [
            basis.origin_wgs84[0] + east / longitude_scale,
            basis.origin_wgs84[1] + north / latitude_scale,
        ]
    })
}

fn map_geometry(
    geometry: &SourceGeometry,
    map_point: &impl Fn([f64; 2]) -> [f64; 2],
) -> SourceGeometry {
    let map_line = |line: &Vec<[f64; 2]>| line.iter().copied().map(map_point).collect();
    match geometry {
        SourceGeometry::Point(point) => SourceGeometry::Point(map_point(*point)),
        SourceGeometry::MultiPoint(points) => SourceGeometry::MultiPoint(map_line(points)),
        SourceGeometry::LineString(line) => SourceGeometry::LineString(map_line(line)),
        SourceGeometry::MultiLineString(lines) => {
            SourceGeometry::MultiLineString(lines.iter().map(map_line).collect())
        }
        SourceGeometry::Polygon(rings) => {
            SourceGeometry::Polygon(rings.iter().map(map_line).collect())
        }
        SourceGeometry::MultiPolygon(polygons) => SourceGeometry::MultiPolygon(
            polygons
                .iter()
                .map(|rings| rings.iter().map(map_line).collect())
                .collect(),
        ),
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
