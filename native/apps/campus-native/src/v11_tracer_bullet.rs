#[cfg(debug_assertions)]
use campus_export::{
    write_foundation_manifest, write_schematic, FoundationManifest, FoundationManifestSchematic,
    VoxelModel,
};
#[cfg(debug_assertions)]
use campus_services::acquisition::{
    AcquisitionClient, AcquisitionTransport, VerifiedBoundaryDiscoverySnapshot,
};
use campus_state::ProjectId;
#[cfg(test)]
use campus_state::Schema2Project;
#[cfg(debug_assertions)]
use campus_state::{
    CampusProjectLibrary, CampusScope, FoundationCategory, FoundationReviewDisposition,
    InstallationId, PinnedAcquisitionEvidence, ResultManifest, SourceObservation,
    V11ConstructionCapability,
};
#[cfg(debug_assertions)]
use serde::de::DeserializeOwned;
#[cfg(debug_assertions)]
use serde::Serialize;
#[cfg(debug_assertions)]
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[cfg(debug_assertions)]
pub struct FixedDatasetTracer<'a, T> {
    library_root: PathBuf,
    output_root: PathBuf,
    campus_target_id: String,
    actor: InstallationId,
    capability: &'a V11ConstructionCapability,
    acquisition_client: &'a AcquisitionClient<T>,
    boundary_job_id: String,
    acquisition_job_id: String,
    project_id: ProjectId,
}

#[cfg(debug_assertions)]
pub struct FixedDatasetTracerRequest {
    pub library_root: PathBuf,
    pub output_root: PathBuf,
    pub campus_scope: CampusScope,
    pub project_name: String,
    pub boundary_job_id: String,
    pub acquisition_job_id: String,
}

#[derive(Debug)]
pub struct FixedDatasetTracerReport {
    pub project_id: ProjectId,
    pub schematic_path: PathBuf,
    pub schematic_bytes: u64,
    pub manifest_path: PathBuf,
}

#[cfg(debug_assertions)]
impl<'a, T: AcquisitionTransport> FixedDatasetTracer<'a, T> {
    pub fn confirm_campus_target(
        request: FixedDatasetTracerRequest,
        actor: InstallationId,
        capability: &'a V11ConstructionCapability,
        acquisition_client: &'a AcquisitionClient<T>,
    ) -> Result<Self, String> {
        let campus_target_id = request.campus_scope.target_id().to_string();
        let mut library = CampusProjectLibrary::open_for_construction(
            &request.library_root,
            campus_target_id.clone(),
            capability,
        )?;
        let project =
            library.create_project(request.campus_scope, request.project_name, actor.clone())?;
        Ok(Self {
            library_root: request.library_root,
            output_root: request.output_root,
            campus_target_id,
            actor,
            capability,
            acquisition_client,
            boundary_job_id: request.boundary_job_id,
            acquisition_job_id: request.acquisition_job_id,
            project_id: project.id().clone(),
        })
    }

    pub fn boundary_candidates(&self) -> Result<VerifiedBoundaryDiscoverySnapshot, String> {
        self.acquisition_client
            .load_boundary_discovery(&self.boundary_job_id)
            .map_err(|error| error.to_string())
    }

    pub fn select_boundary(
        &self,
        snapshot: VerifiedBoundaryDiscoverySnapshot,
        candidate_id: &str,
    ) -> Result<(), String> {
        let mut desk =
            super::v11_boundary_evidence_desk::evidence_desk_from_verified_snapshot(&snapshot)?;
        desk.select_candidate(candidate_id)?;
        let _map_surface = super::v11_boundary_evidence_desk::map_boundary_desk(&desk)
            .ok_or("The Boundary Discovery Snapshot cannot be presented on the evidence desk")?;
        let evidence = desk.to_pinned_evidence()?;
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(&mut library, "confirm Campus Boundary", |project| {
            project.confirm_boundary(evidence, self.actor.clone())
        })
    }

    pub fn acquire_foundation_evidence(&self) -> Result<(), String> {
        let acquisition = self
            .acquisition_client
            .load_acquisition_result(&self.acquisition_job_id)
            .map_err(|error| error.to_string())?;
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            "pin five-category acquisition evidence",
            |project| {
                project.pin_acquisition(
                    PinnedAcquisitionEvidence {
                        manifest: copy_typed::<_, ResultManifest>(&acquisition.manifest)?,
                        observations: copy_typed::<_, Vec<SourceObservation>>(
                            &acquisition.observations,
                        )?,
                    },
                    self.actor.clone(),
                )
            },
        )
    }

    pub fn complete_review(
        &self,
        category: FoundationCategory,
        disposition: FoundationReviewDisposition,
    ) -> Result<(), String> {
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            format!("confirm {category:?} review"),
            |project| project.complete_foundation_review(category, disposition, self.actor.clone()),
        )
    }

    #[cfg(test)]
    pub fn project(&self) -> Result<Schema2Project, String> {
        self.open_library()?.open_project(&self.project_id)
    }

    pub fn generate_and_export(&self) -> Result<FixedDatasetTracerReport, String> {
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            "generate and export fixed-dataset Foundation",
            |project| {
                let generated = arnis_core::generate_foundation(&project.reviewed_projection()?)?;
                let model = VoxelModel {
                    width: generated.width,
                    height: generated.height,
                    length: generated.length,
                    palette: generated.palette,
                    blocks: generated.blocks,
                };
                let non_air_blocks = model.blocks.iter().filter(|block| **block != 0).count();
                project.record_generation(
                    model.width,
                    model.height,
                    model.length,
                    non_air_blocks,
                    self.actor.clone(),
                )?;
                std::fs::create_dir_all(&self.output_root).map_err(|error| error.to_string())?;
                let schematic_path = self
                    .output_root
                    .join(format!("{}.schem", project.id().as_str()));
                write_schematic(&schematic_path, project.name(), &model)?;
                let schematic_bytes = std::fs::metadata(&schematic_path)
                    .map_err(|error| error.to_string())?
                    .len();
                let schematic_sha256 = format!(
                    "{:x}",
                    Sha256::digest(
                        std::fs::read(&schematic_path).map_err(|error| error.to_string())?
                    )
                );
                let manifest_path = self.output_root.join(format!(
                    "{}.foundation-manifest.json",
                    project.id().as_str()
                ));
                let evidence = project
                    .pinned_evidence()
                    .ok_or("Pinned evidence disappeared before export")?;
                write_foundation_manifest(
                    &manifest_path,
                    &FoundationManifest {
                        project_id: project.id().as_str().into(),
                        project_revision: project.workflow().project_revision(),
                        compatibility_profile_id: project
                            .compatibility_profile()
                            .profile_id()
                            .into(),
                        dataset_bundle_id: evidence.acquisition.manifest.bundle.id.clone(),
                        schematic: FoundationManifestSchematic {
                            file_name: file_name(&schematic_path)?,
                            bytes: schematic_bytes,
                            sha256: schematic_sha256.clone(),
                            width: model.width,
                            height: model.height,
                            length: model.length,
                        },
                    },
                )?;
                project.record_export(
                    schematic_sha256,
                    schematic_bytes,
                    file_name(&manifest_path)?,
                )?;
                Ok(FixedDatasetTracerReport {
                    project_id: self.project_id.clone(),
                    schematic_path,
                    schematic_bytes,
                    manifest_path,
                })
            },
        )
    }

    fn open_library(&self) -> Result<CampusProjectLibrary, String> {
        CampusProjectLibrary::open_for_construction(
            &self.library_root,
            self.campus_target_id.clone(),
            self.capability,
        )
    }
}

#[cfg(debug_assertions)]
pub fn bootstrap_if_enabled(
    application_data: &Path,
    enabled: Option<&str>,
) -> Result<Option<FixedDatasetTracerReport>, String> {
    use campus_services::acquisition::fixture_transport::FixtureTransport;

    if enabled != Some("1") {
        return Ok(None);
    }
    let capability = V11ConstructionCapability::request(true, enabled)?;
    let client = AcquisitionClient::new(FixtureTransport::canonical()?);
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let tracer = FixedDatasetTracer::confirm_campus_target(
        FixedDatasetTracerRequest {
            library_root: application_data.join("v1.1-fixed-tracer").join("library"),
            output_root: application_data.join("v1.1-fixed-tracer").join("output"),
            campus_scope: CampusScope::new(
                "gaode:B00155J6JH",
                "East China Normal University Putuo Campus",
                [121.395, 31.202],
            )?,
            project_name: format!("V1.1 fixed dataset tracer {run_id}"),
            boundary_job_id: "fixed-dataset-boundary".into(),
            acquisition_job_id: "fixed-dataset-acquisition".into(),
        },
        InstallationId::new("campus-native-fixed-tracer")?,
        &capability,
        &client,
    )?;
    let boundary = tracer.boundary_candidates()?;
    tracer.select_boundary(boundary, "boundary-osm-relation-100")?;
    tracer.acquire_foundation_evidence()?;
    tracer.complete_review(
        FoundationCategory::Building,
        FoundationReviewDisposition::SelectedEvidence {
            evidence_ids: vec!["obs-osm-relation-42".into()],
        },
    )?;
    tracer.complete_review(
        FoundationCategory::Circulation,
        FoundationReviewDisposition::CompleteEmpty,
    )?;
    for (category, reason) in [
        (FoundationCategory::Water, "relation way/88 missing"),
        (FoundationCategory::Vegetation, "provider page unavailable"),
        (FoundationCategory::Sports, "cancelled before retrieval"),
    ] {
        tracer.complete_review(
            category,
            FoundationReviewDisposition::KnownGap {
                reasons: vec![reason.into()],
            },
        )?;
    }
    tracer.generate_and_export().map(Some)
}

#[cfg(not(debug_assertions))]
pub fn bootstrap_if_enabled(
    _application_data: &Path,
    _enabled: Option<&str>,
) -> Result<Option<FixedDatasetTracerReport>, String> {
    Ok(None)
}

#[cfg(debug_assertions)]
fn copy_typed<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

#[cfg(debug_assertions)]
fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "Output path has no valid file name".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use campus_services::acquisition::{
        fixture_transport::FixtureTransport, AcquisitionClientErrorKind, TransportError,
        TransportRequest, TransportResponse,
    };
    use campus_state::FoundationResumePoint;

    struct UnavailableControlledService;

    impl AcquisitionTransport for UnavailableControlledService {
        fn execute(&self, _request: TransportRequest) -> Result<TransportResponse, TransportError> {
            Err(TransportError {
                explanation: "controlled service unavailable".into(),
            })
        }
    }

    #[test]
    fn user_controlled_fixed_dataset_path_resumes_and_exports() {
        let workspace = tempfile::tempdir().unwrap();
        let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
        let client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());
        let tracer = FixedDatasetTracer::confirm_campus_target(
            FixedDatasetTracerRequest {
                library_root: workspace.path().join("library"),
                output_root: workspace.path().join("output"),
                campus_scope: CampusScope::new(
                    "gaode:B00155J6JH",
                    "East China Normal University Putuo Campus",
                    [121.395, 31.202],
                )
                .unwrap(),
                project_name: "V1.1 fixed dataset tracer".into(),
                boundary_job_id: "live-compatible-boundary-job".into(),
                acquisition_job_id: "live-compatible-acquisition-job".into(),
            },
            InstallationId::new("acceptance-test").unwrap(),
            &capability,
            &client,
        )
        .unwrap();

        let boundary = tracer.boundary_candidates().unwrap();
        assert_eq!(boundary.candidates.len(), 2);
        tracer
            .select_boundary(boundary, "boundary-osm-relation-100")
            .unwrap();
        tracer.acquire_foundation_evidence().unwrap();
        let unavailable_client = AcquisitionClient::new(UnavailableControlledService);
        let service_error = unavailable_client.capabilities().unwrap_err();
        assert_eq!(
            service_error.kind,
            AcquisitionClientErrorKind::TransportUnavailable
        );
        let mut interrupted_library = tracer.open_library().unwrap();
        let mut interrupted_session = campus_state::Schema2ProjectSession::default();
        interrupted_session
            .open_project(&interrupted_library, &tracer.project_id)
            .unwrap();
        interrupted_library
            .inject_next_save_failure(campus_state::SaveFaultPoint::BeforeProjectReplace);
        interrupted_session
            .apply_semantic_operation(
                &mut interrupted_library,
                "fixture interruption checkpoint",
                |project| project.mark_updated(tracer.actor.clone()),
            )
            .unwrap_err();
        drop(interrupted_session);
        let mut recovered_session = campus_state::Schema2ProjectSession::default();
        recovered_session
            .open_project(&interrupted_library, &tracer.project_id)
            .unwrap();
        recovered_session
            .accept_recovery(&interrupted_library)
            .unwrap();
        recovered_session
            .request_save(&mut interrupted_library)
            .unwrap();
        drop(recovered_session);
        assert_eq!(
            tracer.project().unwrap().resume_point(),
            FoundationResumePoint::Review(FoundationCategory::Building)
        );
        tracer
            .complete_review(
                FoundationCategory::Building,
                FoundationReviewDisposition::SelectedEvidence {
                    evidence_ids: vec!["obs-osm-relation-42".into()],
                },
            )
            .unwrap();
        tracer
            .complete_review(
                FoundationCategory::Circulation,
                FoundationReviewDisposition::CompleteEmpty,
            )
            .unwrap();
        for category in [
            FoundationCategory::Water,
            FoundationCategory::Vegetation,
            FoundationCategory::Sports,
        ] {
            tracer
                .complete_review(
                    category,
                    FoundationReviewDisposition::KnownGap {
                        reasons: vec!["fixture coverage gap acknowledged".into()],
                    },
                )
                .unwrap();
        }

        let report = tracer.generate_and_export().unwrap();
        let reopened = tracer.project().unwrap();
        assert_eq!(&report.project_id, reopened.id());
        let evidence = reopened.pinned_evidence().unwrap();
        assert_eq!(evidence.boundary.candidates.len(), 2);
        assert_eq!(evidence.acquisition.observations.len(), 1);
        assert_eq!(
            evidence.acquisition.observations[0]
                .review_geometry_proposal
                .type_name(),
            "MultiPolygon"
        );
        assert_eq!(reopened.foundation_review().entries().len(), 5);
        assert_eq!(reopened.resume_point(), FoundationResumePoint::Complete);
        assert!(report.schematic_bytes > 0);
        assert!(report.schematic_path.exists());
        assert!(report.manifest_path.exists());
        assert_eq!(
            reopened.generated_output().unwrap().project_revision,
            reopened.workflow().project_revision()
        );
        assert_eq!(
            reopened.exported_output().unwrap().project_revision,
            reopened.workflow().project_revision()
        );
    }
}
