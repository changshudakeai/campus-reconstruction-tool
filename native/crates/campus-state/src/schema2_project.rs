use crate::{
    foundation_refresh::{
        compare_foundation_evidence, coverage_digest_for, geometry_digest,
        observation_dependency_snapshot,
    },
    validate_boundary_geometry, BoundaryCandidate, BoundaryCandidateAssessment,
    BoundaryCandidateValidity, BoundaryDiscoverySnapshot, BoundaryEvidenceDesk,
    BuildingBoundaryDecision, BuildingEntityDecision, BuildingEntityReviewLedger,
    BuildingGenerationBasis, BuildingNameEvidence, BuildingNameResolution,
    CandidateReviewDisposition, FoundationAcquisitionCheckpoint, FoundationBatchDecision,
    FoundationBatchReview, FoundationCandidateDecision, FoundationCategory,
    FoundationCategoryProgress, FoundationReviewAction, FoundationReviewConflict,
    FoundationReviewOperation, FoundationReviewQueueProjection, FoundationReviewState,
    FoundationSourceRefreshDifference, KnownFeatureGap, KnownFeatureGapHistoryAction,
    KnownFeatureGapLocation, KnownFeatureGapStatus, ProviderOutcomeStatus, ResultManifest,
    ReviewConflictResolution, ReviewDependencyBasis, ReviewSubjectDependencyBasis,
    ReviewedBuildingEntity, SourceGeometry, SourceObservation,
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

    pub fn request_controlled_release(environment_value: Option<&str>) -> Result<Self, String> {
        if environment_value == Some("1") {
            Ok(Self(()))
        } else {
            Err("Schema-2 controlled release construction requires an explicit runtime gate".into())
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
    #[serde(default)]
    pub confirmed_geometry: Option<SourceGeometry>,
    #[serde(default)]
    pub assessments: BTreeMap<String, BoundaryCandidateAssessment>,
}

impl PinnedBoundaryEvidence {
    pub fn confirmed_geometry(&self) -> Option<&SourceGeometry> {
        self.confirmed_geometry.as_ref().or_else(|| {
            self.candidates
                .iter()
                .find(|candidate| candidate.id == self.selected_candidate_id)
                .map(|candidate| &candidate.geometry)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinnedAcquisitionEvidence {
    pub manifest: ResultManifest,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionRefreshRecord {
    pub previous_manifest: ResultManifest,
    pub incoming_manifest: ResultManifest,
    pub added_observation_ids: Vec<String>,
    #[serde(default, alias = "retiredObservationIds")]
    pub withdrawn_observation_ids: Vec<String>,
    pub composite_snapshot_identity: String,
    #[serde(default)]
    pub difference: Option<FoundationSourceRefreshDifference>,
    #[serde(default)]
    pub retained_previous_observations: Vec<SourceObservation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinnedFoundationEvidence<'a> {
    pub boundary: &'a PinnedBoundaryEvidence,
    pub acquisition: &'a PinnedAcquisitionEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "decision")]
pub enum FoundationReviewDisposition {
    SelectedEvidence {
        evidence_ids: Vec<String>,
    },
    ReviewedBuildingEntities {
        entity_ids: Vec<String>,
        #[serde(default)]
        known_gaps: Vec<KnownBuildingEntityGap>,
    },
    CompleteEmpty,
    KnownGap {
        reasons: Vec<String>,
    },
    ReviewedQueue {
        accepted_evidence_ids: Vec<String>,
        rejected_evidence_ids: Vec<String>,
        deferred_evidence_ids: Vec<String>,
        acknowledged_gap_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnownBuildingEntityGap {
    pub entity_id: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewBasis {
    pub boundary_result_sha256: String,
    pub selected_boundary_id: String,
    pub acquisition_result_sha256: String,
    #[serde(default)]
    pub acquisition_snapshot_identity: String,
    pub classification_rules: String,
    pub conflation_rules: String,
    pub derivation_rules: String,
    #[serde(default)]
    pub building_review_sequence: u64,
    #[serde(default)]
    pub dependencies: ReviewDependencyBasis,
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
    #[serde(default)]
    pub operation_sequence: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReviewLedger {
    entries: Vec<FoundationReviewEntry>,
    #[serde(default)]
    operations: Vec<FoundationReviewOperation>,
}

impl FoundationReviewLedger {
    pub fn disposition(
        &self,
        category: FoundationCategory,
    ) -> Option<&FoundationReviewDisposition> {
        let entry = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.category == category)?;
        let invalidated = self.operations.iter().any(|operation| {
            operation.category == category
                && operation.basis == entry.basis
                && operation.sequence > entry.operation_sequence
                && !matches!(operation.action, FoundationReviewAction::CategoryCompleted)
        });
        (!invalidated).then_some(&entry.after)
    }

    pub fn entries(&self) -> &[FoundationReviewEntry] {
        &self.entries
    }

    pub fn operations(&self) -> &[FoundationReviewOperation] {
        &self.operations
    }

    pub fn current_sequence(&self) -> u64 {
        self.operations
            .last()
            .map(|operation| operation.sequence)
            .unwrap_or(0)
    }

    fn disposition_for_basis(
        &self,
        category: FoundationCategory,
        basis: &FoundationReviewBasis,
    ) -> Option<&FoundationReviewDisposition> {
        let entry = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry.category == category && entry.basis == *basis)?;
        let invalidated = self.operations.iter().any(|operation| {
            operation.category == category
                && operation.basis == *basis
                && operation.sequence > entry.operation_sequence
                && !matches!(operation.action, FoundationReviewAction::CategoryCompleted)
        });
        (!invalidated).then_some(&entry.after)
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
    #[serde(default)]
    pub dependency_basis: ReviewDependencyBasis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFoundationOutput {
    pub project_revision: u64,
    pub schematic_sha256: String,
    pub schematic_bytes: u64,
    pub manifest_file_name: String,
    #[serde(default)]
    pub building_provenance: Vec<ReviewedBuildingEntity>,
    #[serde(default)]
    pub dependency_basis: ReviewDependencyBasis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingFoundationAcquisitionStart {
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FoundationAcquisitionCheckpointPurpose {
    #[default]
    Initial,
    ExplicitRefresh,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FoundationTracerState {
    #[serde(default)]
    boundary_review: Option<BoundaryEvidenceDesk>,
    boundary: Option<PinnedBoundaryEvidence>,
    #[serde(default)]
    pending_acquisition_start: Option<PendingFoundationAcquisitionStart>,
    #[serde(default)]
    acquisition_checkpoint: Option<FoundationAcquisitionCheckpoint>,
    #[serde(default)]
    acquisition_checkpoint_purpose: FoundationAcquisitionCheckpointPurpose,
    acquisition: Option<PinnedAcquisitionEvidence>,
    #[serde(default)]
    acquisition_snapshot_identity: String,
    #[serde(default)]
    acquisition_refresh_history: Vec<AcquisitionRefreshRecord>,
    #[serde(default)]
    coarse_raster_runs: Vec<crate::CoarseRasterSupplementRun>,
    #[serde(default)]
    building_review: BuildingEntityReviewLedger,
    review_ledger: FoundationReviewLedger,
    generation_settings: FoundationGenerationSettings,
    generated: Option<GeneratedFoundationOutput>,
    exported: Option<ExportedFoundationOutput>,
    #[serde(default)]
    stale_generated: Vec<GeneratedFoundationOutput>,
    #[serde(default)]
    stale_exported: Vec<ExportedFoundationOutput>,
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

    pub fn boundary_review(&self) -> Option<&BoundaryEvidenceDesk> {
        self.foundation.boundary_review.as_ref()
    }

    pub fn begin_boundary_review(
        &mut self,
        snapshot: BoundaryDiscoverySnapshot,
        actor: InstallationId,
    ) -> Result<(), String> {
        let desk = BoundaryEvidenceDesk::new(snapshot)?;
        self.mark_updated(actor)?;
        self.foundation.boundary_review = Some(desk);
        Ok(())
    }

    pub fn begin_unavailable_boundary_review(
        &mut self,
        explanation: impl Into<String>,
        suggested_action: impl Into<String>,
        actor: InstallationId,
    ) -> Result<(), String> {
        let desk = BoundaryEvidenceDesk::unavailable(explanation, suggested_action);
        self.mark_updated(actor)?;
        self.foundation.boundary_review = Some(desk);
        Ok(())
    }

    pub fn return_to_campus_target_from_boundary_review(
        &mut self,
        actor: InstallationId,
    ) -> Result<(), String> {
        if self.foundation.boundary_review.is_none() {
            return Ok(());
        }
        self.mark_updated(actor)?;
        self.foundation.boundary_review = None;
        Ok(())
    }

    pub fn edit_boundary_review<T>(
        &mut self,
        actor: InstallationId,
        edit: impl FnOnce(&mut BoundaryEvidenceDesk) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut desk = self
            .foundation
            .boundary_review
            .clone()
            .ok_or("Load automatic Campus Boundary evidence before reviewing it")?;
        let output = edit(&mut desk)?;
        self.mark_updated(actor)?;
        self.foundation.boundary_review = Some(desk);
        Ok(output)
    }

    pub fn confirm_boundary_review(&mut self, actor: InstallationId) -> Result<(), String> {
        let evidence = self
            .foundation
            .boundary_review
            .as_ref()
            .ok_or("Load automatic Campus Boundary evidence before confirming it")?
            .to_pinned_evidence()?;
        self.confirm_boundary(evidence, actor)
    }

    pub fn acquisition_checkpoint(&self) -> Option<&FoundationAcquisitionCheckpoint> {
        self.foundation.acquisition_checkpoint.as_ref()
    }

    pub fn acquisition_checkpoint_purpose(&self) -> FoundationAcquisitionCheckpointPurpose {
        self.foundation.acquisition_checkpoint_purpose
    }

    pub fn pending_acquisition_start(&self) -> Option<&PendingFoundationAcquisitionStart> {
        self.foundation.pending_acquisition_start.as_ref()
    }

    pub fn confirm_boundary_and_queue_acquisition(
        &mut self,
        evidence: PinnedBoundaryEvidence,
        idempotency_key: impl Into<String>,
        actor: InstallationId,
    ) -> Result<(), String> {
        let idempotency_key = idempotency_key.into();
        if idempotency_key.trim().is_empty() {
            return Err("Foundation acquisition requires a stable idempotency key".into());
        }
        self.confirm_boundary(evidence, actor)?;
        self.foundation.pending_acquisition_start =
            Some(PendingFoundationAcquisitionStart { idempotency_key });
        Ok(())
    }

    pub fn foundation_review(&self) -> &FoundationReviewLedger {
        &self.foundation.review_ledger
    }

    pub fn foundation_review_queue(
        &self,
        category: FoundationCategory,
    ) -> Result<FoundationReviewQueueProjection, String> {
        let acquisition = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before opening the Foundation review queue")?;
        let basis = self.review_basis()?;
        let mut dispositions = crate::foundation_review::candidate_dispositions(
            category,
            &basis,
            &acquisition.observations,
            &self.foundation.review_ledger.operations,
        );
        if category == FoundationCategory::Building && !self.foundation.building_review.is_empty() {
            dispositions.values_mut().for_each(|disposition| {
                *disposition = CandidateReviewDisposition::Pending;
            });
            for entity in self.foundation.building_review.entities() {
                if entity.boundary_decision == crate::BuildingBoundaryDecision::Exclude {
                    for evidence_id in &entity.evidence_ids {
                        if let Some(disposition) = dispositions.get_mut(evidence_id) {
                            *disposition = CandidateReviewDisposition::Rejected;
                        }
                    }
                    continue;
                }
                if self
                    .foundation
                    .building_review
                    .reviewed_entities_by_ids(
                        &acquisition.observations,
                        self.building_generation_basis(),
                        std::slice::from_ref(&entity.id),
                    )
                    .is_ok()
                {
                    for evidence_id in &entity.evidence_ids {
                        if let Some(disposition) = dispositions.get_mut(evidence_id) {
                            *disposition = if evidence_id == &entity.primary_observation_id {
                                CandidateReviewDisposition::Accepted
                            } else {
                                CandidateReviewDisposition::SupportingEvidence {
                                    primary_subject_id: entity.primary_observation_id.clone(),
                                }
                            };
                        }
                    }
                } else if entity.name_resolution == BuildingNameResolution::Unnamed {
                    let gap_id = building_entity_name_gap_id(&entity.id);
                    if foundation_gap_acknowledged(
                        FoundationCategory::Building,
                        &gap_id,
                        &self.foundation.review_ledger.operations,
                    ) {
                        for evidence_id in &entity.evidence_ids {
                            if let Some(disposition) = dispositions.get_mut(evidence_id) {
                                *disposition = CandidateReviewDisposition::Deferred {
                                    structured_reason:
                                        "No exclusive building-level name evidence was available"
                                            .into(),
                                    acknowledged_gap_id: gap_id.clone(),
                                };
                            }
                        }
                    }
                }
            }
        }
        let items = acquisition
            .observations
            .iter()
            .filter(|observation| observation.category == category)
            .map(|observation| {
                crate::foundation_review::project_queue_item(
                    observation,
                    dispositions
                        .get(&observation.id)
                        .cloned()
                        .unwrap_or(CandidateReviewDisposition::Pending),
                )
            })
            .collect::<Vec<_>>();
        let provider_outcomes = acquisition
            .manifest
            .coverage_report
            .outcomes
            .iter()
            .filter(|outcome| outcome.category == category)
            .cloned()
            .collect::<Vec<_>>();
        let mut known_gaps = crate::foundation_review::known_gaps_for_category(
            category,
            &provider_outcomes,
            &self.foundation.review_ledger.operations,
        );
        for historical_outcomes in
            self.foundation
                .acquisition_refresh_history
                .iter()
                .map(|record| {
                    record
                        .previous_manifest
                        .coverage_report
                        .outcomes
                        .iter()
                        .filter(|outcome| outcome.category == category)
                        .cloned()
                        .collect::<Vec<_>>()
                })
        {
            for historical_gap in crate::foundation_review::known_gaps_for_category(
                category,
                &historical_outcomes,
                &self.foundation.review_ledger.operations,
            ) {
                if !known_gaps
                    .iter()
                    .any(|existing| existing.id == historical_gap.id)
                {
                    known_gaps.push(historical_gap);
                }
            }
        }
        if category == FoundationCategory::Building && !self.foundation.building_review.is_empty() {
            for entity in self
                .foundation
                .building_review
                .entities()
                .iter()
                .filter(|entity| entity.name_resolution == BuildingNameResolution::Unnamed)
            {
                let gap_id = building_entity_name_gap_id(&entity.id);
                let (status, history) = crate::foundation_review::known_gap_history(
                    category,
                    &gap_id,
                    &self.foundation.review_ledger.operations,
                );
                let acknowledged = status == KnownFeatureGapStatus::Acknowledged;
                let geometry = acquisition
                    .observations
                    .iter()
                    .find(|observation| observation.id == entity.primary_observation_id)
                    .map(|observation| observation.review_geometry_proposal.clone());
                known_gaps.push(KnownFeatureGap {
                    id: gap_id,
                    category,
                    location: KnownFeatureGapLocation {
                        tile_id: format!("building-entity:{}", entity.id),
                        geometry,
                    },
                    attempted_evidence: vec![
                        "No exclusive building-level name evidence was available".into(),
                    ],
                    generation_impact:
                        "The unnamed Building Entity is omitted until name evidence is resolved"
                            .into(),
                    provider: "building-entity-review".into(),
                    tile_id: format!("building-entity:{}", entity.id),
                    acknowledged,
                    status,
                    history,
                });
            }
        }
        let (mut conflicts, resolved_conflict_ids) = crate::foundation_review::review_conflicts(
            category,
            &basis,
            &self.foundation.review_ledger.operations,
        );
        if category != FoundationCategory::Building || self.foundation.building_review.is_empty() {
            for conflict in
                crate::foundation_review::suggested_conflicts(category, &acquisition.observations)
            {
                if !conflicts.iter().any(|existing| existing.id == conflict.id) {
                    conflicts.push(conflict);
                }
            }
        }
        let disposed = items
            .iter()
            .filter(|item| item.disposition.is_terminal())
            .count();
        let pending = items.len().saturating_sub(disposed);
        let unresolved_conflicts = conflicts
            .iter()
            .filter(|conflict| !resolved_conflict_ids.contains(&conflict.id))
            .count();
        let unacknowledged_gaps = known_gaps
            .iter()
            .filter(|gap| gap.status == KnownFeatureGapStatus::Open)
            .count();
        let mut completion_blockers = Vec::new();
        if pending > 0 {
            completion_blockers.push(format!(
                "{pending} pending candidate(s) require a disposition"
            ));
        }
        if unresolved_conflicts > 0 {
            completion_blockers.push(format!(
                "{unresolved_conflicts} unresolved conflict(s) require a review decision"
            ));
        }
        if unacknowledged_gaps > 0 {
            completion_blockers.push(format!(
                "{unacknowledged_gaps} Known Feature Gap(s) require acknowledgement"
            ));
        }
        let complete = self
            .foundation
            .review_ledger
            .disposition_for_basis(category, &basis)
            .is_some();
        Ok(FoundationReviewQueueProjection {
            category,
            basis,
            ledger_sequence: self.foundation.review_ledger.current_sequence(),
            items,
            provider_outcomes,
            known_gaps,
            conflicts,
            resolved_conflict_ids: resolved_conflict_ids.into_iter().collect(),
            progress: FoundationCategoryProgress {
                total: dispositions.len(),
                disposed,
                pending,
                unresolved_conflicts,
                unacknowledged_gaps,
                complete,
                completion_blockers,
            },
        })
    }

    pub fn review_foundation_candidate(
        &mut self,
        category: FoundationCategory,
        subject_id: &str,
        decision: FoundationCandidateDecision,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(category)?;
        if category == FoundationCategory::Building && !self.foundation.building_review.is_empty() {
            return self.review_building_queue_subject(subject_id, decision, actor);
        }
        if !queue.items.iter().any(|item| item.subject_id == subject_id) {
            return Err("The Foundation review subject is not pinned in this category".into());
        }
        match &decision {
            FoundationCandidateDecision::Reject { reason }
                if !reason.is_empty() && reason.trim().is_empty() =>
            {
                return Err("A rejection reason cannot contain only whitespace".into());
            }
            FoundationCandidateDecision::SupportingEvidence { primary_subject_id } => {
                if primary_subject_id == subject_id
                    || !queue
                        .items
                        .iter()
                        .any(|item| item.subject_id == *primary_subject_id)
                {
                    return Err(
                        "Supporting evidence requires a different pinned primary subject".into(),
                    );
                }
            }
            FoundationCandidateDecision::Defer {
                structured_reason,
                acknowledged_gap_id,
            } if structured_reason.trim().is_empty()
                || !queue
                    .known_gaps
                    .iter()
                    .any(|gap| gap.id == *acknowledged_gap_id && gap.acknowledged) =>
            {
                return Err(
                    "A Deferred Source Observation requires a structured reason and linked acknowledged Known Feature Gap"
                        .into(),
                );
            }
            _ => {}
        }
        self.append_foundation_review_operation(
            category,
            vec![subject_id.to_string()],
            queue.basis,
            FoundationReviewAction::Candidate {
                subject_id: subject_id.to_string(),
                decision,
            },
            actor,
        )
    }

    pub fn revoke_foundation_candidate_review(
        &mut self,
        category: FoundationCategory,
        subject_id: &str,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(category)?;
        let item = queue
            .items
            .iter()
            .find(|item| item.subject_id == subject_id)
            .ok_or("The Foundation review subject is not pinned in this category")?;
        if category == FoundationCategory::Building && !self.foundation.building_review.is_empty() {
            return self.revoke_building_entity_decision_for_subject(subject_id, actor);
        }
        if !item.disposition.is_terminal() {
            return Err("The Foundation review subject has no decision to revoke".into());
        }
        self.append_foundation_review_operation(
            category,
            vec![subject_id.to_string()],
            queue.basis,
            FoundationReviewAction::Revoke {
                subject_id: subject_id.to_string(),
            },
            actor,
        )
    }

    pub fn batch_review_foundation(
        &mut self,
        request: FoundationBatchReview,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(request.category)?;
        let requested = request
            .exact_subject_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let pinned = queue
            .items
            .iter()
            .map(|item| item.subject_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if request.exact_subject_ids.is_empty()
            || requested.len() != request.exact_subject_ids.len()
            || !requested.is_subset(&pinned)
        {
            return Err(
                "Batch review exact subject set is empty, duplicated, or no longer pinned".into(),
            );
        }
        if request.expected_basis != queue.basis
            || request.expected_ledger_sequence != queue.ledger_sequence
        {
            return Err(
                "Batch review dependency basis or exact subject set became stale; no decisions were recorded"
                    .into(),
            );
        }
        if request.category == FoundationCategory::Building
            && !self.foundation.building_review.is_empty()
        {
            return self.batch_review_building_queue_subjects(
                &queue,
                &request.exact_subject_ids,
                request.decision,
                actor,
            );
        }
        self.append_foundation_review_operation(
            request.category,
            request.exact_subject_ids,
            queue.basis,
            FoundationReviewAction::Batch {
                decision: request.decision,
            },
            actor,
        )
    }

    pub fn acknowledge_feature_gap(
        &mut self,
        category: FoundationCategory,
        gap_id: &str,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(category)?;
        let gap = queue
            .known_gaps
            .iter()
            .find(|gap| gap.id == gap_id)
            .ok_or("The Known Feature Gap is not linked to this pinned category evidence")?;
        if gap.acknowledged {
            return Err("The Known Feature Gap is already acknowledged".into());
        }
        if gap.status == KnownFeatureGapStatus::Resolved {
            return Err("The Known Feature Gap is already resolved".into());
        }
        self.append_foundation_review_operation(
            category,
            vec![gap_id.to_string()],
            queue.basis,
            FoundationReviewAction::GapAcknowledged {
                gap_id: gap_id.to_string(),
            },
            actor,
        )
    }

    pub fn reopen_feature_gap(
        &mut self,
        category: FoundationCategory,
        gap_id: &str,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(category)?;
        let status = queue
            .known_gaps
            .iter()
            .find(|gap| gap.id == gap_id)
            .map(|gap| gap.status)
            .or_else(|| self.known_feature_gap_status(category, gap_id))
            .ok_or("The Known Feature Gap has no retained evidence-linked history")?;
        if status == KnownFeatureGapStatus::Open {
            return Err("The Known Feature Gap is already open".into());
        }
        self.append_foundation_review_operation(
            category,
            vec![gap_id.to_string()],
            queue.basis,
            FoundationReviewAction::GapReopened {
                gap_id: gap_id.to_string(),
            },
            actor,
        )
    }

    pub fn resolve_feature_gap(
        &mut self,
        category: FoundationCategory,
        gap_id: &str,
        evidence_ids: Vec<String>,
        actor: InstallationId,
    ) -> Result<u64, String> {
        if evidence_ids.is_empty()
            || evidence_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != evidence_ids.len()
        {
            return Err(
                "Known Feature Gap resolution requires a non-empty exact evidence set".into(),
            );
        }
        let acquisition = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin refreshed evidence before resolving a Known Feature Gap")?;
        if evidence_ids.iter().any(|evidence_id| {
            !acquisition.observations.iter().any(|observation| {
                observation.id == *evidence_id && observation.category == category
            })
        }) {
            return Err(
                "Known Feature Gap resolution evidence is not pinned in this category".into(),
            );
        }
        let status = self
            .known_feature_gap_status(category, gap_id)
            .ok_or("The Known Feature Gap has no retained evidence-linked history")?;
        if status == KnownFeatureGapStatus::Resolved {
            return Err("The Known Feature Gap is already resolved".into());
        }
        let queue = self.foundation_review_queue(category)?;
        self.append_foundation_review_operation(
            category,
            std::iter::once(gap_id.to_string())
                .chain(evidence_ids.iter().cloned())
                .collect(),
            queue.basis,
            FoundationReviewAction::GapResolved {
                gap_id: gap_id.to_string(),
                evidence_ids,
            },
            actor,
        )
    }

    pub fn known_feature_gap_history(
        &self,
        category: FoundationCategory,
        gap_id: &str,
    ) -> Vec<KnownFeatureGapHistoryAction> {
        crate::foundation_review::known_gap_history(
            category,
            gap_id,
            &self.foundation.review_ledger.operations,
        )
        .1
    }

    fn known_feature_gap_status(
        &self,
        category: FoundationCategory,
        gap_id: &str,
    ) -> Option<KnownFeatureGapStatus> {
        let observed = self
            .foundation
            .acquisition
            .iter()
            .flat_map(|acquisition| &acquisition.manifest.coverage_report.outcomes)
            .chain(
                self.foundation
                    .acquisition_refresh_history
                    .iter()
                    .flat_map(|record| &record.previous_manifest.coverage_report.outcomes),
            )
            .filter(|outcome| outcome.category == category)
            .any(|outcome| {
                let reason_count = if outcome.gaps.is_empty()
                    && !matches!(
                        outcome.status,
                        ProviderOutcomeStatus::Complete | ProviderOutcomeStatus::CompleteEmpty
                    ) {
                    1
                } else {
                    outcome.gaps.len()
                };
                (0..reason_count)
                    .any(|index| crate::foundation_review::gap_id(outcome, index) == gap_id)
            })
            || self
                .foundation
                .review_ledger
                .operations
                .iter()
                .any(|operation| {
                    operation.category == category
                        && matches!(
                            &operation.action,
                            FoundationReviewAction::GapAcknowledged { gap_id: operation_gap }
                                | FoundationReviewAction::GapReopened { gap_id: operation_gap }
                                | FoundationReviewAction::GapResolved {
                                    gap_id: operation_gap,
                                    ..
                                } if operation_gap == gap_id
                        )
                });
        observed.then(|| {
            crate::foundation_review::known_gap_history(
                category,
                gap_id,
                &self.foundation.review_ledger.operations,
            )
            .0
        })
    }

    pub fn declare_foundation_review_conflict(
        &mut self,
        conflict: FoundationReviewConflict,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(conflict.category)?;
        let subjects = conflict
            .subject_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let pinned = queue
            .items
            .iter()
            .map(|item| item.subject_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if conflict.id.trim().is_empty()
            || conflict.explanation.trim().is_empty()
            || conflict.subject_ids.len() < 2
            || subjects.len() != conflict.subject_ids.len()
            || !subjects.is_subset(&pinned)
            || queue.conflicts.iter().any(|item| item.id == conflict.id)
        {
            return Err("Foundation review conflict identity or subjects are invalid".into());
        }
        self.append_foundation_review_operation(
            conflict.category,
            conflict.subject_ids.clone(),
            queue.basis,
            FoundationReviewAction::ConflictDeclared { conflict },
            actor,
        )
    }

    pub fn resolve_foundation_review_conflict(
        &mut self,
        category: FoundationCategory,
        conflict_id: &str,
        resolution: ReviewConflictResolution,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let queue = self.foundation_review_queue(category)?;
        let conflict = queue
            .conflicts
            .iter()
            .find(|conflict| conflict.id == conflict_id)
            .ok_or("The Foundation review conflict is not pinned in this category")?;
        let (_, resolved) = crate::foundation_review::review_conflicts(
            category,
            &queue.basis,
            &self.foundation.review_ledger.operations,
        );
        if resolved.contains(conflict_id) {
            return Err("The Foundation review conflict is already resolved".into());
        }
        validate_conflict_resolution(conflict, &resolution)?;
        if let ReviewConflictResolution::GeometryRepair {
            subject_id,
            review_geometry_sha256,
        } = &resolution
        {
            let subject = queue
                .items
                .iter()
                .find(|item| item.subject_id == *subject_id)
                .ok_or("The geometry repair subject is not in the current queue")?;
            if subject.review_geometry_derivation.review_geometry_sha256 != *review_geometry_sha256
            {
                return Err(
                    "The geometry repair must select retained, traceable review geometry".into(),
                );
            }
        }
        self.append_foundation_review_operation(
            category,
            conflict.subject_ids.clone(),
            queue.basis,
            FoundationReviewAction::ConflictResolved {
                conflict_id: conflict_id.to_string(),
                resolution,
            },
            actor,
        )
    }

    pub fn complete_foundation_category(
        &mut self,
        category: FoundationCategory,
        actor: InstallationId,
    ) -> Result<(), String> {
        let queue = self.foundation_review_queue(category)?;
        if !queue.progress.completion_blockers.is_empty() {
            return Err(format!(
                "Foundation category completion is blocked: {}",
                queue.progress.completion_blockers.join("; ")
            ));
        }
        if queue.progress.complete {
            return Ok(());
        }
        let accepted_evidence_ids = queue
            .items
            .iter()
            .filter(|item| item.disposition.enters_reviewed_model())
            .map(|item| item.subject_id.clone())
            .collect::<Vec<_>>();
        let rejected_evidence_ids = queue
            .items
            .iter()
            .filter(|item| matches!(item.disposition, CandidateReviewDisposition::Rejected))
            .map(|item| item.subject_id.clone())
            .collect::<Vec<_>>();
        let deferred_evidence_ids = queue
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.disposition,
                    CandidateReviewDisposition::Deferred { .. }
                )
            })
            .map(|item| item.subject_id.clone())
            .collect::<Vec<_>>();
        let acknowledged_gap_ids = queue
            .known_gaps
            .iter()
            .filter(|gap| gap.acknowledged)
            .map(|gap| gap.id.clone())
            .collect::<Vec<_>>();
        let subjects = queue
            .items
            .iter()
            .map(|item| item.subject_id.clone())
            .chain(acknowledged_gap_ids.iter().cloned())
            .collect::<Vec<_>>();
        let completion_disposition = if category == FoundationCategory::Building
            && !self.foundation.building_review.is_empty()
        {
            let acquisition = self
                .foundation
                .acquisition
                .as_ref()
                .ok_or("Pinned acquisition evidence is missing")?;
            let mut entity_ids = Vec::new();
            let mut known_entity_gaps = Vec::new();
            for entity in self
                .foundation
                .building_review
                .entities()
                .iter()
                .filter(|entity| entity.boundary_decision != BuildingBoundaryDecision::Exclude)
            {
                if self
                    .foundation
                    .building_review
                    .reviewed_entities_by_ids(
                        &acquisition.observations,
                        self.building_generation_basis(),
                        std::slice::from_ref(&entity.id),
                    )
                    .is_ok()
                {
                    entity_ids.push(entity.id.clone());
                } else {
                    let gap_id = building_entity_name_gap_id(&entity.id);
                    let acknowledged = queue
                        .known_gaps
                        .iter()
                        .any(|gap| gap.id == gap_id && gap.acknowledged);
                    if entity.name_resolution != BuildingNameResolution::Unnamed || !acknowledged {
                        return Err(
                            "A retained Building Entity is neither reviewed nor linked to an acknowledged gap"
                                .into(),
                        );
                    }
                    known_entity_gaps.push(KnownBuildingEntityGap {
                        entity_id: entity.id.clone(),
                        reasons: vec![
                            "No exclusive building-level name evidence was available".into()
                        ],
                    });
                }
            }
            FoundationReviewDisposition::ReviewedBuildingEntities {
                entity_ids,
                known_gaps: known_entity_gaps,
            }
        } else {
            FoundationReviewDisposition::ReviewedQueue {
                accepted_evidence_ids,
                rejected_evidence_ids,
                deferred_evidence_ids,
                acknowledged_gap_ids,
            }
        };
        self.mark_updated(actor)?;
        let operation_sequence = self.push_foundation_review_operation(
            category,
            subjects.clone(),
            queue.basis.clone(),
            FoundationReviewAction::CategoryCompleted,
        )?;
        let sequence = self.foundation.review_ledger.entries.len() as u64 + 1;
        self.foundation
            .review_ledger
            .entries
            .push(FoundationReviewEntry {
                sequence,
                category,
                subjects,
                basis: queue.basis.clone(),
                before: self
                    .foundation
                    .review_ledger
                    .disposition_for_basis(category, &queue.basis)
                    .cloned(),
                after: completion_disposition,
                recorded_at_unix_ms: now_unix_ms(),
                operation_sequence,
            });
        self.update_review_workflow_after_operation(&queue.basis);
        Ok(())
    }

    pub fn reviewed_features_for_completed_category(
        &self,
        category: FoundationCategory,
    ) -> Result<Vec<SourceObservation>, String> {
        let queue = self.foundation_review_queue(category)?;
        if !queue.progress.complete {
            return Err("The Foundation category is not explicitly complete".into());
        }
        let acquisition = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pinned acquisition evidence is missing")?;
        let non_generating_containers = crate::foundation_review::non_generating_container_ids(
            category,
            &queue.basis,
            &self.foundation.review_ledger.operations,
        );
        let selected = queue
            .items
            .iter()
            .filter(|item| {
                item.disposition.enters_reviewed_model()
                    && !non_generating_containers.contains(&item.subject_id)
            })
            .map(|item| item.subject_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        Ok(acquisition
            .observations
            .iter()
            .filter(|observation| {
                observation.category == category && selected.contains(observation.id.as_str())
            })
            .cloned()
            .collect())
    }

    pub fn building_entity_review(&self) -> &BuildingEntityReviewLedger {
        &self.foundation.building_review
    }

    pub fn acquisition_refresh_history(&self) -> &[AcquisitionRefreshRecord] {
        &self.foundation.acquisition_refresh_history
    }

    pub fn coarse_raster_runs(&self) -> &[crate::CoarseRasterSupplementRun] {
        &self.foundation.coarse_raster_runs
    }

    pub fn coarse_raster_evidence(
        &self,
        category: FoundationCategory,
    ) -> Vec<&crate::CoarseRasterObservation> {
        let Some(acquisition) = self.foundation.acquisition.as_ref() else {
            return Vec::new();
        };
        let Some(boundary) = self.foundation.boundary.as_ref() else {
            return Vec::new();
        };
        let current_outcomes = acquisition
            .manifest
            .coverage_report
            .outcomes
            .iter()
            .filter(|outcome| outcome.category == category)
            .cloned()
            .collect::<Vec<_>>();
        let current_gap_ids = crate::foundation_review::known_gaps_for_category(
            category,
            &current_outcomes,
            &self.foundation.review_ledger.operations,
        )
        .into_iter()
        .filter(|gap| gap.status != KnownFeatureGapStatus::Resolved)
        .map(|gap| gap.id)
        .collect::<std::collections::BTreeSet<_>>();
        self.foundation
            .coarse_raster_runs
            .iter()
            .flat_map(crate::CoarseRasterSupplementRun::observations)
            .filter(|observation| {
                observation.category == category
                    && observation.dataset_bundle_id == acquisition.manifest.bundle.id
                    && observation.clip.boundary_result_sha256 == boundary.manifest.result_sha256
                    && current_gap_ids.contains(&observation.linked_gap_id)
            })
            .collect()
    }

    pub fn record_coarse_raster_supplement(
        &mut self,
        run: crate::CoarseRasterSupplementRun,
        actor: InstallationId,
    ) -> Result<(), String> {
        let acquisition = self.foundation.acquisition.as_ref().ok_or(
            "Pin structured Foundation acquisition evidence before raster supplementation",
        )?;
        let boundary = self
            .foundation
            .boundary
            .as_ref()
            .ok_or("Confirm the Campus Boundary before raster supplementation")?;
        let current_outcomes = acquisition
            .manifest
            .coverage_report
            .outcomes
            .iter()
            .filter(|outcome| outcome.category == run.category)
            .cloned()
            .collect::<Vec<_>>();
        let gaps = crate::foundation_review::known_gaps_for_category(
            run.category,
            &current_outcomes,
            &self.foundation.review_ledger.operations,
        )
        .into_iter()
        .filter(|gap| gap.status != KnownFeatureGapStatus::Resolved)
        .map(|gap| (gap.id, (gap.location.tile_id, gap.location.geometry)))
        .collect::<BTreeMap<_, _>>();
        let boundary_geometry = boundary
            .confirmed_geometry()
            .ok_or("The confirmed Campus Boundary has no reviewable geometry")?;
        crate::validate_new_coarse_raster_run(
            &run,
            &self.foundation.coarse_raster_runs,
            crate::CoarseRasterValidationContext {
                dataset_bundle: &acquisition.manifest.bundle,
                contract_version: &acquisition.manifest.contract_version,
                boundary_result_sha256: &boundary.manifest.result_sha256,
                gaps: &gaps,
                boundary_geometry,
                structured_observations: &acquisition.observations,
            },
        )?;
        self.mark_updated(actor)?;
        self.foundation.coarse_raster_runs.push(run);
        Ok(())
    }

    pub fn coarse_raster_decision(
        &self,
        category: FoundationCategory,
        observation_id: &str,
    ) -> crate::CoarseRasterDecision {
        let Ok(basis) = self.coarse_raster_review_basis(category, observation_id) else {
            return crate::CoarseRasterDecision::Unresolved;
        };
        self.foundation
            .review_ledger
            .operations
            .iter()
            .rev()
            .find_map(|operation| match &operation.action {
                FoundationReviewAction::CoarseRasterDecision {
                    observation_id: operation_id,
                    decision,
                } if operation.category == category
                    && operation_id == observation_id
                    && operation.basis == basis =>
                {
                    Some(decision.clone())
                }
                _ => None,
            })
            .unwrap_or(crate::CoarseRasterDecision::Unresolved)
    }

    fn coarse_raster_review_basis(
        &self,
        category: FoundationCategory,
        observation_id: &str,
    ) -> Result<FoundationReviewBasis, String> {
        let (run, observation) = self
            .foundation
            .coarse_raster_runs
            .iter()
            .find_map(|run| {
                run.observations()
                    .find(|observation| {
                        observation.category == category && observation.id == observation_id
                    })
                    .map(|observation| (run, observation))
            })
            .ok_or("The coarse raster observation is not persisted in this category")?;
        if !self
            .coarse_raster_evidence(category)
            .iter()
            .any(|current| current.id == observation_id)
        {
            return Err(
                "The coarse raster observation is stale or outside the current review basis".into(),
            );
        }
        let manifest = run
            .manifest
            .as_ref()
            .ok_or("The coarse raster observation has no verified result manifest")?;
        let mut basis = self.review_basis()?;
        let identity = format!("coarse-raster:{}:{}", run.id, observation.id);
        basis.dependencies.subjects.insert(
            identity.clone(),
            ReviewSubjectDependencyBasis {
                observation_id: observation.id.clone(),
                upstream_source_record_identity: identity,
                geometry_digest: geometry_digest(&observation.approximate_geometry),
                grouping_digest: manifest.result_sha256.clone(),
                naming_digest: "not-applicable".into(),
                attribute_digest: observation.source.source_sha256.clone(),
                containment_digest: format!(
                    "{}:{}:{}",
                    observation.clip.boundary_result_sha256,
                    observation.clip.linked_gap_id,
                    geometry_digest(&observation.clip.gap_geometry)
                ),
                licence_digest: format!(
                    "{}:{}",
                    observation.source.licence.identifier,
                    observation.source.licence.dataset_release
                ),
                rule_version_digest: format!(
                    "{}:{}:{}",
                    observation.dataset_bundle_id,
                    observation.algorithm.algorithm_version,
                    observation.algorithm.vectorization_version
                ),
                content_digest: observation.derived_sha256.clone(),
            },
        );
        Ok(basis)
    }

    pub fn review_coarse_raster_observation(
        &mut self,
        category: FoundationCategory,
        observation_id: &str,
        decision: crate::CoarseRasterDecision,
        actor: InstallationId,
    ) -> Result<(), String> {
        if matches!(
            &decision,
            crate::CoarseRasterDecision::Rejected { reason } if reason.trim().is_empty()
        ) {
            return Err("Rejected coarse raster evidence requires a reason".into());
        }
        let basis = self.coarse_raster_review_basis(category, observation_id)?;
        let mut before = self.foundation_review_operation_state(category)?;
        before.coarse_raster_decisions.insert(
            observation_id.to_string(),
            self.coarse_raster_decision(category, observation_id),
        );
        let mut after = before.clone();
        after
            .coarse_raster_decisions
            .insert(observation_id.to_string(), decision.clone());
        self.mark_updated(actor)?;
        let sequence = self.foundation.review_ledger.current_sequence() + 1;
        self.foundation
            .review_ledger
            .operations
            .push(FoundationReviewOperation {
                sequence,
                category,
                subjects: vec![observation_id.to_string()],
                basis,
                action: FoundationReviewAction::CoarseRasterDecision {
                    observation_id: observation_id.to_string(),
                    decision,
                },
                before,
                after,
                explanation: Some("Coarse raster Map Candidate review decision".into()),
                carried_from_sequence: None,
                recorded_at_unix_ms: now_unix_ms(),
            });
        Ok(())
    }

    pub fn acquisition_snapshot_identity(&self) -> &str {
        &self.foundation.acquisition_snapshot_identity
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

    pub fn stale_generated_outputs(&self) -> &[GeneratedFoundationOutput] {
        &self.foundation.stale_generated
    }

    pub fn stale_exported_outputs(&self) -> &[ExportedFoundationOutput] {
        &self.foundation.stale_exported
    }

    pub fn withdrawn_refresh_evidence(
        &self,
        upstream_source_record_identity: &str,
    ) -> Option<&SourceObservation> {
        self.foundation
            .acquisition_refresh_history
            .iter()
            .rev()
            .flat_map(|record| record.retained_previous_observations.iter())
            .find(|observation| {
                crate::upstream_source_record_identity(observation)
                    == upstream_source_record_identity
            })
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
        let selected_geometry = evidence.confirmed_geometry();
        let geometry_validity = selected_geometry.map(validate_boundary_geometry);
        let selected_assessment = evidence.assessments.get(&evidence.selected_candidate_id);
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
            || geometry_validity
                .as_ref()
                .is_some_and(|validity| !validity.valid)
            || matches!(
                selected_assessment.map(|assessment| &assessment.validity),
                Some(BoundaryCandidateValidity::Invalid { .. })
            )
        {
            let reason = selected_assessment
                .and_then(|assessment| match &assessment.validity {
                    BoundaryCandidateValidity::Valid => None,
                    BoundaryCandidateValidity::Invalid { reasons } => reasons.first().cloned(),
                })
                .or_else(|| {
                    geometry_validity
                        .as_ref()
                        .and_then(|validity| validity.reasons.first().cloned())
                })
                .unwrap_or_else(|| "boundary evidence is incomplete".into());
            return Err(format!("Confirmed boundary evidence is invalid: {reason}"));
        }
        self.mark_updated(actor)?;
        self.foundation.boundary_review = None;
        self.foundation.boundary = Some(evidence);
        self.foundation.pending_acquisition_start = None;
        self.foundation.acquisition_checkpoint = None;
        self.foundation.acquisition_checkpoint_purpose =
            FoundationAcquisitionCheckpointPurpose::Initial;
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
        let refresh = self.foundation.acquisition_checkpoint_purpose
            == FoundationAcquisitionCheckpointPurpose::ExplicitRefresh;
        if checkpoint.boundary_revision != boundary.manifest.result_sha256
            || (!refresh && checkpoint.bundle != boundary.manifest.bundle)
            || (refresh
                && self
                    .foundation
                    .acquisition
                    .as_ref()
                    .is_some_and(|current| current.manifest.bundle == checkpoint.bundle))
        {
            return Err(
                if refresh {
                    "Explicit Foundation refresh must keep the confirmed boundary and pin a new Dataset Bundle"
                } else {
                    "Foundation acquisition must reuse the confirmed Boundary Discovery Snapshot"
                }
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
        self.foundation.pending_acquisition_start = None;
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

    pub fn record_explicit_refresh_checkpoint(
        &mut self,
        checkpoint: FoundationAcquisitionCheckpoint,
        actor: InstallationId,
    ) -> Result<(), String> {
        let boundary = self
            .foundation
            .boundary
            .as_ref()
            .ok_or("Confirm a Campus Boundary before requesting a Foundation refresh")?;
        let current = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin the initial Foundation acquisition before requesting a refresh")?;
        checkpoint.validate()?;
        if checkpoint.boundary_revision != boundary.manifest.result_sha256
            || checkpoint.bundle == current.manifest.bundle
        {
            return Err(
                "Explicit Foundation refresh must keep the confirmed boundary and pin a new Dataset Bundle"
                    .into(),
            );
        }
        self.mark_updated(actor)?;
        self.foundation.pending_acquisition_start = None;
        self.foundation.acquisition_checkpoint = Some(checkpoint);
        self.foundation.acquisition_checkpoint_purpose =
            FoundationAcquisitionCheckpointPurpose::ExplicitRefresh;
        Ok(())
    }

    pub fn pin_acquisition(
        &mut self,
        evidence: PinnedAcquisitionEvidence,
        actor: InstallationId,
    ) -> Result<(), String> {
        self.pin_acquisition_with_building_mappings(evidence, BTreeMap::new(), actor)
    }

    pub fn pin_acquisition_with_building_mappings(
        &mut self,
        mut evidence: PinnedAcquisitionEvidence,
        building_entity_mappings: BTreeMap<String, String>,
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
        let mut building_review = self.foundation.building_review.clone();
        let mut refresh_history = self.foundation.acquisition_refresh_history.clone();
        let mut acquisition_snapshot_identity = evidence.manifest.result_sha256.clone();
        if let Some(previous) = &self.foundation.acquisition {
            let incoming_manifest = evidence.manifest.clone();
            let incoming_ids = evidence
                .observations
                .iter()
                .map(|observation| observation.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let previous_ids = previous
                .observations
                .iter()
                .map(|observation| observation.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let mut merged = previous.observations.clone();
            for observation in evidence.observations {
                if let Some(existing) = merged.iter().find(|existing| existing.id == observation.id)
                {
                    if existing != &observation {
                        return Err(format!(
                            "Pinned Source Observation identity changed: {}",
                            observation.id
                        ));
                    }
                } else {
                    merged.push(observation);
                }
            }
            evidence.observations = merged;
            let previous_snapshot_identity =
                if self.foundation.acquisition_snapshot_identity.is_empty() {
                    previous.manifest.result_sha256.as_str()
                } else {
                    self.foundation.acquisition_snapshot_identity.as_str()
                };
            let composite_snapshot_identity = format!(
                "composite-v1:{}:{}:{}:{}",
                previous_snapshot_identity.len(),
                previous_snapshot_identity,
                incoming_manifest.result_sha256.len(),
                incoming_manifest.result_sha256
            );
            acquisition_snapshot_identity = composite_snapshot_identity.clone();
            refresh_history.push(AcquisitionRefreshRecord {
                previous_manifest: previous.manifest.clone(),
                incoming_manifest,
                added_observation_ids: incoming_ids.difference(&previous_ids).cloned().collect(),
                withdrawn_observation_ids: previous_ids
                    .difference(&incoming_ids)
                    .cloned()
                    .collect(),
                composite_snapshot_identity,
                difference: None,
                retained_previous_observations: Vec::new(),
            });
            if !building_review.is_empty() {
                building_review.refresh_from_observations_with_mappings(
                    &evidence.observations,
                    building_entity_mappings,
                )?;
            } else if !building_entity_mappings.is_empty() {
                return Err("Building refresh mappings require initialized entity review".into());
            }
        } else if !building_entity_mappings.is_empty() {
            return Err("Building refresh mappings require a previous acquisition".into());
        }
        self.mark_updated(actor)?;
        self.foundation.acquisition = Some(evidence);
        self.foundation.acquisition_snapshot_identity = acquisition_snapshot_identity;
        self.foundation.acquisition_refresh_history = refresh_history;
        self.foundation.building_review = building_review;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.acquisition = DurableTaskState::Confirmed;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn apply_foundation_refresh(
        &mut self,
        evidence: PinnedAcquisitionEvidence,
        refreshed_boundary: Option<PinnedBoundaryEvidence>,
        actor: InstallationId,
    ) -> Result<FoundationSourceRefreshDifference, String> {
        let previous = self
            .foundation
            .acquisition
            .clone()
            .ok_or("Pin the initial Foundation acquisition before requesting a refresh")?;
        let previous_boundary = self
            .foundation
            .boundary
            .as_ref()
            .and_then(PinnedBoundaryEvidence::confirmed_geometry)
            .cloned()
            .ok_or("Confirmed Campus Boundary evidence is missing")?;
        let next_boundary = refreshed_boundary
            .as_ref()
            .and_then(PinnedBoundaryEvidence::confirmed_geometry)
            .cloned()
            .unwrap_or_else(|| previous_boundary.clone());
        if evidence.manifest.bundle.id.trim().is_empty()
            || evidence.manifest.bundle.id == previous.manifest.bundle.id
            || evidence.manifest.result_sha256.trim().is_empty()
            || !FoundationCategory::ALL.into_iter().all(|category| {
                evidence
                    .manifest
                    .coverage_report
                    .outcomes
                    .iter()
                    .any(|outcome| outcome.category == category)
            })
        {
            return Err(
                "An explicit Foundation refresh requires a complete, newly pinned Dataset Bundle"
                    .into(),
            );
        }
        if !matches!(
            next_boundary,
            SourceGeometry::Polygon(_) | SourceGeometry::MultiPolygon(_)
        ) || !validate_boundary_geometry(&next_boundary).valid
        {
            return Err("Refreshed Campus Boundary evidence is invalid".into());
        }
        let upstream_source_record_identities = evidence
            .observations
            .iter()
            .map(crate::upstream_source_record_identity)
            .collect::<Vec<_>>();
        if upstream_source_record_identities
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != upstream_source_record_identities.len()
        {
            return Err(
                "A refreshed Dataset Bundle contains duplicate upstream source-record identities"
                    .into(),
            );
        }

        let difference =
            compare_foundation_evidence(&previous, &evidence, &previous_boundary, &next_boundary);
        let old_basis = self.review_basis()?;
        let old_operations = self.foundation.review_ledger.operations.clone();
        let old_entries = self.foundation.review_ledger.entries.clone();
        let previous_by_id = previous
            .observations
            .iter()
            .map(|observation| {
                (
                    observation.id.clone(),
                    crate::upstream_source_record_identity(observation),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let current_id_by_upstream_identity = evidence
            .observations
            .iter()
            .map(|observation| {
                (
                    crate::upstream_source_record_identity(observation),
                    observation.id.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let incoming_manifest = evidence.manifest.clone();
        let incoming_ids = evidence
            .observations
            .iter()
            .map(|observation| observation.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let previous_ids = previous
            .observations
            .iter()
            .map(|observation| observation.id.clone())
            .collect::<std::collections::BTreeSet<_>>();

        let mut building_review = self.foundation.building_review.clone();
        if !building_review.is_empty() {
            building_review.refresh_from_observations(&evidence.observations)?;
        }
        if let Some(boundary) = refreshed_boundary {
            self.foundation.boundary = Some(boundary);
        }
        self.foundation.acquisition = Some(evidence);
        self.foundation.acquisition_snapshot_identity = incoming_manifest.result_sha256.clone();
        self.foundation.building_review = building_review;
        let new_basis = self.review_basis()?;

        let rule_changes = ChangedBundleRules {
            classification: previous.manifest.bundle.classification_rules
                != incoming_manifest.bundle.classification_rules,
            assembly: previous.manifest.bundle.assembly_rules
                != incoming_manifest.bundle.assembly_rules,
            conflation: previous.manifest.bundle.conflation_rules
                != incoming_manifest.bundle.conflation_rules,
            derivation: previous.manifest.bundle.derivation_rules
                != incoming_manifest.bundle.derivation_rules,
        };
        let latest_gap_operations = old_operations
            .iter()
            .filter(|operation| operation.basis == old_basis)
            .filter_map(|operation| match &operation.action {
                FoundationReviewAction::GapAcknowledged { gap_id }
                | FoundationReviewAction::GapReopened { gap_id }
                | FoundationReviewAction::GapResolved { gap_id, .. } => {
                    Some(((operation.category, gap_id.clone()), operation))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let invalidated_resolved_gaps = latest_gap_operations
            .into_iter()
            .filter_map(|((category, gap_id), operation)| {
                let category_changes = difference.dependency_changes_for(category);
                if matches!(operation.action, FoundationReviewAction::GapResolved { .. })
                    && refresh_invalidates_operation(
                        operation,
                        &category_changes,
                        &rule_changes,
                        &previous_by_id,
                        &difference,
                    )
                {
                    Some((category, gap_id))
                } else {
                    None
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut carried_sequence = BTreeMap::<u64, u64>::new();
        for category in FoundationCategory::ALL {
            let category_changes = difference.dependency_changes_for(category);
            for operation in old_operations
                .iter()
                .filter(|operation| operation.category == category && operation.basis == old_basis)
            {
                if refresh_invalidates_operation(
                    operation,
                    &category_changes,
                    &rule_changes,
                    &previous_by_id,
                    &difference,
                ) {
                    continue;
                }
                let mut carried = operation.clone();
                remap_review_operation(
                    &mut carried,
                    &previous_by_id,
                    &current_id_by_upstream_identity,
                )?;
                let source_sequence = operation.sequence;
                carried.sequence = self.foundation.review_ledger.current_sequence() + 1;
                carried.basis = new_basis.clone();
                carried.carried_from_sequence = Some(source_sequence);
                carried.explanation = Some(match &operation.explanation {
                    Some(explanation) => {
                        format!("Carried forward after unchanged dependencies: {explanation}")
                    }
                    None => "Carried forward after unchanged dependencies".into(),
                });
                carried_sequence.insert(source_sequence, carried.sequence);
                self.foundation.review_ledger.operations.push(carried);
            }
            for (_, gap_id) in invalidated_resolved_gaps
                .iter()
                .filter(|(gap_category, _)| *gap_category == category)
            {
                let mut before = FoundationReviewState::default();
                before.resolved_gap_ids.insert(gap_id.clone());
                self.foundation
                    .review_ledger
                    .operations
                    .push(FoundationReviewOperation {
                        sequence: self.foundation.review_ledger.current_sequence() + 1,
                        category,
                        subjects: vec![gap_id.clone()],
                        basis: new_basis.clone(),
                        action: FoundationReviewAction::GapReopened {
                            gap_id: gap_id.clone(),
                        },
                        before,
                        after: FoundationReviewState::default(),
                        explanation: Some(
                            "Automatically reopened because refresh changed resolution evidence"
                                .into(),
                        ),
                        carried_from_sequence: None,
                        recorded_at_unix_ms: now_unix_ms(),
                    });
            }
            let category_changed =
                category_changes.any() || rule_changes.affects_category(category);
            if !category_changed {
                if let Some(entry) = old_entries.iter().rev().find(|entry| {
                    entry.category == category
                        && entry.basis == old_basis
                        && self
                            .foundation
                            .review_ledger
                            .disposition_for_basis(category, &old_basis)
                            .is_some()
                }) {
                    if let Some(operation_sequence) =
                        carried_sequence.get(&entry.operation_sequence).copied()
                    {
                        let mut carried = entry.clone();
                        carried.sequence = self.foundation.review_ledger.entries.len() as u64 + 1;
                        carried.basis = new_basis.clone();
                        carried.operation_sequence = operation_sequence;
                        remap_review_entry(
                            &mut carried,
                            &previous_by_id,
                            &current_id_by_upstream_identity,
                        )?;
                        self.foundation.review_ledger.entries.push(carried);
                    }
                }
            }
        }

        let formal_output_basis = self
            .foundation
            .generated
            .as_ref()
            .map(|output| &output.dependency_basis)
            .or_else(|| {
                self.foundation
                    .exported
                    .as_ref()
                    .map(|output| &output.dependency_basis)
            });
        let selected_input_changed = formal_output_basis.is_some_and(|basis| {
            difference.observations.iter().any(|change| {
                change.classification == crate::ObservationRefreshClassification::Added
                    || (basis
                        .subjects
                        .contains_key(&change.upstream_source_record_identity)
                        && !matches!(
                            change.classification,
                            crate::ObservationRefreshClassification::Unchanged
                                | crate::ObservationRefreshClassification::CoverageChanged
                        ))
            })
        });
        let legacy_output_basis =
            formal_output_basis.is_some_and(|basis| basis.subjects.is_empty());
        let output_dependencies_changed = selected_input_changed
            || (legacy_output_basis && !difference.changed_categories().is_empty())
            || !difference.coverage.is_empty()
            || rule_changes.any()
            || difference.boundary != crate::BoundaryRefreshClassification::Unchanged;
        if output_dependencies_changed {
            if let Some(generated) = self.foundation.generated.take() {
                self.foundation.stale_generated.push(generated);
            }
            if let Some(exported) = self.foundation.exported.take() {
                self.foundation.stale_exported.push(exported);
            }
        }
        self.mark_updated(actor)?;
        self.foundation
            .acquisition_refresh_history
            .push(AcquisitionRefreshRecord {
                previous_manifest: previous.manifest,
                incoming_manifest,
                added_observation_ids: incoming_ids.difference(&previous_ids).cloned().collect(),
                withdrawn_observation_ids: previous_ids
                    .difference(&incoming_ids)
                    .cloned()
                    .collect(),
                composite_snapshot_identity: self.foundation.acquisition_snapshot_identity.clone(),
                difference: Some(difference.clone()),
                retained_previous_observations: previous.observations,
            });
        self.foundation.pending_acquisition_start = None;
        self.foundation.acquisition_checkpoint = None;
        self.foundation.acquisition_checkpoint_purpose =
            FoundationAcquisitionCheckpointPurpose::Initial;
        self.workflow.acquisition = DurableTaskState::Confirmed;
        self.workflow.review = if self
            .foundation
            .review_ledger
            .is_complete_for_basis(&new_basis)
        {
            DurableTaskState::Confirmed
        } else {
            DurableTaskState::Pending
        };
        self.workflow.generation = if self.foundation.generated.is_some() {
            DurableTaskState::Confirmed
        } else {
            DurableTaskState::Pending
        };
        self.workflow.export = if self.foundation.exported.is_some() {
            DurableTaskState::Confirmed
        } else {
            DurableTaskState::Pending
        };
        Ok(difference)
    }

    pub fn initialize_building_entity_review(
        &mut self,
        name_evidence: Vec<BuildingNameEvidence>,
        actor: InstallationId,
    ) -> Result<(), String> {
        if !self.foundation.building_review.is_empty() {
            return Err("Building Entity review is already initialized".into());
        }
        let observations = &self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Building Entities")?
            .observations;
        let review = BuildingEntityReviewLedger::from_observations(observations, name_evidence)?;
        self.mark_updated(actor)?;
        self.foundation.building_review = review;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(())
    }

    pub fn record_building_entity_decision(
        &mut self,
        decision: BuildingEntityDecision,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let observations = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Building Entities")?
            .observations
            .clone();
        let mut review = self.foundation.building_review.clone();
        let sequence = review.record(decision, &observations)?;
        self.mark_updated(actor)?;
        self.foundation.building_review = review;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(sequence)
    }

    fn review_building_queue_subject(
        &mut self,
        subject_id: &str,
        decision: FoundationCandidateDecision,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let observations = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Building Entities")?
            .observations
            .clone();
        let mut review = self.foundation.building_review.clone();
        let sequence =
            apply_building_queue_decision(&mut review, &observations, subject_id, decision)?;
        self.mark_updated(actor)?;
        self.foundation.building_review = review;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(sequence)
    }

    fn batch_review_building_queue_subjects(
        &mut self,
        queue: &FoundationReviewQueueProjection,
        subject_ids: &[String],
        decision: FoundationBatchDecision,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let observations = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Building Entities")?
            .observations
            .clone();
        let mut review = self.foundation.building_review.clone();
        let mut subjects_by_entity = BTreeMap::<String, Vec<String>>::new();
        for subject_id in subject_ids {
            let entity = review
                .entities()
                .iter()
                .find(|entity| entity.evidence_ids.contains(subject_id))
                .ok_or("The Building queue subject has no stable Building Entity")?;
            subjects_by_entity
                .entry(entity.id.clone())
                .or_default()
                .push(subject_id.clone());
        }
        let mut final_sequence = None;
        for (entity_id, subjects) in subjects_by_entity {
            let entity = review
                .entities()
                .iter()
                .find(|entity| entity.id == entity_id)
                .cloned()
                .ok_or("The Building Entity changed during exact-set batch review")?;
            let subject_id = if subjects.contains(&entity.primary_observation_id) {
                entity.primary_observation_id
            } else {
                subjects[0].clone()
            };
            let candidate_decision = match decision {
                FoundationBatchDecision::Accept => FoundationCandidateDecision::Accept,
                FoundationBatchDecision::Reject => FoundationCandidateDecision::Reject {
                    reason: "explicit exact-set Building queue rejection".into(),
                },
            };
            final_sequence = Some(apply_building_queue_decision(
                &mut review,
                &observations,
                &subject_id,
                candidate_decision,
            )?);
        }
        let sequence = final_sequence.ok_or("The Building exact-set batch is empty")?;
        let before = foundation_review_state_from_queue(queue);
        let original_review =
            std::mem::replace(&mut self.foundation.building_review, review.clone());
        let after_queue = self.foundation_review_queue(FoundationCategory::Building);
        self.foundation.building_review = original_review;
        let after_queue = after_queue?;
        let after = foundation_review_state_from_queue(&after_queue);
        self.mark_updated(actor)?;
        self.foundation.building_review = review;
        let operation_sequence = self.foundation.review_ledger.current_sequence() + 1;
        self.foundation
            .review_ledger
            .operations
            .push(FoundationReviewOperation {
                sequence: operation_sequence,
                category: FoundationCategory::Building,
                subjects: subject_ids.to_vec(),
                basis: after_queue.basis.clone(),
                action: FoundationReviewAction::Batch { decision },
                before,
                after,
                explanation: Some(format!(
                    "Atomic exact-set Building Entity batch covering {} selected observation(s) and ending at entity ledger sequence {sequence}",
                    subject_ids.len()
                )),
                carried_from_sequence: None,
                recorded_at_unix_ms: now_unix_ms(),
            });
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(operation_sequence)
    }

    pub fn revoke_last_building_entity_decision(
        &mut self,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let mut review = self.foundation.building_review.clone();
        let observations = self
            .foundation
            .acquisition
            .as_ref()
            .ok_or("Pin acquisition evidence before reviewing Building Entities")?
            .observations
            .clone();
        let sequence = review.revoke_last(&observations)?;
        self.mark_updated(actor)?;
        self.foundation.building_review = review;
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = DurableTaskState::Pending;
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
        Ok(sequence)
    }

    fn revoke_building_entity_decision_for_subject(
        &mut self,
        subject_id: &str,
        actor: InstallationId,
    ) -> Result<u64, String> {
        let entity_id = self
            .foundation
            .building_review
            .entities()
            .iter()
            .find(|entity| entity.evidence_ids.iter().any(|id| id == subject_id))
            .map(|entity| entity.id.clone())
            .ok_or("The selected Building queue subject has no stable entity")?;
        let revoked = self
            .foundation
            .building_review
            .entries()
            .iter()
            .filter_map(|entry| match entry.decision {
                BuildingEntityDecision::Revoke { target_sequence } => Some(target_sequence),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let latest = self
            .foundation
            .building_review
            .entries()
            .iter()
            .rev()
            .find(|entry| {
                !matches!(
                    entry.decision,
                    BuildingEntityDecision::Revoke { .. }
                        | BuildingEntityDecision::RefreshEvidence { .. }
                ) && !revoked.contains(&entry.sequence)
            })
            .ok_or("There is no Building Entity decision to revoke")?;
        if !latest.subjects.iter().any(|subject| subject == &entity_id) {
            return Err(
                "The selected Building Entity is not the latest reversible entity decision".into(),
            );
        }
        self.revoke_last_building_entity_decision(actor)
    }

    fn append_foundation_review_operation(
        &mut self,
        category: FoundationCategory,
        subjects: Vec<String>,
        basis: FoundationReviewBasis,
        action: FoundationReviewAction,
        actor: InstallationId,
    ) -> Result<u64, String> {
        self.mark_updated(actor)?;
        let sequence =
            self.push_foundation_review_operation(category, subjects, basis.clone(), action)?;
        self.update_review_workflow_after_operation(&basis);
        Ok(sequence)
    }

    fn push_foundation_review_operation(
        &mut self,
        category: FoundationCategory,
        subjects: Vec<String>,
        basis: FoundationReviewBasis,
        action: FoundationReviewAction,
    ) -> Result<u64, String> {
        let mut before = self.foundation_review_operation_state(category)?;
        let mut after = before.clone();
        apply_foundation_review_action_to_state(&mut after, &action, &subjects);
        let explanation = foundation_review_action_explanation(&action);
        let sequence = self.foundation.review_ledger.current_sequence() + 1;
        self.foundation
            .review_ledger
            .operations
            .push(FoundationReviewOperation {
                sequence,
                category,
                subjects,
                basis,
                action,
                before: std::mem::take(&mut before),
                after,
                explanation,
                carried_from_sequence: None,
                recorded_at_unix_ms: now_unix_ms(),
            });
        Ok(sequence)
    }

    fn foundation_review_operation_state(
        &self,
        category: FoundationCategory,
    ) -> Result<FoundationReviewState, String> {
        let queue = self.foundation_review_queue(category)?;
        let mut state = foundation_review_state_from_queue(&queue);
        for operation in self
            .foundation
            .review_ledger
            .operations
            .iter()
            .filter(|operation| operation.category == category && operation.basis == queue.basis)
        {
            if let FoundationReviewAction::CoarseRasterDecision {
                observation_id,
                decision,
            } = &operation.action
            {
                state
                    .coarse_raster_decisions
                    .insert(observation_id.clone(), decision.clone());
            }
        }
        Ok(state)
    }

    fn update_review_workflow_after_operation(&mut self, basis: &FoundationReviewBasis) {
        self.foundation.generated = None;
        self.foundation.exported = None;
        self.workflow.review = if self.foundation.review_ledger.is_complete_for_basis(basis) {
            DurableTaskState::Confirmed
        } else {
            DurableTaskState::Pending
        };
        self.workflow.generation = DurableTaskState::Pending;
        self.workflow.export = DurableTaskState::Pending;
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
                if category == FoundationCategory::Building
                    && !self.foundation.building_review.is_empty()
                {
                    return Err(
                        "Building evidence must be completed as reviewed Building Entities".into(),
                    );
                }
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
            FoundationReviewDisposition::ReviewedBuildingEntities {
                entity_ids,
                known_gaps,
            } => {
                if category != FoundationCategory::Building {
                    return Err(
                        "Reviewed Building Entities can complete only the Building category".into(),
                    );
                }
                let reviewed = self.foundation.building_review.reviewed_entities_by_ids(
                    &acquisition.observations,
                    self.building_generation_basis(),
                    entity_ids,
                )?;
                let reviewed_ids = reviewed
                    .iter()
                    .map(|entity| entity.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let selected_ids = entity_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                let gap_ids = known_gaps
                    .iter()
                    .map(|gap| gap.entity_id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let retained_ids = self
                    .foundation
                    .building_review
                    .entities()
                    .iter()
                    .filter(|entity| {
                        entity.boundary_decision != crate::BuildingBoundaryDecision::Exclude
                    })
                    .map(|entity| entity.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let accounted_ids = selected_ids
                    .union(&gap_ids)
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                if entity_ids.is_empty()
                    || entity_ids.len() != selected_ids.len()
                    || selected_ids != reviewed_ids
                    || known_gaps.len() != gap_ids.len()
                    || known_gaps.iter().any(|gap| {
                        gap.reasons.is_empty()
                            || gap.reasons.iter().any(|reason| reason.trim().is_empty())
                    })
                    || !selected_ids.is_disjoint(&gap_ids)
                    || accounted_ids != retained_ids
                {
                    return Err(
                        "Building review must select the complete reviewed entity projection"
                            .into(),
                    );
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
            FoundationReviewDisposition::ReviewedQueue { .. } => {
                return Err("Queue review completion must use complete_foundation_category".into());
            }
        }
        let basis = self.review_basis()?;
        let subjects = match &disposition {
            FoundationReviewDisposition::SelectedEvidence { evidence_ids } => evidence_ids.clone(),
            FoundationReviewDisposition::ReviewedBuildingEntities {
                entity_ids,
                known_gaps,
            } => entity_ids
                .iter()
                .cloned()
                .chain(known_gaps.iter().map(|gap| gap.entity_id.clone()))
                .collect(),
            FoundationReviewDisposition::ReviewedQueue {
                accepted_evidence_ids,
                rejected_evidence_ids,
                deferred_evidence_ids,
                acknowledged_gap_ids,
            } => accepted_evidence_ids
                .iter()
                .chain(rejected_evidence_ids)
                .chain(deferred_evidence_ids)
                .chain(acknowledged_gap_ids)
                .cloned()
                .collect(),
            _ => vec![format!("foundation-category:{category:?}").to_ascii_lowercase()],
        };
        let before = self
            .foundation
            .review_ledger
            .disposition_for_basis(category, &basis)
            .cloned();
        self.mark_updated(actor)?;
        let operation_sequence = self.push_foundation_review_operation(
            category,
            subjects.clone(),
            basis.clone(),
            FoundationReviewAction::CategoryCompleted,
        )?;
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
                operation_sequence,
            });
        self.update_review_workflow_after_operation(&basis);
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
                FoundationReviewDisposition::ReviewedQueue {
                    accepted_evidence_ids,
                    ..
                } => Some(accepted_evidence_ids.as_slice()),
                FoundationReviewDisposition::ReviewedBuildingEntities { .. } => None,
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let mut selected_features = evidence
            .acquisition
            .observations
            .iter()
            .filter(|observation| selected_ids.contains(&&observation.id))
            .cloned()
            .collect::<Vec<_>>();
        let non_generating_containers = FoundationCategory::ALL
            .into_iter()
            .flat_map(|category| {
                crate::foundation_review::non_generating_container_ids(
                    category,
                    &basis,
                    &self.foundation.review_ledger.operations,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        selected_features
            .retain(|observation| !non_generating_containers.contains(&observation.id));
        let reviewed_feature_resolutions = FoundationCategory::ALL
            .into_iter()
            .flat_map(|category| {
                crate::foundation_review::reviewed_feature_resolutions(
                    category,
                    &basis,
                    &self.foundation.review_ledger.operations,
                )
            })
            .filter(|resolution| {
                selected_features
                    .iter()
                    .any(|feature| feature.id == resolution.subject_id)
            })
            .collect::<Vec<_>>();
        for resolution in &reviewed_feature_resolutions {
            if let Some(review_geometry_sha256) = &resolution.review_geometry_sha256 {
                let feature = selected_features
                    .iter()
                    .find(|feature| feature.id == resolution.subject_id)
                    .ok_or("Reviewed geometry repair subject is not selected")?;
                if feature.derivation.review_geometry_sha256 != *review_geometry_sha256 {
                    return Err(
                        "Reviewed geometry repair does not match the retained review geometry"
                            .into(),
                    );
                }
            }
        }
        let building_entities = match self
            .foundation
            .review_ledger
            .disposition_for_basis(FoundationCategory::Building, &basis)
        {
            Some(FoundationReviewDisposition::ReviewedBuildingEntities { entity_ids, .. }) => {
                let entities = self.foundation.building_review.reviewed_entities_by_ids(
                    &evidence.acquisition.observations,
                    self.building_generation_basis(),
                    entity_ids,
                )?;
                if entities
                    .iter()
                    .map(|entity| entity.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    != entity_ids
                        .iter()
                        .map(String::as_str)
                        .collect::<std::collections::BTreeSet<_>>()
                {
                    return Err("Reviewed Building Entity selection is stale".into());
                }
                entities
            }
            _ => Vec::new(),
        };
        for entity in &building_entities {
            let mut generation_feature = entity
                .source_observations
                .iter()
                .find(|observation| observation.id == entity.primary_observation_id)
                .cloned()
                .ok_or("Reviewed Building Entity primary evidence is missing")?;
            generation_feature.id = entity.id.clone();
            generation_feature.review_geometry_proposal =
                entity.generation_geometry.wgs84_generator_geometry();
            selected_features.push(generation_feature);
            for (part_id, geometry) in entity
                .part_observation_ids
                .iter()
                .zip(entity.generation_geometry.wgs84_part_generator_geometries())
            {
                if part_id == &entity.primary_observation_id {
                    continue;
                }
                let mut part_feature = entity
                    .source_observations
                    .iter()
                    .find(|observation| observation.id == *part_id)
                    .cloned()
                    .ok_or("Reviewed Building Entity part evidence is missing")?;
                part_feature.id = format!("{}:part:{part_id}", entity.id);
                part_feature.review_geometry_proposal = geometry;
                selected_features.push(part_feature);
            }
        }
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
            reviewed_feature_resolutions,
            building_entities,
            generation_settings: self.foundation.generation_settings.clone(),
        })
    }

    fn building_generation_basis(&self) -> BuildingGenerationBasis {
        let origin_wgs84 = self
            .foundation
            .boundary
            .as_ref()
            .and_then(|boundary| {
                boundary
                    .candidates
                    .iter()
                    .find(|candidate| candidate.id == boundary.selected_candidate_id)
            })
            .and_then(|candidate| candidate.geometry.all_points().first().copied())
            .unwrap_or([0.0, 0.0]);
        BuildingGenerationBasis {
            origin_wgs84,
            orientation_degrees: self.foundation.generation_settings.orientation_degrees,
            blocks_per_meter: self.foundation.generation_settings.blocks_per_meter,
            rule_version: self
                .foundation
                .acquisition
                .as_ref()
                .map(|acquisition| acquisition.manifest.bundle.derivation_rules.clone())
                .unwrap_or_default(),
        }
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
        let boundary_geometry = boundary
            .confirmed_geometry()
            .ok_or("Confirmed Campus Boundary geometry is missing")?;
        let subjects = acquisition
            .observations
            .iter()
            .map(|observation| {
                let subject = observation_dependency_snapshot(observation).basis;
                (subject.upstream_source_record_identity.clone(), subject)
            })
            .collect();
        let coverage_digest = FoundationCategory::ALL
            .into_iter()
            .map(|category| {
                format!(
                    "{category:?}={}",
                    coverage_digest_for(&acquisition.manifest.coverage_report.outcomes, category)
                )
            })
            .collect::<Vec<_>>()
            .join("|");
        let dependencies = ReviewDependencyBasis {
            boundary_digest: geometry_digest(boundary_geometry),
            coverage_digest,
            classification_rules: acquisition.manifest.bundle.classification_rules.clone(),
            assembly_rules: acquisition.manifest.bundle.assembly_rules.clone(),
            conflation_rules: acquisition.manifest.bundle.conflation_rules.clone(),
            derivation_rules: acquisition.manifest.bundle.derivation_rules.clone(),
            subjects,
        };
        Ok(FoundationReviewBasis {
            boundary_result_sha256: boundary.manifest.result_sha256.clone(),
            selected_boundary_id: boundary.selected_candidate_id.clone(),
            acquisition_result_sha256: acquisition.manifest.result_sha256.clone(),
            acquisition_snapshot_identity: if self
                .foundation
                .acquisition_snapshot_identity
                .is_empty()
            {
                acquisition.manifest.result_sha256.clone()
            } else {
                self.foundation.acquisition_snapshot_identity.clone()
            },
            classification_rules: acquisition.manifest.bundle.classification_rules.clone(),
            conflation_rules: acquisition.manifest.bundle.conflation_rules.clone(),
            derivation_rules: acquisition.manifest.bundle.derivation_rules.clone(),
            building_review_sequence: self.foundation.building_review.entries().len() as u64,
            dependencies,
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
        let projection = self.reviewed_projection()?;
        let dependency_basis = self.formal_output_dependency_basis(&projection)?;
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
            dependency_basis,
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
        let projection = self
            .reviewed_projection()
            .map_err(|error| format!("Generate from the current reviewed projection: {error}"))?;
        let dependency_basis = self.formal_output_dependency_basis(&projection)?;
        let building_provenance = projection.building_entities;
        let generated = self
            .foundation
            .generated
            .as_ref()
            .ok_or("Generate the current reviewed projection before export")?;
        if !generated
            .dependency_basis
            .content_equivalent(&dependency_basis)
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
            building_provenance,
            dependency_basis,
        });
        self.workflow.export = DurableTaskState::Confirmed;
        Ok(())
    }

    fn formal_output_dependency_basis(
        &self,
        projection: &ReviewedFoundationProjection,
    ) -> Result<ReviewDependencyBasis, String> {
        let selected_observation_ids = projection
            .selected_features
            .iter()
            .map(|observation| observation.id.as_str())
            .chain(
                projection
                    .building_entities
                    .iter()
                    .flat_map(|entity| entity.evidence_ids.iter().map(String::as_str)),
            )
            .collect::<std::collections::BTreeSet<_>>();
        let mut dependency_basis = self.review_basis()?.dependencies;
        dependency_basis.subjects.retain(|_, subject| {
            selected_observation_ids.contains(subject.observation_id.as_str())
        });
        Ok(dependency_basis)
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

fn validate_conflict_resolution(
    conflict: &FoundationReviewConflict,
    resolution: &ReviewConflictResolution,
) -> Result<(), String> {
    let subjects = conflict
        .subject_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let valid = match resolution {
        ReviewConflictResolution::KeepSeparate => true,
        ReviewConflictResolution::Grouping {
            group_id,
            primary_subject_id,
            supporting_subject_ids,
        } => {
            let supporting = supporting_subject_ids
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            !group_id.trim().is_empty()
                && subjects.contains(primary_subject_id.as_str())
                && !supporting.is_empty()
                && supporting.len() == supporting_subject_ids.len()
                && supporting.is_subset(&subjects)
                && !supporting.contains(primary_subject_id.as_str())
        }
        ReviewConflictResolution::Containment {
            container_id,
            member_id,
            ..
        } => {
            conflict.kind == crate::FoundationReviewConflictKind::Containment
                && container_id != member_id
                && subjects.contains(container_id.as_str())
                && subjects.contains(member_id.as_str())
        }
        ReviewConflictResolution::Naming {
            subject_id,
            display_name,
            evidence_ids,
        } => {
            conflict.kind == crate::FoundationReviewConflictKind::Naming
                && subjects.contains(subject_id.as_str())
                && !display_name.trim().is_empty()
                && !evidence_ids.is_empty()
        }
        ReviewConflictResolution::GeometryRepair {
            subject_id,
            review_geometry_sha256,
        } => {
            subjects.contains(subject_id.as_str())
                && review_geometry_sha256.len() == 64
                && review_geometry_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        }
        ReviewConflictResolution::Attribute {
            subject_id,
            attribute,
            provenance_ids,
        } => {
            conflict.kind == crate::FoundationReviewConflictKind::Attribute
                && subjects.contains(subject_id.as_str())
                && !attribute.trim().is_empty()
                && !provenance_ids.is_empty()
        }
    };
    valid
        .then_some(())
        .ok_or_else(|| "The conflict resolution does not match the exact conflict subjects".into())
}

fn foundation_review_state_from_queue(
    queue: &FoundationReviewQueueProjection,
) -> FoundationReviewState {
    FoundationReviewState {
        candidate_dispositions: queue
            .items
            .iter()
            .map(|item| (item.subject_id.clone(), item.disposition.clone()))
            .collect(),
        acknowledged_gap_ids: queue
            .known_gaps
            .iter()
            .filter(|gap| gap.acknowledged)
            .map(|gap| gap.id.clone())
            .collect(),
        resolved_gap_ids: queue
            .known_gaps
            .iter()
            .filter(|gap| gap.status == KnownFeatureGapStatus::Resolved)
            .map(|gap| gap.id.clone())
            .collect(),
        unresolved_conflict_ids: queue
            .conflicts
            .iter()
            .filter(|conflict| !queue.resolved_conflict_ids.contains(&conflict.id))
            .map(|conflict| conflict.id.clone())
            .collect(),
        coarse_raster_decisions: BTreeMap::new(),
        category_complete: queue.progress.complete,
    }
}

fn apply_foundation_review_action_to_state(
    state: &mut FoundationReviewState,
    action: &FoundationReviewAction,
    subjects: &[String],
) {
    if !matches!(action, FoundationReviewAction::CategoryCompleted) {
        state.category_complete = false;
    }
    match action {
        FoundationReviewAction::Candidate {
            subject_id,
            decision,
        } => {
            let disposition = match decision {
                FoundationCandidateDecision::Accept => CandidateReviewDisposition::Accepted,
                FoundationCandidateDecision::Reject { .. } => CandidateReviewDisposition::Rejected,
                FoundationCandidateDecision::SupportingEvidence { primary_subject_id } => {
                    CandidateReviewDisposition::SupportingEvidence {
                        primary_subject_id: primary_subject_id.clone(),
                    }
                }
                FoundationCandidateDecision::Defer {
                    structured_reason,
                    acknowledged_gap_id,
                } => CandidateReviewDisposition::Deferred {
                    structured_reason: structured_reason.clone(),
                    acknowledged_gap_id: acknowledged_gap_id.clone(),
                },
            };
            state
                .candidate_dispositions
                .insert(subject_id.clone(), disposition);
        }
        FoundationReviewAction::Batch { decision } => {
            let disposition = match decision {
                FoundationBatchDecision::Accept => CandidateReviewDisposition::Accepted,
                FoundationBatchDecision::Reject => CandidateReviewDisposition::Rejected,
            };
            for subject_id in subjects {
                state
                    .candidate_dispositions
                    .insert(subject_id.clone(), disposition.clone());
            }
        }
        FoundationReviewAction::Revoke { subject_id } => {
            state
                .candidate_dispositions
                .insert(subject_id.clone(), CandidateReviewDisposition::Pending);
        }
        FoundationReviewAction::ConflictDeclared { conflict } => {
            state.unresolved_conflict_ids.insert(conflict.id.clone());
        }
        FoundationReviewAction::ConflictResolved {
            conflict_id,
            resolution,
        } => {
            state.unresolved_conflict_ids.remove(conflict_id);
            if let ReviewConflictResolution::Grouping {
                primary_subject_id,
                supporting_subject_ids,
                ..
            } = resolution
            {
                state.candidate_dispositions.insert(
                    primary_subject_id.clone(),
                    CandidateReviewDisposition::Accepted,
                );
                for subject_id in supporting_subject_ids {
                    state.candidate_dispositions.insert(
                        subject_id.clone(),
                        CandidateReviewDisposition::SupportingEvidence {
                            primary_subject_id: primary_subject_id.clone(),
                        },
                    );
                }
            }
        }
        FoundationReviewAction::GapAcknowledged { gap_id } => {
            state.acknowledged_gap_ids.insert(gap_id.clone());
        }
        FoundationReviewAction::GapReopened { gap_id } => {
            state.acknowledged_gap_ids.remove(gap_id);
            state.resolved_gap_ids.remove(gap_id);
        }
        FoundationReviewAction::GapResolved { gap_id, .. } => {
            state.acknowledged_gap_ids.remove(gap_id);
            state.resolved_gap_ids.insert(gap_id.clone());
        }
        FoundationReviewAction::CoarseRasterDecision {
            observation_id,
            decision,
        } => {
            state
                .coarse_raster_decisions
                .insert(observation_id.clone(), decision.clone());
        }
        FoundationReviewAction::CategoryCompleted => {
            state.category_complete = true;
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ChangedBundleRules {
    classification: bool,
    assembly: bool,
    conflation: bool,
    derivation: bool,
}

impl ChangedBundleRules {
    fn any(self) -> bool {
        self.classification || self.assembly || self.conflation || self.derivation
    }

    fn affects_category(self, category: FoundationCategory) -> bool {
        self.classification
            || self.derivation
            || self.conflation
            || (self.assembly && category == FoundationCategory::Building)
    }

    fn invalidates(self, category: FoundationCategory, action: &FoundationReviewAction) -> bool {
        let assembly = self.assembly && category == FoundationCategory::Building;
        match action {
            FoundationReviewAction::ConflictResolved { resolution, .. } => match resolution {
                ReviewConflictResolution::KeepSeparate
                | ReviewConflictResolution::Grouping { .. } => assembly || self.conflation,
                ReviewConflictResolution::Containment { .. } => assembly || self.conflation,
                ReviewConflictResolution::Naming { .. } => self.classification,
                ReviewConflictResolution::GeometryRepair { .. } => self.derivation,
                ReviewConflictResolution::Attribute { .. } => {
                    self.classification || self.derivation
                }
            },
            FoundationReviewAction::ConflictDeclared { conflict } => match conflict.kind {
                crate::FoundationReviewConflictKind::GeometryOverlap
                | crate::FoundationReviewConflictKind::EntityMatch
                | crate::FoundationReviewConflictKind::Containment => {
                    assembly || self.conflation || self.derivation
                }
                crate::FoundationReviewConflictKind::Classification
                | crate::FoundationReviewConflictKind::Attribute
                | crate::FoundationReviewConflictKind::Naming => self.classification,
            },
            FoundationReviewAction::Candidate { .. } | FoundationReviewAction::Batch { .. } => {
                self.classification || assembly || self.conflation || self.derivation
            }
            FoundationReviewAction::CategoryCompleted
            | FoundationReviewAction::CoarseRasterDecision { .. } => true,
            FoundationReviewAction::Revoke { .. }
            | FoundationReviewAction::GapAcknowledged { .. }
            | FoundationReviewAction::GapReopened { .. }
            | FoundationReviewAction::GapResolved { .. } => false,
        }
    }
}

fn refresh_invalidates_operation(
    operation: &FoundationReviewOperation,
    category_changes: &crate::ChangedReviewDependencies,
    rule_changes: &ChangedBundleRules,
    previous_by_id: &BTreeMap<String, String>,
    difference: &FoundationSourceRefreshDifference,
) -> bool {
    if matches!(operation.action, FoundationReviewAction::CategoryCompleted) {
        return category_changes.any() || rule_changes.affects_category(operation.category);
    }
    if matches!(
        operation.action,
        FoundationReviewAction::GapAcknowledged { .. } | FoundationReviewAction::GapReopened { .. }
    ) {
        return category_changes.coverage;
    }
    if rule_changes.invalidates(operation.category, &operation.action) {
        return true;
    }
    let mut evidence_ids = operation.subjects.clone();
    match &operation.action {
        FoundationReviewAction::Candidate { subject_id, .. }
        | FoundationReviewAction::Revoke { subject_id } => {
            evidence_ids.push(subject_id.clone());
        }
        FoundationReviewAction::ConflictDeclared { conflict } => {
            evidence_ids.extend(conflict.subject_ids.iter().cloned());
        }
        FoundationReviewAction::ConflictResolved { resolution, .. } => match resolution {
            ReviewConflictResolution::KeepSeparate => {}
            ReviewConflictResolution::Grouping {
                primary_subject_id,
                supporting_subject_ids,
                ..
            } => {
                evidence_ids.push(primary_subject_id.clone());
                evidence_ids.extend(supporting_subject_ids.iter().cloned());
            }
            ReviewConflictResolution::Containment {
                container_id,
                member_id,
                ..
            } => {
                evidence_ids.push(container_id.clone());
                evidence_ids.push(member_id.clone());
            }
            ReviewConflictResolution::Naming {
                subject_id,
                evidence_ids: name_evidence,
                ..
            } => {
                evidence_ids.push(subject_id.clone());
                evidence_ids.extend(name_evidence.iter().cloned());
            }
            ReviewConflictResolution::GeometryRepair { subject_id, .. }
            | ReviewConflictResolution::Attribute { subject_id, .. } => {
                evidence_ids.push(subject_id.clone());
            }
        },
        FoundationReviewAction::GapResolved {
            evidence_ids: resolution_evidence,
            ..
        } => evidence_ids.extend(resolution_evidence.iter().cloned()),
        _ => {}
    }
    evidence_ids
        .iter()
        .filter_map(|id| previous_by_id.get(id))
        .any(|upstream_identity| {
            difference.observations.iter().any(|change| {
                change.upstream_source_record_identity == *upstream_identity
                    && match change.classification {
                        crate::ObservationRefreshClassification::Withdrawn => true,
                        crate::ObservationRefreshClassification::Changed => {
                            operation_depends_on_change(
                                &operation.action,
                                &change.changed_dependencies,
                            )
                        }
                        crate::ObservationRefreshClassification::Unchanged
                        | crate::ObservationRefreshClassification::Added
                        | crate::ObservationRefreshClassification::CoverageChanged => false,
                    }
            })
        })
}

fn operation_depends_on_change(
    action: &FoundationReviewAction,
    changes: &crate::ChangedReviewDependencies,
) -> bool {
    match action {
        FoundationReviewAction::ConflictResolved { resolution, .. } => match resolution {
            ReviewConflictResolution::KeepSeparate | ReviewConflictResolution::Grouping { .. } => {
                changes.geometry || changes.grouping || changes.rule_version
            }
            ReviewConflictResolution::Containment { .. } => {
                changes.geometry
                    || changes.grouping
                    || changes.containment
                    || changes.boundary
                    || changes.rule_version
            }
            ReviewConflictResolution::Naming { .. } => {
                changes.naming || changes.licence || changes.rule_version
            }
            ReviewConflictResolution::GeometryRepair { .. } => {
                changes.geometry || changes.containment || changes.boundary || changes.rule_version
            }
            ReviewConflictResolution::Attribute { .. } => {
                changes.attribute || changes.licence || changes.rule_version
            }
        },
        FoundationReviewAction::ConflictDeclared { conflict } => match conflict.kind {
            crate::FoundationReviewConflictKind::GeometryOverlap => {
                changes.geometry || changes.grouping || changes.rule_version
            }
            crate::FoundationReviewConflictKind::EntityMatch => {
                changes.geometry || changes.grouping || changes.containment || changes.rule_version
            }
            crate::FoundationReviewConflictKind::Classification => {
                changes.attribute || changes.rule_version
            }
            crate::FoundationReviewConflictKind::Naming => {
                changes.naming || changes.licence || changes.rule_version
            }
            crate::FoundationReviewConflictKind::Attribute => {
                changes.attribute || changes.licence || changes.rule_version
            }
            crate::FoundationReviewConflictKind::Containment => {
                changes.geometry || changes.containment || changes.boundary || changes.rule_version
            }
        },
        FoundationReviewAction::GapAcknowledged { .. }
        | FoundationReviewAction::GapReopened { .. } => changes.coverage,
        FoundationReviewAction::CategoryCompleted
        | FoundationReviewAction::Candidate { .. }
        | FoundationReviewAction::Batch { .. }
        | FoundationReviewAction::Revoke { .. }
        | FoundationReviewAction::GapResolved { .. }
        | FoundationReviewAction::CoarseRasterDecision { .. } => changes.any(),
    }
}

fn remap_review_entry(
    entry: &mut FoundationReviewEntry,
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) -> Result<(), String> {
    remap_ids(
        &mut entry.subjects,
        previous_by_id,
        current_id_by_upstream_identity,
    )?;
    remap_disposition(
        &mut entry.after,
        previous_by_id,
        current_id_by_upstream_identity,
    )?;
    if let Some(before) = &mut entry.before {
        remap_disposition(before, previous_by_id, current_id_by_upstream_identity)?;
    }
    Ok(())
}

fn remap_review_operation(
    operation: &mut FoundationReviewOperation,
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) -> Result<(), String> {
    remap_ids(
        &mut operation.subjects,
        previous_by_id,
        current_id_by_upstream_identity,
    )?;
    match &mut operation.action {
        FoundationReviewAction::Candidate {
            subject_id,
            decision,
        } => {
            remap_id(subject_id, previous_by_id, current_id_by_upstream_identity)?;
            if let FoundationCandidateDecision::SupportingEvidence { primary_subject_id } = decision
            {
                remap_id(
                    primary_subject_id,
                    previous_by_id,
                    current_id_by_upstream_identity,
                )?;
            }
        }
        FoundationReviewAction::Revoke { subject_id } => {
            remap_id(subject_id, previous_by_id, current_id_by_upstream_identity)?;
        }
        FoundationReviewAction::ConflictDeclared { conflict } => {
            remap_ids(
                &mut conflict.subject_ids,
                previous_by_id,
                current_id_by_upstream_identity,
            )?;
        }
        FoundationReviewAction::ConflictResolved { resolution, .. } => match resolution {
            ReviewConflictResolution::KeepSeparate => {}
            ReviewConflictResolution::Grouping {
                primary_subject_id,
                supporting_subject_ids,
                ..
            } => {
                remap_id(
                    primary_subject_id,
                    previous_by_id,
                    current_id_by_upstream_identity,
                )?;
                remap_ids(
                    supporting_subject_ids,
                    previous_by_id,
                    current_id_by_upstream_identity,
                )?;
            }
            ReviewConflictResolution::Containment {
                container_id,
                member_id,
                ..
            } => {
                remap_id(
                    container_id,
                    previous_by_id,
                    current_id_by_upstream_identity,
                )?;
                remap_id(member_id, previous_by_id, current_id_by_upstream_identity)?;
            }
            ReviewConflictResolution::Naming {
                subject_id,
                evidence_ids,
                ..
            } => {
                remap_id(subject_id, previous_by_id, current_id_by_upstream_identity)?;
                remap_ids(
                    evidence_ids,
                    previous_by_id,
                    current_id_by_upstream_identity,
                )?;
            }
            ReviewConflictResolution::GeometryRepair { subject_id, .. }
            | ReviewConflictResolution::Attribute { subject_id, .. } => {
                remap_id(subject_id, previous_by_id, current_id_by_upstream_identity)?;
            }
        },
        FoundationReviewAction::GapResolved { evidence_ids, .. } => {
            remap_ids(
                evidence_ids,
                previous_by_id,
                current_id_by_upstream_identity,
            )?;
        }
        _ => {}
    }
    remap_review_state(
        &mut operation.before,
        previous_by_id,
        current_id_by_upstream_identity,
    );
    remap_review_state(
        &mut operation.after,
        previous_by_id,
        current_id_by_upstream_identity,
    );
    Ok(())
}

fn remap_review_state(
    state: &mut FoundationReviewState,
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) {
    let mut remapped = BTreeMap::new();
    for (mut subject_id, mut disposition) in std::mem::take(&mut state.candidate_dispositions) {
        if remap_id(
            &mut subject_id,
            previous_by_id,
            current_id_by_upstream_identity,
        )
        .is_err()
        {
            continue;
        }
        if let CandidateReviewDisposition::SupportingEvidence { primary_subject_id } =
            &mut disposition
        {
            if remap_id(
                primary_subject_id,
                previous_by_id,
                current_id_by_upstream_identity,
            )
            .is_err()
            {
                continue;
            }
        }
        remapped.insert(subject_id, disposition);
    }
    state.candidate_dispositions = remapped;
}

fn remap_disposition(
    disposition: &mut FoundationReviewDisposition,
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) -> Result<(), String> {
    match disposition {
        FoundationReviewDisposition::SelectedEvidence { evidence_ids } => remap_ids(
            evidence_ids,
            previous_by_id,
            current_id_by_upstream_identity,
        ),
        FoundationReviewDisposition::ReviewedQueue {
            accepted_evidence_ids,
            rejected_evidence_ids,
            deferred_evidence_ids,
            ..
        } => {
            remap_ids(
                accepted_evidence_ids,
                previous_by_id,
                current_id_by_upstream_identity,
            )?;
            remap_ids(
                rejected_evidence_ids,
                previous_by_id,
                current_id_by_upstream_identity,
            )?;
            remap_ids(
                deferred_evidence_ids,
                previous_by_id,
                current_id_by_upstream_identity,
            )
        }
        _ => Ok(()),
    }
}

fn remap_ids(
    ids: &mut [String],
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) -> Result<(), String> {
    for id in ids {
        remap_id(id, previous_by_id, current_id_by_upstream_identity)?;
    }
    Ok(())
}

fn remap_id(
    id: &mut String,
    previous_by_id: &BTreeMap<String, String>,
    current_id_by_upstream_identity: &BTreeMap<String, String>,
) -> Result<(), String> {
    let Some(upstream_identity) = previous_by_id.get(id) else {
        return Ok(());
    };
    let current_id = current_id_by_upstream_identity
        .get(upstream_identity)
        .ok_or_else(|| {
            format!("Cannot carry a review decision for withdrawn evidence {upstream_identity}")
        })?;
    *id = current_id.clone();
    Ok(())
}

fn apply_building_queue_decision(
    review: &mut BuildingEntityReviewLedger,
    observations: &[SourceObservation],
    subject_id: &str,
    decision: FoundationCandidateDecision,
) -> Result<u64, String> {
    let entity = review
        .entities()
        .iter()
        .find(|entity| entity.evidence_ids.iter().any(|id| id == subject_id))
        .cloned()
        .ok_or("The Building queue subject has no stable Building Entity")?;
    match decision {
        FoundationCandidateDecision::Accept => {
            if entity.primary_observation_id == subject_id
                && entity.unresolved_overlap_groups.is_empty()
                && entity.boundary_decision == BuildingBoundaryDecision::RetainWhole
                && entity.name_resolution != BuildingNameResolution::Pending
            {
                return Err("The Building Entity already has this reviewed disposition".into());
            }
            review.record(
                BuildingEntityDecision::ResolveFromQueue {
                    entity_id: entity.id,
                    primary_observation_id: subject_id.to_string(),
                    boundary_decision: BuildingBoundaryDecision::RetainWhole,
                    leave_unnamed_reason: (entity.name_resolution
                        == BuildingNameResolution::Pending)
                        .then(|| {
                            "The reviewer accepted this Building Entity without exclusive name evidence"
                                .into()
                        }),
                },
                observations,
            )
        }
        FoundationCandidateDecision::Reject { .. } => review.record(
            BuildingEntityDecision::SetBoundary {
                entity_id: entity.id,
                decision: BuildingBoundaryDecision::Exclude,
            },
            observations,
        ),
        FoundationCandidateDecision::SupportingEvidence { primary_subject_id } => review.record(
            BuildingEntityDecision::SetPrimary {
                entity_id: entity.id,
                observation_id: primary_subject_id,
            },
            observations,
        ),
        FoundationCandidateDecision::Defer { .. } => Err(
            "A Building Entity deferral must be recorded as a structured entity-level gap".into(),
        ),
    }
}

fn building_entity_name_gap_id(entity_id: &str) -> String {
    format!("gap:building-entity:{entity_id}:name")
}

fn foundation_gap_acknowledged(
    category: FoundationCategory,
    gap_id: &str,
    operations: &[FoundationReviewOperation],
) -> bool {
    crate::foundation_review::known_gap_history(category, gap_id, operations).0
        == KnownFeatureGapStatus::Acknowledged
}

fn foundation_review_action_explanation(action: &FoundationReviewAction) -> Option<String> {
    match action {
        FoundationReviewAction::Candidate {
            decision: FoundationCandidateDecision::Reject { reason },
            ..
        } if !reason.trim().is_empty() => Some(reason.clone()),
        FoundationReviewAction::Candidate {
            decision:
                FoundationCandidateDecision::Defer {
                    structured_reason, ..
                },
            ..
        } => Some(structured_reason.clone()),
        FoundationReviewAction::ConflictDeclared { conflict } => Some(conflict.explanation.clone()),
        FoundationReviewAction::ConflictResolved { resolution, .. } => {
            Some(format!("Resolved as {resolution:?}"))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedFoundationProjection {
    pub boundary: SourceGeometry,
    pub selected_features: Vec<SourceObservation>,
    pub reviewed_feature_resolutions: Vec<crate::ReviewedFeatureResolution>,
    pub building_entities: Vec<ReviewedBuildingEntity>,
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
    if let Some(acquisition) = &project.foundation.acquisition {
        project
            .foundation
            .building_review
            .validate_against(&acquisition.observations)?;
    } else if !project.foundation.building_review.is_empty() {
        return Err("Building Entity review requires pinned acquisition evidence".into());
    }
    crate::validate_persisted_coarse_raster_runs(&project.foundation.coarse_raster_runs)?;
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
