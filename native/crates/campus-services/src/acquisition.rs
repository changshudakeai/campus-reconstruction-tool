use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
#[cfg(debug_assertions)]
use std::io::Write;

use flate2::read::GzDecoder;
#[cfg(debug_assertions)]
use flate2::{Compression, GzBuilder};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT_VERSION: &str = "1.0.0";

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
    authorization: String,
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
            authorization: format!("Bearer {credential}"),
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
        .header(AUTHORIZATION, &self.authorization)
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
        let body = response.bytes().map_err(|error| TransportError {
            explanation: format!("could not read acquisition service response: {error}"),
        })?;
        Ok(TransportResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverageReport {
    pub outcomes: Vec<ProviderOutcome>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FoundationCategory {
    Building,
    Circulation,
    Water,
    Vegetation,
    Sports,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LicenceRecord {
    pub identifier: String,
    pub url: String,
    pub attribution: String,
    pub dataset_release: String,
    pub acquired_at: String,
    #[serde(default)]
    pub upstream_obligations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
pub struct AttributeProvenance {
    pub attribute: String,
    pub value: serde_json::Value,
    pub source_observation_id: String,
    pub original_value: serde_json::Value,
    pub unit: String,
    pub derivation: AttributeDerivation,
    pub rule_version: String,
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

#[derive(Debug, Clone)]
pub struct VerifiedBoundaryDiscoverySnapshot {
    pub manifest: ResultManifest,
    pub candidates: Vec<BoundaryCandidate>,
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

    pub fn load_boundary_discovery(
        &self,
        job_id: &str,
    ) -> Result<VerifiedBoundaryDiscoverySnapshot, AcquisitionClientError> {
        let (manifest, candidates) = self.load_result(JobKind::Boundary, job_id)?;
        Ok(VerifiedBoundaryDiscoverySnapshot {
            manifest,
            candidates,
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

    fn load_result<Record: DeserializeOwned>(
        &self,
        job_kind: JobKind,
        job_id: &str,
    ) -> Result<(ResultManifest, Vec<Record>), AcquisitionClientError> {
        let job_kind = job_kind.path_segment();
        let response = self.execute(TransportRequest::get(format!(
            "/v1/{job_kind}/{job_id}/manifest"
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
            let response = self.execute(TransportRequest::get(format!(
                "/v1/{job_kind}/{job_id}/chunks/{}?cursor={}",
                chunk.id, chunk.stable_cursor
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
            let mut decoded = Vec::new();
            GzDecoder::new(response.body.as_slice())
                .read_to_end(&mut decoded)
                .map_err(|_| integrity_error("A downloaded result chunk is not valid gzip."))?;
            if decoded.len() as u64 != chunk.uncompressed_bytes {
                return Err(integrity_error(
                    "A downloaded result chunk has an unexpected decoded size.",
                ));
            }
            if sha256_hex(&decoded) != chunk.sha256 {
                return Err(integrity_error(
                    "A decoded result chunk failed SHA-256 verification.",
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
        let response = self
            .transport
            .execute(request)
            .map_err(|error| AcquisitionClientError {
                kind: AcquisitionClientErrorKind::TransportUnavailable,
                explanation: error.explanation,
                action: "Check connectivity and resume the pinned job.".into(),
            })?;
        if !(200..300).contains(&response.status) {
            return Err(map_service_failure(response.status, &response.body));
        }
        Ok(response)
    }
}

fn integrity_error(explanation: impl Into<String>) -> AcquisitionClientError {
    AcquisitionClientError {
        kind: AcquisitionClientErrorKind::IntegrityFailure,
        explanation: explanation.into(),
        action: "Discard this download and resume the pinned result chunk.".into(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(debug_assertions)]
fn fixture_result_parts(
    manifest_template: &serde_json::Value,
    bundle: &serde_json::Value,
    coverage_report: &serde_json::Value,
    records: &[serde_json::Value],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut canonical = Vec::new();
    for record in records {
        canonical.extend(serde_json::to_vec(record).map_err(|error| error.to_string())?);
        canonical.push(b'\n');
    }
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder
        .write_all(&canonical)
        .expect("Vec writes cannot fail");
    let compressed = encoder.finish().expect("Vec writes cannot fail");
    let chunk_template = &manifest_template["chunks"][0];
    let declared_chunk_sha = chunk_template["sha256"]
        .as_str()
        .ok_or("fixture chunk is missing sha256")?;
    let declared_result_sha = manifest_template["result_sha256"]
        .as_str()
        .ok_or("fixture manifest is missing result_sha256")?;
    let declared_bytes = chunk_template["uncompressed_bytes"]
        .as_u64()
        .ok_or("fixture chunk is missing uncompressed_bytes")?;
    if sha256_hex(&canonical) != declared_chunk_sha
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
        "bundle": bundle,
        "coverage_report": coverage_report,
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
            let (acquisition_manifest, acquisition_chunk) = fixture_result_parts(
                &fixture["manifest"],
                &fixture["bundle"],
                &fixture["coverage_report"],
                fixture["observations"]
                    .as_array()
                    .expect("canonical fixture observations are an array"),
            )?;
            let (boundary_manifest, boundary_chunk) = fixture_result_parts(
                &boundary["manifest"],
                &boundary["bundle"],
                &boundary["coverage_report"],
                boundary["candidates"]
                    .as_array()
                    .expect("boundary fixture candidates are an array"),
            )?;
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
