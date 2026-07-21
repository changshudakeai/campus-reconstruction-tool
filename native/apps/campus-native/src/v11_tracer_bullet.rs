use campus_export::{
    write_foundation_manifest, write_schematic, FoundationManifest, FoundationManifestSchematic,
    VoxelModel,
};
use campus_services::acquisition::project_acquisition::{
    ProjectAcquisitionCoordinator, ProjectAcquisitionProgress,
};
use campus_services::acquisition::{
    AcquisitionClient, AcquisitionJobState, AcquisitionTransport, CampusBoundaryCandidateQuery,
    CoarseRasterSupplementRequest, VerifiedBoundaryDiscoverySnapshot,
};
#[cfg(debug_assertions)]
use campus_state::CampusScope;
#[cfg(test)]
use campus_state::Schema2Project;
use campus_state::{
    CampusProjectLibrary, FoundationCategory, FoundationResumePoint, InstallationId, ProjectId,
    V11ConstructionCapability,
};
#[cfg(debug_assertions)]
use campus_state::{PinnedAcquisitionEvidence, ResultManifest, SourceObservation};
use campus_tool_protocol::{
    read_message, write_message, MapBoundaryDesk, MapBoundaryDeskRequest,
    MapBoundaryHandleSelection, MapFoundationReviewDeskRequest, ToolCommand, ToolEvent, ToolKind,
    PROTOCOL_VERSION,
};
#[cfg(target_os = "windows")]
use rand::Rng;
#[cfg(debug_assertions)]
use serde::de::DeserializeOwned;
#[cfg(debug_assertions)]
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

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

pub struct BoundaryDeskMapOptions {
    pub js_api_key: String,
    pub security_code: String,
    pub zoom: f64,
    pub pitch: f64,
    pub rotation: f64,
    pub english: bool,
}

pub struct FoundationReviewDeskMapOptions {
    pub js_api_key: String,
    pub security_code: String,
    pub zoom: f64,
    pub pitch: f64,
    pub rotation: f64,
    pub english: bool,
}

trait ToolSessionTransport {
    fn receive_event(&mut self) -> Result<ToolEvent, String>;
    fn send_command(&mut self, command: ToolCommand) -> Result<(), String>;
}

#[derive(Default)]
struct BoundaryToolInteraction {
    candidate_id: Option<String>,
    adjustment_enabled: bool,
    selected_handle: Option<MapBoundaryHandleSelection>,
}

#[cfg(target_os = "windows")]
struct BoundaryNamedPipeTransport {
    runtime: tokio::runtime::Runtime,
    server: NamedPipeServer,
}

#[cfg(target_os = "windows")]
impl ToolSessionTransport for BoundaryNamedPipeTransport {
    fn receive_event(&mut self) -> Result<ToolEvent, String> {
        let runtime = &self.runtime;
        let server = &mut self.server;
        runtime.block_on(read_message(server))
    }

    fn send_command(&mut self, command: ToolCommand) -> Result<(), String> {
        let runtime = &self.runtime;
        let server = &mut self.server;
        runtime.block_on(write_message(server, &command))
    }
}

#[derive(Debug)]
pub struct FixedDatasetTracerReport {
    pub project_id: ProjectId,
    pub schematic_path: PathBuf,
    pub schematic_bytes: u64,
    pub manifest_path: PathBuf,
}

#[derive(Debug)]
pub enum ProductionWorkflowOutcome {
    Advanced,
    Cancelled,
    Exported(FixedDatasetTracerReport),
    Complete,
}

pub fn continue_active_project<T: AcquisitionTransport>(
    context: &super::v11_project_library::ActiveProjectContext,
    capability: &V11ConstructionCapability,
    acquisition_client: &AcquisitionClient<T>,
    boundary_options: BoundaryDeskMapOptions,
    review_options: FoundationReviewDeskMapOptions,
) -> Result<ProductionWorkflowOutcome, String> {
    let library = CampusProjectLibrary::open_for_construction(
        &context.library_root,
        context.campus_target_id.clone(),
        capability,
    )?;
    let project = library.open_project(&context.project_id)?;
    if project.resume_point() != context.resume_point {
        return Err("The active project changed before its current task started".into());
    }

    let boundary_job_id = if context.resume_point == FoundationResumePoint::BoundaryReview
        && project
            .boundary_review()
            .and_then(|review| review.snapshot())
            .is_none()
    {
        let scope = project.campus_scope();
        acquisition_client
            .start_boundary_discovery(&CampusBoundaryCandidateQuery::new(
                scope.canonical_name(),
                Vec::new(),
                scope.anchor_wgs84(),
                2_500.0,
                format!("{}:controlled-boundary-v1", project.id().as_str()),
            )?)
            .map_err(|error| error.to_string())?
            .job_id
    } else {
        String::new()
    };

    let tracer = FixedDatasetTracer {
        library_root: context.library_root.clone(),
        output_root: context.output_root.clone(),
        campus_target_id: context.campus_target_id.clone(),
        actor: context.actor.clone(),
        capability,
        acquisition_client,
        boundary_job_id,
        acquisition_job_id: format!("{}:controlled-foundation-v1", context.project_id.as_str()),
        project_id: context.project_id.clone(),
    };

    match context.resume_point {
        FoundationResumePoint::BoundaryReview => {
            let outcome = tracer.run_installed_boundary_tool(boundary_options)?;
            if outcome == super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::Ignored {
                Ok(ProductionWorkflowOutcome::Cancelled)
            } else {
                Ok(ProductionWorkflowOutcome::Advanced)
            }
        }
        FoundationResumePoint::Acquisition => {
            tracer.resume_persisted_foundation_acquisition()?;
            Ok(ProductionWorkflowOutcome::Advanced)
        }
        FoundationResumePoint::Review(_) => {
            let outcome = tracer.run_installed_foundation_review_tool(review_options)?;
            if outcome
                == super::v11_foundation_review_desk::FoundationReviewToolEventOutcome::Ignored
            {
                Ok(ProductionWorkflowOutcome::Cancelled)
            } else {
                Ok(ProductionWorkflowOutcome::Advanced)
            }
        }
        FoundationResumePoint::Generation | FoundationResumePoint::Export => tracer
            .generate_and_export()
            .map(ProductionWorkflowOutcome::Exported),
        FoundationResumePoint::Complete => Ok(ProductionWorkflowOutcome::Complete),
    }
}

impl<'a, T: AcquisitionTransport> FixedDatasetTracer<'a, T> {
    #[cfg(debug_assertions)]
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

    pub fn open_boundary_review(&self) -> Result<MapBoundaryDesk, String> {
        let project = self.open_library()?.open_project(&self.project_id)?;
        if let Some(review) = project.boundary_review() {
            return Ok(super::v11_boundary_evidence_desk::map_boundary_desk(review));
        }
        self.refresh_boundary_review()
    }

    fn refresh_boundary_review(&self) -> Result<MapBoundaryDesk, String> {
        match self.boundary_candidates() {
            Ok(snapshot) => self.begin_boundary_review(snapshot),
            Err(error) => {
                let mut library = self.open_library()?;
                let mut session = campus_state::Schema2ProjectSession::default();
                session.open_project(&library, &self.project_id)?;
                session.apply_semantic_operation(
                    &mut library,
                    "record unavailable Campus Boundary evidence",
                    |project| {
                        project.begin_unavailable_boundary_review(
                            error.clone(),
                            "Retry the same boundary discovery job or return to Campus Target confirmation",
                            self.actor.clone(),
                        )
                    },
                )?;
                Ok(super::v11_boundary_evidence_desk::map_boundary_desk(
                    session
                        .active()
                        .and_then(campus_state::Schema2Project::boundary_review)
                        .ok_or("Unavailable Boundary review was not persisted")?,
                ))
            }
        }
    }

    pub fn open_boundary_review_command(
        &self,
        options: BoundaryDeskMapOptions,
    ) -> Result<ToolCommand, String> {
        let desk = self.open_boundary_review()?;
        let project = self.open_library()?.open_project(&self.project_id)?;
        let scope = project.campus_scope();
        let anchor = campus_services::wgs84_to_gcj02(campus_state::GeoPoint {
            lng: scope.anchor_wgs84()[0],
            lat: scope.anchor_wgs84()[1],
        });
        Ok(ToolCommand::OpenBoundaryDesk {
            request: Box::new(MapBoundaryDeskRequest {
                campus_name: scope.canonical_name().into(),
                center_lng: anchor.lng,
                center_lat: anchor.lat,
                zoom: options.zoom,
                pitch: options.pitch,
                rotation: options.rotation,
                js_api_key: options.js_api_key,
                security_code: options.security_code,
                desk,
                english: options.english,
            }),
        })
    }

    pub fn handle_boundary_tool_event(
        &self,
        event: ToolEvent,
    ) -> Result<super::v11_boundary_evidence_desk::BoundaryToolEventOutcome, String> {
        if matches!(event, ToolEvent::MapBoundaryRetryRequested) {
            self.refresh_boundary_review()?;
            return Ok(super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReviewUpdated);
        }
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        if matches!(event, ToolEvent::MapBoundaryReturnToCampusRequested) {
            session.apply_semantic_operation(
                &mut library,
                "return to Campus Target from Boundary review",
                |project| project.return_to_campus_target_from_boundary_review(self.actor.clone()),
            )?;
            return Ok(
                super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReturnToCampusTargetRequested,
            );
        }
        if matches!(event, ToolEvent::MapBoundaryConfirmed { .. })
            && session
                .active()
                .is_some_and(|project| project.pending_acquisition_start().is_some())
        {
            return self.start_persisted_boundary_acquisition(&mut library, &mut session);
        }
        let description = boundary_event_description(&event);
        let outcome = session.apply_semantic_operation(&mut library, description, |project| {
            super::v11_boundary_evidence_desk::apply_boundary_tool_event(
                project,
                &self.acquisition_job_id,
                self.actor.clone(),
                event,
            )
        })?;
        if outcome == super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::AcquisitionQueued
        {
            self.start_persisted_boundary_acquisition(&mut library, &mut session)
        } else {
            Ok(outcome)
        }
    }

    pub fn handle_boundary_tool_event_with_response(
        &self,
        event: ToolEvent,
    ) -> Result<
        (
            super::v11_boundary_evidence_desk::BoundaryToolEventOutcome,
            Option<ToolCommand>,
        ),
        String,
    > {
        let outcome = self.handle_boundary_tool_event(event)?;
        let response = match outcome {
            super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReviewUpdated => {
                let project = self.open_library()?.open_project(&self.project_id)?;
                project.boundary_review().map(|review| {
                    ToolCommand::UpdateBoundaryDesk {
                        desk: super::v11_boundary_evidence_desk::map_boundary_desk(review),
                    }
                })
            }
            super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::AcquisitionStarted
            | super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReturnToCampusTargetRequested => {
                Some(ToolCommand::Shutdown)
            }
            _ => None,
        };
        Ok((outcome, response))
    }

    fn drive_boundary_tool_session(
        &self,
        options: BoundaryDeskMapOptions,
        transport: &mut impl ToolSessionTransport,
    ) -> Result<super::v11_boundary_evidence_desk::BoundaryToolEventOutcome, String> {
        let initial_command = self.open_boundary_review_command(options)?;
        let mut interaction = BoundaryToolInteraction {
            candidate_id: match &initial_command {
                ToolCommand::OpenBoundaryDesk { request } => {
                    request.desk.selected_candidate_id.clone()
                }
                _ => None,
            },
            ..BoundaryToolInteraction::default()
        };
        transport.send_command(initial_command)?;
        loop {
            let event = transport.receive_event()?;
            match &event {
                ToolEvent::MapBoundaryAdjustmentChanged {
                    candidate_id,
                    enabled,
                } => {
                    if let Err(message) =
                        self.update_boundary_adjustment(&mut interaction, candidate_id, *enabled)
                    {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                    }
                    continue;
                }
                ToolEvent::MapBoundaryHandleSelected {
                    candidate_id,
                    selection,
                } => {
                    if let Err(message) = require_boundary_adjustment(&interaction, candidate_id) {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                    interaction.selected_handle = Some(*selection);
                    continue;
                }
                ToolEvent::MapBoundaryOperation {
                    candidate_id,
                    operation,
                } => {
                    if let Err(message) =
                        validate_boundary_session_operation(&interaction, candidate_id, operation)
                    {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                }
                _ => {}
            }
            let candidate_selection = match &event {
                ToolEvent::MapBoundaryCandidateSelected { candidate_id } => {
                    Some(candidate_id.clone())
                }
                _ => None,
            };
            let clear_handle = matches!(event, ToolEvent::MapBoundaryOperation { .. });
            if matches!(
                event,
                ToolEvent::Closed {
                    tool: ToolKind::Map
                }
            ) {
                return Ok(super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::Ignored);
            }
            let (outcome, response) = match self.handle_boundary_tool_event_with_response(event) {
                Ok(result) => result,
                Err(message) => {
                    transport.send_command(ToolCommand::ShowTaskError { message })?;
                    continue;
                }
            };
            if let Some(candidate_id) = candidate_selection {
                interaction = BoundaryToolInteraction {
                    candidate_id: Some(candidate_id),
                    ..BoundaryToolInteraction::default()
                };
            } else if clear_handle {
                interaction.selected_handle = None;
            }
            let Some(command) = response else {
                continue;
            };
            let session_finished = matches!(command, ToolCommand::Shutdown);
            transport.send_command(command)?;
            if session_finished {
                return Ok(outcome);
            }
        }
    }

    fn update_boundary_adjustment(
        &self,
        interaction: &mut BoundaryToolInteraction,
        candidate_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        if enabled {
            let project = self.open_library()?.open_project(&self.project_id)?;
            let desk = project
                .boundary_review()
                .ok_or("Load automatic Campus Boundary evidence before adjusting it")?;
            if desk.selected_candidate_id() != Some(candidate_id) || !desk.projection().can_adjust {
                return Err("Select a valid automatic Campus Boundary candidate first".into());
            }
        } else if interaction.candidate_id.as_deref() != Some(candidate_id) {
            return Err("The adjustment event does not match the selected candidate".into());
        }
        interaction.candidate_id = Some(candidate_id.to_string());
        interaction.adjustment_enabled = enabled;
        interaction.selected_handle = None;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn run_installed_boundary_tool(
        &self,
        options: BoundaryDeskMapOptions,
    ) -> Result<super::v11_boundary_evidence_desk::BoundaryToolEventOutcome, String> {
        let executable = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or("native executable has no parent")?
            .join("campus-map.exe");
        if !executable.is_file() {
            return Err("campus-map.exe is not installed beside the main application".into());
        }
        let pipe = format!(
            r"\\.\pipe\campus-boundary-evidence-{:032x}",
            rand::rng().random::<u128>()
        );
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let mut child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let mut transport = BoundaryNamedPipeTransport { runtime, server };
        let result = (|| {
            transport
                .runtime
                .block_on(transport.server.connect())
                .map_err(|error| error.to_string())?;
            let hello: ToolCommand = {
                let runtime = &transport.runtime;
                let server = &mut transport.server;
                runtime.block_on(read_message(server))?
            };
            match hello {
                ToolCommand::Hello {
                    protocol_version,
                    session_token,
                    tool: ToolKind::Map,
                } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                _ => return Err("Boundary map helper handshake rejected".into()),
            }
            self.drive_boundary_tool_session(options, &mut transport)
        })();
        if result.is_err() {
            let _ = child.kill();
        }
        let _ = child.wait();
        result
    }

    #[cfg(not(target_os = "windows"))]
    fn run_installed_boundary_tool(
        &self,
        _options: BoundaryDeskMapOptions,
    ) -> Result<super::v11_boundary_evidence_desk::BoundaryToolEventOutcome, String> {
        Err("Campus Boundary map review is supported only on Windows".into())
    }

    fn begin_boundary_review(
        &self,
        snapshot: VerifiedBoundaryDiscoverySnapshot,
    ) -> Result<MapBoundaryDesk, String> {
        let desk =
            super::v11_boundary_evidence_desk::evidence_desk_from_verified_snapshot(&snapshot)?;
        let durable_snapshot = desk
            .snapshot()
            .cloned()
            .ok_or("The Boundary Discovery Snapshot cannot be persisted")?;
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            "open automatic Campus Boundary evidence desk",
            |project| project.begin_boundary_review(durable_snapshot, self.actor.clone()),
        )?;
        Ok(super::v11_boundary_evidence_desk::map_boundary_desk(
            session
                .active()
                .and_then(campus_state::Schema2Project::boundary_review)
                .ok_or("Boundary review was not persisted")?,
        ))
    }

    fn start_persisted_boundary_acquisition(
        &self,
        library: &mut CampusProjectLibrary,
        session: &mut campus_state::Schema2ProjectSession,
    ) -> Result<super::v11_boundary_evidence_desk::BoundaryToolEventOutcome, String> {
        let coordinator = ProjectAcquisitionCoordinator::new(self.acquisition_client);
        let progress = session.apply_semantic_operation(
            library,
            "start persisted five-category acquisition request",
            |project| coordinator.start_queued_boundary_acquisition(project, self.actor.clone()),
        )?;
        if progress != ProjectAcquisitionProgress::Started {
            return Err("Persisted Campus Boundary acquisition did not start".into());
        }
        Ok(super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::AcquisitionStarted)
    }

    fn complete_explicit_foundation_refresh(&self) -> Result<ProjectAcquisitionProgress, String> {
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        let current_bundle_id = session
            .active()
            .and_then(campus_state::Schema2Project::pinned_evidence)
            .map(|evidence| evidence.acquisition.manifest.bundle.id.clone())
            .ok_or("Pin the initial Foundation acquisition before requesting a refresh")?;
        let idempotency_key = format!(
            "{}:explicit-foundation-refresh:{current_bundle_id}",
            self.project_id.as_str()
        );
        let coordinator = ProjectAcquisitionCoordinator::new(self.acquisition_client);
        session.apply_semantic_operation(
            &mut library,
            "start explicit Foundation Dataset Bundle refresh",
            |project| {
                coordinator.start_explicit_refresh(project, &idempotency_key, self.actor.clone())
            },
        )?;
        session.apply_semantic_operation(
            &mut library,
            "pin explicit Foundation refresh manifest",
            |project| coordinator.pin_manifest(project, self.actor.clone()),
        )?;
        loop {
            let progress = session.apply_semantic_operation(
                &mut library,
                "persist explicit Foundation refresh evidence chunk",
                |project| coordinator.download_next_chunk(project, self.actor.clone()),
            )?;
            if progress == ProjectAcquisitionProgress::AllChunksVerified {
                break;
            }
        }
        session.apply_semantic_operation(
            &mut library,
            "finalize explicit Foundation refresh",
            |project| coordinator.finalize(project, self.actor.clone()),
        )
    }

    fn resume_persisted_foundation_acquisition(&self) -> Result<(), String> {
        let coordinator = ProjectAcquisitionCoordinator::new(self.acquisition_client);
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            "refresh controlled Foundation acquisition status",
            |project| coordinator.reconnect(project, self.actor.clone()),
        )?;
        session.apply_semantic_operation(
            &mut library,
            "pin controlled Foundation acquisition manifest",
            |project| coordinator.pin_manifest(project, self.actor.clone()),
        )?;
        loop {
            let progress = session.apply_semantic_operation(
                &mut library,
                "persist controlled Foundation acquisition chunk",
                |project| coordinator.download_next_chunk(project, self.actor.clone()),
            )?;
            if progress == ProjectAcquisitionProgress::AllChunksVerified {
                break;
            }
        }
        let progress = session.apply_semantic_operation(
            &mut library,
            "finalize controlled Foundation acquisition",
            |project| coordinator.finalize(project, self.actor.clone()),
        )?;
        if progress != ProjectAcquisitionProgress::EvidencePinned {
            return Err("Controlled Foundation evidence was not pinned".into());
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
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
                )?;
                project.initialize_building_entity_review(Vec::new(), self.actor.clone())
            },
        )
    }

    fn open_foundation_review_command(
        &self,
        options: FoundationReviewDeskMapOptions,
        active_category: FoundationCategory,
        selected_subject_id: Option<&str>,
    ) -> Result<ToolCommand, String> {
        let project = self.open_library()?.open_project(&self.project_id)?;
        let scope = project.campus_scope();
        let anchor = scope.anchor_wgs84();
        let anchor = campus_services::wgs84_to_gcj02(campus_state::GeoPoint {
            lng: anchor[0],
            lat: anchor[1],
        });
        let desk = super::v11_foundation_review_desk::map_foundation_review_desk(
            &project,
            active_category,
            selected_subject_id,
        )?;
        Ok(ToolCommand::OpenFoundationReviewDesk {
            request: Box::new(MapFoundationReviewDeskRequest {
                campus_name: scope.canonical_name().into(),
                center_lng: anchor.lng,
                center_lat: anchor.lat,
                zoom: options.zoom,
                pitch: options.pitch,
                rotation: options.rotation,
                js_api_key: options.js_api_key,
                security_code: options.security_code,
                boundary: super::v11_foundation_review_desk::map_confirmed_boundary(&project),
                desk,
                english: options.english,
            }),
        })
    }

    fn request_controlled_coarse_raster_supplement(
        &self,
        category: FoundationCategory,
        gap_id: &str,
    ) -> Result<(), String> {
        let project = self.open_library()?.open_project(&self.project_id)?;
        let evidence = project
            .pinned_evidence()
            .ok_or("Coarse raster supplementation requires pinned Foundation evidence")?;
        let gap = project
            .foundation_review_queue(category)?
            .known_gaps
            .into_iter()
            .find(|gap| {
                gap.id == gap_id && gap.status != campus_state::KnownFeatureGapStatus::Resolved
            })
            .ok_or("The selected Known Feature Gap is no longer current")?;
        let gap_geometry = gap
            .location
            .geometry
            .ok_or("The controlled Coverage Report did not preserve gap geometry")?;
        let algorithm_version = match category {
            FoundationCategory::Water => "coarse-gap-water-v1.0.0",
            FoundationCategory::Vegetation => "coarse-gap-vegetation-v1.0.0",
            _ => {
                return Err(
                    "Coarse raster supplementation supports only water or vegetation gaps".into(),
                )
            }
        };
        let bundle = serde_json::from_value(
            serde_json::to_value(&evidence.acquisition.manifest.bundle)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let gap_geometry = serde_json::from_value(
            serde_json::to_value(&gap_geometry).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let request = CoarseRasterSupplementRequest::new(
            bundle,
            evidence.boundary.manifest.result_sha256.clone(),
            match category {
                FoundationCategory::Water => {
                    campus_services::acquisition::FoundationCategory::Water
                }
                FoundationCategory::Vegetation => {
                    campus_services::acquisition::FoundationCategory::Vegetation
                }
                _ => unreachable!(),
            },
            gap.id.clone(),
            gap.location.tile_id,
            gap_geometry,
            algorithm_version,
            format!(
                "{}/coarse-raster/{}/{}",
                self.project_id.as_str(),
                evidence.acquisition.manifest.bundle.id,
                gap.id
            ),
        )?;
        let started = self
            .acquisition_client
            .start_coarse_raster_supplement(&request)
            .map_err(|error| error.to_string())?;
        let has_previous_job_run = project
            .coarse_raster_runs()
            .iter()
            .any(|run| run.job_id == started.job_id);
        let retried = has_previous_job_run && coarse_raster_job_is_retryable(&started);
        let started = if retried {
            self.acquisition_client
                .retry_coarse_raster_supplement(&started)
                .map_err(|error| error.to_string())?
        } else {
            started
        };
        let terminal = if matches!(
            started.state,
            AcquisitionJobState::Complete
                | AcquisitionJobState::Partial
                | AcquisitionJobState::Failed
                | AcquisitionJobState::Cancelled
        ) {
            started
        } else {
            self.acquisition_client
                .coarse_raster_job(&started)
                .map_err(|error| error.to_string())?
        };
        if matches!(
            terminal.state,
            AcquisitionJobState::Queued | AcquisitionJobState::Running
        ) {
            return Err(format!(
                "Coarse raster job {} is still running; retry preserves the same idempotent job",
                terminal.job_id
            ));
        }
        let previous_job_runs = project
            .coarse_raster_runs()
            .iter()
            .filter(|run| run.job_id == terminal.job_id)
            .collect::<Vec<_>>();
        if !retried
            && previous_job_runs.iter().any(|run| {
                matches!(
                    run.outcome,
                    campus_state::CoarseRasterRunOutcome::Proposals { .. }
                        | campus_state::CoarseRasterRunOutcome::UnusableCoverage { .. }
                )
            })
        {
            return Ok(());
        }
        if !retried
            && !coarse_raster_job_is_retryable(&terminal)
            && previous_job_runs.iter().any(|run| {
                matches!(
                    run.outcome,
                    campus_state::CoarseRasterRunOutcome::ProviderFailure { .. }
                )
            })
        {
            return Ok(());
        }
        let run = super::v11_acquisition_client::materialize_coarse_raster_supplement(
            self.acquisition_client,
            &request,
            &terminal,
            format!(
                "coarse-raster-run:{}:{}",
                terminal.job_id,
                previous_job_runs.len() + 1
            ),
            format!("unix-ms:{}", now_unix_ms()),
        )?;
        let mut library = self.open_library()?;
        let mut session = campus_state::Schema2ProjectSession::default();
        session.open_project(&library, &self.project_id)?;
        session.apply_semantic_operation(
            &mut library,
            "record controlled coarse raster gap evidence",
            |project| project.record_coarse_raster_supplement(run, self.actor.clone()),
        )?;
        Ok(())
    }
    fn drive_foundation_review_tool_session(
        &self,
        options: FoundationReviewDeskMapOptions,
        transport: &mut impl ToolSessionTransport,
    ) -> Result<super::v11_foundation_review_desk::FoundationReviewToolEventOutcome, String> {
        let project = self.open_library()?.open_project(&self.project_id)?;
        let mut active_category = match project.resume_point() {
            campus_state::FoundationResumePoint::Review(category) => category,
            _ => FoundationCategory::Building,
        };
        let mut selected_subject_id = None;
        transport.send_command(self.open_foundation_review_command(
            options,
            active_category,
            selected_subject_id.as_deref(),
        )?)?;
        loop {
            let event = transport.receive_event()?;
            if matches!(
                event,
                ToolEvent::Closed {
                    tool: ToolKind::Map
                }
            ) {
                return Ok(
                    super::v11_foundation_review_desk::FoundationReviewToolEventOutcome::Ignored,
                );
            }
            if let ToolEvent::Error { message } = event {
                return Err(format!("Foundation review map helper failed: {message}"));
            }
            if matches!(event, ToolEvent::MapFoundationRefreshRequested) {
                let progress = match self.complete_explicit_foundation_refresh() {
                    Ok(progress) => progress,
                    Err(message) => {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                };
                if progress != ProjectAcquisitionProgress::EvidencePinned {
                    transport.send_command(ToolCommand::ShowTaskError {
                        message: "Explicit Foundation refresh did not pin verified evidence".into(),
                    })?;
                    continue;
                }
                let project = self.open_library()?.open_project(&self.project_id)?;
                transport.send_command(ToolCommand::UpdateFoundationReviewDesk {
                    desk: super::v11_foundation_review_desk::map_foundation_review_desk(
                        &project,
                        active_category,
                        selected_subject_id.as_deref(),
                    )?,
                })?;
                continue;
            }
            if let ToolEvent::MapCoarseRasterSupplementRequested { category, gap_id } = &event {
                active_category = match super::v11_foundation_review_desk::parse_category(category)
                {
                    Ok(category) => category,
                    Err(message) => {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                };
                if let Err(message) =
                    self.request_controlled_coarse_raster_supplement(active_category, gap_id)
                {
                    transport.send_command(ToolCommand::ShowTaskError { message })?;
                    continue;
                }
                let project = self.open_library()?.open_project(&self.project_id)?;
                transport.send_command(ToolCommand::UpdateFoundationReviewDesk {
                    desk: super::v11_foundation_review_desk::map_foundation_review_desk(
                        &project,
                        active_category,
                        selected_subject_id.as_deref(),
                    )?,
                })?;
                continue;
            }
            if let ToolEvent::MapFoundationReviewCategorySelected { category } = &event {
                active_category = match super::v11_foundation_review_desk::parse_category(category)
                {
                    Ok(category) => category,
                    Err(message) => {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                };
                selected_subject_id = None;
                let project = self.open_library()?.open_project(&self.project_id)?;
                transport.send_command(ToolCommand::UpdateFoundationReviewDesk {
                    desk: super::v11_foundation_review_desk::map_foundation_review_desk(
                        &project,
                        active_category,
                        None,
                    )?,
                })?;
                continue;
            }
            if let ToolEvent::MapFoundationReviewCandidateSelected {
                category,
                subject_id,
            } = &event
            {
                active_category = match super::v11_foundation_review_desk::parse_category(category)
                {
                    Ok(category) => category,
                    Err(message) => {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                };
                selected_subject_id = Some(subject_id.clone());
                let project = self.open_library()?.open_project(&self.project_id)?;
                transport.send_command(ToolCommand::UpdateFoundationReviewDesk {
                    desk: super::v11_foundation_review_desk::map_foundation_review_desk(
                        &project,
                        active_category,
                        selected_subject_id.as_deref(),
                    )?,
                })?;
                continue;
            }
            let description = foundation_review_event_description(&event);
            let mut library = self.open_library()?;
            let mut session = campus_state::Schema2ProjectSession::default();
            session.open_project(&library, &self.project_id)?;
            let outcome =
                match session.apply_semantic_operation(&mut library, description, |project| {
                    super::v11_foundation_review_desk::apply_foundation_review_tool_event(
                        project,
                        self.actor.clone(),
                        event,
                    )
                }) {
                    Ok(outcome) => outcome,
                    Err(message) => {
                        transport.send_command(ToolCommand::ShowTaskError { message })?;
                        continue;
                    }
                };
            let project = self.open_library()?.open_project(&self.project_id)?;
            if let campus_state::FoundationResumePoint::Review(category) = project.resume_point() {
                if matches!(
                    outcome,
                    super::v11_foundation_review_desk::FoundationReviewToolEventOutcome::CategoryCompleted { .. }
                ) {
                    active_category = category;
                    selected_subject_id = None;
                }
                transport.send_command(ToolCommand::UpdateFoundationReviewDesk {
                    desk: super::v11_foundation_review_desk::map_foundation_review_desk(
                        &project,
                        active_category,
                        selected_subject_id.as_deref(),
                    )?,
                })?;
            } else {
                transport.send_command(ToolCommand::Shutdown)?;
                return Ok(outcome);
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn run_installed_foundation_review_tool(
        &self,
        options: FoundationReviewDeskMapOptions,
    ) -> Result<super::v11_foundation_review_desk::FoundationReviewToolEventOutcome, String> {
        let executable = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or("native executable has no parent")?
            .join("campus-map.exe");
        if !executable.is_file() {
            return Err("campus-map.exe is not installed beside the main application".into());
        }
        let pipe = format!(
            r"\\.\pipe\campus-foundation-review-{:032x}",
            rand::rng().random::<u128>()
        );
        let token = format!("{:032x}", rand::rng().random::<u128>());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe)
            .map_err(|error| error.to_string())?;
        let mut child = Command::new(executable)
            .arg(&pipe)
            .arg(&token)
            .spawn()
            .map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let mut transport = BoundaryNamedPipeTransport { runtime, server };
        let result = (|| {
            transport
                .runtime
                .block_on(transport.server.connect())
                .map_err(|error| error.to_string())?;
            let hello: ToolCommand = {
                let runtime = &transport.runtime;
                let server = &mut transport.server;
                runtime.block_on(read_message(server))?
            };
            match hello {
                ToolCommand::Hello {
                    protocol_version,
                    session_token,
                    tool: ToolKind::Map,
                } if protocol_version == PROTOCOL_VERSION && session_token == token => {}
                _ => return Err("Foundation review map helper handshake rejected".into()),
            }
            self.drive_foundation_review_tool_session(options, &mut transport)
        })();
        if result.is_err() {
            let _ = child.kill();
        }
        let _ = child.wait();
        result
    }

    #[cfg(not(target_os = "windows"))]
    fn run_installed_foundation_review_tool(
        &self,
        _options: FoundationReviewDeskMapOptions,
    ) -> Result<super::v11_foundation_review_desk::FoundationReviewToolEventOutcome, String> {
        Err("Foundation review map desk is supported only on Windows".into())
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

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn coarse_raster_job_is_retryable(
    job: &campus_services::acquisition::AcquisitionJobStatus,
) -> bool {
    job.failure
        .as_ref()
        .is_some_and(|failure| failure.retryable)
        || job
            .outcomes
            .iter()
            .filter_map(|outcome| outcome.failure.as_ref())
            .any(|failure| failure.retryable)
}

fn boundary_event_description(event: &ToolEvent) -> &'static str {
    match event {
        ToolEvent::MapBoundaryCandidateSelected { .. } => "select Campus Boundary candidate",
        ToolEvent::MapBoundaryOperation { .. } => "edit Campus Boundary",
        ToolEvent::MapBoundaryConfirmed { .. } => {
            "confirm Campus Boundary and start five-category acquisition"
        }
        ToolEvent::MapBoundaryRetryRequested => "retry Campus Boundary discovery",
        ToolEvent::MapBoundaryReturnToCampusRequested => {
            "return to Campus Target from Boundary review"
        }
        ToolEvent::Error { .. } => "record Campus Boundary map failure",
        ToolEvent::Closed { .. } => "record Campus Boundary map cancellation",
        _ => "ignore unrelated Boundary map event",
    }
}

fn foundation_review_event_description(event: &ToolEvent) -> &'static str {
    match event {
        ToolEvent::MapFoundationReviewDecisionRequested { .. } => {
            "record Foundation candidate review decision"
        }
        ToolEvent::MapCoarseRasterSupplementRequested { .. } => {
            "request controlled coarse raster gap evidence"
        }
        ToolEvent::MapCoarseRasterDecisionRequested { .. } => {
            "record coarse raster evidence review decision"
        }
        ToolEvent::MapFoundationBatchReviewRequested { .. } => {
            "record atomic Foundation batch review"
        }
        ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
            acknowledged: true, ..
        } => "acknowledge Known Feature Gap",
        ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
            acknowledged: false,
            ..
        } => "reopen Known Feature Gap",
        ToolEvent::MapFoundationConflictResolutionRequested { .. } => {
            "resolve Foundation review conflict"
        }
        ToolEvent::MapFoundationCategoryCompletionRequested { .. } => {
            "explicitly complete Foundation category review"
        }
        ToolEvent::MapFoundationRefreshRequested => {
            "start explicit Foundation Dataset Bundle refresh"
        }
        _ => "ignore unrelated Foundation review event",
    }
}

fn require_boundary_adjustment(
    interaction: &BoundaryToolInteraction,
    candidate_id: &str,
) -> Result<(), String> {
    if interaction.candidate_id.as_deref() != Some(candidate_id) {
        return Err("The Boundary interaction does not match the selected candidate".into());
    }
    if !interaction.adjustment_enabled {
        return Err("Enter Campus Boundary adjustment mode before selecting a handle".into());
    }
    Ok(())
}

fn validate_boundary_session_operation(
    interaction: &BoundaryToolInteraction,
    candidate_id: &str,
    operation: &campus_tool_protocol::MapBoundaryEditOperation,
) -> Result<(), String> {
    use campus_tool_protocol::MapBoundaryEditOperation;

    if interaction.candidate_id.as_deref() != Some(candidate_id) {
        return Err("The Boundary operation does not match the selected candidate".into());
    }
    match operation {
        MapBoundaryEditOperation::MoveVertex { vertex_index, .. }
        | MapBoundaryEditOperation::DeleteVertex { vertex_index } => {
            require_boundary_adjustment(interaction, candidate_id)?;
            if interaction.selected_handle
                != Some(MapBoundaryHandleSelection::Vertex {
                    vertex_index: *vertex_index,
                })
            {
                return Err("Select this Campus Boundary vertex before changing it".into());
            }
        }
        MapBoundaryEditOperation::InsertVertex { edge_index } => {
            require_boundary_adjustment(interaction, candidate_id)?;
            if interaction.selected_handle
                != Some(MapBoundaryHandleSelection::Edge {
                    edge_index: *edge_index,
                })
            {
                return Err("Select this Campus Boundary edge before inserting a vertex".into());
            }
        }
        MapBoundaryEditOperation::RestoreCandidateOriginal => {
            require_boundary_adjustment(interaction, candidate_id)?;
        }
        MapBoundaryEditOperation::Undo => {}
    }
    Ok(())
}

#[cfg(debug_assertions)]
pub fn bootstrap_if_enabled(
    application_data: &Path,
    enabled: Option<&str>,
    _production_client: Option<&super::v11_acquisition_client::ProductionAcquisitionClient>,
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
    let outcome = tracer.run_installed_boundary_tool(BoundaryDeskMapOptions {
        js_api_key: std::env::var("GAODE_JS_API_KEY")
            .or_else(|_| std::env::var("VITE_GAODE_JS_API_KEY"))
            .unwrap_or_default(),
        security_code: std::env::var("GAODE_SECURITY_CODE")
            .or_else(|_| std::env::var("VITE_GAODE_SECURITY_CODE"))
            .unwrap_or_default(),
        zoom: 17.0,
        pitch: 45.0,
        rotation: 0.0,
        english: false,
    })?;
    if outcome != super::v11_boundary_evidence_desk::BoundaryToolEventOutcome::AcquisitionStarted {
        return Ok(None);
    }
    tracer.acquire_foundation_evidence()?;
    tracer.run_installed_foundation_review_tool(FoundationReviewDeskMapOptions {
        js_api_key: std::env::var("GAODE_JS_API_KEY")
            .or_else(|_| std::env::var("VITE_GAODE_JS_API_KEY"))
            .unwrap_or_default(),
        security_code: std::env::var("GAODE_SECURITY_CODE")
            .or_else(|_| std::env::var("VITE_GAODE_SECURITY_CODE"))
            .unwrap_or_default(),
        zoom: 17.0,
        pitch: 45.0,
        rotation: 0.0,
        english: false,
    })?;
    tracer.generate_and_export().map(Some)
}

#[cfg(debug_assertions)]
fn copy_typed<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

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
    use std::cell::Cell;
    use std::collections::{BTreeMap, VecDeque};

    struct MemoryBoundaryTransport {
        events: VecDeque<ToolEvent>,
        commands: Vec<ToolCommand>,
    }

    impl ToolSessionTransport for MemoryBoundaryTransport {
        fn receive_event(&mut self) -> Result<ToolEvent, String> {
            self.events
                .pop_front()
                .ok_or("test event queue exhausted".into())
        }

        fn send_command(&mut self, command: ToolCommand) -> Result<(), String> {
            self.commands.push(command);
            Ok(())
        }
    }

    struct UnavailableControlledService;

    impl AcquisitionTransport for UnavailableControlledService {
        fn execute(&self, _request: TransportRequest) -> Result<TransportResponse, TransportError> {
            Err(TransportError {
                explanation: "controlled service unavailable".into(),
            })
        }
    }

    struct StartUnavailableTransport(FixtureTransport);

    impl AcquisitionTransport for StartUnavailableTransport {
        fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
            if request.path == "/v1/capabilities" {
                Err(TransportError {
                    explanation: "controlled service unavailable during acquisition start".into(),
                })
            } else {
                self.0.execute(request)
            }
        }
    }

    fn gzip_stored(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() <= u16::MAX as usize);
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        crc = !crc;
        let length = bytes.len() as u16;
        let mut gzip = vec![0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0xff, 0x01];
        gzip.extend_from_slice(&length.to_le_bytes());
        gzip.extend_from_slice(&(!length).to_le_bytes());
        gzip.extend_from_slice(bytes);
        gzip.extend_from_slice(&crc.to_le_bytes());
        gzip.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        gzip
    }

    struct RetryThenProposalTransport {
        fixture: FixtureTransport,
        retried: Cell<bool>,
        empty_manifest: Vec<u8>,
        empty_chunk: Vec<u8>,
        empty_cursor: String,
        proposal_manifest: Vec<u8>,
        proposal_chunk: Vec<u8>,
        proposal_cursor: String,
    }

    impl RetryThenProposalTransport {
        fn canonical(boundary_result_sha256: &str) -> Self {
            let fixture_value: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../contracts/acquisition/v1/fixtures/canonical-coarse-raster.json"
            ))
            .unwrap();
            let empty_chunk = vec![
                0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x0a, 0xe3, 0x02, 0x00, 0x93,
                0x06, 0xd7, 0x32, 0x01, 0x00, 0x00, 0x00,
            ];
            let empty_cursor = "coarse-raster-empty-cursor-v1".to_string();
            let empty_manifest = serde_json::to_vec(&serde_json::json!({
                "contract_version": fixture_value["contract_version"],
                "bundle": fixture_value["bundle"],
                "coverage_report": { "outcomes": [] },
                "licences": [],
                "chunks": [{
                    "id": "coarse-raster-empty-chunk-0001",
                    "stable_cursor": empty_cursor,
                    "content_type": "application/x-ndjson",
                    "content_encoding": "gzip",
                    "sha256": format!("{:x}", Sha256::digest(&empty_chunk)),
                    "uncompressed_bytes": 1
                }],
                "result_sha256": format!("{:x}", Sha256::digest(b"\n"))
            }))
            .unwrap();

            let mut observation = fixture_value["observations"][0].clone();
            observation["clip"]["boundaryResultSha256"] = serde_json::json!(boundary_result_sha256);
            observation["structuredConflictObservationIds"] =
                serde_json::json!(["obs-osm-relation-42"]);
            observation["exclusions"][0]["structuredObservationIds"] =
                serde_json::json!(["obs-osm-relation-42"]);
            let mut proposal_records = serde_json::to_vec(&observation).unwrap();
            proposal_records.push(b'\n');
            let proposal_chunk = gzip_stored(&proposal_records);
            let proposal_cursor = "coarse-raster-proposal-cursor-v1".to_string();
            let proposal_manifest = serde_json::to_vec(&serde_json::json!({
                "contract_version": fixture_value["contract_version"],
                "bundle": fixture_value["bundle"],
                "coverage_report": fixture_value["coverage_report"],
                "licences": [observation["source"]["licence"].clone()],
                "chunks": [{
                    "id": "coarse-raster-proposal-chunk-0001",
                    "stable_cursor": proposal_cursor,
                    "content_type": "application/x-ndjson",
                    "content_encoding": "gzip",
                    "sha256": format!("{:x}", Sha256::digest(&proposal_chunk)),
                    "uncompressed_bytes": proposal_records.len()
                }],
                "result_sha256": format!("{:x}", Sha256::digest(&proposal_records))
            }))
            .unwrap();
            Self {
                fixture: FixtureTransport::canonical().unwrap(),
                retried: Cell::new(false),
                empty_manifest,
                empty_chunk,
                empty_cursor,
                proposal_manifest,
                proposal_chunk,
                proposal_cursor,
            }
        }

        fn job_response(&self, failure: bool) -> Result<TransportResponse, TransportError> {
            let fixture_value: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../contracts/acquisition/v1/fixtures/canonical-coarse-raster.json"
            ))
            .map_err(|error| TransportError {
                explanation: error.to_string(),
            })?;
            let failure = failure.then(|| {
                serde_json::json!({
                    "code": "coarse_raster_temporarily_empty",
                    "scope": "sentinel-2-l2a/water/31-121-1",
                    "retryable": true,
                    "explanation": "The first verified result contained no usable component.",
                    "suggested_action": "Retry the same pinned coarse raster job."
                })
            });
            Ok(TransportResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: serde_json::to_vec(&serde_json::json!({
                    "job_id": "coarse-raster-retry-same-job",
                    "contract_version": campus_services::acquisition::CONTRACT_VERSION,
                    "bundle_id": fixture_value["bundle"]["id"],
                    "state": "complete",
                    "outcomes": [],
                    "failure": failure
                }))
                .map_err(|error| TransportError {
                    explanation: error.to_string(),
                })?,
            })
        }
    }

    impl AcquisitionTransport for RetryThenProposalTransport {
        fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
            if request.path == "/v1/coarse-raster-jobs" {
                return self.job_response(true);
            }
            if request.path.ends_with("/retry") {
                self.retried.set(true);
                return self.job_response(false);
            }
            if request.path.contains("/coarse-raster-jobs/") {
                let (manifest, chunk, cursor) = if self.retried.get() {
                    (
                        &self.proposal_manifest,
                        &self.proposal_chunk,
                        &self.proposal_cursor,
                    )
                } else {
                    (&self.empty_manifest, &self.empty_chunk, &self.empty_cursor)
                };
                if request.path.ends_with("/manifest") {
                    return Ok(TransportResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        body: manifest.clone(),
                    });
                }
                return Ok(TransportResponse {
                    status: 200,
                    headers: BTreeMap::from([("x-stable-cursor".into(), cursor.clone())]),
                    body: chunk.clone(),
                });
            }
            self.fixture.execute(request)
        }
    }
    #[test]
    fn boundary_session_rejects_an_operation_without_the_explicit_handle_selection() {
        let interaction = BoundaryToolInteraction {
            candidate_id: Some("candidate-1".into()),
            adjustment_enabled: true,
            selected_handle: Some(MapBoundaryHandleSelection::Vertex { vertex_index: 1 }),
        };
        let error = validate_boundary_session_operation(
            &interaction,
            "candidate-1",
            &campus_tool_protocol::MapBoundaryEditOperation::MoveVertex {
                vertex_index: 2,
                coordinate: campus_tool_protocol::MapCoordinate { lng: 1.0, lat: 2.0 },
            },
        )
        .unwrap_err();
        assert!(error.contains("Select this Campus Boundary vertex"));
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

        let mut transport = MemoryBoundaryTransport {
            events: VecDeque::from([
                ToolEvent::MapBoundaryCandidateSelected {
                    candidate_id: "boundary-osm-relation-100".into(),
                },
                ToolEvent::MapBoundaryAdjustmentChanged {
                    candidate_id: "boundary-osm-relation-100".into(),
                    enabled: true,
                },
                ToolEvent::MapBoundaryHandleSelected {
                    candidate_id: "boundary-osm-relation-100".into(),
                    selection: MapBoundaryHandleSelection::Edge { edge_index: 0 },
                },
                ToolEvent::MapBoundaryOperation {
                    candidate_id: "boundary-osm-relation-100".into(),
                    operation: campus_tool_protocol::MapBoundaryEditOperation::InsertVertex {
                        edge_index: 0,
                    },
                },
                ToolEvent::MapBoundaryOperation {
                    candidate_id: "boundary-osm-relation-100".into(),
                    operation: campus_tool_protocol::MapBoundaryEditOperation::Undo,
                },
                ToolEvent::MapBoundaryOperation {
                    candidate_id: "boundary-osm-relation-100".into(),
                    operation:
                        campus_tool_protocol::MapBoundaryEditOperation::RestoreCandidateOriginal,
                },
                ToolEvent::MapBoundaryConfirmed {
                    candidate_id: "boundary-osm-relation-100".into(),
                },
            ]),
            commands: Vec::new(),
        };
        let outcome = tracer
            .drive_boundary_tool_session(
                BoundaryDeskMapOptions {
                    js_api_key: "test-key".into(),
                    security_code: "test-security-code".into(),
                    zoom: 17.0,
                    pitch: 45.0,
                    rotation: 0.0,
                    english: true,
                },
                &mut transport,
            )
            .unwrap();
        let commands = transport.commands;
        assert_eq!(
            outcome,
            crate::v11_boundary_evidence_desk::BoundaryToolEventOutcome::AcquisitionStarted
        );
        assert!(matches!(
            commands.first(),
            Some(ToolCommand::OpenBoundaryDesk { .. })
        ));
        let Some(ToolCommand::OpenBoundaryDesk { request }) = commands.first() else {
            unreachable!()
        };
        assert_eq!(
            request.desk.working_points.len(),
            4,
            "closed polygon rings expose only unique editable vertices"
        );
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(command, ToolCommand::UpdateBoundaryDesk { .. }))
                .count(),
            4
        );
        assert_eq!(
            commands
                .iter()
                .filter_map(|command| match command {
                    ToolCommand::UpdateBoundaryDesk { desk } => Some(desk.working_points.len()),
                    _ => None,
                })
                .next_back(),
            Some(4)
        );
        assert!(matches!(commands.last(), Some(ToolCommand::Shutdown)));
        let after_confirmation = tracer.project().unwrap();
        assert!(after_confirmation.boundary_evidence().is_some());
        assert_eq!(
            after_confirmation
                .acquisition_checkpoint()
                .unwrap()
                .request_identity
                .idempotency_key,
            "live-compatible-acquisition-job"
        );
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
        let mut review_transport = MemoryBoundaryTransport {
            events: VecDeque::from([
                ToolEvent::MapFoundationReviewDecisionRequested {
                    category: "building".into(),
                    subject_id: "obs-osm-relation-42".into(),
                    decision: campus_tool_protocol::MapFoundationCandidateDecision::Accept,
                },
                ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
                    category: "building".into(),
                    gap_id: "gap:building-entity:building:osm:relation/42:name".into(),
                    acknowledged: true,
                },
                ToolEvent::MapFoundationCategoryCompletionRequested {
                    category: "building".into(),
                },
                ToolEvent::MapFoundationCategoryCompletionRequested {
                    category: "circulation".into(),
                },
                ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
                    category: "water".into(),
                    gap_id: "gap:water:osm:31-121-1:0".into(),
                    acknowledged: true,
                },
                ToolEvent::MapFoundationCategoryCompletionRequested {
                    category: "water".into(),
                },
                ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
                    category: "vegetation".into(),
                    gap_id: "gap:vegetation:overture:31-121-2:0".into(),
                    acknowledged: true,
                },
                ToolEvent::MapFoundationCategoryCompletionRequested {
                    category: "vegetation".into(),
                },
                ToolEvent::MapKnownFeatureGapAcknowledgementRequested {
                    category: "sports".into(),
                    gap_id: "gap:sports:osm:31-121-3:0".into(),
                    acknowledged: true,
                },
                ToolEvent::MapFoundationCategoryCompletionRequested {
                    category: "sports".into(),
                },
            ]),
            commands: Vec::new(),
        };
        tracer
            .drive_foundation_review_tool_session(
                FoundationReviewDeskMapOptions {
                    js_api_key: "test-key".into(),
                    security_code: "test-security-code".into(),
                    zoom: 17.0,
                    pitch: 45.0,
                    rotation: 0.0,
                    english: true,
                },
                &mut review_transport,
            )
            .unwrap();
        assert!(matches!(
            review_transport.commands.first(),
            Some(ToolCommand::OpenFoundationReviewDesk { .. })
        ));
        assert!(review_transport.commands.iter().any(|command| matches!(
            command,
            ToolCommand::UpdateFoundationReviewDesk { desk }
                if desk.active_category == "water"
                    && desk.known_gaps.iter().all(|gap| gap.acknowledged)
        )));
        assert!(matches!(
            review_transport.commands.last(),
            Some(ToolCommand::Shutdown)
        ));

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

    #[test]
    fn retrying_same_empty_coarse_raster_job_persists_new_proposals() {
        let workspace = tempfile::tempdir().unwrap();
        let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
        let fixture_client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());
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
                project_name: "Coarse raster retry regression".into(),
                boundary_job_id: "live-compatible-boundary-job".into(),
                acquisition_job_id: "live-compatible-acquisition-job".into(),
            },
            InstallationId::new("acceptance-test").unwrap(),
            &capability,
            &fixture_client,
        )
        .unwrap();
        let snapshot = tracer.boundary_candidates().unwrap();
        tracer.begin_boundary_review(snapshot).unwrap();
        tracer
            .handle_boundary_tool_event(ToolEvent::MapBoundaryCandidateSelected {
                candidate_id: "boundary-osm-relation-100".into(),
            })
            .unwrap();
        tracer
            .handle_boundary_tool_event(ToolEvent::MapBoundaryConfirmed {
                candidate_id: "boundary-osm-relation-100".into(),
            })
            .unwrap();
        tracer.acquire_foundation_evidence().unwrap();

        let boundary_result_sha256 = tracer
            .project()
            .unwrap()
            .pinned_evidence()
            .unwrap()
            .boundary
            .manifest
            .result_sha256
            .clone();
        let retry_client = AcquisitionClient::new(RetryThenProposalTransport::canonical(
            &boundary_result_sha256,
        ));
        let retry_tracer = FixedDatasetTracer {
            library_root: tracer.library_root.clone(),
            output_root: tracer.output_root.clone(),
            campus_target_id: tracer.campus_target_id.clone(),
            actor: tracer.actor.clone(),
            capability: tracer.capability,
            acquisition_client: &retry_client,
            boundary_job_id: tracer.boundary_job_id.clone(),
            acquisition_job_id: tracer.acquisition_job_id.clone(),
            project_id: tracer.project_id.clone(),
        };

        retry_tracer
            .request_controlled_coarse_raster_supplement(
                FoundationCategory::Water,
                "gap:water:osm:31-121-1:0",
            )
            .unwrap();
        let first = retry_tracer.project().unwrap();
        assert_eq!(first.coarse_raster_runs().len(), 1);
        assert!(matches!(
            first.coarse_raster_runs()[0].outcome,
            campus_state::CoarseRasterRunOutcome::UnusableCoverage { .. }
        ));

        retry_tracer
            .request_controlled_coarse_raster_supplement(
                FoundationCategory::Water,
                "gap:water:osm:31-121-1:0",
            )
            .unwrap();
        let retried = retry_tracer.project().unwrap();
        assert_eq!(retried.coarse_raster_runs().len(), 2);
        assert!(matches!(
            retried.coarse_raster_runs()[1].outcome,
            campus_state::CoarseRasterRunOutcome::Proposals { .. }
        ));
    }

    #[test]
    fn unavailable_boundary_discovery_persists_a_recoverable_desk() {
        let workspace = tempfile::tempdir().unwrap();
        let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
        let client = AcquisitionClient::new(UnavailableControlledService);
        let tracer = FixedDatasetTracer::confirm_campus_target(
            FixedDatasetTracerRequest {
                library_root: workspace.path().join("library"),
                output_root: workspace.path().join("output"),
                campus_scope: CampusScope::new(
                    "gaode:test",
                    "Unavailable Campus",
                    [121.395, 31.202],
                )
                .unwrap(),
                project_name: "Unavailable boundary review".into(),
                boundary_job_id: "unavailable-boundary".into(),
                acquisition_job_id: "unavailable-acquisition".into(),
            },
            InstallationId::new("acceptance-test").unwrap(),
            &capability,
            &client,
        )
        .unwrap();

        let command = tracer
            .open_boundary_review_command(BoundaryDeskMapOptions {
                js_api_key: "test-key".into(),
                security_code: "test-security-code".into(),
                zoom: 17.0,
                pitch: 45.0,
                rotation: 0.0,
                english: true,
            })
            .unwrap();
        let ToolCommand::OpenBoundaryDesk { request } = command else {
            panic!("boundary review did not produce the map-helper command");
        };
        let surface = request.desk;
        assert!(surface.candidates.is_empty());
        assert!(surface
            .recovery_message
            .as_deref()
            .unwrap()
            .contains("unavailable"));
        let mut cancelled_transport = MemoryBoundaryTransport {
            events: VecDeque::from([ToolEvent::Closed {
                tool: ToolKind::Map,
            }]),
            commands: Vec::new(),
        };
        let cancellation = tracer
            .drive_boundary_tool_session(
                BoundaryDeskMapOptions {
                    js_api_key: "test-key".into(),
                    security_code: "test-security-code".into(),
                    zoom: 17.0,
                    pitch: 45.0,
                    rotation: 0.0,
                    english: true,
                },
                &mut cancelled_transport,
            )
            .unwrap();
        assert_eq!(
            cancellation,
            crate::v11_boundary_evidence_desk::BoundaryToolEventOutcome::Ignored
        );
        assert_eq!(
            cancelled_transport.commands.len(),
            1,
            "a closed helper receives no impossible projection update"
        );
        tracer
            .handle_boundary_tool_event(ToolEvent::Error {
                message: "WebView2 process exited".into(),
            })
            .unwrap();
        let persisted = tracer.project().unwrap();
        let review = persisted.boundary_review().unwrap();
        assert_eq!(
            review.projection().recovery_actions,
            vec![
                campus_state::BoundaryRecoveryAction::Retry,
                campus_state::BoundaryRecoveryAction::ReturnToCampusTarget,
            ]
        );
        assert!(review
            .projection()
            .actionable_feedback
            .as_deref()
            .unwrap()
            .contains("WebView2 process exited"));
        assert!(persisted.boundary_evidence().is_none());

        let reopened = tracer.open_boundary_review().unwrap();
        assert!(reopened
            .recovery_message
            .as_deref()
            .unwrap()
            .contains("WebView2 process exited"));
        assert_eq!(
            tracer
                .handle_boundary_tool_event(ToolEvent::MapBoundaryRetryRequested)
                .unwrap(),
            crate::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReviewUpdated
        );
        assert_eq!(
            tracer
                .handle_boundary_tool_event(ToolEvent::MapBoundaryReturnToCampusRequested)
                .unwrap(),
            crate::v11_boundary_evidence_desk::BoundaryToolEventOutcome::ReturnToCampusTargetRequested
        );
        assert!(tracer.project().unwrap().boundary_review().is_none());
    }

    #[test]
    fn failed_service_start_keeps_the_durable_boundary_and_retry_identity() {
        let workspace = tempfile::tempdir().unwrap();
        let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
        let client = AcquisitionClient::new(StartUnavailableTransport(
            FixtureTransport::canonical().unwrap(),
        ));
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
                project_name: "Durable acquisition handoff".into(),
                boundary_job_id: "live-compatible-boundary-job".into(),
                acquisition_job_id: "stable-retry-identity".into(),
            },
            InstallationId::new("acceptance-test").unwrap(),
            &capability,
            &client,
        )
        .unwrap();

        let snapshot = tracer.boundary_candidates().unwrap();
        tracer.begin_boundary_review(snapshot).unwrap();
        tracer
            .handle_boundary_tool_event(ToolEvent::MapBoundaryCandidateSelected {
                candidate_id: "boundary-osm-relation-100".into(),
            })
            .unwrap();
        let error = tracer
            .handle_boundary_tool_event(ToolEvent::MapBoundaryConfirmed {
                candidate_id: "boundary-osm-relation-100".into(),
            })
            .unwrap_err();
        assert!(error.contains("unavailable"));

        let reopened = tracer.project().unwrap();
        assert!(reopened.boundary_evidence().is_some());
        assert!(reopened.acquisition_checkpoint().is_none());
        assert_eq!(
            reopened
                .pending_acquisition_start()
                .unwrap()
                .idempotency_key,
            "stable-retry-identity"
        );
    }
}
