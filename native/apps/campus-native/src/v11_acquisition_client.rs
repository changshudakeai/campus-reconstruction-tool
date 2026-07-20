use campus_services::acquisition::{
    AcquisitionClient, AcquisitionJobState, AcquisitionJobStatus, AcquisitionTransport,
    CoarseRasterSupplementRequest, HttpsTransport, TransportError,
};

pub type ProductionAcquisitionClient = AcquisitionClient<HttpsTransport>;

pub fn materialize_coarse_raster_supplement<T: AcquisitionTransport>(
    client: &AcquisitionClient<T>,
    request: &CoarseRasterSupplementRequest,
    pinned_job: &AcquisitionJobStatus,
    run_id: impl Into<String>,
    requested_at: impl Into<String>,
) -> Result<campus_state::CoarseRasterSupplementRun, String> {
    let capabilities = client.capabilities().map_err(|error| error.to_string())?;
    let request_identity = campus_state::AcquisitionRequestIdentity::new(
        request.idempotency_key(),
        request
            .content_sha256()
            .map_err(|error| error.to_string())?,
    )?;
    let failure = || {
        pinned_job
            .failure
            .as_ref()
            .or_else(|| {
                pinned_job
                    .outcomes
                    .iter()
                    .find_map(|outcome| outcome.failure.as_ref())
            })
            .map(|failure| {
                serde_json::from_value::<campus_state::ServiceFailure>(
                    serde_json::to_value(failure).expect("service failure is serializable"),
                )
                .expect("service and state failure contracts match")
            })
    };
    let (manifest, outcome) = match pinned_job.state {
        AcquisitionJobState::Failed | AcquisitionJobState::Cancelled => (
            None,
            campus_state::CoarseRasterRunOutcome::ProviderFailure {
                failure: failure()
                    .ok_or("A failed coarse raster job must preserve its structured failure")?,
            },
        ),
        AcquisitionJobState::Partial => (
            None,
            campus_state::CoarseRasterRunOutcome::ProviderFailure {
                failure: failure()
                    .ok_or("A partial coarse raster job must preserve its structured failure")?,
            },
        ),
        AcquisitionJobState::Complete => {
            let (manifest, observations) = client
                .load_coarse_raster_result::<campus_state::CoarseRasterObservation>(pinned_job)
                .map_err(|error| error.to_string())?;
            let manifest = serde_json::from_value::<campus_state::ResultManifest>(
                serde_json::to_value(manifest).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            if observations.is_empty() {
                let unusable = failure().unwrap_or(campus_state::ServiceFailure {
                    code: "coarse-raster-empty".into(),
                    scope: request.linked_gap_id().into(),
                    retryable: false,
                    explanation: "The verified raster result contains no usable large component."
                        .into(),
                    suggested_action: "Leave the Known Feature Gap unresolved.".into(),
                });
                (
                    Some(manifest),
                    campus_state::CoarseRasterRunOutcome::UnusableCoverage { failure: unusable },
                )
            } else {
                (
                    Some(manifest),
                    campus_state::CoarseRasterRunOutcome::Proposals { observations },
                )
            }
        }
        AcquisitionJobState::Queued | AcquisitionJobState::Running => {
            return Err(
                "The coarse raster job is not terminal; poll or resume the pinned job".into(),
            );
        }
    };
    Ok(campus_state::CoarseRasterSupplementRun {
        id: run_id.into(),
        category: match request.category() {
            campus_services::acquisition::FoundationCategory::Water => {
                campus_state::FoundationCategory::Water
            }
            campus_services::acquisition::FoundationCategory::Vegetation => {
                campus_state::FoundationCategory::Vegetation
            }
            _ => return Err("Coarse raster runs support only water or vegetation".into()),
        },
        linked_gap_id: request.linked_gap_id().into(),
        dataset_bundle_id: request.bundle().id.clone(),
        requested_at: requested_at.into(),
        job_id: pinned_job.job_id.clone(),
        contract_version: pinned_job.contract_version.clone(),
        request_identity,
        retention_days: capabilities.retention_days,
        manifest,
        outcome,
    })
}
pub fn production_client(
    service_url: impl Into<String>,
    installation_credential: impl Into<String>,
) -> Result<ProductionAcquisitionClient, TransportError> {
    HttpsTransport::new(service_url, installation_credential).map(AcquisitionClient::new)
}

pub fn production_client_if_configured(
    service_url: Option<&str>,
) -> Result<Option<ProductionAcquisitionClient>, String> {
    let Some(service_url) = service_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let credential =
        keyring::Entry::new("CampusReconstructionTool", "acquisition-service-credential")
            .map_err(|error| format!("could not open the acquisition credential store: {error}"))?
            .get_password()
            .map_err(|error| {
                format!("could not read the acquisition service credential: {error}")
            })?;
    production_client(service_url, credential)
        .map(Some)
        .map_err(|error| error.explanation)
}

#[cfg(debug_assertions)]
pub fn bootstrap_fixture_if_enabled(
    is_debug_build: bool,
    enabled: Option<&str>,
) -> Result<Option<(usize, usize)>, String> {
    use campus_services::acquisition::fixture_transport::FixtureTransport;

    if !is_debug_build || enabled != Some("1") {
        return Ok(None);
    }
    let client = AcquisitionClient::new(FixtureTransport::canonical()?);
    let boundary = client
        .load_boundary_discovery("desktop-fixture-boundary")
        .map_err(|error| error.to_string())?;
    let acquisition = client
        .load_acquisition_result("desktop-fixture-acquisition")
        .map_err(|error| error.to_string())?;
    Ok(Some((
        boundary.candidates.len(),
        acquisition.observations.len(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_development_path_consumes_both_fixed_results() {
        assert_eq!(
            bootstrap_fixture_if_enabled(true, Some("1")).unwrap(),
            Some((2, 1))
        );
    }

    #[test]
    fn production_client_rejects_plaintext_remote_transport() {
        assert!(production_client("http://example.com", "secret").is_err());
    }

    #[test]
    fn shared_coarse_raster_fixture_materializes_as_typed_state_evidence() {
        use campus_services::acquisition::{
            fixture_transport::FixtureTransport, DatasetBundle, FoundationCategory, SourceGeometry,
        };

        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
        ))
        .unwrap();
        let bundle: DatasetBundle = serde_json::from_value(fixture["bundle"].clone()).unwrap();
        let request = CoarseRasterSupplementRequest::new(
            bundle,
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            FoundationCategory::Water,
            "gap:water:osm:31-121-1:0",
            "31/121/1",
            SourceGeometry::Polygon(vec![vec![
                [121.397, 31.218],
                [121.410, 31.218],
                [121.410, 31.230],
                [121.397, 31.230],
                [121.397, 31.218],
            ]]),
            "coarse-gap-water-v1.0.0",
            "typed-state-coarse-raster-replay",
        )
        .unwrap();
        let client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());
        let job = client.start_coarse_raster_supplement(&request).unwrap();

        let run = materialize_coarse_raster_supplement(
            &client,
            &request,
            &job,
            "coarse-run-shared-fixture",
            "unix-ms:1",
        )
        .unwrap();

        let campus_state::CoarseRasterRunOutcome::Proposals { observations } = run.outcome else {
            panic!("shared coarse fixture did not materialize proposals");
        };
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].id, "raster-water-east-v1");
        assert_eq!(
            observations[0].clip.gap_geometry,
            campus_state::SourceGeometry::Polygon(vec![vec![
                [121.397, 31.218],
                [121.410, 31.218],
                [121.410, 31.230],
                [121.397, 31.230],
                [121.397, 31.218],
            ]])
        );
    }

    #[test]
    fn partial_coarse_raster_status_preserves_its_retryable_failure() {
        use campus_services::acquisition::{
            DatasetBundle, FoundationCategory, ServiceFailure, SourceGeometry, TransportRequest,
            TransportResponse,
        };
        use std::collections::BTreeMap;

        struct CapabilitiesTransport(Vec<u8>);
        impl AcquisitionTransport for CapabilitiesTransport {
            fn execute(
                &self,
                _request: TransportRequest,
            ) -> Result<TransportResponse, TransportError> {
                Ok(TransportResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: self.0.clone(),
                })
            }
        }

        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
        ))
        .unwrap();
        let bundle: DatasetBundle = serde_json::from_value(fixture["bundle"].clone()).unwrap();
        let capabilities = serde_json::to_vec(&serde_json::json!({
            "contract_versions": [campus_services::acquisition::CONTRACT_VERSION],
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
        }))
        .unwrap();
        let request = CoarseRasterSupplementRequest::new(
            bundle,
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            FoundationCategory::Water,
            "gap:water:osm:31-121-1:0",
            "31/121/1",
            SourceGeometry::Polygon(vec![vec![
                [121.397, 31.218],
                [121.410, 31.218],
                [121.410, 31.230],
                [121.397, 31.230],
                [121.397, 31.218],
            ]]),
            "coarse-gap-water-v1.0.0",
            "partial-coarse-raster",
        )
        .unwrap();
        let failure = ServiceFailure {
            code: "cloud-cover".into(),
            scope: "31/121/1".into(),
            retryable: true,
            explanation: "Cloud cover interrupted the pinned observation.".into(),
            suggested_action: "Retry the same job.".into(),
        };
        let job = AcquisitionJobStatus {
            job_id: "coarse-raster-job-1".into(),
            contract_version: campus_services::acquisition::CONTRACT_VERSION.into(),
            bundle_id: request.bundle().id.clone(),
            state: AcquisitionJobState::Partial,
            outcomes: Vec::new(),
            failure: Some(failure),
            negotiated_bundle: None,
        };
        let client = AcquisitionClient::new(CapabilitiesTransport(capabilities));

        let run = materialize_coarse_raster_supplement(
            &client,
            &request,
            &job,
            "coarse-run-1",
            "unix-ms:1",
        )
        .unwrap();

        let campus_state::CoarseRasterRunOutcome::ProviderFailure { failure } = run.outcome else {
            panic!("partial status was incorrectly converted to proposals");
        };
        assert!(failure.retryable);
        assert_eq!(failure.code, "cloud-cover");
    }

    #[test]
    fn production_client_is_optional_until_a_service_is_configured() {
        assert!(production_client_if_configured(None).unwrap().is_none());
    }
}
