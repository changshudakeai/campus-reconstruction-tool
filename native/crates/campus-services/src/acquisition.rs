use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

#[cfg(debug_assertions)]
use base64::Engine;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT_VERSION: &str = "1.0.0";

#[path = "project_acquisition.rs"]
pub mod project_acquisition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMethod {
    Get,
    Post,
}

#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub method: TransportMethod,
    pub path: String,
    pub body: Option<Vec<u8>>,
}

impl TransportRequest {
    fn get(path: impl Into<String>) -> Self {
        Self {
            method: TransportMethod::Get,
            path: path.into(),
            body: None,
        }
    }

    fn post(path: impl Into<String>, body: Option<Vec<u8>>) -> Self {
        Self {
            method: TransportMethod::Post,
            path: path.into(),
            body,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransportResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    pub explanation: String,
}

pub trait AcquisitionTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError>;
}

pub struct HttpsTransport {
    client: Client,
    base_url: String,
    installation_credential: String,
}

impl std::fmt::Debug for HttpsTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpsTransport")
            .field("base_url", &self.base_url)
            .field("installation_credential", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl HttpsTransport {
    pub fn new(
        base_url: impl Into<String>,
        installation_credential: impl Into<String>,
    ) -> Result<Self, TransportError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let parsed = reqwest::Url::parse(&base_url).map_err(|error| TransportError {
            explanation: format!("invalid acquisition service URL: {error}"),
        })?;
        let developer_local_http = cfg!(debug_assertions)
            && parsed.scheme() == "http"
            && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
        if parsed.scheme() != "https" && !developer_local_http {
            return Err(TransportError {
                explanation: "the acquisition service requires validated HTTPS".into(),
            });
        }
        let credential = installation_credential.into();
        if credential.trim().is_empty() {
            return Err(TransportError {
                explanation: "an installation-scoped acquisition credential is required".into(),
            });
        }
        Ok(Self {
            client: Client::builder().build().map_err(|error| TransportError {
                explanation: format!("could not create acquisition HTTPS client: {error}"),
            })?,
            base_url,
            installation_credential: credential,
        })
    }
}

impl AcquisitionTransport for HttpsTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        let url = format!("{}{}", self.base_url, request.path);
        let builder = match request.method {
            TransportMethod::Get => self.client.get(url),
            TransportMethod::Post => self.client.post(url),
        }
        .bearer_auth(&self.installation_credential)
        .header(ACCEPT, "application/json, application/gzip");
        let builder = match request.body {
            Some(body) => builder.header(CONTENT_TYPE, "application/json").body(body),
            None => builder,
        };
        let response = builder.send().map_err(|error| TransportError {
            explanation: format!("acquisition service request failed: {error}"),
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
            })
            .collect();
        let json_response = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.starts_with("application/json")
                    || value.starts_with("application/problem+json")
            });
        let body = response.bytes().map_err(|error| TransportError {
            explanation: format!("could not read acquisition service response: {error}"),
        })?;
        let body = if json_response {
            redact_secret(&body, self.installation_credential.as_bytes())
        } else {
            body.to_vec()
        };
        Ok(TransportResponse {
            status,
            headers,
            body,
        })
    }
}

fn redact_secret(body: &[u8], secret: &[u8]) -> Vec<u8> {
    if secret.is_empty() || body.len() < secret.len() {
        return body.to_vec();
    }
    let mut redacted = Vec::with_capacity(body.len());
    let mut offset = 0;
    while offset < body.len() {
        if body[offset..].starts_with(secret) {
            redacted.extend_from_slice(b"[REDACTED]");
            offset += secret.len();
        } else {
            redacted.push(body[offset]);
            offset += 1;
        }
    }
    redacted
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceLimits {
    pub area_square_metres: u64,
    pub boundary_vertices: u64,
    pub tiles: u64,
    pub observations: u64,
    pub result_bytes: u64,
    pub concurrent_jobs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AcquisitionCapabilities {
    pub contract_versions: Vec<String>,
    pub supported_bundles: Vec<DatasetBundle>,
    pub limits: ServiceLimits,
    pub retention_days: u64,
    #[serde(default)]
    pub quota_remaining: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct CampusBoundaryCandidateQuery {
    name: String,
    aliases: Vec<String>,
    anchor_wgs84: [f64; 2],
    search_radius_m: f64,
    idempotency_key: String,
}

impl CampusBoundaryCandidateQuery {
    pub fn new(
        name: impl Into<String>,
        aliases: Vec<String>,
        anchor_wgs84: [f64; 2],
        search_radius_m: f64,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        let name = name.into();
        let idempotency_key = idempotency_key.into();
        if name.trim().is_empty() {
            return Err("A confirmed Campus Target name is required.".into());
        }
        if aliases.iter().any(|alias| alias.trim().is_empty()) {
            return Err("Campus Target aliases cannot be empty.".into());
        }
        if !anchor_wgs84.iter().all(|coordinate| coordinate.is_finite())
            || !(-180.0..=180.0).contains(&anchor_wgs84[0])
            || !(-90.0..=90.0).contains(&anchor_wgs84[1])
        {
            return Err(
                "The Campus Target anchor must be a valid WGS-84 longitude/latitude.".into(),
            );
        }
        if !search_radius_m.is_finite() || search_radius_m <= 0.0 {
            return Err("The boundary search radius must be a positive finite distance.".into());
        }
        if idempotency_key.trim().is_empty() {
            return Err("Boundary discovery requires a stable idempotency key.".into());
        }
        Ok(Self {
            name,
            aliases,
            anchor_wgs84,
            search_radius_m,
            idempotency_key,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcquisitionJobState {
    Queued,
    Running,
    Partial,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AcquisitionJobStatus {
    pub job_id: String,
    pub contract_version: String,
    pub bundle_id: String,
    pub state: AcquisitionJobState,
    #[serde(default)]
    pub outcomes: Vec<ProviderOutcome>,
    #[serde(default)]
    pub failure: Option<ServiceFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negotiated_bundle: Option<DatasetBundle>,
}

#[derive(Serialize)]
struct BoundaryCampusTarget<'a> {
    name: &'a str,
    aliases: &'a [String],
    anchor_wgs84: [f64; 2],
    search_radius_m: f64,
}

#[derive(Serialize)]
struct BoundaryRequestContent<'a> {
    contract_version: &'static str,
    bundle_id: &'a str,
    campus_target: BoundaryCampusTarget<'a>,
}

#[derive(Serialize)]
struct RequestIdentity<'a> {
    idempotency_key: &'a str,
    content_sha256: String,
}

#[derive(Serialize)]
struct BoundaryRequest<'a> {
    contract_version: &'static str,
    request_identity: RequestIdentity<'a>,
    bundle_id: &'a str,
    campus_target: BoundaryCampusTarget<'a>,
}

#[derive(Debug, Clone)]
pub struct FoundationAcquisitionRequest {
    bundle: DatasetBundle,
    boundary_revision: String,
    boundary_wgs84: SourceGeometry,
    idempotency_key: String,
}

impl FoundationAcquisitionRequest {
    pub fn new(
        bundle: DatasetBundle,
        boundary_revision: impl Into<String>,
        boundary_wgs84: SourceGeometry,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        let boundary_revision = boundary_revision.into();
        let idempotency_key = idempotency_key.into();
        if bundle.id.trim().is_empty() {
            return Err(
                "Foundation acquisition requires the Boundary Discovery Dataset Bundle.".into(),
            );
        }
        if boundary_revision.trim().is_empty() {
            return Err("Foundation acquisition requires the confirmed boundary revision.".into());
        }
        if idempotency_key.trim().is_empty() {
            return Err("Foundation acquisition requires a stable idempotency key.".into());
        }
        if !matches!(
            boundary_wgs84,
            SourceGeometry::Polygon(_) | SourceGeometry::MultiPolygon(_)
        ) || !valid_area_geometry(&boundary_wgs84)
        {
            return Err(
                "Foundation acquisition requires a valid confirmed WGS-84 boundary.".into(),
            );
        }
        Ok(Self {
            bundle,
            boundary_revision,
            boundary_wgs84,
            idempotency_key,
        })
    }

    pub fn bundle(&self) -> &DatasetBundle {
        &self.bundle
    }

    pub fn boundary_revision(&self) -> &str {
        &self.boundary_revision
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub fn content_sha256(&self) -> Result<String, AcquisitionClientError> {
        let content = AcquisitionRequestContent {
            contract_version: CONTRACT_VERSION,
            bundle_id: &self.bundle.id,
            boundary_revision: &self.boundary_revision,
            boundary_wgs84: &self.boundary_wgs84,
            categories: FoundationCategory::ALL,
        };
        let canonical = serde_json::to_value(&content).map_err(|error| {
            invalid_request_error(format!(
                "Could not encode the acquisition request identity: {error}"
            ))
        })?;
        serde_json::to_vec(&canonical)
            .map(|bytes| sha256_hex(&bytes))
            .map_err(|error| {
                invalid_request_error(format!(
                    "Could not canonicalize the acquisition request identity: {error}"
                ))
            })
    }
}

#[derive(Serialize)]
struct AcquisitionRequestContent<'a> {
    contract_version: &'static str,
    bundle_id: &'a str,
    boundary_revision: &'a str,
    boundary_wgs84: &'a SourceGeometry,
    categories: [FoundationCategory; 5],
}

#[derive(Serialize)]
struct AcquisitionRequest<'a> {
    contract_version: &'static str,
    request_identity: RequestIdentity<'a>,
    bundle_id: &'a str,
    boundary_revision: &'a str,
    boundary_wgs84: &'a SourceGeometry,
    categories: [FoundationCategory; 5],
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderOutcomeStatus {
    Complete,
    CompleteEmpty,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OutcomeScope {
    pub provider: String,
    pub category: FoundationCategory,
    pub tile_id: String,
}

impl OutcomeScope {
    pub fn new(
        provider: impl Into<String>,
        category: FoundationCategory,
        tile_id: impl Into<String>,
    ) -> Result<Self, String> {
        let provider = provider.into();
        let tile_id = tile_id.into();
        if provider.trim().is_empty() || tile_id.trim().is_empty() {
            return Err("A retry scope requires a provider and tile identifier.".into());
        }
        Ok(Self {
            provider,
            category,
            tile_id,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverageReport {
    pub outcomes: Vec<ProviderOutcome>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LicenceRecord {
    pub identifier: String,
    pub url: String,
    pub attribution: String,
    pub dataset_release: String,
    pub acquired_at: String,
    #[serde(default)]
    pub upstream_obligations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResultChunk {
    pub id: String,
    pub stable_cursor: String,
    pub content_type: String,
    pub content_encoding: String,
    pub sha256: String,
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResultManifest {
    pub contract_version: String,
    pub bundle: DatasetBundle,
    pub coverage_report: CoverageReport,
    pub licences: Vec<LicenceRecord>,
    pub chunks: Vec<ResultChunk>,
    pub result_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompleteRelation {
    pub relation_id: String,
    pub assembly_status: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoordinateSemantics {
    pub crs: String,
    pub axis_order: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeSemantics {
    pub dataset_release: String,
    pub acquired_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeometryDerivationRecord {
    pub rule_version: String,
    pub steps: Vec<String>,
    pub source_geometry_sha256: String,
    pub review_geometry_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AcquisitionSuggestion {
    pub kind: String,
    pub rule_version: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AttributeDerivation {
    Direct,
    Derived,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum MetreUnit {
    #[serde(rename = "m")]
    Metres,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum LevelUnit {
    #[serde(rename = "levels")]
    Levels,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum NoUnit {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundaryRankingEvidence {
    pub name_match: f64,
    pub distance_m: f64,
    pub contains_anchor: bool,
    pub area_m2: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundaryCandidate {
    pub id: String,
    pub rank: u32,
    pub geometry: SourceGeometry,
    pub lineage: SourceLineage,
    pub licence: LicenceRecord,
    pub ranking_evidence: BoundaryRankingEvidence,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServiceFailure {
    pub code: String,
    pub scope: String,
    pub retryable: bool,
    pub explanation: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquisitionClientErrorKind {
    AuthenticationRequired,
    IncompatibleContract,
    RetryableScope,
    IntegrityFailure,
    ServiceFailure,
    InvalidResponse,
    TransportUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionClientError {
    pub kind: AcquisitionClientErrorKind,
    pub explanation: String,
    pub action: String,
}

impl std::fmt::Display for AcquisitionClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} Action: {}", self.explanation, self.action)
    }
}

impl std::error::Error for AcquisitionClientError {}

pub fn map_service_failure(status: u16, body: &[u8]) -> AcquisitionClientError {
    let parsed = serde_json::from_slice::<ServiceFailure>(body).ok();
    if status == 401 || status == 403 {
        return AcquisitionClientError {
            kind: AcquisitionClientErrorKind::AuthenticationRequired,
            explanation: parsed
                .as_ref()
                .map(|failure| failure.explanation.clone())
                .unwrap_or_else(|| "The acquisition service credential was rejected.".into()),
            action: parsed
                .map(|failure| failure.suggested_action)
                .unwrap_or_else(|| "Repair or replace the acquisition service credential.".into()),
        };
    }
    match parsed {
        Some(failure) if failure.retryable => AcquisitionClientError {
            kind: AcquisitionClientErrorKind::RetryableScope,
            explanation: format!("{} ({})", failure.explanation, failure.scope),
            action: failure.suggested_action,
        },
        Some(failure) => AcquisitionClientError {
            kind: AcquisitionClientErrorKind::ServiceFailure,
            explanation: format!("{} ({})", failure.explanation, failure.scope),
            action: failure.suggested_action,
        },
        None => AcquisitionClientError {
            kind: AcquisitionClientErrorKind::InvalidResponse,
            explanation: format!("The acquisition service returned HTTP {status}."),
            action: "Open diagnostics and retry the request.".into(),
        },
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedAcquisitionResult {
    pub manifest: ResultManifest,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VerifiedAcquisitionChunk {
    pub descriptor: ResultChunk,
    pub canonical_ndjson: String,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone)]
pub struct VerifiedAcquisitionDelivery {
    pub manifest: ResultManifest,
    pub verified_chunks: Vec<VerifiedAcquisitionChunk>,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone)]
pub struct VerifiedBoundaryDiscoverySnapshot {
    pub manifest: ResultManifest,
    pub candidates: Vec<BoundaryCandidate>,
    pub validity: BTreeMap<String, BoundaryCandidateValidity>,
    pub derivations: BTreeMap<String, BoundaryCandidateDerivation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum BoundaryCandidateValidity {
    Valid,
    Invalid { reasons: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryCandidateDerivation {
    pub source_records: Vec<String>,
    pub rule_versions: Vec<String>,
    pub steps: Vec<String>,
}

pub struct AcquisitionClient<T> {
    transport: T,
}

#[derive(Debug, Clone, Copy)]
enum JobKind {
    Boundary,
    Acquisition,
}

impl JobKind {
    fn path_segment(self) -> &'static str {
        match self {
            Self::Boundary => "boundary-jobs",
            Self::Acquisition => "acquisition-jobs",
        }
    }
}

impl<T: AcquisitionTransport> AcquisitionClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn capabilities(&self) -> Result<AcquisitionCapabilities, AcquisitionClientError> {
        let response = self.execute(TransportRequest::get("/v1/capabilities"))?;
        let capabilities: AcquisitionCapabilities = serde_json::from_slice(&response.body)
            .map_err(|error| AcquisitionClientError {
                kind: AcquisitionClientErrorKind::InvalidResponse,
                explanation: format!("The acquisition capabilities are invalid: {error}"),
                action: "Update the desktop or contact the controlled-service operator.".into(),
            })?;
        if !capabilities
            .contract_versions
            .iter()
            .any(|version| version == CONTRACT_VERSION)
        {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IncompatibleContract,
                explanation: format!(
                    "The controlled service does not support desktop contract {CONTRACT_VERSION}."
                ),
                action: "Update the desktop or select a compatible controlled service.".into(),
            });
        }
        if capabilities.supported_bundles.is_empty() {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IncompatibleContract,
                explanation: "The controlled service offers no compatible Dataset Bundle.".into(),
                action: "Contact the controlled-service operator before retrying.".into(),
            });
        }
        Ok(capabilities)
    }

    pub fn start_boundary_discovery(
        &self,
        query: &CampusBoundaryCandidateQuery,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        let capabilities = self.capabilities()?;
        let requested_area = std::f64::consts::PI * query.search_radius_m.powi(2);
        if requested_area > capabilities.limits.area_square_metres as f64 {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::ServiceFailure,
                explanation: format!(
                    "The boundary search covers {requested_area:.0} m², above the service limit of {} m².",
                    capabilities.limits.area_square_metres
                ),
                action: "Reduce the bounded search radius and submit a new request.".into(),
            });
        }
        let bundle = &capabilities.supported_bundles[0];
        let content = BoundaryRequestContent {
            contract_version: CONTRACT_VERSION,
            bundle_id: &bundle.id,
            campus_target: BoundaryCampusTarget {
                name: &query.name,
                aliases: &query.aliases,
                anchor_wgs84: query.anchor_wgs84,
                search_radius_m: query.search_radius_m,
            },
        };
        let canonical_content = serde_json::to_value(&content).map_err(|error| {
            invalid_request_error(format!("Could not encode the boundary request: {error}"))
        })?;
        let content_bytes = serde_json::to_vec(&canonical_content).map_err(|error| {
            invalid_request_error(format!("Could not encode the boundary request: {error}"))
        })?;
        let request = BoundaryRequest {
            contract_version: CONTRACT_VERSION,
            request_identity: RequestIdentity {
                idempotency_key: &query.idempotency_key,
                content_sha256: sha256_hex(&content_bytes),
            },
            bundle_id: &bundle.id,
            campus_target: BoundaryCampusTarget {
                name: &query.name,
                aliases: &query.aliases,
                anchor_wgs84: query.anchor_wgs84,
                search_radius_m: query.search_radius_m,
            },
        };
        let body = serde_json::to_vec(&request).map_err(|error| {
            invalid_request_error(format!("Could not encode the boundary request: {error}"))
        })?;
        let response = self.execute(TransportRequest::post("/v1/boundary-jobs", Some(body)))?;
        let mut job = decode_job_status(&response.body)?;
        if job.contract_version != CONTRACT_VERSION || job.bundle_id != bundle.id {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IncompatibleContract,
                explanation: "The boundary job changed its negotiated contract or Dataset Bundle."
                    .into(),
                action: "Do not use this result; contact the controlled-service operator.".into(),
            });
        }
        job.negotiated_bundle = Some(bundle.clone());
        Ok(job)
    }

    pub fn start_foundation_acquisition(
        &self,
        request: &FoundationAcquisitionRequest,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        let capabilities = self.capabilities()?;
        if !capabilities
            .supported_bundles
            .iter()
            .any(|bundle| bundle == &request.bundle)
        {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IncompatibleContract,
                explanation:
                    "The controlled service cannot replay the Boundary Discovery Dataset Bundle."
                        .into(),
                action:
                    "Keep the confirmed boundary and reconnect to its original controlled service."
                        .into(),
            });
        }
        let boundary_vertices = geometry_point_count(&request.boundary_wgs84);
        if boundary_vertices > capabilities.limits.boundary_vertices as usize {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::ServiceFailure,
                explanation: format!(
                    "The confirmed boundary has {boundary_vertices} vertices, above the service limit of {}.",
                    capabilities.limits.boundary_vertices
                ),
                action: "Simplify and reconfirm the Campus Boundary before acquisition.".into(),
            });
        }
        let body = serde_json::to_vec(&AcquisitionRequest {
            contract_version: CONTRACT_VERSION,
            request_identity: RequestIdentity {
                idempotency_key: &request.idempotency_key,
                content_sha256: request.content_sha256()?,
            },
            bundle_id: &request.bundle.id,
            boundary_revision: &request.boundary_revision,
            boundary_wgs84: &request.boundary_wgs84,
            categories: FoundationCategory::ALL,
        })
        .map_err(|error| {
            invalid_request_error(format!("Could not encode the acquisition request: {error}"))
        })?;
        let response = self.execute(TransportRequest::post("/v1/acquisition-jobs", Some(body)))?;
        let mut job = decode_job_status(&response.body)?;
        if job.contract_version != CONTRACT_VERSION || job.bundle_id != request.bundle.id {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IntegrityFailure,
                explanation:
                    "The acquisition job changed its negotiated contract or Dataset Bundle.".into(),
                action: "Keep the confirmed boundary and retry its pinned acquisition request."
                    .into(),
            });
        }
        job.negotiated_bundle = Some(request.bundle.clone());
        Ok(job)
    }

    pub fn acquisition_job(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.control_job(JobKind::Acquisition, pinned, TransportMethod::Get, None)
    }

    pub fn retry_foundation_acquisition(
        &self,
        pinned: &AcquisitionJobStatus,
        scopes: &[OutcomeScope],
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        let body =
            serde_json::to_vec(&serde_json::json!({ "scopes": scopes })).map_err(|error| {
                invalid_request_error(format!(
                    "Could not encode acquisition retry scopes: {error}"
                ))
            })?;
        self.control_job(
            JobKind::Acquisition,
            pinned,
            TransportMethod::Post,
            Some(body),
        )
    }

    pub fn continue_foundation_acquisition(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.retry_foundation_acquisition(pinned, &[])
    }

    pub fn cancel_foundation_acquisition(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.control_job(JobKind::Acquisition, pinned, TransportMethod::Post, None)
    }

    pub fn boundary_job(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.control_boundary_job(pinned, TransportMethod::Get, None)
    }

    pub fn retry_boundary_job(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.control_boundary_job(
            pinned,
            TransportMethod::Post,
            Some(br#"{"scopes":[]}"#.to_vec()),
        )
    }

    pub fn cancel_boundary_job(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        self.control_boundary_job(pinned, TransportMethod::Post, None)
    }

    pub fn resume_boundary_discovery(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<VerifiedBoundaryDiscoverySnapshot, AcquisitionClientError> {
        let snapshot = self.load_boundary_discovery(&pinned.job_id)?;
        if snapshot.manifest.contract_version != pinned.contract_version
            || snapshot.manifest.bundle.id != pinned.bundle_id
            || pinned
                .negotiated_bundle
                .as_ref()
                .is_some_and(|bundle| bundle != &snapshot.manifest.bundle)
        {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IntegrityFailure,
                explanation:
                    "The resumed Boundary Discovery Snapshot does not match the pinned job.".into(),
                action: "Keep the prior job evidence and retry the same pinned job.".into(),
            });
        }
        Ok(snapshot)
    }

    fn control_boundary_job(
        &self,
        pinned: &AcquisitionJobStatus,
        method: TransportMethod,
        body: Option<Vec<u8>>,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        let action = match (method, body.is_some()) {
            (TransportMethod::Get, _) => None,
            (TransportMethod::Post, true) => Some("retry"),
            (TransportMethod::Post, false) => Some("cancel"),
        };
        let path = boundary_job_path(&pinned.job_id, action);
        let request = match method {
            TransportMethod::Get => TransportRequest::get(path),
            TransportMethod::Post => TransportRequest::post(path, body),
        };
        let response = self.execute(request)?;
        let mut current = decode_job_status(&response.body)?;
        if current.job_id != pinned.job_id
            || current.bundle_id != pinned.bundle_id
            || current.contract_version != CONTRACT_VERSION
        {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IntegrityFailure,
                explanation:
                    "The controlled service changed the pinned boundary job identity or Dataset Bundle."
                        .into(),
                action: "Keep the prior job evidence and contact the controlled-service operator."
                    .into(),
            });
        }
        current.negotiated_bundle = pinned.negotiated_bundle.clone();
        Ok(current)
    }

    fn control_job(
        &self,
        job_kind: JobKind,
        pinned: &AcquisitionJobStatus,
        method: TransportMethod,
        body: Option<Vec<u8>>,
    ) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
        let action = match (method, body.is_some()) {
            (TransportMethod::Get, _) => None,
            (TransportMethod::Post, true) => Some("retry"),
            (TransportMethod::Post, false) => Some("cancel"),
        };
        let path = job_path(job_kind, &pinned.job_id, action);
        let request = match method {
            TransportMethod::Get => TransportRequest::get(path),
            TransportMethod::Post => TransportRequest::post(path, body),
        };
        let response = self.execute(request)?;
        let mut current = decode_job_status(&response.body)?;
        if current.job_id != pinned.job_id
            || current.bundle_id != pinned.bundle_id
            || current.contract_version != pinned.contract_version
        {
            return Err(integrity_error(
                "The controlled service changed the pinned acquisition job identity, contract, or Dataset Bundle.",
            ));
        }
        current.negotiated_bundle = pinned.negotiated_bundle.clone();
        Ok(current)
    }

    pub fn load_boundary_discovery(
        &self,
        job_id: &str,
    ) -> Result<VerifiedBoundaryDiscoverySnapshot, AcquisitionClientError> {
        let (manifest, candidates) = self.load_result(JobKind::Boundary, job_id)?;
        let (validity, derivations) = assess_boundary_candidates(&manifest.bundle, &candidates)?;
        Ok(VerifiedBoundaryDiscoverySnapshot {
            manifest,
            candidates,
            validity,
            derivations,
        })
    }

    pub fn load_acquisition_result(
        &self,
        job_id: &str,
    ) -> Result<VerifiedAcquisitionResult, AcquisitionClientError> {
        let (manifest, observations) = self.load_result(JobKind::Acquisition, job_id)?;
        Ok(VerifiedAcquisitionResult {
            manifest,
            observations,
        })
    }

    pub fn resume_foundation_delivery(
        &self,
        pinned: &AcquisitionJobStatus,
        previously_verified: &[VerifiedAcquisitionChunk],
    ) -> Result<VerifiedAcquisitionDelivery, AcquisitionClientError> {
        let manifest = self.foundation_manifest(pinned)?;
        let mut verified_by_id = BTreeMap::new();
        for verified in previously_verified {
            validate_cached_chunk(&manifest, verified)?;
            if verified_by_id
                .insert(verified.descriptor.id.clone(), verified.clone())
                .is_some()
            {
                return Err(integrity_error(
                    "A verified acquisition chunk was supplied more than once.",
                ));
            }
        }
        for descriptor in &manifest.chunks {
            if verified_by_id.contains_key(&descriptor.id) {
                continue;
            }
            let verified = self.download_foundation_chunk(pinned, &manifest, descriptor)?;
            verified_by_id.insert(descriptor.id.clone(), verified);
        }
        let verified_chunks = manifest
            .chunks
            .iter()
            .map(|descriptor| {
                verified_by_id.remove(&descriptor.id).ok_or_else(|| {
                    integrity_error("A manifest result chunk is missing after acquisition resume.")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !verified_by_id.is_empty() {
            return Err(integrity_error(
                "The resume checkpoint contains a chunk outside the pinned manifest.",
            ));
        }
        self.finalize_foundation_delivery(pinned, manifest, verified_chunks)
    }

    pub fn foundation_manifest(
        &self,
        pinned: &AcquisitionJobStatus,
    ) -> Result<ResultManifest, AcquisitionClientError> {
        let response = self.execute(TransportRequest::get(format!(
            "/v1/acquisition-jobs/{}/manifest",
            pinned.job_id
        )))?;
        let manifest: ResultManifest =
            serde_json::from_slice(&response.body).map_err(|error| AcquisitionClientError {
                kind: AcquisitionClientErrorKind::InvalidResponse,
                explanation: format!("The acquisition manifest is invalid: {error}"),
                action: "Resume the pinned job or contact support with diagnostics.".into(),
            })?;
        validate_acquisition_manifest(&manifest, pinned)?;
        Ok(manifest)
    }

    pub fn download_foundation_chunk(
        &self,
        pinned: &AcquisitionJobStatus,
        manifest: &ResultManifest,
        descriptor: &ResultChunk,
    ) -> Result<VerifiedAcquisitionChunk, AcquisitionClientError> {
        validate_acquisition_manifest(manifest, pinned)?;
        if !manifest
            .chunks
            .iter()
            .any(|expected| expected == descriptor)
        {
            return Err(integrity_error(
                "The requested result chunk is not declared by the pinned manifest.",
            ));
        }
        let response = self.execute(TransportRequest::get(result_chunk_path(
            JobKind::Acquisition.path_segment(),
            &pinned.job_id,
            &descriptor.id,
            &descriptor.stable_cursor,
        )))?;
        verify_acquisition_chunk(descriptor, response)
    }

    pub fn finalize_foundation_delivery(
        &self,
        pinned: &AcquisitionJobStatus,
        manifest: ResultManifest,
        verified_chunks: Vec<VerifiedAcquisitionChunk>,
    ) -> Result<VerifiedAcquisitionDelivery, AcquisitionClientError> {
        validate_acquisition_manifest(&manifest, pinned)?;
        if verified_chunks.len() != manifest.chunks.len() {
            return Err(integrity_error(
                "Foundation acquisition cannot finalize before every manifest chunk is verified.",
            ));
        }
        let mut canonical_result = Vec::new();
        let mut observations = Vec::new();
        for (descriptor, verified) in manifest.chunks.iter().zip(&verified_chunks) {
            if &verified.descriptor != descriptor {
                return Err(integrity_error(
                    "Verified acquisition chunks are not in pinned manifest order.",
                ));
            }
            validate_cached_chunk(&manifest, verified)?;
            canonical_result.extend_from_slice(verified.canonical_ndjson.as_bytes());
            observations.extend(verified.observations.iter().cloned());
        }
        if sha256_hex(&canonical_result) != manifest.result_sha256 {
            return Err(integrity_error(
                "The assembled acquisition result failed SHA-256 verification.",
            ));
        }
        for observation in &observations {
            if !manifest
                .licences
                .iter()
                .any(|licence| licence == &observation.licence)
            {
                return Err(integrity_error(
                    "An acquisition observation is missing from the Acquisition Licence Manifest.",
                ));
            }
        }
        Ok(VerifiedAcquisitionDelivery {
            manifest,
            verified_chunks,
            observations,
        })
    }

    fn load_result<Record: DeserializeOwned>(
        &self,
        job_kind: JobKind,
        job_id: &str,
    ) -> Result<(ResultManifest, Vec<Record>), AcquisitionClientError> {
        let job_kind_segment = job_kind.path_segment();
        let response = self.execute(TransportRequest::get(format!(
            "/v1/{job_kind_segment}/{job_id}/manifest"
        )))?;
        let manifest: ResultManifest =
            serde_json::from_slice(&response.body).map_err(|error| AcquisitionClientError {
                kind: AcquisitionClientErrorKind::InvalidResponse,
                explanation: format!("The acquisition manifest is invalid: {error}"),
                action: "Update the desktop or contact support with diagnostics.".into(),
            })?;
        if manifest.contract_version != CONTRACT_VERSION {
            return Err(AcquisitionClientError {
                kind: AcquisitionClientErrorKind::IncompatibleContract,
                explanation: format!(
                    "Service contract {} is incompatible with desktop contract {CONTRACT_VERSION}.",
                    manifest.contract_version
                ),
                action: "Update the desktop or select a compatible controlled service.".into(),
            });
        }
        validate_provider_outcomes(&manifest.coverage_report.outcomes)?;
        if matches!(job_kind, JobKind::Acquisition)
            && !FoundationCategory::ALL.into_iter().all(|category| {
                manifest
                    .coverage_report
                    .outcomes
                    .iter()
                    .any(|outcome| outcome.category == category)
            })
        {
            return Err(integrity_error(
                "The acquisition manifest does not report every requested Foundation category.",
            ));
        }
        let mut cursors = BTreeSet::new();
        if manifest.chunks.iter().any(|chunk| {
            chunk.stable_cursor.is_empty()
                || !cursors.insert(chunk.stable_cursor.clone())
                || chunk.content_type != "application/x-ndjson"
                || chunk.content_encoding != "gzip"
        }) {
            return Err(integrity_error(
                "The result manifest contains an unstable cursor or unsupported chunk encoding.",
            ));
        }

        let mut canonical_result = Vec::new();
        let mut records = Vec::new();
        for chunk in &manifest.chunks {
            let response = self.execute(TransportRequest::get(result_chunk_path(
                job_kind_segment,
                job_id,
                &chunk.id,
                &chunk.stable_cursor,
            )))?;
            if response
                .headers
                .get("x-stable-cursor")
                .is_none_or(|cursor| cursor != &chunk.stable_cursor)
            {
                return Err(integrity_error(
                    "The service returned a different resume cursor than the pinned manifest.",
                ));
            }
            if sha256_hex(&response.body) != chunk.sha256 {
                return Err(integrity_error(
                    "A downloaded gzip result chunk failed SHA-256 verification.",
                ));
            }
            let mut decoded = Vec::new();
            GzDecoder::new(response.body.as_slice())
                .read_to_end(&mut decoded)
                .map_err(|_| integrity_error("A downloaded result chunk is not valid gzip."))?;
            if decoded.len() as u64 != chunk.uncompressed_bytes {
                return Err(integrity_error(
                    "A downloaded result chunk has an unexpected decoded size.",
                ));
            }
            for line in decoded
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                records.push(serde_json::from_slice(line).map_err(|error| {
                    AcquisitionClientError {
                        kind: AcquisitionClientErrorKind::InvalidResponse,
                        explanation: format!("A typed acquisition record is invalid: {error}"),
                        action: "Redownload the affected chunk or contact support.".into(),
                    }
                })?);
            }
            canonical_result.extend_from_slice(&decoded);
        }
        if sha256_hex(&canonical_result) != manifest.result_sha256 {
            return Err(integrity_error(
                "The assembled acquisition result failed SHA-256 verification.",
            ));
        }
        Ok((manifest, records))
    }

    fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, AcquisitionClientError> {
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 0..MAX_ATTEMPTS {
            let response = match self.transport.execute(request.clone()) {
                Ok(response) => response,
                Err(_) if attempt + 1 < MAX_ATTEMPTS => {
                    retry_backoff(attempt);
                    continue;
                }
                Err(error) => {
                    return Err(AcquisitionClientError {
                        kind: AcquisitionClientErrorKind::TransportUnavailable,
                        explanation: error.explanation,
                        action: "Check connectivity and resume the pinned job.".into(),
                    });
                }
            };
            if (200..300).contains(&response.status) {
                return Ok(response);
            }
            let structured_retry = serde_json::from_slice::<ServiceFailure>(&response.body)
                .is_ok_and(|failure| failure.retryable);
            let transient_status = matches!(response.status, 429 | 500 | 502 | 503 | 504);
            if attempt + 1 < MAX_ATTEMPTS && (structured_retry || transient_status) {
                retry_backoff(attempt);
                continue;
            }
            return Err(map_service_failure(response.status, &response.body));
        }
        unreachable!("the bounded retry loop always returns")
    }
}

fn retry_backoff(attempt: usize) {
    #[cfg(test)]
    let base_millis = 1;
    #[cfg(not(test))]
    let base_millis = 100;
    std::thread::sleep(std::time::Duration::from_millis(
        base_millis * (1_u64 << attempt),
    ));
}

fn integrity_error(explanation: impl Into<String>) -> AcquisitionClientError {
    AcquisitionClientError {
        kind: AcquisitionClientErrorKind::IntegrityFailure,
        explanation: explanation.into(),
        action: "Discard this download and resume the pinned result chunk.".into(),
    }
}

fn invalid_request_error(explanation: impl Into<String>) -> AcquisitionClientError {
    AcquisitionClientError {
        kind: AcquisitionClientErrorKind::InvalidResponse,
        explanation: explanation.into(),
        action: "Correct the confirmed Campus Target inputs and retry.".into(),
    }
}

fn decode_job_status(body: &[u8]) -> Result<AcquisitionJobStatus, AcquisitionClientError> {
    let status: AcquisitionJobStatus =
        serde_json::from_slice(body).map_err(|error| AcquisitionClientError {
            kind: AcquisitionClientErrorKind::InvalidResponse,
            explanation: format!("The controlled service returned an invalid job status: {error}"),
            action: "Open diagnostics and resume the pinned request later.".into(),
        })?;
    validate_provider_outcomes(&status.outcomes)?;
    Ok(status)
}

fn validate_provider_outcomes(outcomes: &[ProviderOutcome]) -> Result<(), AcquisitionClientError> {
    let mut scopes = BTreeSet::new();
    for outcome in outcomes {
        let scope = (
            outcome.provider.as_str(),
            outcome.category,
            outcome.tile_id.as_str(),
        );
        if outcome.provider.trim().is_empty()
            || outcome.tile_id.trim().is_empty()
            || !scopes.insert(scope)
        {
            return Err(integrity_error(
                "The coverage report contains a missing or duplicate provider/category/tile scope.",
            ));
        }
        if matches!(
            outcome.status,
            ProviderOutcomeStatus::Complete | ProviderOutcomeStatus::CompleteEmpty
        ) && (!outcome.pagination_exhausted || !outcome.relation_members_complete)
        {
            return Err(integrity_error(
                "Complete coverage requires explicit page exhaustion and complete relation membership.",
            ));
        }
        if outcome.status == ProviderOutcomeStatus::CompleteEmpty
            && (outcome.raw_count != 0 || outcome.deduplicated_count != 0)
        {
            return Err(integrity_error(
                "Complete-empty coverage cannot declare retrieved observations.",
            ));
        }
        if outcome.deduplicated_count > outcome.raw_count {
            return Err(integrity_error(
                "Coverage cannot contain more deduplicated observations than raw observations.",
            ));
        }
        if matches!(
            outcome.status,
            ProviderOutcomeStatus::Partial
                | ProviderOutcomeStatus::Failed
                | ProviderOutcomeStatus::Cancelled
        ) && outcome.failure.is_none()
        {
            return Err(integrity_error(
                "Incomplete coverage must retain its structured failure and recovery action.",
            ));
        }
    }
    Ok(())
}

fn validate_acquisition_manifest(
    manifest: &ResultManifest,
    pinned: &AcquisitionJobStatus,
) -> Result<(), AcquisitionClientError> {
    if manifest.contract_version != pinned.contract_version
        || manifest.bundle.id != pinned.bundle_id
        || pinned
            .negotiated_bundle
            .as_ref()
            .is_some_and(|bundle| bundle != &manifest.bundle)
    {
        return Err(integrity_error(
            "The acquisition manifest changed the pinned contract or Dataset Bundle.",
        ));
    }
    validate_provider_outcomes(&manifest.coverage_report.outcomes)?;
    if !FoundationCategory::ALL.into_iter().all(|category| {
        manifest
            .coverage_report
            .outcomes
            .iter()
            .any(|outcome| outcome.category == category)
    }) {
        return Err(integrity_error(
            "The acquisition manifest does not report every requested Foundation category.",
        ));
    }
    let mut chunk_ids = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    if manifest.result_sha256.len() != 64
        || !manifest
            .result_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.chunks.iter().any(|chunk| {
            chunk.id.trim().is_empty()
                || !chunk_ids.insert(chunk.id.as_str())
                || chunk.stable_cursor.trim().is_empty()
                || !cursors.insert(chunk.stable_cursor.as_str())
                || chunk.content_type != "application/x-ndjson"
                || chunk.content_encoding != "gzip"
                || chunk.sha256.len() != 64
                || !chunk.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(integrity_error(
            "The acquisition manifest contains invalid result or chunk integrity metadata.",
        ));
    }
    Ok(())
}

fn validate_cached_chunk(
    manifest: &ResultManifest,
    verified: &VerifiedAcquisitionChunk,
) -> Result<(), AcquisitionClientError> {
    if !manifest
        .chunks
        .iter()
        .any(|descriptor| descriptor == &verified.descriptor)
    {
        return Err(integrity_error(
            "A resume checkpoint chunk is not declared by the pinned manifest.",
        ));
    }
    if verified.canonical_ndjson.as_bytes().len() as u64 != verified.descriptor.uncompressed_bytes {
        return Err(integrity_error(
            "A resume checkpoint chunk has an unexpected decoded size.",
        ));
    }
    let decoded = parse_observations(verified.canonical_ndjson.as_bytes())?;
    let decoded_values = decoded
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            integrity_error(format!("Could not verify cached observations: {error}"))
        })?;
    let cached_values = verified
        .observations
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            integrity_error(format!("Could not verify cached observations: {error}"))
        })?;
    if decoded_values != cached_values {
        return Err(integrity_error(
            "A resume checkpoint chunk is not self-contained.",
        ));
    }
    Ok(())
}

fn verify_acquisition_chunk(
    descriptor: &ResultChunk,
    response: TransportResponse,
) -> Result<VerifiedAcquisitionChunk, AcquisitionClientError> {
    if response
        .headers
        .get("x-stable-cursor")
        .is_none_or(|cursor| cursor != &descriptor.stable_cursor)
    {
        return Err(integrity_error(
            "The service returned a different resume cursor than the pinned manifest.",
        ));
    }
    if sha256_hex(&response.body) != descriptor.sha256 {
        return Err(integrity_error(
            "A downloaded gzip result chunk failed SHA-256 verification.",
        ));
    }
    let mut decoded = Vec::new();
    GzDecoder::new(response.body.as_slice())
        .read_to_end(&mut decoded)
        .map_err(|_| integrity_error("A downloaded result chunk is not valid gzip."))?;
    if decoded.len() as u64 != descriptor.uncompressed_bytes {
        return Err(integrity_error(
            "A downloaded result chunk has an unexpected decoded size.",
        ));
    }
    let observations = parse_observations(&decoded)?;
    let canonical_ndjson = String::from_utf8(decoded)
        .map_err(|_| integrity_error("A downloaded acquisition chunk is not UTF-8 NDJSON."))?;
    Ok(VerifiedAcquisitionChunk {
        descriptor: descriptor.clone(),
        canonical_ndjson,
        observations,
    })
}

fn parse_observations(bytes: &[u8]) -> Result<Vec<SourceObservation>, AcquisitionClientError> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_slice(line).map_err(|error| AcquisitionClientError {
                kind: AcquisitionClientErrorKind::InvalidResponse,
                explanation: format!("A typed acquisition observation is invalid: {error}"),
                action: "Redownload the affected chunk or contact support.".into(),
            })
        })
        .collect()
}

fn assess_boundary_candidates(
    bundle: &DatasetBundle,
    candidates: &[BoundaryCandidate],
) -> Result<
    (
        BTreeMap<String, BoundaryCandidateValidity>,
        BTreeMap<String, BoundaryCandidateDerivation>,
    ),
    AcquisitionClientError,
> {
    let mut ids = BTreeSet::new();
    let mut validity = BTreeMap::new();
    let mut derivations = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let expected_rank = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if candidate.rank != expected_rank || !ids.insert(&candidate.id) {
            return Err(integrity_error(
                "Boundary candidates are not uniquely ranked in stable order.",
            ));
        }
        let mut reasons = Vec::new();
        if !matches!(
            &candidate.geometry,
            SourceGeometry::Polygon(_) | SourceGeometry::MultiPolygon(_)
        ) {
            reasons.push("candidate geometry is not a Polygon or MultiPolygon".into());
        }
        if !matches!(candidate.lineage.provider.as_str(), "osm" | "overture") {
            reasons.push("candidate provider is not OSM or Overture".into());
        }
        if candidate.id.trim().is_empty()
            || candidate.lineage.source_record_id.trim().is_empty()
            || candidate.lineage.dataset_release.trim().is_empty()
            || candidate.licence.identifier.trim().is_empty()
            || candidate.licence.dataset_release.trim().is_empty()
        {
            reasons.push("candidate lineage or licence evidence is incomplete".into());
        }
        let ranking = &candidate.ranking_evidence;
        if !ranking.name_match.is_finite()
            || !(0.0..=1.0).contains(&ranking.name_match)
            || !ranking.distance_m.is_finite()
            || ranking.distance_m < 0.0
            || !ranking.area_m2.is_finite()
            || ranking.area_m2 <= 0.0
        {
            reasons.push("candidate ranking evidence is invalid".into());
        }
        if !valid_area_geometry(&candidate.geometry) {
            reasons.push("candidate area geometry has invalid coordinates or rings".into());
        }
        validity.insert(
            candidate.id.clone(),
            if reasons.is_empty() {
                BoundaryCandidateValidity::Valid
            } else {
                BoundaryCandidateValidity::Invalid { reasons }
            },
        );
        let mut source_records = vec![candidate.lineage.source_record_id.clone()];
        source_records.extend(candidate.lineage.upstream_records.iter().cloned());
        let mut steps = vec![format!(
            "classified {} from {}",
            candidate.lineage.original_classification, candidate.lineage.provider
        )];
        if let Some(relation) = &candidate.lineage.relation {
            source_records.extend(relation.member_ids.iter().cloned());
            steps.push(format!(
                "assembled relation {} with status {}",
                relation.relation_id, relation.assembly_status
            ));
        }
        source_records.sort();
        source_records.dedup();
        derivations.insert(
            candidate.id.clone(),
            BoundaryCandidateDerivation {
                source_records,
                rule_versions: vec![
                    bundle.classification_rules.clone(),
                    bundle.assembly_rules.clone(),
                    bundle.derivation_rules.clone(),
                ],
                steps,
            },
        );
    }
    Ok((validity, derivations))
}

fn valid_area_geometry(geometry: &SourceGeometry) -> bool {
    fn valid_position(position: &[f64; 2]) -> bool {
        position[0].is_finite()
            && position[1].is_finite()
            && (-180.0..=180.0).contains(&position[0])
            && (-90.0..=90.0).contains(&position[1])
    }
    fn valid_ring(ring: &[[f64; 2]]) -> bool {
        ring.len() >= 4 && ring.first() == ring.last() && ring.iter().all(valid_position)
    }
    match geometry {
        SourceGeometry::Polygon(rings) => {
            !rings.is_empty() && rings.iter().all(|ring| valid_ring(ring))
        }
        SourceGeometry::MultiPolygon(polygons) => {
            !polygons.is_empty()
                && polygons
                    .iter()
                    .all(|rings| !rings.is_empty() && rings.iter().all(|ring| valid_ring(ring)))
        }
        _ => false,
    }
}

fn geometry_point_count(geometry: &SourceGeometry) -> usize {
    match geometry {
        SourceGeometry::Point(_) => 1,
        SourceGeometry::MultiPoint(points) | SourceGeometry::LineString(points) => points.len(),
        SourceGeometry::MultiLineString(lines) | SourceGeometry::Polygon(lines) => {
            lines.iter().map(Vec::len).sum()
        }
        SourceGeometry::MultiPolygon(polygons) => polygons
            .iter()
            .flat_map(|polygon| polygon.iter())
            .map(Vec::len)
            .sum(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn result_chunk_path(job_kind: &str, job_id: &str, chunk_id: &str, cursor: &str) -> String {
    let mut url = reqwest::Url::parse(&format!(
        "https://contract.invalid/v1/{job_kind}/{job_id}/chunks/{chunk_id}"
    ))
    .expect("fixed acquisition result path is a valid URL");
    url.query_pairs_mut().append_pair("cursor", cursor);
    format!(
        "{}?{}",
        url.path(),
        url.query().expect("cursor query was appended")
    )
}

fn boundary_job_path(job_id: &str, action: Option<&str>) -> String {
    job_path(JobKind::Boundary, job_id, action)
}

fn job_path(job_kind: JobKind, job_id: &str, action: Option<&str>) -> String {
    let mut url = reqwest::Url::parse(&format!(
        "https://contract.invalid/v1/{}",
        job_kind.path_segment()
    ))
    .expect("fixed controlled-service job base path is valid");
    {
        let mut segments = url
            .path_segments_mut()
            .expect("fixed controlled-service job URL is hierarchical");
        segments.push(job_id);
        if let Some(action) = action {
            segments.push(action);
        }
    }
    url.path().to_owned()
}

#[cfg(debug_assertions)]
fn fixture_result_parts(
    fixture: &serde_json::Value,
    records_key: &str,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let records = fixture[records_key]
        .as_array()
        .ok_or_else(|| format!("fixture {records_key} must be an array"))?;
    let manifest_template = &fixture["manifest"];
    let chunk_template = &manifest_template["chunks"][0];
    let chunk_id = chunk_template["id"]
        .as_str()
        .ok_or("fixture chunk is missing id")?;
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(
            fixture["transport_chunks"][chunk_id]
                .as_str()
                .ok_or("fixture transport chunk is missing")?,
        )
        .map_err(|error| format!("fixture transport chunk is invalid base64: {error}"))?;
    let mut canonical = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .read_to_end(&mut canonical)
        .map_err(|error| format!("fixture transport chunk is invalid gzip: {error}"))?;
    let declared_chunk_sha = chunk_template["sha256"]
        .as_str()
        .ok_or("fixture chunk is missing sha256")?;
    let declared_result_sha = manifest_template["result_sha256"]
        .as_str()
        .ok_or("fixture manifest is missing result_sha256")?;
    let declared_bytes = chunk_template["uncompressed_bytes"]
        .as_u64()
        .ok_or("fixture chunk is missing uncompressed_bytes")?;
    if sha256_hex(&compressed) != declared_chunk_sha
        || sha256_hex(&canonical) != declared_result_sha
        || canonical.len() as u64 != declared_bytes
    {
        return Err("fixture integrity metadata does not match canonical transport bytes".into());
    }
    let licences = records
        .iter()
        .filter_map(|record| record.get("licence").cloned())
        .collect::<Vec<_>>();
    let manifest = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "bundle": fixture["bundle"],
        "coverage_report": fixture["coverage_report"],
        "licences": licences,
        "chunks": [{
            "id": chunk_template["id"],
            "stable_cursor": chunk_template["stable_cursor"],
            "content_type": "application/x-ndjson",
            "content_encoding": "gzip",
            "sha256": declared_chunk_sha,
            "uncompressed_bytes": declared_bytes
        }],
        "result_sha256": declared_result_sha
    });
    Ok((
        serde_json::to_vec(&manifest).map_err(|error| error.to_string())?,
        compressed,
    ))
}

#[cfg(debug_assertions)]
pub mod fixture_transport {
    use super::*;

    pub struct FixtureTransport {
        acquisition_manifest: Vec<u8>,
        acquisition_chunk: Vec<u8>,
        boundary_manifest: Vec<u8>,
        boundary_chunk: Vec<u8>,
        acquisition_cursor: String,
        boundary_cursor: String,
    }

    impl FixtureTransport {
        pub fn canonical() -> Result<Self, String> {
            let fixture: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
            ))
            .map_err(|error| error.to_string())?;
            let boundary: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
            ))
            .map_err(|error| error.to_string())?;
            let (acquisition_manifest, acquisition_chunk) =
                fixture_result_parts(&fixture, "observations")?;
            let (boundary_manifest, boundary_chunk) =
                fixture_result_parts(&boundary, "candidates")?;
            Ok(Self {
                acquisition_manifest,
                acquisition_chunk,
                boundary_manifest,
                boundary_chunk,
                acquisition_cursor: fixture["manifest"]["chunks"][0]["stable_cursor"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                boundary_cursor: boundary["manifest"]["chunks"][0]["stable_cursor"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            })
        }
    }

    impl AcquisitionTransport for FixtureTransport {
        fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
            let boundary = request.path.contains("/boundary-jobs/");
            if request.path.ends_with("/manifest") {
                Ok(TransportResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: if boundary {
                        self.boundary_manifest.clone()
                    } else {
                        self.acquisition_manifest.clone()
                    },
                })
            } else {
                Ok(TransportResponse {
                    status: 200,
                    headers: BTreeMap::from([(
                        "x-stable-cursor".into(),
                        if boundary {
                            self.boundary_cursor.clone()
                        } else {
                            self.acquisition_cursor.clone()
                        },
                    )]),
                    body: if boundary {
                        self.boundary_chunk.clone()
                    } else {
                        self.acquisition_chunk.clone()
                    },
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[derive(Clone)]
    struct RecordingTransport {
        requests: Rc<RefCell<Vec<TransportRequest>>>,
        responses: Rc<RefCell<VecDeque<TransportResponse>>>,
    }

    impl RecordingTransport {
        fn new(responses: Vec<TransportResponse>) -> Self {
            Self {
                requests: Rc::new(RefCell::new(Vec::new())),
                responses: Rc::new(RefCell::new(responses.into())),
            }
        }
    }

    impl AcquisitionTransport for RecordingTransport {
        fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
            self.requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| TransportError {
                    explanation: "test transport ran out of responses".into(),
                })
        }
    }

    fn json_response(value: serde_json::Value) -> TransportResponse {
        TransportResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        }
    }

    fn capabilities_response() -> TransportResponse {
        json_response(serde_json::json!({
            "contract_versions": [CONTRACT_VERSION],
            "supported_bundles": [{
                "id": "cn-campus-2026-06",
                "osm_snapshot": "2026-06-01",
                "overture_release": "2026-06-18.0",
                "output_schema": "source-observation-v1",
                "classification_rules": "classification-v1",
                "assembly_rules": "assembly-v1",
                "conflation_rules": "conflation-v1",
                "derivation_rules": "derivation-v1"
            }],
            "limits": {
                "area_square_metres": 100000000,
                "boundary_vertices": 10000,
                "tiles": 10000,
                "observations": 1000000,
                "result_bytes": 1000000000,
                "concurrent_jobs": 2
            },
            "retention_days": 30,
            "quota_remaining": 100
        }))
    }

    #[test]
    fn live_boundary_flow_negotiates_and_sends_only_confirmed_target_inputs() {
        let transport = RecordingTransport::new(vec![
            capabilities_response(),
            json_response(serde_json::json!({
                "job_id": "boundary-job-7",
                "contract_version": CONTRACT_VERSION,
                "bundle_id": "cn-campus-2026-06",
                "state": "running"
            })),
        ]);
        let requests = transport.requests.clone();
        let client = AcquisitionClient::new(transport);
        let query = CampusBoundaryCandidateQuery::new(
            "East China Normal University Putuo Campus",
            vec!["ECNU Putuo".into()],
            [121.395, 31.202],
            2_000.0,
            "installation-42:boundary:putuo",
        )
        .unwrap();

        let job = client.start_boundary_discovery(&query).unwrap();

        assert_eq!(job.job_id, "boundary-job-7");
        assert_eq!(job.bundle_id, "cn-campus-2026-06");
        let requests = requests.borrow();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, TransportMethod::Get);
        assert_eq!(requests[0].path, "/v1/capabilities");
        assert_eq!(requests[1].method, TransportMethod::Post);
        assert_eq!(requests[1].path, "/v1/boundary-jobs");
        let body: serde_json::Value =
            serde_json::from_slice(requests[1].body.as_deref().unwrap()).unwrap();
        assert_eq!(
            body.as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "bundle_id".into(),
                "campus_target".into(),
                "contract_version".into(),
                "request_identity".into(),
            ])
        );
        assert_eq!(
            body["campus_target"],
            serde_json::json!({
                "name": "East China Normal University Putuo Campus",
                "aliases": ["ECNU Putuo"],
                "anchor_wgs84": [121.395, 31.202],
                "search_radius_m": 2000.0
            })
        );
        assert_eq!(
            body["request_identity"]["idempotency_key"],
            "installation-42:boundary:putuo"
        );
        let content = serde_json::to_vec(&serde_json::json!({
            "contract_version": CONTRACT_VERSION,
            "bundle_id": "cn-campus-2026-06",
            "campus_target": body["campus_target"].clone()
        }))
        .unwrap();
        assert_eq!(
            body["request_identity"]["content_sha256"],
            sha256_hex(&content)
        );
    }

    #[test]
    fn reconnect_retry_and_cancel_preserve_the_boundary_job_identity() {
        let status = || {
            json_response(serde_json::json!({
                "job_id": "boundary-job-7",
                "contract_version": CONTRACT_VERSION,
                "bundle_id": "cn-campus-2026-06",
                "state": "running"
            }))
        };
        let transport = RecordingTransport::new(vec![status(), status(), status()]);
        let requests = transport.requests.clone();
        let client = AcquisitionClient::new(transport);
        let pinned = AcquisitionJobStatus {
            job_id: "boundary-job-7".into(),
            contract_version: CONTRACT_VERSION.into(),
            bundle_id: "cn-campus-2026-06".into(),
            state: AcquisitionJobState::Running,
            outcomes: Vec::new(),
            failure: None,
            negotiated_bundle: None,
        };

        assert_eq!(client.boundary_job(&pinned).unwrap().job_id, pinned.job_id);
        assert_eq!(
            client.retry_boundary_job(&pinned).unwrap().bundle_id,
            pinned.bundle_id
        );
        assert_eq!(
            client.cancel_boundary_job(&pinned).unwrap().job_id,
            pinned.job_id
        );
        let requests = requests.borrow();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/v1/boundary-jobs/boundary-job-7",
                "/v1/boundary-jobs/boundary-job-7/retry",
                "/v1/boundary-jobs/boundary-job-7/cancel",
            ]
        );
        assert_eq!(
            requests[1].body.as_deref(),
            Some(br#"{"scopes":[]}"#.as_slice())
        );
        assert!(requests[2].body.is_none());
    }

    #[test]
    fn oversized_boundary_query_fails_before_job_submission_without_clipping() {
        let transport = RecordingTransport::new(vec![capabilities_response()]);
        let requests = transport.requests.clone();
        let client = AcquisitionClient::new(transport);
        let query = CampusBoundaryCandidateQuery::new(
            "Oversized Campus",
            Vec::new(),
            [121.4, 31.2],
            10_000.0,
            "installation-42:oversized",
        )
        .unwrap();

        let error = client.start_boundary_discovery(&query).unwrap_err();

        assert_eq!(error.kind, AcquisitionClientErrorKind::ServiceFailure);
        assert!(error.explanation.contains("100000000 m²"));
        assert_eq!(requests.borrow().len(), 1);
        assert_eq!(requests.borrow()[0].path, "/v1/capabilities");
    }

    #[test]
    fn transient_failures_retry_the_same_versioned_request() {
        let mut unavailable = json_response(serde_json::json!({
            "code": "temporarily_unavailable",
            "scope": "capabilities",
            "retryable": true,
            "explanation": "The service is restarting.",
            "suggested_action": "Retry."
        }));
        unavailable.status = 503;
        let transport = RecordingTransport::new(vec![unavailable, capabilities_response()]);
        let requests = transport.requests.clone();
        let client = AcquisitionClient::new(transport);

        let capabilities = client.capabilities().unwrap();

        assert_eq!(capabilities.supported_bundles[0].id, "cn-campus-2026-06");
        let requests = requests.borrow();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, requests[1].path);
        assert_eq!(requests[0].body, requests[1].body);
    }

    #[test]
    fn invalid_boundary_candidate_remains_available_with_diagnostic_reasons() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
        ))
        .unwrap();
        let mut candidate: BoundaryCandidate =
            serde_json::from_value(fixture["candidates"][0].clone()).unwrap();
        candidate.geometry = SourceGeometry::LineString(vec![[121.4, 31.2], [121.5, 31.3]]);

        let bundle: DatasetBundle = serde_json::from_value(fixture["bundle"].clone()).unwrap();
        let (validity, derivations) = assess_boundary_candidates(&bundle, &[candidate]).unwrap();

        let BoundaryCandidateValidity::Invalid { reasons } = &validity["boundary-osm-relation-100"]
        else {
            panic!("invalid candidate was incorrectly accepted");
        };
        assert!(reasons.iter().any(|reason| reason.contains("Polygon")));
        assert!(reasons
            .iter()
            .any(|reason| reason.contains("area geometry")));
        assert!(derivations["boundary-osm-relation-100"]
            .steps
            .iter()
            .any(|step| step.contains("assembled relation")));
    }

    #[test]
    fn stable_cursor_is_encoded_as_an_opaque_query_value() {
        assert_eq!(
            result_chunk_path("acquisition-jobs", "job-1", "chunk-1", "page=2&token=a+b%#"),
            "/v1/acquisition-jobs/job-1/chunks/chunk-1?cursor=page%3D2%26token%3Da%2Bb%25%23"
        );
    }
}
