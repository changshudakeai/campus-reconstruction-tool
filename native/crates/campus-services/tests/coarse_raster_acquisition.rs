use campus_services::acquisition::fixture_transport::FixtureTransport;
use campus_services::acquisition::{
    AcquisitionClient, AcquisitionTransport, CoarseRasterSupplementRequest, DatasetBundle,
    FoundationCategory, SourceGeometry, TransportError, TransportRequest, TransportResponse,
    CONTRACT_VERSION,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

#[derive(Clone)]
struct RecordingTransport {
    requests: Rc<RefCell<Vec<TransportRequest>>>,
    responses: Rc<RefCell<VecDeque<TransportResponse>>>,
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

fn response(value: serde_json::Value) -> TransportResponse {
    TransportResponse {
        status: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn bundle() -> DatasetBundle {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["bundle"].clone()).unwrap()
}

fn gap_geometry() -> SourceGeometry {
    SourceGeometry::Polygon(vec![vec![
        [121.397, 31.218],
        [121.410, 31.218],
        [121.410, 31.230],
        [121.397, 31.230],
        [121.397, 31.218],
    ]])
}

#[test]
fn coarse_raster_job_uses_the_controlled_bundle_and_stable_replay_identity() {
    let bundle = bundle();
    let capabilities = serde_json::json!({
        "contract_versions": [CONTRACT_VERSION],
        "supported_bundles": [bundle.clone()],
        "limits": {
            "area_square_metres": 100000000,
            "boundary_vertices": 10000,
            "tiles": 10000,
            "observations": 1000000,
            "result_bytes": 1000000000,
            "concurrent_jobs": 2
        },
        "retention_days": 30
    });
    let job = serde_json::json!({
        "job_id": "coarse-raster-water-1",
        "contract_version": CONTRACT_VERSION,
        "bundle_id": bundle.id,
        "state": "running",
        "outcomes": []
    });
    let transport = RecordingTransport {
        requests: Rc::new(RefCell::new(Vec::new())),
        responses: Rc::new(RefCell::new(
            vec![
                response(capabilities.clone()),
                response(job.clone()),
                response(capabilities),
                response(job),
            ]
            .into(),
        )),
    };
    let client = AcquisitionClient::new(transport.clone());
    let request = CoarseRasterSupplementRequest::new(
        bundle,
        "boundary-result-sha256",
        FoundationCategory::Water,
        "gap:water:osm:31-121-1:0",
        "31/121/1",
        gap_geometry(),
        "coarse-gap-water-v1.0.0",
        "project-1/coarse-water-gap-1",
    )
    .unwrap();

    let first = client.start_coarse_raster_supplement(&request).unwrap();
    let second = client.start_coarse_raster_supplement(&request).unwrap();
    assert_eq!(first.job_id, second.job_id);

    let requests = transport.requests.borrow();
    assert_eq!(requests[1].path, "/v1/coarse-raster-jobs");
    assert_eq!(requests[3].path, "/v1/coarse-raster-jobs");
    assert_eq!(requests[1].body, requests[3].body);
    let body: serde_json::Value =
        serde_json::from_slice(requests[1].body.as_ref().unwrap()).unwrap();
    assert_eq!(
        body["request_identity"]["idempotency_key"],
        "project-1/coarse-water-gap-1"
    );
    assert_eq!(body["algorithm_version"], "coarse-gap-water-v1.0.0");
    assert_eq!(body["gap_geometry"]["type"], "Polygon");
}

#[test]
fn shared_coarse_raster_fixture_replays_through_the_rust_client() {
    let client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());
    let request = CoarseRasterSupplementRequest::new(
        bundle(),
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        FoundationCategory::Water,
        "gap:water:osm:31-121-1:0",
        "31/121/1",
        gap_geometry(),
        "coarse-gap-water-v1.0.0",
        "project-1/shared-coarse-raster-replay",
    )
    .unwrap();

    let job = client.start_coarse_raster_supplement(&request).unwrap();
    let (manifest, observations) = client
        .load_coarse_raster_result::<serde_json::Value>(&job)
        .unwrap();

    assert_eq!(
        job.state,
        campus_services::acquisition::AcquisitionJobState::Complete
    );
    assert_eq!(
        manifest.result_sha256,
        "6427c56d16e9e3e20fe3e6fcc88dd7ee9f8e321e84b3996575a277821fe60c92"
    );
    assert_eq!(observations[0]["id"], "raster-water-east-v1");
    assert_eq!(observations[0]["clip"]["gapGeometry"]["type"], "Polygon");
}
