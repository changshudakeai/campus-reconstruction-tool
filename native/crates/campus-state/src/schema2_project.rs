use crate::{
    BoundaryCandidate, FoundationAcquisitionCheckpoint, FoundationCategory, ProviderOutcomeStatus,
    ResultManifest, SourceGeometry, SourceObservation,
};
use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

mod portable_project;
mod schema1_migration;
pub use portable_project::*;
pub use schema1_migration::*;

pub const SCHEMA_2_VERSION: u32 = 2;
const LIBRARY_INDEX_FILE: &str = "library-index.json";
const PROJECT_FILE_NAME: &str = "project.campus.json";
const PREVIOUS_PROJECT_FILE_NAME: &str = "previous-confirmed.campus.json";
const RECOVERY_PROJECT_FILE_NAME: &str = "recovery.campus.json";
const SAVE_STAGE_FILE_NAME: &str = "project.save-stage.json";
const SAVE_ROLLBACK_FILE_NAME: &str = "project.save-rollback.json";
const DEVELOPMENT_CANONICAL_ROLE: &str = "developmentCanonical";
const PROJECT_HISTORY_LIMIT: usize = 50;

#[derive(Debug)]
pub struct V11ConstructionCapability(());

impl V11ConstructionCapability {
    pub fn request(
        development_build: bool,
        environment_value: Option<&str>,
    ) -> Result<Self, String> {
        if v11_construction_enabled(development_build, environment_value) {
            Ok(Self(()))
        } else {
            Err("Schema-2 project construction is not enabled for this build".into())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct InstallationId(String);

impl InstallationId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("Installation ID cannot be empty".into());
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CampusScope {
    target_id: String,
    canonical_name: String,
    anchor_wgs84: [f64; 2],
    #[serde(default)]
    gaode_poi_id: Option<String>,
    #[serde(default, flatten)]
    optional_state: Map<String, Value>,
}

impl CampusScope {
    pub fn new(
        target_id: impl Into<String>,
        canonical_name: impl Into<String>,
        anchor_wgs84: [f64; 2],
    ) -> Result<Self, String> {
        let target_id = target_id.into();
        let canonical_name = canonical_name.into();
        if target_id.trim().is_empty() || canonical_name.trim().is_empty() {
            return Err("Campus scope requires target identity and canonical name".into());
        }
        if !anchor_wgs84.iter().all(|coordinate| coordinate.is_finite()) {
            return Err("Campus anchor must contain finite WGS-84 coordinates".into());
        }
        Ok(Self {
            target_id,
            canonical_name,
            anchor_wgs84,
            gaode_poi_id: None,
            optional_state: Map::new(),
        })
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn canonical_name(&self) -> &str {
        &self.canonical_name
    }

    pub fn anchor_wgs84(&self) -> [f64; 2] {
        self.anchor_wgs84
    }

    pub fn gaode_poi_id(&self) -> Option<&str> {
        self.gaode_poi_id.as_deref()
    }

    pub fn with_gaode_poi_id(mut self, gaode_poi_id: impl Into<String>) -> Result<Self, String> {
        let gaode_poi_id = gaode_poi_id.into();
        if gaode_poi_id.trim().is_empty() {
            return Err("Gaode POI ID cannot be empty".into());
        }
        self.gaode_poi_id = Some(gaode_poi_id);
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAudit {
    created_at_unix_ms: u64,
    created_by: InstallationId,
    updated_at_unix_ms: u64,
    updated_by: InstallationId,
    #[serde(default, flatten)]
    optional_state: Map<String, Value>,
}

impl ProjectAudit {
    pub fn created_by(&self) -> &InstallationId {
        &self.created_by
    }

    pub fn updated_by(&self) -> &InstallationId {
        &self.updated_by
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct V11CompatibilityProfile {
    profile_id: String,
    edition: String,
    minecraft_version: String,
    preview_profile_id: String,
    export_profile_id: String,
    block_catalog_id: String,
    #[serde(default, flatten)]
    optional_state: Map<String, Value>,
}

impl V11CompatibilityProfile {
    pub fn minecraft_java_26_1_2() -> Self {
        Self {
            profile_id: "minecraft-java-26.1.2-axiom-v1".into(),
            edition: "Java Edition".into(),
            minecraft_version: "26.1.2".into(),
            preview_profile_id: "minecraft-java-26.1.2-preview-v1".into(),
            export_profile_id: "minecraft-java-26.1.2-sponge-v3-v1".into(),
            block_catalog_id: "minecraft-java-26.1.2-blocks-v1".into(),
            optional_state: Map::new(),
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn edition(&self) -> &str {
        &self.edition
    }

    pub fn minecraft_version(&self) -> &str {
        &self.minecraft_version
    }

    pub fn preview_profile_id(&self) -> &str {
        &self.preview_profile_id
    }

    pub fn export_profile_id(&self) -> &str {
        &self.export_profile_id
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DurableTaskState {
    Pending,
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DurableWorkflowState {
    project_revision: u64,
    campus_target: DurableTaskState,
    boundary: DurableTaskState,
    acquisition: DurableTaskState,
    review: DurableTaskState,
    generation: DurableTaskState,
    export: DurableTaskState,
    #[serde(default, flatten)]
    optional_state: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinnedBoundaryEvidence {
    pub manifest: ResultManifest,
    pub candidates: Vec<BoundaryCandidate>,
    pub selected_candidate_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinnedAcquisitionEvidence {
    pub manifest: ResultManifest,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinnedFoundationEvidence<'a> {
    pub boundary: &'a PinnedBoundaryEvidence,
    pub acquisition: &'a PinnedAcquisitionEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "decision")]
pub enum FoundationReviewDisposition {
    SelectedEvidence { evidence_ids: Vec<String> },
    CompleteEmpty,
    KnownGap { reasons: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewBasis {
    pub boundary_result_sha256: String,
    pub selected_boundary_id: String,
    pub acquisition_result_sha256: String,
    pub classification_rules: String,
    pub conflation_rules: String,
    pub derivation_rules: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewEntry {
    pub sequence: u64,
    pub category: FoundationCategory,
    pub subjects: Vec<String>,
    pub basis: FoundationReviewBasis,
    pub before: Option<FoundationReviewDisposition>,
    pub after: FoundationReviewDisposition,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewLedger {
    entries: Vec<FoundationReviewEntry>,
}

impl FoundationReviewLedger {
    pub fn disposition(
        &self,
        category: FoundationCategory,
    ) -> Option<&FoundationReviewDisposition> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.category == category)
            .map(|entry| &entry.after)
    }

    pub fn entries(&self) -> &[FoundationReviewEntry] {
        &self.entries
    }

    fn disposition_for_basis(
        &self,
        category: FoundationCategory,
        basis: &FoundationReviewBasis,
    ) -> Option<&FoundationReviewDisposition> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.category == category && entry.basis == *basis)
            .map(|entry| &entry.after)
    }

    fn is_complete_for_basis(&self, basis: &FoundationReviewBasis) -> bool {
        FoundationCategory::ALL
            .into_iter()
            .all(|category| self.disposition_for_basis(category, basis).is_some())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationGenerationSettings {
    pub orientation_degrees: f64,
    pub blocks_per_meter: f64,
    pub style_id: String,
    pub surface_block: String,
    pub generators: BTreeMap<FoundationCategory, FoundationGeneratorStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationGeneratorStyle {
    pub generator_id: String,
    pub block: String,
    pub height: usize,
}

impl Default for FoundationGenerationSettings {
    fn default() -> Self {
        Self {
            orientation_degrees: 0.0,
            blocks_per_meter: 0.02,
            style_id: "v1.1-fixed-campus-style".into(),
            surface_block: "minecraft:grass_block".into(),
            generators: [
                (
                    FoundationCategory::Building,
                    FoundationGeneratorStyle {
                        generator_id: "foundation-building-footprint-v1".into(),
                        block: "minecraft:stone_bricks".into(),
                        height: 4,
                    },
                ),
                (
                    FoundationCategory::Circulation,
                    FoundationGeneratorStyle {
                        generator_id: "foundation-circulation-centreline-v1".into(),
                        block: "minecraft:stone".into(),
                        height: 2,
                    },
                ),
                (
                    FoundationCategory::Water,
                    FoundationGeneratorStyle {
                        generator_id: "foundation-water-surface-v1".into(),
                        block: "minecraft:water".into(),
                        height: 2,
                    },
                ),
                (
                    FoundationCategory::Vegetation,
                    FoundationGeneratorStyle {
                        generator_id: "foundation-vegetation-v1".into(),
                        block: "minecraft:moss_block".into(),
                        height: 2,
                    },
                ),
                (
                    FoundationCategory::Sports,
                    FoundationGeneratorStyle {
                        generator_id: "foundation-sports-surface-v1".into(),
                        block: "minecraft:green_concrete".into(),
                        height: 2,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedFoundationOutput {
    pub project_revision: u64,
    pub compatibility_profile_id: String,
    pub width: usize,
    pub height: usize,
    pub length: usize,
    pub non_air_blocks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFoundationOutput {
    pub project_revision: u64,
    pub schematic_sha256: String,
    pub schematic_bytes: u64,
    pub manifest_file_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FoundationTracerState {
    boundary: Option<PinnedBoundaryEvidence>,
    #[serde(default)]
    acquisition_checkpoint: Option<FoundationAcquisitionCheckpoint>,
    acquisition: Option<PinnedAcquisitionEvidence>,
    review_ledger: FoundationReviewLedger,
    generation_settings: FoundationGenerationSettings,
    generated: Option<GeneratedFoundationOutput>,
    exported: Option<ExportedFoundationOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationResumePoint {
    BoundaryReview,
    Acquisition,
    Review(FoundationCategory),
    Generation,
    Export,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ProjectHistoryOperation {
    sequence: u64,
    description: String,
    completed_at_unix_ms: u64,
    before: Value,
    after: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ProjectDurabilityState {
    #[serde(default)]
    last_confirmed_save_unix_ms: Option<u64>,
    #[serde(default)]
    next_history_sequence: u64,
    #[serde(default)]
    undo: Vec<ProjectHistoryOperation>,
    #[serde(default)]
    redo: Vec<ProjectHistoryOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectHistorySummary {
    sequence: u64,
    description: String,
    completed_at_unix_ms: u64,
}

impl ProjectHistorySummary {
    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn completed_at_unix_ms(&self) -> u64 {
        self.completed_at_unix_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSaveStatus {
    Saving,
    Saved { completed_at_unix_ms: u64 },
    Failed { reason: String },
}

impl ProjectSaveStatus {
    pub fn retry_reason(&self) -> Option<&str> {
        match self {
            Self::Failed { reason } => Some(reason),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFaultPoint {
    BeforeStageWrite,
    AfterStageWrite,
    AfterStageValidation,
    BeforePreviousConfirmedWrite,
    AfterPreviousConfirmedWrite,
    BeforeProjectReplace,
    AfterProjectReplace,
    BeforeIndexReplace,
    AfterIndexReplace,
}

impl SaveFaultPoint {
    pub const ALL: [Self; 9] = [
        Self::BeforeStageWrite,
        Self::AfterStageWrite,
        Self::AfterStageValidation,
        Self::BeforePreviousConfirmedWrite,
        Self::AfterPreviousConfirmedWrite,
        Self::BeforeProjectReplace,
        Self::AfterProjectReplace,
        Self::BeforeIndexReplace,
        Self::AfterIndexReplace,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRecoveryEnvelope {
    captured_at_unix_ms: u64,
    project: Schema2Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSaveRollback {
    project_bytes: Vec<u8>,
    library_index_bytes: Vec<u8>,
    previous_project_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecoveryCandidate {
    captured_at_unix_ms: u64,
    project_revision: u64,
    recent_operations: Vec<String>,
}

impl ProjectRecoveryCandidate {
    pub fn captured_at_unix_ms(&self) -> u64 {
        self.captured_at_unix_ms
    }

    pub fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub fn recent_operations(&self) -> &[String] {
        &self.recent_operations
    }
}

impl DurableWorkflowState {
    fn new() -> Self {
        Self {
            project_revision: 0,
            campus_target: DurableTaskState::Confirmed,
            boundary: DurableTaskState::Pending,
            acquisition: DurableTaskState::Pending,
            review: DurableTaskState::Pending,
            generation: DurableTaskState::Pending,
            export: DurableTaskState::Pending,
            optional_state: Map::new(),
        }
    }

    pub fn project_revision(&self) -> u64 {
        self.project_revision
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schema2Project {
    schema_version: u32,
    project_id: ProjectId,
    name: String,
    campus_scope: CampusScope,
    audit: ProjectAudit,
    compatibility_profile: V11CompatibilityProfile,
    workflow: DurableWorkflowState,
    #[serde(default)]
    foundation: FoundationTracerState,
    #[serde(default)]
    durability: ProjectDurabilityState,
    #[serde(default, flatten)]
    optional_state: Map<String, Value>,
}

impl Schema2Project {
    fn new(scope: CampusScope, name: String, actor: InstallationId) -> Result<Self, String> {
        validate_project_name(&name)?;
        let now = now_unix_ms();
        Ok(Self {
            schema_version: SCHEMA_2_VERSION,
            project_id: ProjectId::generate(),
            name,
            campus_scope: scope,
            audit: ProjectAudit {
                created_at_unix_ms: now,
                created_by: actor.clone(),
                updated_at_unix_ms: now,
                updated_by: actor,
                optional_state: Map::new(),
            },
            compatibility_profile: V11CompatibilityProfile::minecraft_java_26_1_2(),
            workflow: DurableWorkflowState::new(),
            foundation: FoundationTracerState::default(),
            durability: ProjectDurabilityState::default(),
            optional_state: Map::new(),
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn id(&self) -> &ProjectId {
        &self.project_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn campus_scope(&self) -> &CampusScope {
        &self.campus_scope
    }

    pub fn audit(&self) -> &ProjectAudit {
        &self.audit
    }

    pub fn compatibility_profile(&self) -> &V11CompatibilityProfile {
        &self.compatibility_profile
    }

    pub fn workflow(&self) -> &DurableWorkflowState {
        &self.workflow
    }

    pub fn pinned_evidence(&self) -> Option<PinnedFoundationEvidence<'_>> {
        Some(PinnedFoundationEvidence {
            boundary: self.foundation.boundary.as_ref()?,
            acquisition: self.foundation.acquisition.as_ref()?,
        })
    }

    pub fn boundary_evidence(&self) -> Option<&PinnedBoundaryEvidence> {
        self.foundation.boundary.as_ref()
    }

    pub fn acquisition_checkpoint(&self) -> Option<&FoundationAcquisitionCheckpoint> {
        self.foundation.acquisition_checkpoint.as_ref()
    }

    pub fn foundation_review(&self) -> &FoundationReviewLedger {
        &self.foundation.review_ledger
    }

    pub fn generation_settings(&self) -> &FoundationGenerationSettings {
        &self.foundation.generation_settings
    }

    pub fn generated_output(&self) -> Option<&GeneratedFoundationOutput> {
        self.foundation.generated.as_ref()
    }

    pub fn exported_output(&self) -> Option<&ExportedFoundationOutput> {
        self.foundation.exported.as_ref()
    }

    pub fn resume_point(&self) -> FoundationResumePoint {
        if self.foundation.boundary.is_none() {
            return FoundationResumePoint::BoundaryReview;
        }
        if self.foundation.acquisition.is_none() {
            return FoundationResumePoint::Acquisition;
        }
        let basis = self
            .review_basis()
            .expect("pinned boundary and acquisition produce a review basis");
        for category in FoundationCategory::ALL {
            if self
                .foundation
                .review_ledger
                .disposition_for_basis(category, &basis)
                .is_none()
            {
                return FoundationResumePoint::Review(category);
            }
        }
        if self.foundation.generated.is_none() {
            return FoundationResumePoint::Generation;
        }
        if self.foundation.exported.is_none() {
            return FoundationResumePoint::Export;
        }
        FoundationResumePoint::Complete
    }

    pub fn confirm_boundary(
        &mut self,
        evidence: PinnedBoundaryEvidence,
        actor: InstallationId,
    ) -> Result<(), String> {
        let selected_geometry = evidence
            .candidates
            .iter()
            .find(|candidate| candidate.id == evidence.selected_candidate_id)
            .map(|candidate| &candidate.geometry);
        if evidence.manifest.bundle.id.trim().is_empty()
            || evidence.manifest.result_sha256.trim().is_empty()
            || evidence.selected_candidate_id.trim().is_empty()
            || evidence.candidates.is_empty()
            || selected_geometry.is_none()
            || selected_geometry.is_some_and(|geometry| geometry.all_points().len() < 4)
            || selected_geometry.is_some_and(|geometry| {
                geometry
                    .all_points()
                    .iter()
                    .flatten()
                    .any(|coordinate| !coordinate.is_finite())
            })
        {
            return Err("Confirmed boundary evidence is incomplete or invalid".into());
        }
        self.mark_updated(actor)?;
        self.foundation.boundary = Some(evidence);
        self.foundation.acquisition_checkpoint = None;
        self.foundation.acquisition = None;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.boundary = DurableTaskState::Confirmed;
        self.workflow.acquisition = DurableTaskState::Pending;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn record_acquisition_checkpoint(
        &mut self,
        checkpoint: FoundationAcquisitionCheckpoint,
        actor: InstallationId,
    ) -> Result<(), String> {
        let boundary = self
            .foundation
            .boundary
            .as_ref()
            .ok_or("Confirm a Campus Boundary before starting Foundation acquisition")?;
        checkpoint.validate()?;
        if checkpoint.bundle != boundary.manifest.bundle
            || checkpoint.boundary_revision != boundary.manifest.result_sha256
        {
            return Err(
                "Foundation acquisition must reuse the confirmed Boundary Discovery Snapshot"
                    .into(),
            );
        }
        if let Some(previous) = &self.foundation.acquisition_checkpoint {
            if checkpoint.job_id != previous.job_id
                || checkpoint.contract_version != previous.contract_version
                || checkpoint.bundle != previous.bundle
                || checkpoint.boundary_revision != previous.boundary_revision
                || checkpoint.request_identity != previous.request_identity
                || checkpoint.retention_days != previous.retention_days
            {
                return Err(
                    "Resumed Foundation acquisition changed its pinned request or bundle identity"
                        .into(),
                );
            }
            for successful in previous.outcomes.iter().filter(|outcome| {
                matches!(
                    outcome.status,
                    ProviderOutcomeStatus::Complete | ProviderOutcomeStatus::CompleteEmpty
                )
            }) {
                if !checkpoint
                    .outcomes
                    .iter()
                    .any(|current| current == successful)
                {
                    return Err(
                        "Resumed Foundation acquisition discarded successful provider evidence"
                            .into(),
                    );
                }
            }
            for verified in &previous.verified_chunks {
                if !checkpoint
                    .verified_chunks
                    .iter()
                    .any(|current| current == verified)
                {
                    return Err(
                        "Resumed Foundation acquisition discarded a verified result chunk".into(),
                    );
                }
            }
        }
        self.mark_updated(actor)?;
        self.foundation.acquisition_checkpoint = Some(checkpoint);
        if self.foundation.acquisition.is_none() {
            self.foundation.generated = None;
            self.foundation.exported = None;
            self.workflow.acquisition = DurableTaskState::Pending;
            self.workflow.review = DurableTaskState::Pending;
            self.workflow.generation = DurableTaskState::Pending;
            self.workflow.export = DurableTaskState::Pending;
        }
        Ok(())
    }

    pub fn pin_acquisition(
        &mut self,
        evidence: PinnedAcquisitionEvidence,
        actor: InstallationId,
    ) -> Result<(), String> {
        let boundary = self
            .foundation
            .boundary
            .as_ref()
            .ok_or("Confirm a Campus Boundary before pinning acquisition evidence")?;
        if evidence.manifest.bundle.id != boundary.manifest.bundle.id {
            return Err(
                "Boundary and acquisition evidence must use the same Dataset Bundle".into(),
            );
        }
        if evidence.manifest.result_sha256.trim().is_empty()
            || !FoundationCategory::ALL.into_iter().all(|category| {
                evidence
                    .manifest
                    .coverage_report
                    .outcomes
                    .iter()
                    .any(|outcome| outcome.category == category)
            })
        {
            return Err("Pinned acquisition evidence is incomplete".into());
        }
        self.mark_updated(actor)?;
        self.foundation.acquisition = Some(evidence);
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.acquisition = DurableTaskState::Confirmed;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn complete_foundation_review(
        &mut self,
        category: FoundationCategory,
        disposition: FoundationReviewDisposition,
        actor: InstallationId,
    ) -> Result<(), String> {
        let acquisition = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Foundation categories")?;
        match &disposition {
            FoundationReviewDisposition::SelectedEvidence { evidence_ids } => {
                if evidence_ids.is_empty()
                    || evidence_ids.iter().any(|id| {
                        !acquisition
                            .observations
                            .iter()
                            .any(|item| item.category == category && item.id == *id)
                    })
                {
                    return Err("Selected review evidence is not pinned for this category".into());
                }
            }
            FoundationReviewDisposition::CompleteEmpty => {
                if acquisition
                    .observations
                    .iter()
                    .any(|item| item.category == category)
                    || acquisition
                        .manifest
                        .coverage_report
                        .outcomes
                        .iter()
                        .filter(|outcome| outcome.category == category)
                        .any(|outcome| !outcome.gaps.is_empty())
                {
                    return Err(
                        "A non-empty or incomplete category cannot be completed empty".into(),
                    );
                }
            }
            FoundationReviewDisposition::KnownGap { reasons } if reasons.is_empty() => {
                return Err("A known gap requires an explicit reason".into());
            }
            FoundationReviewDisposition::KnownGap { .. } => {}
        }
        let basis = self.review_basis()?;
        let subjects = match &disposition {
            FoundationReviewDisposition::SelectedEvidence { evidence_ids } => evidence_ids.clone(),
            _ => vec![format!("foundation-category:{category:?}").to_ascii_lowercase()],
        };
        let before = self
            .foundation
            .review_ledger
            .disposition_for_basis(category, &basis)
            .cloned();
        self.mark_updated(actor)?;
        let sequence = self.foundation.review_ledger.entries.len() as u64 + 1;
        self.foundation
            .review_ledger
            .entries
            .push(FoundationReviewEntry {
                sequence,
                category,
                subjects,
                basis: basis.clone(),
                before,
                after: disposition,
                recorded_at_unix_ms: now_unix_ms(),
            });
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = if self.foundation.review_ledger.is_complete_for_basis(&basis) {
            DurableTaskState::Confirmed
        } else {
            DurableTaskState::Pending
        };
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn reviewed_projection(&self) -> Result<ReviewedFoundationProjection, String> {
        let basis = self.review_basis()?;
        if !self.foundation.review_ledger.is_complete_for_basis(&basis) {
            return Err("All five Foundation categories must be explicitly reviewed".into());
        }
        let evidence = self
            .pinned_evidence()
            .ok_or("Pinned Foundation evidence is incomplete")?;
        let selected_ids = FoundationCategory::ALL
            .iter()
            .filter_map(|category| {
                self.foundation
                    .review_ledger
                    .disposition_for_basis(*category, &basis)
            })
            .filter_map(|disposition| match disposition {
                FoundationReviewDisposition::SelectedEvidence { evidence_ids } => {
                    Some(evidence_ids.as_slice())
                }
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let selected_features = evidence
            .acquisition
            .observations
            .iter()
            .filter(|observation| selected_ids.contains(&&observation.id))
            .cloned()
            .collect();
        Ok(ReviewedFoundationProjection {
            boundary: evidence
                .boundary
                .candidates
                .iter()
                .find(|candidate| candidate.id == evidence.boundary.selected_candidate_id)
                .ok_or("Selected Campus Boundary candidate is missing")?
                .geometry
                .clone(),
            selected_features,
            generation_settings: self.foundation.generation_settings.clone(),
        })
    }

    fn review_basis(&self) -> Result<FoundationReviewBasis, String> {
        let boundary = self
            .foundation
            .boundary
            .as_ref()
            .ok_or("Confirmed Campus Boundary evidence is missing")?;
        let acquisition = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pinned Foundation acquisition evidence is missing")?;
        Ok(FoundationReviewBasis {
            boundary_result_sha256: boundary.manifest.result_sha256.clone(),
            selected_boundary_id: boundary.selected_candidate_id.clone(),
            acquisition_result_sha256: acquisition.manifest.result_sha256.clone(),
            classification_rules: acquisition.manifest.bundle.classification_rules.clone(),
            conflation_rules: acquisition.manifest.bundle.conflation_rules.clone(),
            derivation_rules: acquisition.manifest.bundle.derivation_rules.clone(),
        })
    }

    pub fn record_generation(
        &mut self,
        width: usize,
        height: usize,
        length: usize,
        non_air_blocks: usize,
        actor: InstallationId,
    ) -> Result<(), String> {
        self.reviewed_projection()?;
        if width == 0 || height == 0 || length == 0 || non_air_blocks == 0 {
            return Err("Generated Foundation output must be non-empty".into());
        }
        self.mark_updated(actor)?;
        self.foundation.generated = Some(GeneratedFoundationOutput {
            project_revision: self.workflow.project_revision,
            compatibility_profile_id: self.compatibility_profile.profile_id.clone(),
            width,
            height,
            length,
            non_air_blocks,
        });
        self.foundation.exported = None;
        self.workflow.generation = DurableTaskState::Confirmed;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn record_export(
        &mut self,
        schematic_sha256: String,
        schematic_bytes: u64,
        manifest_file_name: String,
    ) -> Result<(), String> {
        let generated = self
            .foundation
            .generated
            .as_ref()
            .ok_or("Generate the current reviewed projection before export")?;
        if generated.project_revision != self.workflow.project_revision
            || schematic_sha256.trim().is_empty()
            || schematic_bytes == 0
            || manifest_file_name.trim().is_empty()
        {
            return Err("Export metadata is incomplete or stale".into());
        }
        self.foundation.exported = Some(ExportedFoundationOutput {
            project_revision: self.workflow.project_revision,
            schematic_sha256,
            schematic_bytes,
            manifest_file_name,
        });
        self.workflow.export = DurableTaskState::Confirmed;
        Ok(())
    }

    pub fn mark_updated(&mut self, actor: InstallationId) -> Result<(), String> {
        self.workflow.project_revision = self
            .workflow
            .project_revision
            .checked_add(1)
            .ok_or("Project revision is exhausted")?;
        self.audit.updated_at_unix_ms = now_unix_ms();
        self.audit.updated_by = actor;
        Ok(())
    }

    fn rename(&mut self, name: String, actor: InstallationId) -> Result<(), String> {
        validate_project_name(&name)?;
        self.name = name;
        self.mark_updated(actor)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewedFoundationProjection {
    pub boundary: SourceGeometry,
    pub selected_features: Vec<SourceObservation>,
    pub generation_settings: FoundationGenerationSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CampusProjectLibraryRecord {
    project_id: ProjectId,
    campus_target_id: String,
    name: String,
    managed_relative_path: String,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    #[serde(default)]
    latest_successful_save_unix_ms: u64,
    compatibility_profile_id: String,
    #[serde(default)]
    internal_role: Option<String>,
}

impl CampusProjectLibraryRecord {
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    pub fn managed_relative_path(&self) -> &str {
        &self.managed_relative_path
    }

    pub fn latest_successful_save_unix_ms(&self) -> u64 {
        if self.latest_successful_save_unix_ms == 0 {
            self.updated_at_unix_ms
        } else {
            self.latest_successful_save_unix_ms
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndex {
    #[serde(default)]
    projects: Vec<CampusProjectLibraryRecord>,
}

#[derive(Debug)]
pub struct CampusProjectLibrary {
    root: PathBuf,
    campus_target_id: String,
    index: LibraryIndex,
    construction_enabled: bool,
    next_save_fault: Option<SaveFaultPoint>,
    next_save_interruption: Option<SaveFaultPoint>,
    next_migration_fault: Option<MigrationFaultPoint>,
    next_portable_fault: Option<PortableTransferFaultPoint>,
}

impl CampusProjectLibrary {
    pub fn open(
        root: impl AsRef<Path>,
        campus_target_id: impl Into<String>,
    ) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        let campus_target_id = campus_target_id.into();
        if campus_target_id.trim().is_empty() {
            return Err("Campus Project Library requires a Campus Target ID".into());
        }
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let index_path = root.join(LIBRARY_INDEX_FILE);
        let mut index = if index_path.exists() {
            serde_json::from_slice(&fs::read(&index_path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("Invalid campus project library: {error}"))?
        } else {
            LibraryIndex::default()
        };
        recover_interrupted_save(&root, &mut index)?;
        recover_interrupted_portable_import(&root, &mut index)?;
        let library = Self {
            root,
            campus_target_id,
            index,
            construction_enabled: false,
            next_save_fault: None,
            next_save_interruption: None,
            next_migration_fault: None,
            next_portable_fault: None,
        };
        for record in &library.index.projects {
            if record.campus_target_id != library.campus_target_id {
                return Err("Campus Project Library contains a record from another campus".into());
            }
            library.managed_path(record)?;
        }
        Ok(library)
    }

    pub fn open_for_construction(
        root: impl AsRef<Path>,
        campus_target_id: impl Into<String>,
        _capability: &V11ConstructionCapability,
    ) -> Result<Self, String> {
        let mut library = Self::open(root, campus_target_id)?;
        library.construction_enabled = true;
        Ok(library)
    }

    pub fn create_project(
        &mut self,
        scope: CampusScope,
        name: impl Into<String>,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        self.create_registered_project(scope, name.into(), actor, None)
    }

    pub fn create_development_canonical_project(
        &mut self,
        scope: CampusScope,
        name: impl Into<String>,
        actor: InstallationId,
    ) -> Result<Schema2Project, String> {
        self.create_registered_project(
            scope,
            name.into(),
            actor,
            Some(DEVELOPMENT_CANONICAL_ROLE.into()),
        )
    }

    fn create_registered_project(
        &mut self,
        scope: CampusScope,
        name: String,
        actor: InstallationId,
        internal_role: Option<String>,
    ) -> Result<Schema2Project, String> {
        if !self.construction_enabled {
            return Err(
                "Schema-2 project construction requires the internal development gate".into(),
            );
        }
        if scope.target_id() != self.campus_target_id {
            return Err("Project Campus Target does not match this Campus Project Library".into());
        }
        validate_project_name(&name)?;
        self.ensure_name_available(&name, None)?;
        let project = Schema2Project::new(scope, name, actor)?;
        let relative = format!("projects/{}/{PROJECT_FILE_NAME}", project.id().as_str());
        let record = record_from_project(&project, relative, internal_role);
        atomic_write_json(&self.root.join(record.managed_relative_path()), &project)?;
        self.index.projects.push(record);
        self.save_index()?;
        Ok(project)
    }

    pub fn record(&self, project_id: &ProjectId) -> Option<&CampusProjectLibraryRecord> {
        self.index
            .projects
            .iter()
            .find(|record| record.project_id == *project_id)
    }

    pub fn find_by_name(&self, name: &str) -> Result<&CampusProjectLibraryRecord, String> {
        self.index
            .projects
            .iter()
            .find(|record| record.name == name)
            .ok_or_else(|| format!("Project not found in campus library: {name}"))
    }

    pub fn development_canonical_record(&self) -> Option<&CampusProjectLibraryRecord> {
        self.index
            .projects
            .iter()
            .find(|record| record.internal_role.as_deref() == Some(DEVELOPMENT_CANONICAL_ROLE))
    }

    pub fn open_project(&self, project_id: &ProjectId) -> Result<Schema2Project, String> {
        let record = self
            .record(project_id)
            .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;
        let path = self.managed_path(record)?;
        let project = decode_schema2_project(&fs::read(path).map_err(|error| error.to_string())?)?;
        if project.id() != project_id
            || project.campus_scope().target_id() != record.campus_target_id
        {
            return Err("Managed project identity does not match its campus library record".into());
        }
        Ok(project)
    }

    pub fn save_project(&mut self, project: &Schema2Project) -> Result<(), String> {
        self.save_project_at(project).map(|_| ())
    }

    fn save_project_at(&mut self, project: &Schema2Project) -> Result<u64, String> {
        validate_schema2_project(project)?;
        if project.campus_scope().target_id() != self.campus_target_id {
            return Err("Project Campus Target does not match this Campus Project Library".into());
        }
        let record_position = self
            .index
            .projects
            .iter()
            .position(|record| record.project_id == *project.id())
            .ok_or_else(|| format!("Project not found: {}", project.id().as_str()))?;
        if self.index.projects[record_position].campus_target_id != self.campus_target_id {
            return Err("Project library record does not match its Campus Target scope".into());
        }
        let path = self.managed_path(&self.index.projects[record_position])?;
        let old_project_bytes = fs::read(&path).map_err(|error| error.to_string())?;
        decode_schema2_project(&old_project_bytes)
            .map_err(|error| format!("Previous confirmed project is invalid: {error}"))?;
        let completed_at_unix_ms = project
            .durability
            .last_confirmed_save_unix_ms
            .map_or_else(now_unix_ms, |previous| {
                now_unix_ms().max(previous.saturating_add(1))
            });
        let mut confirmed = project.clone();
        confirmed.durability.last_confirmed_save_unix_ms = Some(completed_at_unix_ms);
        let new_project_bytes =
            serde_json::to_vec_pretty(&confirmed).map_err(|error| error.to_string())?;
        let project_directory = path.parent().ok_or("Managed project path has no parent")?;
        let stage_path = project_directory.join(SAVE_STAGE_FILE_NAME);
        let previous_path = project_directory.join(PREVIOUS_PROJECT_FILE_NAME);
        let rollback_path = project_directory.join(SAVE_ROLLBACK_FILE_NAME);
        let old_previous_bytes = read_optional_file(&previous_path)?;
        let relative = self.index.projects[record_position]
            .managed_relative_path
            .clone();
        let internal_role = self.index.projects[record_position].internal_role.clone();
        let mut next_index = self.index.clone();
        next_index.projects[record_position] =
            record_from_project(&confirmed, relative, internal_role);
        let old_index_bytes =
            serde_json::to_vec_pretty(&self.index).map_err(|error| error.to_string())?;
        atomic_write_json(
            &rollback_path,
            &ProjectSaveRollback {
                project_bytes: old_project_bytes.clone(),
                library_index_bytes: old_index_bytes.clone(),
                previous_project_bytes: old_previous_bytes.clone(),
            },
        )?;

        self.fail_save_at(SaveFaultPoint::BeforeStageWrite)?;
        self.interrupt_save_at(SaveFaultPoint::BeforeStageWrite)?;
        write_and_sync(&stage_path, &new_project_bytes)?;
        if let Err(error) = self.fail_save_at(SaveFaultPoint::AfterStageWrite) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::AfterStageWrite)?;
        let staged = fs::read(&stage_path).map_err(|error| error.to_string())?;
        decode_schema2_project(&staged)
            .map_err(|error| format!("Staged project validation failed: {error}"))?;
        if let Err(error) = self.fail_save_at(SaveFaultPoint::AfterStageValidation) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::AfterStageValidation)?;
        if let Err(error) = self.fail_save_at(SaveFaultPoint::BeforePreviousConfirmedWrite) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::BeforePreviousConfirmedWrite)?;
        atomic_write_bytes(&previous_path, &old_project_bytes)?;
        if let Err(error) = self.fail_save_at(SaveFaultPoint::AfterPreviousConfirmedWrite) {
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::AfterPreviousConfirmedWrite)?;
        if let Err(error) = self.fail_save_at(SaveFaultPoint::BeforeProjectReplace) {
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::BeforeProjectReplace)?;
        if let Err(error) = atomic_write_bytes(&path, &new_project_bytes) {
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        if let Err(error) = self.fail_save_at(SaveFaultPoint::AfterProjectReplace) {
            let _ = atomic_write_bytes(&path, &old_project_bytes);
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::AfterProjectReplace)?;

        if let Err(error) = self.fail_save_at(SaveFaultPoint::BeforeIndexReplace) {
            let _ = atomic_write_bytes(&path, &old_project_bytes);
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::BeforeIndexReplace)?;
        if let Err(error) = atomic_write_json(&self.root.join(LIBRARY_INDEX_FILE), &next_index) {
            let _ = atomic_write_bytes(&path, &old_project_bytes);
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        if let Err(error) = self.fail_save_at(SaveFaultPoint::AfterIndexReplace) {
            let _ = atomic_write_bytes(&path, &old_project_bytes);
            let _ = atomic_write_bytes(&self.root.join(LIBRARY_INDEX_FILE), &old_index_bytes);
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        self.interrupt_save_at(SaveFaultPoint::AfterIndexReplace)?;
        if let Err(error) = fs::remove_file(&rollback_path) {
            let _ = atomic_write_bytes(&path, &old_project_bytes);
            let _ = atomic_write_bytes(&self.root.join(LIBRARY_INDEX_FILE), &old_index_bytes);
            restore_optional_file(&previous_path, old_previous_bytes.as_deref())?;
            remove_file_if_present(&stage_path);
            return Err(error.to_string());
        }
        self.index = next_index;
        remove_file_if_present(&stage_path);
        Ok(completed_at_unix_ms)
    }

    pub fn inject_next_save_failure(&mut self, point: SaveFaultPoint) {
        self.next_save_fault = Some(point);
    }

    pub fn inject_next_save_interruption(&mut self, point: SaveFaultPoint) {
        self.next_save_interruption = Some(point);
    }

    pub fn previous_confirmed_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Schema2Project, String> {
        let path = self.previous_confirmed_path(project_id)?;
        decode_schema2_project(&fs::read(path).map_err(|error| error.to_string())?)
    }

    pub fn recovery_path(&self, project_id: &ProjectId) -> Result<PathBuf, String> {
        Ok(self
            .project_directory(project_id)?
            .join(RECOVERY_PROJECT_FILE_NAME))
    }

    pub fn recovery_candidate(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<ProjectRecoveryCandidate>, String> {
        let Some(envelope) = self.read_recovery(project_id)? else {
            return Ok(None);
        };
        let confirmed = self.open_project(project_id)?;
        if envelope.project.id() != project_id
            || envelope.project.campus_scope().target_id() != self.campus_target_id
        {
            return Err("Project Recovery State is incoherent with the confirmed project".into());
        }
        let last_confirmed = confirmed
            .durability
            .last_confirmed_save_unix_ms
            .unwrap_or(confirmed.audit.updated_at_unix_ms);
        if envelope.captured_at_unix_ms <= last_confirmed {
            return Ok(None);
        }
        let mut operations = envelope
            .project
            .durability
            .undo
            .iter()
            .chain(envelope.project.durability.redo.iter())
            .collect::<Vec<_>>();
        operations.sort_by_key(|operation| operation.sequence);
        Ok(Some(ProjectRecoveryCandidate {
            captured_at_unix_ms: envelope.captured_at_unix_ms,
            project_revision: envelope.project.workflow().project_revision(),
            recent_operations: operations
                .into_iter()
                .rev()
                .take(5)
                .map(|operation| operation.description.clone())
                .collect(),
        }))
    }

    pub fn discard_recovery_candidate(&self, project_id: &ProjectId) -> Result<(), String> {
        let path = self.recovery_path(project_id)?;
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn rename_project(
        &mut self,
        project_id: &ProjectId,
        name: impl Into<String>,
        actor: InstallationId,
    ) -> Result<(), String> {
        let name = name.into();
        let mut project = self.open_project(project_id)?;
        self.ensure_name_available(&name, Some(project_id))?;
        project.rename(name, actor)?;
        self.save_project(&project)
    }

    fn ensure_name_available(
        &self,
        name: &str,
        except_project_id: Option<&ProjectId>,
    ) -> Result<(), String> {
        if self
            .index
            .projects
            .iter()
            .any(|record| record.name == name && except_project_id != Some(&record.project_id))
        {
            Err(format!(
                "Project name already exists in this campus: {name}"
            ))
        } else {
            Ok(())
        }
    }

    fn managed_path(&self, record: &CampusProjectLibraryRecord) -> Result<PathBuf, String> {
        let relative = Path::new(&record.managed_relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("Campus project library contains an unsafe managed path".into());
        }
        Ok(self.root.join(relative))
    }

    fn save_index(&self) -> Result<(), String> {
        atomic_write_json(&self.root.join(LIBRARY_INDEX_FILE), &self.index)
    }

    fn write_recovery(&self, project: &Schema2Project) -> Result<(), String> {
        validate_schema2_project(project)?;
        if project.campus_scope().target_id() != self.campus_target_id {
            return Err("Recovery project does not match this Campus Target".into());
        }
        atomic_write_json(
            &self.recovery_path(project.id())?,
            &ProjectRecoveryEnvelope {
                captured_at_unix_ms: project
                    .durability
                    .last_confirmed_save_unix_ms
                    .map_or_else(now_unix_ms, |previous| {
                        now_unix_ms().max(previous.saturating_add(1))
                    }),
                project: project.clone(),
            },
        )
    }

    fn read_recovery(
        &self,
        project_id: &ProjectId,
    ) -> Result<Option<ProjectRecoveryEnvelope>, String> {
        let path = self.recovery_path(project_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let envelope: ProjectRecoveryEnvelope =
            serde_json::from_slice(&fs::read(path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("Invalid Project Recovery State: {error}"))?;
        validate_schema2_project(&envelope.project)
            .map_err(|error| format!("Invalid Project Recovery State: {error}"))?;
        Ok(Some(envelope))
    }

    fn previous_confirmed_path(&self, project_id: &ProjectId) -> Result<PathBuf, String> {
        Ok(self
            .project_directory(project_id)?
            .join(PREVIOUS_PROJECT_FILE_NAME))
    }

    fn project_directory(&self, project_id: &ProjectId) -> Result<PathBuf, String> {
        let record = self
            .record(project_id)
            .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;
        self.managed_path(record)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Managed project path has no parent".into())
    }

    fn fail_save_at(&mut self, point: SaveFaultPoint) -> Result<(), String> {
        if self.next_save_fault == Some(point) {
            self.next_save_fault = None;
            Err(format!("injected save failure at {point:?}"))
        } else {
            Ok(())
        }
    }

    fn interrupt_save_at(&mut self, point: SaveFaultPoint) -> Result<(), String> {
        if self.next_save_interruption == Some(point) {
            self.next_save_interruption = None;
            Err(format!("injected process interruption at {point:?}"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
pub struct Schema2ProjectSession {
    active: Option<Schema2Project>,
    save_status: ProjectSaveStatus,
    maintenance_warning: Option<String>,
    dirty: bool,
}

impl Default for Schema2ProjectSession {
    fn default() -> Self {
        Self {
            active: None,
            save_status: ProjectSaveStatus::Saved {
                completed_at_unix_ms: 0,
            },
            maintenance_warning: None,
            dirty: false,
        }
    }
}

impl Schema2ProjectSession {
    pub fn active(&self) -> Option<&Schema2Project> {
        self.active.as_ref()
    }

    pub fn open_project(
        &mut self,
        library: &CampusProjectLibrary,
        project_id: &ProjectId,
    ) -> Result<(), String> {
        let candidate = library.open_project(project_id)?;
        let saved_at = candidate
            .durability
            .last_confirmed_save_unix_ms
            .unwrap_or(candidate.audit.updated_at_unix_ms);
        self.active = Some(candidate);
        self.save_status = ProjectSaveStatus::Saved {
            completed_at_unix_ms: saved_at,
        };
        self.maintenance_warning = None;
        self.dirty = false;
        Ok(())
    }

    pub fn save_status(&self) -> &ProjectSaveStatus {
        &self.save_status
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn maintenance_warning(&self) -> Option<&str> {
        self.maintenance_warning.as_deref()
    }

    pub fn history(&self) -> Vec<ProjectHistorySummary> {
        let Some(project) = &self.active else {
            return Vec::new();
        };
        let mut operations = project
            .durability
            .undo
            .iter()
            .chain(project.durability.redo.iter())
            .collect::<Vec<_>>();
        operations.sort_by_key(|operation| operation.sequence);
        operations
            .into_iter()
            .map(|operation| ProjectHistorySummary {
                sequence: operation.sequence,
                description: operation.description.clone(),
                completed_at_unix_ms: operation.completed_at_unix_ms,
            })
            .collect()
    }

    pub fn can_undo(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|project| !project.durability.undo.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|project| !project.durability.redo.is_empty())
    }

    pub fn apply_semantic_operation<T>(
        &mut self,
        library: &mut CampusProjectLibrary,
        description: impl Into<String>,
        mutation: impl FnOnce(&mut Schema2Project) -> Result<T, String>,
    ) -> Result<T, String> {
        let previous = self.active.clone().ok_or("No schema-2 project is open")?;
        let before = project_snapshot(&previous)?;
        let mut next = previous.clone();
        let output = mutation(&mut next)?;
        let after = project_snapshot(&next)?;
        if before == after {
            return Ok(output);
        }
        let sequence = next.durability.next_history_sequence;
        next.durability.next_history_sequence = sequence.saturating_add(1);
        next.durability.redo.clear();
        next.durability.undo.push(ProjectHistoryOperation {
            sequence,
            description: description.into(),
            completed_at_unix_ms: now_unix_ms(),
            before,
            after,
        });
        trim_history(&mut next.durability);
        self.active = Some(next);
        self.dirty = true;
        self.request_save(library)?;
        Ok(output)
    }

    pub fn request_save(&mut self, library: &mut CampusProjectLibrary) -> Result<(), String> {
        let project = self
            .active
            .as_ref()
            .ok_or("No schema-2 project is open")?
            .clone();
        let project_id = project.id().clone();
        self.save_status = ProjectSaveStatus::Saving;
        if let Err(error) = library.write_recovery(&project) {
            self.save_status = ProjectSaveStatus::Failed {
                reason: error.clone(),
            };
            self.dirty = true;
            return Err(error);
        }
        match library.save_project_at(&project) {
            Ok(completed_at_unix_ms) => {
                if let Some(active) = self.active.as_mut() {
                    active.durability.last_confirmed_save_unix_ms = Some(completed_at_unix_ms);
                }
                self.save_status = ProjectSaveStatus::Saved {
                    completed_at_unix_ms,
                };
                self.dirty = false;
                self.maintenance_warning = library
                    .discard_recovery_candidate(&project_id)
                    .err()
                    .map(|error| format!("Saved, but recovery cleanup must be retried: {error}"));
                Ok(())
            }
            Err(error) => {
                self.save_status = ProjectSaveStatus::Failed {
                    reason: error.clone(),
                };
                self.dirty = true;
                Err(error)
            }
        }
    }

    pub fn retry_save(&mut self, library: &mut CampusProjectLibrary) -> Result<(), String> {
        self.request_save(library)
    }

    pub fn undo(&mut self, library: &mut CampusProjectLibrary) -> Result<(), String> {
        let current = self.active.clone().ok_or("No schema-2 project is open")?;
        let mut durability = current.durability.clone();
        let operation = durability
            .undo
            .pop()
            .ok_or("No Project History Operation is available to undo")?;
        let mut previous = project_from_snapshot(operation.before.clone())?;
        durability.redo.push(operation);
        previous.durability = durability;
        self.active = Some(previous);
        self.dirty = true;
        self.request_save(library)
    }

    pub fn redo(&mut self, library: &mut CampusProjectLibrary) -> Result<(), String> {
        let current = self.active.clone().ok_or("No schema-2 project is open")?;
        let mut durability = current.durability.clone();
        let operation = durability
            .redo
            .pop()
            .ok_or("No Project History Operation is available to redo")?;
        let mut next = project_from_snapshot(operation.after.clone())?;
        durability.undo.push(operation);
        next.durability = durability;
        self.active = Some(next);
        self.dirty = true;
        self.request_save(library)
    }

    pub fn prepare_context_change(
        &mut self,
        library: &mut CampusProjectLibrary,
    ) -> Result<(), String> {
        if self.dirty {
            self.request_save(library)?;
        }
        Ok(())
    }

    pub fn switch_project(
        &mut self,
        library: &mut CampusProjectLibrary,
        project_id: &ProjectId,
    ) -> Result<(), String> {
        self.prepare_context_change(library)?;
        let candidate = library.open_project(project_id)?;
        let saved_at = candidate
            .durability
            .last_confirmed_save_unix_ms
            .unwrap_or(candidate.audit.updated_at_unix_ms);
        self.active = Some(candidate);
        self.save_status = ProjectSaveStatus::Saved {
            completed_at_unix_ms: saved_at,
        };
        self.maintenance_warning = None;
        self.dirty = false;
        Ok(())
    }

    pub fn gate_context_change<T>(
        &mut self,
        library: &mut CampusProjectLibrary,
        change: impl FnOnce(&mut CampusProjectLibrary) -> Result<T, String>,
    ) -> Result<T, String> {
        self.prepare_context_change(library)?;
        change(library)
    }

    pub fn available_recovery(
        &self,
        library: &CampusProjectLibrary,
    ) -> Result<Option<ProjectRecoveryCandidate>, String> {
        let project = self.active.as_ref().ok_or("No schema-2 project is open")?;
        library.recovery_candidate(project.id())
    }

    pub fn accept_recovery(&mut self, library: &CampusProjectLibrary) -> Result<(), String> {
        let project_id = self
            .active
            .as_ref()
            .ok_or("No schema-2 project is open")?
            .id()
            .clone();
        library
            .recovery_candidate(&project_id)?
            .ok_or("No Project Recovery State is available")?;
        let recovered = library
            .read_recovery(&project_id)?
            .ok_or("No Project Recovery State is available")?
            .project;
        self.active = Some(recovered);
        self.dirty = true;
        Ok(())
    }

    pub fn discard_recovery(&mut self, library: &CampusProjectLibrary) -> Result<(), String> {
        let project_id = self
            .active
            .as_ref()
            .ok_or("No schema-2 project is open")?
            .id()
            .clone();
        library.discard_recovery_candidate(&project_id)
    }
}

pub fn decode_schema2_project(bytes: &[u8]) -> Result<Schema2Project, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let schema = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or("Project is missing a numeric schemaVersion")?;
    if schema > u64::from(SCHEMA_2_VERSION) {
        return Err(format!(
            "Project schema {schema} is newer than supported schema {SCHEMA_2_VERSION}"
        ));
    }
    if schema != u64::from(SCHEMA_2_VERSION) {
        return Err(format!(
            "Project schema {schema} is not schema {SCHEMA_2_VERSION}; migration is required"
        ));
    }
    let project: Schema2Project =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_schema2_project(&project)?;
    Ok(project)
}

pub fn v11_construction_enabled(development_build: bool, environment_value: Option<&str>) -> bool {
    development_build && environment_value == Some("1")
}

fn validate_schema2_project(project: &Schema2Project) -> Result<(), String> {
    if project.schema_version != SCHEMA_2_VERSION {
        return Err(format!(
            "Only schema {SCHEMA_2_VERSION} projects may be written by the V1.1 kernel"
        ));
    }
    uuid::Uuid::parse_str(project.project_id.as_str())
        .map_err(|_| "Project ID must be a UUID".to_string())?;
    validate_project_name(&project.name)?;
    let expected = V11CompatibilityProfile::minecraft_java_26_1_2();
    let profile = &project.compatibility_profile;
    if profile.profile_id != expected.profile_id
        || profile.edition != expected.edition
        || profile.minecraft_version != expected.minecraft_version
        || profile.preview_profile_id != expected.preview_profile_id
        || profile.export_profile_id != expected.export_profile_id
        || profile.block_catalog_id != expected.block_catalog_id
    {
        return Err("V1.1 requires the Minecraft Java Edition 26.1.2 compatibility profile".into());
    }
    if let Some(checkpoint) = &project.foundation.acquisition_checkpoint {
        checkpoint.validate()?;
        let boundary =
            project.foundation.boundary.as_ref().ok_or(
                "A Foundation acquisition checkpoint requires confirmed boundary evidence",
            )?;
        if checkpoint.bundle != boundary.manifest.bundle
            || checkpoint.boundary_revision != boundary.manifest.result_sha256
        {
            return Err(
                "Foundation acquisition checkpoint does not match confirmed boundary evidence"
                    .into(),
            );
        }
    }
    validate_project_history(project)?;
    Ok(())
}

fn validate_project_history(project: &Schema2Project) -> Result<(), String> {
    let durability = &project.durability;
    if durability.undo.len() + durability.redo.len() > PROJECT_HISTORY_LIMIT {
        return Err(format!(
            "Project history exceeds the {PROJECT_HISTORY_LIMIT}-operation limit"
        ));
    }
    let mut operations = durability
        .undo
        .iter()
        .chain(durability.redo.iter())
        .collect::<Vec<_>>();
    operations.sort_by_key(|operation| operation.sequence);
    for pair in operations.windows(2) {
        if pair[0].sequence == pair[1].sequence {
            return Err("Project history contains duplicate operation sequences".into());
        }
        if pair[0].after != pair[1].before {
            return Err(
                "Project history snapshots do not form one coherent operation chain".into(),
            );
        }
    }
    for operation in &operations {
        for snapshot in [&operation.before, &operation.after] {
            let snapshot_project: Schema2Project = serde_json::from_value(snapshot.clone())
                .map_err(|error| {
                    format!("Project history contains an invalid snapshot: {error}")
                })?;
            if snapshot_project.id() != project.id()
                || snapshot_project.campus_scope().target_id() != project.campus_scope().target_id()
            {
                return Err("Project history snapshot identity is incoherent".into());
            }
        }
    }
    let current = project_snapshot(project)?;
    if let Some(operation) = durability.undo.iter().max_by_key(|entry| entry.sequence) {
        if operation.after != current {
            return Err("Project history undo branch does not reach the working state".into());
        }
    }
    if let Some(operation) = durability.redo.iter().min_by_key(|entry| entry.sequence) {
        if operation.before != current {
            return Err("Project history redo branch does not begin at the working state".into());
        }
    }
    Ok(())
}

fn validate_project_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("Project name cannot be empty".into())
    } else {
        Ok(())
    }
}

fn record_from_project(
    project: &Schema2Project,
    managed_relative_path: String,
    internal_role: Option<String>,
) -> CampusProjectLibraryRecord {
    CampusProjectLibraryRecord {
        project_id: project.id().clone(),
        campus_target_id: project.campus_scope().target_id().into(),
        name: project.name().into(),
        managed_relative_path,
        created_at_unix_ms: project.audit.created_at_unix_ms,
        updated_at_unix_ms: project.audit.updated_at_unix_ms,
        latest_successful_save_unix_ms: project
            .durability
            .last_confirmed_save_unix_ms
            .unwrap_or(project.audit.updated_at_unix_ms),
        compatibility_profile_id: project.compatibility_profile.profile_id().into(),
        internal_role,
    }
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Managed path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    atomic_write_bytes(path, &bytes)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Managed path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|error| error.to_string())
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Managed path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn remove_file_if_present(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn restore_optional_file(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
    match bytes {
        Some(bytes) => atomic_write_bytes(path, bytes),
        None if path.exists() => fs::remove_file(path).map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

fn recover_interrupted_save(root: &Path, index: &mut LibraryIndex) -> Result<(), String> {
    for record in index.projects.clone() {
        let relative = Path::new(&record.managed_relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("Campus project library contains an unsafe managed path".into());
        }
        let project_path = root.join(relative);
        let project_directory = project_path
            .parent()
            .ok_or("Managed project path has no parent")?;
        let rollback_path = project_directory.join(SAVE_ROLLBACK_FILE_NAME);
        if !rollback_path.exists() {
            continue;
        }
        let rollback: ProjectSaveRollback =
            serde_json::from_slice(&fs::read(&rollback_path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("Invalid save rollback journal: {error}"))?;
        atomic_write_bytes(&project_path, &rollback.project_bytes)?;
        atomic_write_bytes(
            &root.join(LIBRARY_INDEX_FILE),
            &rollback.library_index_bytes,
        )?;
        restore_optional_file(
            &project_directory.join(PREVIOUS_PROJECT_FILE_NAME),
            rollback.previous_project_bytes.as_deref(),
        )?;
        remove_file_if_present(&project_directory.join(SAVE_STAGE_FILE_NAME));
        fs::remove_file(&rollback_path).map_err(|error| error.to_string())?;
        *index = serde_json::from_slice(&rollback.library_index_bytes)
            .map_err(|error| format!("Invalid rolled-back campus project library: {error}"))?;
        break;
    }
    Ok(())
}

fn project_snapshot(project: &Schema2Project) -> Result<Value, String> {
    let mut value = serde_json::to_value(project).map_err(|error| error.to_string())?;
    value
        .as_object_mut()
        .ok_or("Schema-2 project snapshot must be a JSON object")?
        .remove("durability");
    Ok(value)
}

fn project_from_snapshot(snapshot: Value) -> Result<Schema2Project, String> {
    let project: Schema2Project =
        serde_json::from_value(snapshot).map_err(|error| error.to_string())?;
    validate_schema2_project(&project)?;
    Ok(project)
}

fn trim_history(durability: &mut ProjectDurabilityState) {
    while durability.undo.len() + durability.redo.len() > PROJECT_HISTORY_LIMIT {
        let oldest_undo = durability.undo.first().map(|operation| operation.sequence);
        let oldest_redo = durability.redo.first().map(|operation| operation.sequence);
        match (oldest_undo, oldest_redo) {
            (Some(undo), Some(redo)) if redo < undo => {
                durability.redo.remove(0);
            }
            (Some(_), _) => {
                durability.undo.remove(0);
            }
            (None, Some(_)) => {
                durability.redo.remove(0);
            }
            (None, None) => break,
        }
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
