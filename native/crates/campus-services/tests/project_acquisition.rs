use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use base64::Engine;
use campus_services::acquisition::project_acquisition::{
    ProjectAcquisitionCoordinator, ProjectAcquisitionProgress,
};
use campus_services::acquisition::{
    AcquisitionClient, AcquisitionTransport, TransportError, TransportRequest, TransportResponse,
    CONTRACT_VERSION,
};
use campus_state::{
    CampusProjectLibrary, CampusScope, FoundationAcquisitionCheckpointPurpose, InstallationId,
    PinnedAcquisitionEvidence, PinnedBoundaryEvidence, ResultManifest, Schema2ProjectSession,
    SourceGeometry, SourceObservation, V11ConstructionCapability,
};

const ACQUISITION_FIXTURE: &str =
    include_str!("../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json");
const BOUNDARY_FIXTURE: &str =
    include_str!("../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json");

#[derive(Clone)]
struct SequentialTransport {
    requests: Rc<RefCell<Vec<TransportRequest>>>,
    responses: Rc<RefCell<VecDeque<TransportResponse>>>,
}

impl SequentialTransport {
    fn new(responses: Vec<TransportResponse>) -> Self {
        Self {
            requests: Rc::new(RefCell::new(Vec::new())),
            responses: Rc::new(RefCell::new(responses.into())),
        }
    }
}

impl AcquisitionTransport for SequentialTransport {
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

fn scope() -> CampusScope {
    CampusScope::new(
        "gaode:B00155J6JH",
        "East China Normal University Putuo Campus",
        [121.395, 31.202],
    )
    .unwrap()
}

fn actor() -> InstallationId {
    InstallationId::new("live-acquisition-test").unwrap()
}

fn boundary_evidence() -> PinnedBoundaryEvidence {
    let fixture: serde_json::Value = serde_json::from_str(BOUNDARY_FIXTURE).unwrap();
    PinnedBoundaryEvidence {
        manifest: serde_json::from_value(serde_json::json!({
            "contract_version": fixture["contract_version"],
            "bundle": fixture["bundle"],
            "coverage_report": fixture["coverage_report"],
            "licences": fixture["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|candidate| candidate["licence"].clone())
                .collect::<Vec<_>>(),
            "chunks": fixture["manifest"]["chunks"],
            "result_sha256": fixture["manifest"]["result_sha256"]
        }))
        .unwrap(),
        candidates: serde_json::from_value(fixture["candidates"].clone()).unwrap(),
        selected_candidate_id: "boundary-osm-relation-100".into(),
        confirmed_geometry: None,
        assessments: Default::default(),
    }
}

fn service_responses() -> Vec<TransportResponse> {
    let fixture: serde_json::Value = serde_json::from_str(ACQUISITION_FIXTURE).unwrap();
    let capabilities = || {
        json_response(serde_json::json!({
            "contract_versions": [CONTRACT_VERSION],
            "supported_bundles": [fixture["bundle"]],
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
    };
    let manifest = json_response(serde_json::json!({
        "contract_version": fixture["contract_version"],
        "bundle": fixture["bundle"],
        "coverage_report": fixture["coverage_report"],
        "licences": fixture["observations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|observation| observation["licence"].clone())
            .collect::<Vec<_>>(),
        "chunks": fixture["manifest"]["chunks"],
        "result_sha256": fixture["manifest"]["result_sha256"]
    }));
    let descriptor = &fixture["manifest"]["chunks"][0];
    let chunk = TransportResponse {
        status: 200,
        headers: BTreeMap::from([(
            "x-stable-cursor".into(),
            descriptor["stable_cursor"].as_str().unwrap().into(),
        )]),
        body: base64::engine::general_purpose::STANDARD
            .decode(
                fixture["transport_chunks"][descriptor["id"].as_str().unwrap()]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
    };
    let status = || {
        json_response(serde_json::json!({
            "job_id": "acquisition-job-9",
            "contract_version": CONTRACT_VERSION,
            "bundle_id": fixture["bundle"]["id"],
            "state": "partial",
            "outcomes": fixture["coverage_report"]["outcomes"]
        }))
    };
    vec![
        capabilities(),
        capabilities(),
        status(),
        manifest,
        chunk,
        status(),
    ]
}

fn pinned_acquisition_evidence() -> PinnedAcquisitionEvidence {
    let fixture: serde_json::Value = serde_json::from_str(ACQUISITION_FIXTURE).unwrap();
    PinnedAcquisitionEvidence {
        manifest: serde_json::from_value::<ResultManifest>(serde_json::json!({
            "contract_version": fixture["contract_version"],
            "bundle": fixture["bundle"],
            "coverage_report": fixture["coverage_report"],
            "licences": fixture["observations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|observation| observation["licence"].clone())
                .collect::<Vec<_>>(),
            "chunks": fixture["manifest"]["chunks"],
            "result_sha256": fixture["manifest"]["result_sha256"]
        }))
        .unwrap(),
        observations: serde_json::from_value::<Vec<SourceObservation>>(
            fixture["observations"].clone(),
        )
        .unwrap(),
    }
}

fn explicit_refresh_start_responses() -> Vec<TransportResponse> {
    let fixture: serde_json::Value = serde_json::from_str(ACQUISITION_FIXTURE).unwrap();
    let mut next_bundle = fixture["bundle"].clone();
    next_bundle["id"] = serde_json::json!("bundle-v1-next");
    next_bundle["osm_snapshot"] = serde_json::json!("osm-2026-07-15");
    next_bundle["overture_release"] = serde_json::json!("2026-07-15.0");
    let capabilities = || {
        json_response(serde_json::json!({
            "contract_versions": [CONTRACT_VERSION],
            "supported_bundles": [next_bundle, fixture["bundle"]],
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
    };
    let status = json_response(serde_json::json!({
        "job_id": "acquisition-refresh-job-10",
        "contract_version": CONTRACT_VERSION,
        "bundle_id": "bundle-v1-next",
        "state": "partial",
        "outcomes": fixture["coverage_report"]["outcomes"]
    }));
    let manifest = json_response(serde_json::json!({
        "contract_version": fixture["contract_version"],
        "bundle": next_bundle,
        "coverage_report": fixture["coverage_report"],
        "licences": fixture["observations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|observation| observation["licence"].clone())
            .collect::<Vec<_>>(),
        "chunks": fixture["manifest"]["chunks"],
        "result_sha256": fixture["manifest"]["result_sha256"]
    }));
    let descriptor = &fixture["manifest"]["chunks"][0];
    let chunk = TransportResponse {
        status: 200,
        headers: BTreeMap::from([(
            "x-stable-cursor".into(),
            descriptor["stable_cursor"].as_str().unwrap().into(),
        )]),
        body: base64::engine::general_purpose::STANDARD
            .decode(
                fixture["transport_chunks"][descriptor["id"].as_str().unwrap()]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
    };
    vec![capabilities(), capabilities(), status, manifest, chunk]
}

#[test]
fn native_project_saves_closes_reopens_and_resumes_a_partially_delivered_live_job() {
    let directory = std::env::temp_dir().join(format!(
        "campus-live-acquisition-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(&directory, "gaode:B00155J6JH", &capability)
            .unwrap();
    let project = library
        .create_project(scope(), "live resumable acquisition", actor())
        .unwrap();
    let project_id = project.id().clone();
    let transport = SequentialTransport::new(service_responses());
    let requests = transport.requests.clone();
    let client = AcquisitionClient::new(transport);
    let coordinator = ProjectAcquisitionCoordinator::new(&client);
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id).unwrap();
    let edited_boundary = SourceGeometry::Polygon(vec![vec![
        [121.397, 31.218],
        [121.411, 31.218],
        [121.410, 31.230],
        [121.397, 31.230],
        [121.397, 31.218],
    ]]);
    let mut reviewed_boundary = boundary_evidence();
    reviewed_boundary.confirmed_geometry = Some(edited_boundary.clone());
    session
        .apply_semantic_operation(
            &mut library,
            "persist boundary and queue five-category acquisition",
            |project| {
                project.confirm_boundary_and_queue_acquisition(
                    reviewed_boundary,
                    "installation-42:project-7:foundation-baseline",
                    actor(),
                )
            },
        )
        .unwrap();
    assert!(requests.borrow().is_empty());
    assert!(session
        .active()
        .unwrap()
        .pending_acquisition_start()
        .is_some());
    session
        .apply_semantic_operation(
            &mut library,
            "start persisted five-category acquisition",
            |project| {
                coordinator
                    .start_queued_boundary_acquisition(project, actor())
                    .map(|_| ())
            },
        )
        .unwrap();
    assert!(session
        .active()
        .unwrap()
        .pending_acquisition_start()
        .is_none());
    let acquisition_request = requests
        .borrow()
        .iter()
        .find(|request| request.path.ends_with("/acquisition-jobs"))
        .unwrap()
        .body
        .as_ref()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body).unwrap())
        .unwrap();
    assert_eq!(
        acquisition_request["boundary_wgs84"],
        serde_json::to_value(&edited_boundary).unwrap(),
        "the first five-category job must use the reviewed geometry, not the source candidate"
    );
    session
        .apply_semantic_operation(&mut library, "pin live manifest", |project| {
            coordinator.pin_manifest(project, actor()).map(|_| ())
        })
        .unwrap();
    assert_eq!(
        session
            .apply_semantic_operation(&mut library, "persist verified chunk", |project| {
                coordinator.download_next_chunk(project, actor())
            })
            .unwrap(),
        ProjectAcquisitionProgress::ChunkPersisted {
            chunk_id: "observations-0001".into()
        }
    );
    drop(session);
    drop(library);

    let mut reopened_library = CampusProjectLibrary::open(&directory, "gaode:B00155J6JH").unwrap();
    let mut reopened = Schema2ProjectSession::default();
    reopened
        .open_project(&reopened_library, &project_id)
        .unwrap();
    let checkpoint = reopened.active().unwrap().acquisition_checkpoint().unwrap();
    assert_eq!(checkpoint.job_id, "acquisition-job-9");
    assert_eq!(checkpoint.verified_chunks.len(), 1);
    assert_eq!(checkpoint.verified_chunks[0].observations.len(), 1);
    reopened
        .apply_semantic_operation(
            &mut reopened_library,
            "pin verified acquisition evidence",
            |project| coordinator.finalize(project, actor()).map(|_| ()),
        )
        .unwrap();
    let pinned = reopened.active().unwrap().pinned_evidence().unwrap();
    assert_eq!(pinned.acquisition.observations.len(), 1);
    reopened
        .apply_semantic_operation(
            &mut reopened_library,
            "reconnect pinned acquisition job",
            |project| coordinator.reconnect(project, actor()).map(|_| ()),
        )
        .unwrap();
    assert_eq!(
        reopened
            .active()
            .unwrap()
            .pinned_evidence()
            .unwrap()
            .acquisition
            .observations
            .len(),
        1,
        "delivery controls must preserve already pinned successful evidence"
    );
    assert_eq!(
        requests
            .borrow()
            .iter()
            .filter(|request| request.path.contains("/chunks/"))
            .count(),
        1
    );
    drop(reopened);
    drop(reopened_library);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn user_explicit_refresh_pins_a_new_bundle_without_replacing_current_evidence() {
    let directory = std::env::temp_dir().join(format!(
        "campus-explicit-refresh-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(&directory, "gaode:B00155J6JH", &capability)
            .unwrap();
    let mut project = library
        .create_project(scope(), "explicit controlled refresh", actor())
        .unwrap();
    project
        .confirm_boundary(boundary_evidence(), actor())
        .unwrap();
    project
        .pin_acquisition(pinned_acquisition_evidence(), actor())
        .unwrap();
    let current_result = project
        .pinned_evidence()
        .unwrap()
        .acquisition
        .manifest
        .result_sha256
        .clone();
    let transport = SequentialTransport::new(explicit_refresh_start_responses());
    let requests = transport.requests.clone();
    let client = AcquisitionClient::new(transport);
    let coordinator = ProjectAcquisitionCoordinator::new(&client);

    coordinator
        .start_explicit_refresh(
            &mut project,
            "installation-42:project-7:explicit-refresh-2026-07",
            actor(),
        )
        .unwrap();

    assert_eq!(
        project.acquisition_checkpoint_purpose(),
        FoundationAcquisitionCheckpointPurpose::ExplicitRefresh
    );
    assert_eq!(
        project.acquisition_checkpoint().unwrap().bundle.id,
        "bundle-v1-next"
    );
    assert_eq!(
        project
            .pinned_evidence()
            .unwrap()
            .acquisition
            .manifest
            .result_sha256,
        current_result,
        "starting a refresh must not replace current evidence before verified finalization"
    );
    let request = requests
        .borrow()
        .iter()
        .find(|request| request.path.ends_with("/acquisition-jobs"))
        .unwrap()
        .body
        .as_ref()
        .map(|body| serde_json::from_slice::<serde_json::Value>(body).unwrap())
        .unwrap();
    assert_eq!(request["bundle_id"], "bundle-v1-next");
    coordinator.pin_manifest(&mut project, actor()).unwrap();
    assert_eq!(
        coordinator
            .download_next_chunk(&mut project, actor())
            .unwrap(),
        ProjectAcquisitionProgress::ChunkPersisted {
            chunk_id: "observations-0001".into()
        }
    );
    assert_eq!(
        coordinator.finalize(&mut project, actor()).unwrap(),
        ProjectAcquisitionProgress::EvidencePinned
    );
    assert_eq!(
        project
            .pinned_evidence()
            .unwrap()
            .acquisition
            .manifest
            .bundle
            .id,
        "bundle-v1-next"
    );
    assert_eq!(project.acquisition_refresh_history().len(), 1);
    assert_eq!(
        project.acquisition_checkpoint_purpose(),
        FoundationAcquisitionCheckpointPurpose::Initial
    );
    std::fs::remove_dir_all(directory).unwrap();
}
