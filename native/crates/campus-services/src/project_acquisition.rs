use crate::acquisition::{
    AcquisitionClient, AcquisitionJobStatus, AcquisitionTransport, FoundationAcquisitionRequest,
    OutcomeScope, ResultManifest, SourceGeometry, VerifiedAcquisitionChunk,
};
use campus_state::{
    AcquisitionRequestIdentity, FoundationAcquisitionCheckpoint,
    FoundationAcquisitionCheckpointPurpose, InstallationId, PinnedAcquisitionEvidence,
    Schema2Project,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectAcquisitionProgress {
    Started,
    StatusRefreshed,
    ChunkPersisted { chunk_id: String },
    AllChunksVerified,
    EvidencePinned,
}

pub struct ProjectAcquisitionCoordinator<'a, T> {
    client: &'a AcquisitionClient<T>,
}

impl<'a, T: AcquisitionTransport> ProjectAcquisitionCoordinator<'a, T> {
    pub fn new(client: &'a AcquisitionClient<T>) -> Self {
        Self { client }
    }

    pub fn start_after_boundary_confirmation(
        &self,
        project: &mut Schema2Project,
        idempotency_key: &str,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        if let Some(existing) = project.acquisition_checkpoint() {
            if existing.request_identity.idempotency_key == idempotency_key {
                return Ok(ProjectAcquisitionProgress::Started);
            }
            return Err(
                "This project already has a different pinned Foundation acquisition request".into(),
            );
        }
        let boundary = project
            .boundary_evidence()
            .ok_or("Confirm a Campus Boundary before starting Foundation acquisition")?;
        let confirmed_geometry = boundary
            .confirmed_geometry()
            .ok_or("The confirmed Campus Boundary geometry is unavailable")?;
        let request = FoundationAcquisitionRequest::new(
            copy_typed(&boundary.manifest.bundle)?,
            boundary.manifest.result_sha256.clone(),
            copy_typed::<_, SourceGeometry>(confirmed_geometry)?,
            idempotency_key,
        )?;
        let capabilities = self
            .client
            .capabilities()
            .map_err(|error| error.to_string())?;
        if capabilities.retention_days < 30 {
            return Err(
                "The controlled service cannot retain acquisition delivery for at least 30 days"
                    .into(),
            );
        }
        let status = self
            .client
            .start_foundation_acquisition(&request)
            .map_err(|error| error.to_string())?;
        let checkpoint = FoundationAcquisitionCheckpoint::new(
            status.job_id.clone(),
            status.contract_version.clone(),
            copy_typed(request.bundle())?,
            request.boundary_revision(),
            AcquisitionRequestIdentity::new(
                request.idempotency_key(),
                request
                    .content_sha256()
                    .map_err(|error| error.to_string())?,
            )?,
            copy_typed(&status.state)?,
            copy_typed(&status.outcomes)?,
            copy_typed(&status.failure)?,
            capabilities.retention_days,
        )?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::Started)
    }

    pub fn start_queued_boundary_acquisition(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let idempotency_key = project
            .pending_acquisition_start()
            .ok_or("Persist a pending Foundation acquisition start before contacting the service")?
            .idempotency_key
            .clone();
        self.start_after_boundary_confirmation(project, &idempotency_key, actor)
    }

    pub fn start_explicit_refresh(
        &self,
        project: &mut Schema2Project,
        idempotency_key: &str,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        if idempotency_key.trim().is_empty() {
            return Err("Explicit Foundation refresh requires a stable idempotency key".into());
        }
        if let Some(existing) = project.acquisition_checkpoint() {
            if project.acquisition_checkpoint_purpose()
                == FoundationAcquisitionCheckpointPurpose::ExplicitRefresh
                && existing.request_identity.idempotency_key == idempotency_key
            {
                return Ok(ProjectAcquisitionProgress::Started);
            }
            return Err("This project already has a different Foundation acquisition job".into());
        }
        let boundary = project
            .boundary_evidence()
            .ok_or("Confirm a Campus Boundary before requesting a Foundation refresh")?;
        let current = project
            .pinned_evidence()
            .ok_or("Pin the initial Foundation acquisition before requesting a refresh")?;
        let confirmed_geometry = boundary
            .confirmed_geometry()
            .ok_or("The confirmed Campus Boundary geometry is unavailable")?;
        let capabilities = self
            .client
            .capabilities()
            .map_err(|error| error.to_string())?;
        if capabilities.retention_days < 30 {
            return Err(
                "The controlled service cannot retain acquisition delivery for at least 30 days"
                    .into(),
            );
        }
        let current_precedence = capabilities
            .refresh_bundle_precedence
            .iter()
            .position(|bundle_id| bundle_id == &current.acquisition.manifest.bundle.id)
            .ok_or(
                "The controlled service did not rank the currently pinned Dataset Bundle for refresh",
            )?;
        let bundle = capabilities.refresh_bundle_precedence[..current_precedence]
            .iter()
            .find_map(|bundle_id| {
                capabilities
                    .supported_bundles
                    .iter()
                    .find(|bundle| bundle.id == *bundle_id)
            })
            .ok_or("The controlled service has no newer Dataset Bundle for an explicit refresh")?
            .clone();
        let request = FoundationAcquisitionRequest::new(
            bundle,
            boundary.manifest.result_sha256.clone(),
            copy_typed::<_, SourceGeometry>(confirmed_geometry)?,
            idempotency_key,
        )?;
        let status = self
            .client
            .start_foundation_acquisition(&request)
            .map_err(|error| error.to_string())?;
        let checkpoint = FoundationAcquisitionCheckpoint::new(
            status.job_id.clone(),
            status.contract_version.clone(),
            copy_typed(request.bundle())?,
            request.boundary_revision(),
            AcquisitionRequestIdentity::new(
                request.idempotency_key(),
                request
                    .content_sha256()
                    .map_err(|error| error.to_string())?,
            )?,
            copy_typed(&status.state)?,
            copy_typed(&status.outcomes)?,
            copy_typed(&status.failure)?,
            capabilities.retention_days,
        )?;
        project.record_explicit_refresh_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::Started)
    }

    pub fn reconnect(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let previous = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before reconnecting")?;
        let current = self
            .client
            .acquisition_job(&service_status(&previous)?)
            .map_err(|error| error.to_string())?;
        let checkpoint = checkpoint_with_status(previous, &current)?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::StatusRefreshed)
    }

    pub fn retry_scopes(
        &self,
        project: &mut Schema2Project,
        scopes: &[OutcomeScope],
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let previous = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before retrying")?;
        let current = self
            .client
            .retry_foundation_acquisition(&service_status(&previous)?, scopes)
            .map_err(|error| error.to_string())?;
        let checkpoint = checkpoint_with_status(previous, &current)?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::StatusRefreshed)
    }

    pub fn continue_job(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        self.retry_scopes(project, &[], actor)
    }

    pub fn cancel(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let previous = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before cancelling")?;
        let current = self
            .client
            .cancel_foundation_acquisition(&service_status(&previous)?)
            .map_err(|error| error.to_string())?;
        let checkpoint = checkpoint_with_status(previous, &current)?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::StatusRefreshed)
    }

    pub fn pin_manifest(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let mut checkpoint = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before loading its manifest")?;
        let manifest = self
            .client
            .foundation_manifest(&service_status(&checkpoint)?)
            .map_err(|error| error.to_string())?;
        checkpoint.record_manifest(copy_typed(&manifest)?)?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::StatusRefreshed)
    }

    pub fn download_next_chunk(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let mut checkpoint = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before downloading evidence")?;
        let state_manifest = checkpoint
            .manifest
            .as_ref()
            .ok_or("Pin the acquisition manifest before downloading evidence")?;
        let manifest: ResultManifest = copy_typed(state_manifest)?;
        let Some(descriptor) = manifest.chunks.iter().find(|descriptor| {
            !checkpoint
                .verified_chunks
                .iter()
                .any(|verified| verified.descriptor.id == descriptor.id)
        }) else {
            return Ok(ProjectAcquisitionProgress::AllChunksVerified);
        };
        let verified = self
            .client
            .download_foundation_chunk(&service_status(&checkpoint)?, &manifest, descriptor)
            .map_err(|error| error.to_string())?;
        let chunk_id = verified.descriptor.id.clone();
        checkpoint.record_verified_chunk(copy_typed(&verified)?)?;
        project.record_acquisition_checkpoint(checkpoint, actor)?;
        Ok(ProjectAcquisitionProgress::ChunkPersisted { chunk_id })
    }

    pub fn finalize(
        &self,
        project: &mut Schema2Project,
        actor: InstallationId,
    ) -> Result<ProjectAcquisitionProgress, String> {
        let checkpoint = project
            .acquisition_checkpoint()
            .cloned()
            .ok_or("Start Foundation acquisition before finalizing evidence")?;
        let manifest: ResultManifest = copy_typed(
            checkpoint
                .manifest
                .as_ref()
                .ok_or("Pin the acquisition manifest before finalizing evidence")?,
        )?;
        let verified_chunks = checkpoint
            .verified_chunks
            .iter()
            .map(copy_typed::<_, VerifiedAcquisitionChunk>)
            .collect::<Result<Vec<_>, _>>()?;
        let delivery = self
            .client
            .finalize_foundation_delivery(&service_status(&checkpoint)?, manifest, verified_chunks)
            .map_err(|error| error.to_string())?;
        let evidence = PinnedAcquisitionEvidence {
            manifest: copy_typed(&delivery.manifest)?,
            observations: copy_typed(&delivery.observations)?,
        };
        if project.acquisition_checkpoint_purpose()
            == FoundationAcquisitionCheckpointPurpose::ExplicitRefresh
        {
            project.apply_foundation_refresh(evidence, None, actor)?;
        } else {
            project.pin_acquisition(evidence, actor)?;
        }
        Ok(ProjectAcquisitionProgress::EvidencePinned)
    }
}

fn checkpoint_with_status(
    mut checkpoint: FoundationAcquisitionCheckpoint,
    status: &AcquisitionJobStatus,
) -> Result<FoundationAcquisitionCheckpoint, String> {
    if checkpoint.job_id != status.job_id
        || checkpoint.contract_version != status.contract_version
        || checkpoint.bundle.id != status.bundle_id
    {
        return Err("Controlled-service status changed the pinned acquisition identity".into());
    }
    checkpoint.state = copy_typed(&status.state)?;
    checkpoint.outcomes = copy_typed(&status.outcomes)?;
    checkpoint.failure = copy_typed(&status.failure)?;
    Ok(checkpoint)
}

fn service_status(
    checkpoint: &FoundationAcquisitionCheckpoint,
) -> Result<AcquisitionJobStatus, String> {
    Ok(AcquisitionJobStatus {
        job_id: checkpoint.job_id.clone(),
        contract_version: checkpoint.contract_version.clone(),
        bundle_id: checkpoint.bundle.id.clone(),
        state: copy_typed(&checkpoint.state)?,
        outcomes: copy_typed(&checkpoint.outcomes)?,
        failure: copy_typed(&checkpoint.failure)?,
        negotiated_bundle: Some(copy_typed(&checkpoint.bundle)?),
    })
}

fn copy_typed<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, String> {
    serde_json::from_value(serde_json::to_value(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
