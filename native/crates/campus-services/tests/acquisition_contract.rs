use std::collections::BTreeMap;
use std::io::Write;

use campus_services::acquisition::{
    AcquisitionClient, AcquisitionClientErrorKind, AcquisitionTransport, TransportRequest,
    TransportResponse, CONTRACT_VERSION,
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

#[cfg(feature = "fixture-transport")]
#[test]
fn feature_gated_fixture_transport_serves_both_public_client_paths() {
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
