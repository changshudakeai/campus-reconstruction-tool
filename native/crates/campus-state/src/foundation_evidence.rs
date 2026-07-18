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
        if chunk.canonical_ndjson.as_bytes().len() as u64 != chunk.descriptor.uncompressed_bytes {
            return Err("Verified acquisition chunk has an unexpected decoded size".into());
        }
        let decoded = chunk
            .canonical_ndjson
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str::<SourceObservation>(line))
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
