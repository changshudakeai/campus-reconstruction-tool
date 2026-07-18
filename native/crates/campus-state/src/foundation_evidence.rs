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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildingEntityReviewEntry {
    pub sequence: u64,
    pub subjects: Vec<String>,
    pub decision: BuildingEntityDecision,
    pub before: Vec<BuildingEntity>,
    pub after: Vec<BuildingEntity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuildingGenerationBasis {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewedBuildingEntity {
    pub id: String,
    pub display_name: String,
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
    evidence: Vec<BuildingEvidenceDescriptor>,
    name_evidence: Vec<BuildingNameEvidence>,
    duplicate_deliveries: BTreeMap<String, Vec<String>>,
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
        }
        validate_name_evidence(&name_evidence)?;
        Ok(Self {
            evidence: canonical_evidence,
            name_evidence,
            duplicate_deliveries,
            entities,
            entries: Vec::new(),
        })
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
        if matches!(decision, BuildingEntityDecision::Revoke { .. }) {
            return Err("Use revoke_last to preserve append-only reversal semantics".into());
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
        });
        self.entities = after;
        Ok(sequence)
    }

    pub fn revoke_last(&mut self) -> Result<u64, String> {
        let target = self
            .entries
            .iter()
            .rev()
            .find(|entry| !matches!(entry.decision, BuildingEntityDecision::Revoke { .. }))
            .ok_or("There is no Building Entity decision to revoke")?
            .clone();
        if self.entries.last().map(|entry| entry.sequence) != Some(target.sequence) {
            return Err("Only the latest Building Entity decision can be revoked".into());
        }
        let before = self.entities.clone();
        let after = target.before;
        let sequence = self.entries.len() as u64 + 1;
        self.entries.push(BuildingEntityReviewEntry {
            sequence,
            subjects: target.subjects,
            decision: BuildingEntityDecision::Revoke {
                target_sequence: target.sequence,
            },
            before,
            after: after.clone(),
        });
        self.entities = after;
        Ok(sequence)
    }

    pub fn reviewed_entities(
        &self,
        observations: &[SourceObservation],
        basis: BuildingGenerationBasis,
    ) -> Result<Vec<ReviewedBuildingEntity>, String> {
        if !basis.orientation_degrees.is_finite()
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
                        "Straddling Building requires retain or exclude review: {}",
                        entity.id
                    ));
                }
                let display_name = entity.display_name.clone().ok_or_else(|| {
                    format!(
                        "Building naming must be resolved after conflation: {}",
                        entity.id
                    )
                })?;
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
                            .map(|observation| observation.review_geometry_proposal.clone())
                            .ok_or_else(|| format!("Building part evidence is missing: {id}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let review_geometry = primary.review_geometry_proposal.clone();
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
                        geometry: review_geometry,
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
        let properties = serde_json::to_string(&self.original_properties)
            .map_err(|error| format!("Source properties are not canonical: {error}"))?;
        Ok(format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{properties}",
            self.lineage.provider,
            self.lineage.dataset_release,
            self.lineage.source_record_id,
            self.lineage.source_record_version,
            self.geometry_sha256
        ))
    }
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
        }
        BuildingEntityDecision::Merge {
            entity_ids,
            merged_entity_id,
            primary_observation_id,
            part_observation_ids,
        } => {
            if entity_ids.len() < 2
                || merged_entity_id.trim().is_empty()
                || entities.iter().any(|entity| {
                    entity.id == *merged_entity_id && !entity_ids.contains(&entity.id)
                })
            {
                return Err("A merge requires distinct source entities and a new stable ID".into());
            }
            let mut merged_evidence = Vec::new();
            for entity_id in entity_ids {
                let entity = entity_ref(entities, entity_id)?;
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
                merged_from: entity_ids.clone(),
                automatic_name_poi_id: None,
            });
        }
        BuildingEntityDecision::Split { entity_id, outputs } => {
            let source = entity_ref(entities, entity_id)?.clone();
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
                    merged_from: Vec::new(),
                    automatic_name_poi_id: None,
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
    if left == CampusBoundaryRelationship::Straddling
        || right == CampusBoundaryRelationship::Straddling
        || left != right
    {
        CampusBoundaryRelationship::Straddling
    } else {
        left
    }
}

fn initial_boundary_decision(relationship: CampusBoundaryRelationship) -> BuildingBoundaryDecision {
    match relationship {
        CampusBoundaryRelationship::Inside => BuildingBoundaryDecision::RetainWhole,
        CampusBoundaryRelationship::Outside => BuildingBoundaryDecision::Exclude,
        CampusBoundaryRelationship::Straddling => BuildingBoundaryDecision::Pending,
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
        | BuildingEntityDecision::AssignName { entity_id, .. } => vec![entity_id.clone()],
        BuildingEntityDecision::Merge { entity_ids, .. }
        | BuildingEntityDecision::KeepSeparate { entity_ids } => entity_ids.clone(),
        BuildingEntityDecision::Revoke { target_sequence } => {
            vec![format!("building-review-entry:{target_sequence}")]
        }
    }
}
