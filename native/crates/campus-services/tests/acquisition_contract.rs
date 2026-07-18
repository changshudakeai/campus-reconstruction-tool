use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::rc::Rc;

use campus_services::acquisition::{
    AcquisitionClient, AcquisitionClientErrorKind, AcquisitionTransport, DatasetBundle,
    FoundationAcquisitionRequest, FoundationCategory, OutcomeScope, SourceGeometry,
    TransportRequest, TransportResponse, VerifiedAcquisitionChunk, CONTRACT_VERSION,
};
use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};

const SHARED_FIXTURE: &str =
    include_str!("../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json");
const BOUNDARY_FIXTURE: &str =
    include_str!("../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json");

#[derive(Clone)]
struct SharedFixtureTransport {
    manifest: Vec<u8>,
    chunk: Vec<u8>,
    cursor: String,
}

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
    fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, campus_services::acquisition::TransportError> {
        self.requests.borrow_mut().push(request);
        self.responses.borrow_mut().pop_front().ok_or_else(|| {
            campus_services::acquisition::TransportError {
                explanation: "test transport ran out of responses".into(),
            }
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

fn canonical_bundle() -> DatasetBundle {
    serde_json::from_value(serde_json::json!({
        "id": "cn-campus-2026-06",
        "osm_snapshot": "osm-cn-2026-06-30",
        "overture_release": "2026-06-17.0",
        "output_schema": "source-observation-1.0.0",
        "classification_rules": "classification-1.0.0",
        "assembly_rules": "assembly-1.0.0",
        "conflation_rules": "conflation-1.0.0",
        "derivation_rules": "derivation-1.0.0"
    }))
    .unwrap()
}

fn capabilities_response() -> TransportResponse {
    json_response(serde_json::json!({
        "contract_versions": [CONTRACT_VERSION],
        "supported_bundles": [canonical_bundle()],
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

impl AcquisitionTransport for SharedFixtureTransport {
    fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, campus_services::acquisition::TransportError> {
        if request.path.ends_with("/manifest") {
            return Ok(TransportResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: self.manifest.clone(),
            });
        }
        Ok(TransportResponse {
            status: 200,
            headers: BTreeMap::from([("x-stable-cursor".into(), self.cursor.clone())]),
            body: self.chunk.clone(),
        })
    }
}

fn result_parts(
    bundle: &serde_json::Value,
    coverage_report: &serde_json::Value,
    records: &[serde_json::Value],
) -> (Vec<u8>, Vec<u8>) {
    let mut canonical = Vec::new();
    for record in records {
        canonical.extend(serde_json::to_vec(record).unwrap());
        canonical.push(b'\n');
    }
    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(&canonical).unwrap();
    let compressed = encoder.finish().unwrap();
    let sha = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let manifest = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "bundle": bundle,
        "coverage_report": coverage_report,
        "licences": [],
        "chunks": [{
            "id": "observations-0001",
            "stable_cursor": "v1:observations:0001:end",
            "content_type": "application/x-ndjson",
            "content_encoding": "gzip",
            "sha256": sha(&compressed),
            "uncompressed_bytes": canonical.len()
        }],
        "result_sha256": sha(&canonical)
    });
    (serde_json::to_vec(&manifest).unwrap(), compressed)
}

fn two_chunk_result_parts() -> (Vec<u8>, Vec<Vec<u8>>, Vec<String>) {
    let fixture: serde_json::Value = serde_json::from_str(SHARED_FIXTURE).unwrap();
    let mut records = vec![fixture["observations"][0].clone(); 2];
    records[1]["id"] = serde_json::json!("obs-osm-relation-43");
    records[1]["lineage"]["source_record_id"] = serde_json::json!("relation/43");
    let mut compressed_chunks = Vec::new();
    let mut canonical_chunks = Vec::new();
    let mut descriptors = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let canonical = format!("{}\n", serde_json::to_string(record).unwrap());
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), Compression::default());
        encoder.write_all(canonical.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        descriptors.push(serde_json::json!({
            "id": format!("observations-{:04}", index + 1),
            "stable_cursor": format!("v1:observations:{:04}:end", index + 1),
            "content_type": "application/x-ndjson",
            "content_encoding": "gzip",
            "sha256": format!("{:x}", Sha256::digest(&compressed)),
            "uncompressed_bytes": canonical.len()
        }));
        compressed_chunks.push(compressed);
        canonical_chunks.push(canonical);
    }
    let canonical_result = canonical_chunks.concat();
    let manifest = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "bundle": fixture["bundle"],
        "coverage_report": fixture["coverage_report"],
        "licences": records
            .iter()
            .map(|record| record["licence"].clone())
            .collect::<Vec<_>>(),
        "chunks": descriptors,
        "result_sha256": format!("{:x}", Sha256::digest(canonical_result.as_bytes()))
    });
    (
        serde_json::to_vec(&manifest).unwrap(),
        compressed_chunks,
        canonical_chunks,
    )
}

#[test]
fn confirmed_boundary_starts_one_idempotent_five_category_job_on_its_dataset_bundle() {
    let transport = RecordingTransport::new(vec![
        capabilities_response(),
        json_response(serde_json::json!({
            "job_id": "acquisition-job-9",
            "contract_version": CONTRACT_VERSION,
            "bundle_id": "cn-campus-2026-06",
            "state": "running",
            "outcomes": []
        })),
    ]);
    let requests = transport.requests.clone();
    let client = AcquisitionClient::new(transport);
    let request = FoundationAcquisitionRequest::new(
        canonical_bundle(),
        "boundary-result-sha-256",
        SourceGeometry::Polygon(vec![vec![
            [121.395, 31.202],
            [121.405, 31.202],
            [121.405, 31.212],
            [121.395, 31.202],
        ]]),
        "installation-42:project-7:foundation-baseline",
    )
    .unwrap();

    let job = client.start_foundation_acquisition(&request).unwrap();

    assert_eq!(job.job_id, "acquisition-job-9");
    assert_eq!(job.bundle_id, "cn-campus-2026-06");
    assert_eq!(job.negotiated_bundle.as_ref(), Some(&canonical_bundle()));
    let requests = requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/v1/capabilities");
    assert_eq!(requests[1].path, "/v1/acquisition-jobs");
    let body: serde_json::Value =
        serde_json::from_slice(requests[1].body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["categories"],
        serde_json::json!(["building", "circulation", "water", "vegetation", "sports"])
    );
    assert_eq!(body["bundle_id"], "cn-campus-2026-06");
    assert_eq!(body["boundary_revision"], "boundary-result-sha-256");
    assert_eq!(
        body["request_identity"]["idempotency_key"],
        "installation-42:project-7:foundation-baseline"
    );
    let mut canonical_content = body.clone();
    canonical_content
        .as_object_mut()
        .unwrap()
        .remove("request_identity");
    assert_eq!(
        body["request_identity"]["content_sha256"],
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&canonical_content).unwrap())
        )
    );
}

#[test]
fn reconnect_scoped_retry_and_cancel_preserve_the_acquisition_job_and_outcomes() {
    let outcomes = serde_json::json!([
        {
            "provider": "osm",
            "category": "building",
            "tile_id": "tile-1",
            "status": "complete",
            "pagination_exhausted": true,
            "raw_count": 2,
            "deduplicated_count": 2,
            "relation_members_complete": true,
            "gaps": []
        },
        {
            "provider": "overture",
            "category": "vegetation",
            "tile_id": "tile-2",
            "status": "failed",
            "pagination_exhausted": false,
            "raw_count": 0,
            "deduplicated_count": 0,
            "relation_members_complete": true,
            "gaps": ["provider page unavailable"],
            "failure": {
                "code": "provider_unavailable",
                "scope": "overture/vegetation/tile-2",
                "retryable": true,
                "explanation": "Pinned provider page did not respond.",
                "suggested_action": "Retry this scope."
            }
        }
    ]);
    let status = |state: &str| {
        json_response(serde_json::json!({
            "job_id": "acquisition-job-9",
            "contract_version": CONTRACT_VERSION,
            "bundle_id": "cn-campus-2026-06",
            "state": state,
            "outcomes": outcomes
        }))
    };
    let transport = RecordingTransport::new(vec![
        status("partial"),
        status("running"),
        status("cancelled"),
    ]);
    let requests = transport.requests.clone();
    let client = AcquisitionClient::new(transport);
    let pinned = campus_services::acquisition::AcquisitionJobStatus {
        job_id: "acquisition-job-9".into(),
        contract_version: CONTRACT_VERSION.into(),
        bundle_id: canonical_bundle().id.clone(),
        state: campus_services::acquisition::AcquisitionJobState::Partial,
        outcomes: serde_json::from_value(outcomes).unwrap(),
        failure: None,
        negotiated_bundle: Some(canonical_bundle()),
    };
    let scope = OutcomeScope::new("overture", FoundationCategory::Vegetation, "tile-2").unwrap();

    let reconnected = client.acquisition_job(&pinned).unwrap();
    let retried = client
        .retry_foundation_acquisition(&reconnected, &[scope])
        .unwrap();
    let cancelled = client.cancel_foundation_acquisition(&retried).unwrap();

    assert_eq!(reconnected.job_id, pinned.job_id);
    assert_eq!(reconnected.outcomes.len(), 2);
    assert_eq!(
        reconnected.outcomes[0].status,
        campus_services::acquisition::ProviderOutcomeStatus::Complete
    );
    assert_eq!(
        reconnected.outcomes[1].status,
        campus_services::acquisition::ProviderOutcomeStatus::Failed
    );
    assert_eq!(retried.bundle_id, pinned.bundle_id);
    assert_eq!(cancelled.job_id, pinned.job_id);
    assert_eq!(
        cancelled.state,
        campus_services::acquisition::AcquisitionJobState::Cancelled
    );
    let requests = requests.borrow();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/v1/acquisition-jobs/acquisition-job-9",
            "/v1/acquisition-jobs/acquisition-job-9/retry",
            "/v1/acquisition-jobs/acquisition-job-9/cancel"
        ]
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(requests[1].body.as_deref().unwrap()).unwrap(),
        serde_json::json!({
            "scopes": [{
                "provider": "overture",
                "category": "vegetation",
                "tile_id": "tile-2"
            }]
        })
    );
}

#[test]
fn complete_coverage_requires_page_exhaustion_and_complete_relation_membership() {
    let transport = RecordingTransport::new(vec![json_response(serde_json::json!({
        "job_id": "acquisition-job-9",
        "contract_version": CONTRACT_VERSION,
        "bundle_id": "cn-campus-2026-06",
        "state": "complete",
        "outcomes": [{
            "provider": "osm",
            "category": "water",
            "tile_id": "tile-3",
            "status": "complete",
            "pagination_exhausted": false,
            "raw_count": 1,
            "deduplicated_count": 1,
            "relation_members_complete": false,
            "gaps": ["relation way/88 missing"]
        }]
    }))]);
    let client = AcquisitionClient::new(transport);
    let pinned = campus_services::acquisition::AcquisitionJobStatus {
        job_id: "acquisition-job-9".into(),
        contract_version: CONTRACT_VERSION.into(),
        bundle_id: canonical_bundle().id.clone(),
        state: campus_services::acquisition::AcquisitionJobState::Running,
        outcomes: Vec::new(),
        failure: None,
        negotiated_bundle: Some(canonical_bundle()),
    };

    let error = client.acquisition_job(&pinned).unwrap_err();

    assert_eq!(error.kind, AcquisitionClientErrorKind::IntegrityFailure);
    assert!(error.explanation.contains("page exhaustion"));
    assert!(error.explanation.contains("relation membership"));
}

#[test]
fn failed_coverage_without_a_structured_failure_is_rejected() {
    let transport = RecordingTransport::new(vec![json_response(serde_json::json!({
        "job_id": "acquisition-job-9",
        "contract_version": CONTRACT_VERSION,
        "bundle_id": "cn-campus-2026-06",
        "state": "failed",
        "outcomes": [{
            "provider": "overture",
            "category": "vegetation",
            "tile_id": "tile-2",
            "status": "failed",
            "pagination_exhausted": false,
            "raw_count": 0,
            "deduplicated_count": 0,
            "relation_members_complete": true,
            "gaps": ["provider page unavailable"]
        }]
    }))]);
    let client = AcquisitionClient::new(transport);
    let pinned = campus_services::acquisition::AcquisitionJobStatus {
        job_id: "acquisition-job-9".into(),
        contract_version: CONTRACT_VERSION.into(),
        bundle_id: canonical_bundle().id.clone(),
        state: campus_services::acquisition::AcquisitionJobState::Running,
        outcomes: Vec::new(),
        failure: None,
        negotiated_bundle: Some(canonical_bundle()),
    };

    let error = client.acquisition_job(&pinned).unwrap_err();

    assert_eq!(error.kind, AcquisitionClientErrorKind::IntegrityFailure);
    assert!(error.explanation.contains("structured failure"));
}

#[test]
fn resume_download_skips_verified_chunks_and_reassembles_the_pinned_result() {
    let (manifest, compressed, canonical) = two_chunk_result_parts();
    let manifest_value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    let first_observations = vec![serde_json::from_str(canonical[0].trim_end()).unwrap()];
    let first = VerifiedAcquisitionChunk {
        descriptor: serde_json::from_value(manifest_value["chunks"][0].clone()).unwrap(),
        canonical_ndjson: canonical[0].clone(),
        observations: first_observations,
    };
    let transport = RecordingTransport::new(vec![
        TransportResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: manifest,
        },
        TransportResponse {
            status: 200,
            headers: BTreeMap::from([(
                "x-stable-cursor".into(),
                "v1:observations:0002:end".into(),
            )]),
            body: compressed[1].clone(),
        },
    ]);
    let requests = transport.requests.clone();
    let client = AcquisitionClient::new(transport);
    let pinned = campus_services::acquisition::AcquisitionJobStatus {
        job_id: "acquisition-job-9".into(),
        contract_version: CONTRACT_VERSION.into(),
        bundle_id: canonical_bundle().id.clone(),
        state: campus_services::acquisition::AcquisitionJobState::Partial,
        outcomes: Vec::new(),
        failure: None,
        negotiated_bundle: Some(canonical_bundle()),
    };

    let delivery = client
        .resume_foundation_delivery(&pinned, &[first])
        .unwrap();

    assert_eq!(delivery.verified_chunks.len(), 2);
    assert_eq!(delivery.observations.len(), 2);
    assert_eq!(delivery.observations[0].id, "obs-osm-relation-42");
    assert_eq!(delivery.observations[1].id, "obs-osm-relation-43");
    let requests = requests.borrow();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].path,
        "/v1/acquisition-jobs/acquisition-job-9/manifest"
    );
    assert!(requests[1].path.contains("observations-0002"));
    assert!(!requests
        .iter()
        .any(|request| request.path.contains("observations-0001")));
}

#[test]
fn shared_fixture_decodes_without_provider_payload_types() {
    let fixture: serde_json::Value = serde_json::from_str(SHARED_FIXTURE).unwrap();
    let (manifest, compressed) = result_parts(
        &fixture["bundle"],
        &fixture["coverage_report"],
        fixture["observations"].as_array().unwrap(),
    );
    let client = AcquisitionClient::new(SharedFixtureTransport {
        manifest,
        chunk: compressed,
        cursor: "v1:observations:0001:end".into(),
    });

    let result = client.load_acquisition_result("fixture-job").unwrap();

    assert_eq!(result.manifest.contract_version, CONTRACT_VERSION);
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.observations[0].lineage.provider, "osm");
    assert_eq!(result.observations[0].geometry.type_name(), "MultiPolygon");
}

#[test]
fn boundary_snapshot_uses_the_same_verified_transport_boundary() {
    let fixture: serde_json::Value = serde_json::from_str(BOUNDARY_FIXTURE).unwrap();
    let (manifest, compressed) = result_parts(
        &fixture["bundle"],
        &fixture["coverage_report"],
        fixture["candidates"].as_array().unwrap(),
    );
    let client = AcquisitionClient::new(SharedFixtureTransport {
        manifest,
        chunk: compressed,
        cursor: "v1:observations:0001:end".into(),
    });

    let snapshot = client
        .load_boundary_discovery("fixture-boundary-job")
        .unwrap();

    assert_eq!(snapshot.candidates.len(), 2);
    assert_eq!(snapshot.candidates[0].rank, 1);
    assert_eq!(
        snapshot.candidates[0]
            .lineage
            .relation
            .as_ref()
            .unwrap()
            .assembly_status,
        "complete"
    );
}

#[test]
fn rejects_cursor_drift_before_committing_observations() {
    let fixture: serde_json::Value = serde_json::from_str(SHARED_FIXTURE).unwrap();
    let (manifest, compressed) = result_parts(
        &fixture["bundle"],
        &fixture["coverage_report"],
        fixture["observations"].as_array().unwrap(),
    );
    let client = AcquisitionClient::new(SharedFixtureTransport {
        manifest,
        chunk: compressed,
        cursor: "changed-cursor".into(),
    });

    let error = client.load_acquisition_result("fixture-job").unwrap_err();

    assert_eq!(error.kind, AcquisitionClientErrorKind::IntegrityFailure);
    assert!(error.action.contains("resume"));
}

#[test]
fn rejects_corrupt_chunk_and_maps_structured_service_failure() {
    let fixture: serde_json::Value = serde_json::from_str(SHARED_FIXTURE).unwrap();
    let (manifest, mut compressed) = result_parts(
        &fixture["bundle"],
        &fixture["coverage_report"],
        fixture["observations"].as_array().unwrap(),
    );
    compressed[0] ^= 0xff;
    let client = AcquisitionClient::new(SharedFixtureTransport {
        manifest,
        chunk: compressed,
        cursor: "v1:observations:0001:end".into(),
    });
    assert_eq!(
        client
            .load_acquisition_result("fixture-job")
            .unwrap_err()
            .kind,
        AcquisitionClientErrorKind::IntegrityFailure
    );

    let error = campus_services::acquisition::map_service_failure(
        503,
        br#"{"code":"provider_unavailable","scope":"osm/water/tile-1","retryable":true,"explanation":"Pinned source timed out.","suggested_action":"Retry this tile."}"#,
    );
    assert_eq!(error.kind, AcquisitionClientErrorKind::RetryableScope);
    assert_eq!(error.action, "Retry this tile.");
}

#[test]
fn rejects_an_incompatible_contract_before_downloading_chunks() {
    let fixture: serde_json::Value = serde_json::from_str(SHARED_FIXTURE).unwrap();
    let (manifest, compressed) = result_parts(
        &fixture["bundle"],
        &fixture["coverage_report"],
        fixture["observations"].as_array().unwrap(),
    );
    let mut manifest_value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    manifest_value["contract_version"] = serde_json::json!("2.0.0");
    let client = AcquisitionClient::new(SharedFixtureTransport {
        manifest: serde_json::to_vec(&manifest_value).unwrap(),
        chunk: compressed,
        cursor: "v1:observations:0001:end".into(),
    });

    let error = client.load_acquisition_result("fixture-job").unwrap_err();

    assert_eq!(error.kind, AcquisitionClientErrorKind::IncompatibleContract);
}

#[cfg(debug_assertions)]
#[test]
fn debug_only_fixture_transport_serves_both_public_client_paths() {
    use campus_services::acquisition::fixture_transport::FixtureTransport;

    let client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());

    assert_eq!(
        client
            .load_boundary_discovery("fixture-boundary-job")
            .unwrap()
            .candidates
            .len(),
        2
    );
    assert_eq!(
        client
            .load_acquisition_result("fixture-acquisition-job")
            .unwrap()
            .observations
            .len(),
        1
    );
}
