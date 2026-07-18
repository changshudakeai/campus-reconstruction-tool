use super::*;
use crate::{
    decode_schema1_project, CampusProject, FoundationSourceProvider, FoundationSourceStatus,
    ReviewDecision,
};

const LEGACY_MIGRATION_STATE_KEY: &str = "legacyMigration";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationDisposition {
    Preserved,
    Transformed,
    Quarantined,
    Omitted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReportEntry {
    pub subject: String,
    pub disposition: MigrationDisposition,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Schema1MigrationReport {
    pub entries: Vec<MigrationReportEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LegacySourceFormat {
    NativeSchema1,
    LegacyWebPortable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigrationLineage {
    pub source_schema_version: u32,
    pub source_format: LegacySourceFormat,
    pub source_file_name: String,
    pub backup_file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAssertion {
    pub subject_id: String,
    pub decision: ReviewDecision,
    pub source_snapshot_id: Option<String>,
    pub lineage: LegacyMigrationLineage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NeedsReconfirmation {
    pub subject_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LegacyEvidenceKind {
    ManualGeometry,
    ScreenshotDerivedGeometry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyEvidence {
    pub subject_id: String,
    pub kind: LegacyEvidenceKind,
    pub lineage: LegacyMigrationLineage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalLegacyArtifact {
    pub kind: String,
    pub legacy_path: String,
    pub compatibility_status: String,
    pub satisfies_v11_completion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigrationState {
    pub lineage: LegacyMigrationLineage,
    pub source_project: Value,
    pub legacy_assertions: Vec<LegacyAssertion>,
    pub needs_reconfirmation: Vec<NeedsReconfirmation>,
    pub legacy_evidence: Vec<LegacyEvidence>,
    pub historical_artifacts: Vec<HistoricalLegacyArtifact>,
    pub report: Schema1MigrationReport,
}

#[derive(Debug)]
pub struct Schema1MigrationOutcome {
    pub project: Schema2Project,
    pub backup_path: PathBuf,
    pub report: Schema1MigrationReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationFaultPoint {
    AfterBackupWrite,
    AfterCandidateStageWrite,
    AfterCandidateValidation,
    BeforeProjectReplace,
    AfterProjectReplace,
    AfterIndexReplace,
}

impl MigrationFaultPoint {
    pub const ALL: [Self; 6] = [
        Self::AfterBackupWrite,
        Self::AfterCandidateStageWrite,
        Self::AfterCandidateValidation,
        Self::BeforeProjectReplace,
        Self::AfterProjectReplace,
        Self::AfterIndexReplace,
    ];
}

impl Schema2Project {
    pub fn legacy_migration(&self) -> Result<Option<LegacyMigrationState>, String> {
        self.optional_state
            .get(LEGACY_MIGRATION_STATE_KEY)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("Invalid legacy migration state: {error}"))
    }
}

impl CampusProjectLibrary {
    pub fn schema1_backup_path(source_path: impl AsRef<Path>) -> Result<PathBuf, String> {
        sibling_path(source_path.as_ref(), "schema-1-backup.campus.json")
    }

    pub fn inject_next_migration_failure(&mut self, point: MigrationFaultPoint) {
        self.next_migration_fault = Some(point);
    }

    pub fn migrate_managed_schema1_project(
        &mut self,
        source_path: impl AsRef<Path>,
        actor: InstallationId,
    ) -> Result<Schema1MigrationOutcome, String> {
        let source_path = source_path.as_ref();
        let managed_relative_path = self.validate_schema1_source_path(source_path)?;
        let original_bytes = fs::read(source_path).map_err(|error| error.to_string())?;
        let backup_path = Self::schema1_backup_path(source_path)?;
        write_explicit_backup(&backup_path, &original_bytes)?;
        self.fail_migration_at(MigrationFaultPoint::AfterBackupWrite)?;

        let backup_bytes = fs::read(&backup_path).map_err(|error| error.to_string())?;
        let source_value: Value =
            serde_json::from_slice(&backup_bytes).map_err(|error| error.to_string())?;
        let legacy_project = decode_schema1_project(&backup_bytes)?;
        let source_format = source_format(&source_value)?;
        validate_legacy_paths(&legacy_project)?;
        self.ensure_name_available(&legacy_project.name, None)?;

        let lineage = LegacyMigrationLineage {
            source_schema_version: legacy_project.schema_version,
            source_format,
            source_file_name: file_name(source_path)?,
            backup_file_name: file_name(&backup_path)?,
        };
        let migration =
            build_legacy_migration_state(&legacy_project, source_value, lineage.clone());
        let report = migration.report.clone();
        let mut candidate =
            build_schema2_candidate(&self.campus_target_id, &legacy_project, actor)?;
        candidate.optional_state.insert(
            LEGACY_MIGRATION_STATE_KEY.into(),
            serde_json::to_value(&migration).map_err(|error| error.to_string())?,
        );
        validate_schema2_project(&candidate)?;

        let candidate_bytes =
            serde_json::to_vec_pretty(&candidate).map_err(|error| error.to_string())?;
        let stage_path = sibling_path(source_path, "schema-2-migration-stage.json")?;
        write_and_sync(&stage_path, &candidate_bytes)?;
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::AfterCandidateStageWrite) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        let staged_bytes = fs::read(&stage_path).map_err(|error| error.to_string())?;
        let staged_project = decode_schema2_project(&staged_bytes)
            .map_err(|error| format!("Staged migration candidate is invalid: {error}"))?;
        if staged_project != candidate {
            remove_file_if_present(&stage_path);
            return Err("Staged migration candidate changed during validation".into());
        }
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::AfterCandidateValidation) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }

        let old_index = self.index.clone();
        let index_path = self.root.join(LIBRARY_INDEX_FILE);
        let old_index_bytes = read_optional_file(&index_path)?;
        let mut next_index = old_index.clone();
        next_index
            .projects
            .push(record_from_project(&candidate, managed_relative_path, None));
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::BeforeProjectReplace) {
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        atomic_write_bytes(source_path, &candidate_bytes)?;
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::AfterProjectReplace) {
            rollback_migration(
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            );
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        if let Err(error) = atomic_write_json(&index_path, &next_index) {
            rollback_migration(
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            );
            remove_file_if_present(&stage_path);
            return Err(error);
        }
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::AfterIndexReplace) {
            rollback_migration(
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            );
            remove_file_if_present(&stage_path);
            return Err(error);
        }

        self.index = next_index;
        remove_file_if_present(&stage_path);
        Ok(Schema1MigrationOutcome {
            project: candidate,
            backup_path,
            report,
        })
    }

    fn validate_schema1_source_path(&self, source_path: &Path) -> Result<String, String> {
        if !source_path.is_file() {
            return Err("Managed schema-1 project does not exist".into());
        }
        let root = fs::canonicalize(&self.root).map_err(|error| error.to_string())?;
        let source = fs::canonicalize(source_path).map_err(|error| error.to_string())?;
        let relative = source
            .strip_prefix(&root)
            .map_err(|_| "Schema-1 migration source is outside the Campus Project Library")?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || relative == Path::new(LIBRARY_INDEX_FILE)
        {
            return Err("Schema-1 migration source is not a safe managed project path".into());
        }
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }

    fn fail_migration_at(&mut self, point: MigrationFaultPoint) -> Result<(), String> {
        if self.next_migration_fault == Some(point) {
            self.next_migration_fault = None;
            Err(format!("injected migration failure at {point:?}"))
        } else {
            Ok(())
        }
    }
}

fn build_schema2_candidate(
    campus_target_id: &str,
    legacy: &CampusProject,
    actor: InstallationId,
) -> Result<Schema2Project, String> {
    let canonical_name = legacy
        .campus_target
        .as_ref()
        .map(|target| target.name.as_str())
        .unwrap_or(&legacy.campus_name);
    let anchor = legacy
        .campus_target
        .as_ref()
        .map(|target| [target.wgs84.lng, target.wgs84.lat])
        .unwrap_or([legacy.map_view.center.lng, legacy.map_view.center.lat]);
    let scope = CampusScope::new(campus_target_id, canonical_name, anchor)?;
    let mut candidate = Schema2Project::new(scope, legacy.name.clone(), actor)?;
    candidate.workflow.campus_target = if legacy.campus_target.is_some() {
        DurableTaskState::Confirmed
    } else {
        DurableTaskState::Pending
    };
    candidate.foundation.generation_settings =
        migrated_generation_settings(&legacy.foundation_style_pack, legacy);
    Ok(candidate)
}

fn migrated_generation_settings(
    style_pack: &crate::FoundationStylePack,
    legacy: &CampusProject,
) -> FoundationGenerationSettings {
    let mut settings = FoundationGenerationSettings {
        orientation_degrees: legacy.orientation_degrees,
        blocks_per_meter: legacy.blocks_per_meter,
        style_id: style_pack.id.clone(),
        ..FoundationGenerationSettings::default()
    };
    settings.surface_block = style_pack
        .features
        .get("campus")
        .and_then(|style| style.blocks.first())
        .cloned()
        .unwrap_or(settings.surface_block);
    for (category, legacy_key) in [
        (FoundationCategory::Building, "building"),
        (FoundationCategory::Circulation, "road"),
        (FoundationCategory::Water, "water"),
        (FoundationCategory::Vegetation, "vegetation"),
        (FoundationCategory::Sports, "sports"),
    ] {
        let Some(legacy_style) = style_pack.features.get(legacy_key) else {
            continue;
        };
        let Some(current_style) = settings.generators.get_mut(&category) else {
            continue;
        };
        current_style.generator_id = legacy_style.generator.clone();
        if let Some(block) = legacy_style.blocks.first() {
            current_style.block = block.clone();
        }
    }
    settings
}

fn build_legacy_migration_state(
    legacy: &CampusProject,
    source_project: Value,
    lineage: LegacyMigrationLineage,
) -> LegacyMigrationState {
    let mut report = Schema1MigrationReport::default();
    let mut needs_reconfirmation = BTreeMap::<String, String>::new();
    let mut legacy_assertions = Vec::new();
    let mut legacy_evidence = Vec::new();

    report_entry(
        &mut report,
        "campus-target",
        MigrationDisposition::Transformed,
        "Legacy Campus Target evidence is retained while schema 2 uses the confirmed library scope",
    );
    if legacy.campus_target.is_none() {
        needs_reconfirmation.insert(
            "campus-target:identity".into(),
            "missing-legacy-campus-target-evidence".into(),
        );
    }
    if legacy.boundary.is_empty() {
        report_entry(
            &mut report,
            "boundary",
            MigrationDisposition::Omitted,
            "The legacy project contained no Campus Boundary",
        );
    } else {
        report_entry(
            &mut report,
            "boundary",
            MigrationDisposition::Quarantined,
            "Legacy boundary geometry is retained for review but is not controlled evidence",
        );
        needs_reconfirmation.insert(
            "boundary:campus".into(),
            "legacy-boundary-requires-controlled-evidence".into(),
        );
    }

    report_entry(
        &mut report,
        "orientation-and-scale",
        MigrationDisposition::Transformed,
        "Orientation and scale are mapped to schema-2 generation settings",
    );
    report_entry(
        &mut report,
        "source-snapshots",
        MigrationDisposition::Preserved,
        "Legacy source snapshots and provider outcomes are retained with legacy lineage",
    );

    let candidate_by_id = legacy
        .candidates
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    let snapshot_by_id = legacy
        .foundation_source_snapshots
        .iter()
        .map(|snapshot| (snapshot.id.as_str(), snapshot))
        .collect::<BTreeMap<_, _>>();
    let mut ledger_subjects = BTreeMap::<String, ()>::new();
    for entry in &legacy.foundation_review_ledger {
        let subject_id = format!("candidate:{}", entry.candidate_id);
        ledger_subjects.insert(entry.candidate_id.clone(), ());
        let Some(candidate) = candidate_by_id.get(entry.candidate_id.as_str()) else {
            needs_reconfirmation.insert(subject_id, "missing-legacy-subject".into());
            continue;
        };
        if candidate.review != entry.decision {
            needs_reconfirmation.insert(subject_id, "contradictory-legacy-decision".into());
            continue;
        }
        let source_snapshot_id = entry
            .source_snapshot_id
            .as_deref()
            .or(candidate.source_snapshot_id.as_deref());
        let source_is_complete = source_snapshot_id
            .and_then(|id| snapshot_by_id.get(id))
            .is_some_and(|snapshot| snapshot.status == FoundationSourceStatus::Complete);
        if source_is_complete {
            legacy_assertions.push(LegacyAssertion {
                subject_id,
                decision: entry.decision,
                source_snapshot_id: source_snapshot_id.map(str::to_owned),
                lineage: lineage.clone(),
            });
        } else {
            needs_reconfirmation.insert(subject_id, "missing-source-snapshot".into());
        }
    }
    for candidate in &legacy.candidates {
        if candidate.review == ReviewDecision::Pending
            || ledger_subjects.contains_key(&candidate.id)
        {
            continue;
        }
        needs_reconfirmation.insert(
            format!("candidate:{}", candidate.id),
            "missing-source-snapshot".into(),
        );
    }
    for suppression in &legacy.building_suppressions {
        legacy_assertions.push(LegacyAssertion {
            subject_id: format!("suppression:{}", suppression.source_id),
            decision: ReviewDecision::Rejected,
            source_snapshot_id: None,
            lineage: lineage.clone(),
        });
    }
    legacy_assertions.sort_by(|left, right| left.subject_id.cmp(&right.subject_id));
    report_entry(
        &mut report,
        "review-decisions",
        if needs_reconfirmation
            .keys()
            .any(|subject| subject.starts_with("candidate:"))
        {
            MigrationDisposition::Quarantined
        } else {
            MigrationDisposition::Transformed
        },
        "Provable decisions become legacy assertions; uncertain subjects require reconfirmation",
    );

    for feature in &legacy.features {
        if feature.source_id.is_none() {
            let subject_id = format!("feature:{}", feature.id);
            needs_reconfirmation.insert(
                subject_id.clone(),
                "manual-or-screenshot-derived-geometry".into(),
            );
            legacy_evidence.push(LegacyEvidence {
                subject_id,
                kind: LegacyEvidenceKind::ManualGeometry,
                lineage: lineage.clone(),
            });
        }
    }
    for snapshot in &legacy.foundation_source_snapshots {
        if snapshot.provider == FoundationSourceProvider::VisualFeatureProvider {
            let subject_id = format!("source-snapshot:{}", snapshot.id);
            needs_reconfirmation.insert(
                subject_id.clone(),
                "manual-or-screenshot-derived-geometry".into(),
            );
            legacy_evidence.push(LegacyEvidence {
                subject_id,
                kind: LegacyEvidenceKind::ScreenshotDerivedGeometry,
                lineage: lineage.clone(),
            });
        }
    }
    if legacy.visual_capture_path.is_some() {
        let subject_id = "visual-capture:legacy".to_string();
        needs_reconfirmation.insert(
            subject_id.clone(),
            "manual-or-screenshot-derived-geometry".into(),
        );
        legacy_evidence.push(LegacyEvidence {
            subject_id,
            kind: LegacyEvidenceKind::ScreenshotDerivedGeometry,
            lineage: lineage.clone(),
        });
    }
    legacy_evidence.sort_by(|left, right| left.subject_id.cmp(&right.subject_id));

    for (subject, is_empty) in [
        ("features", legacy.features.is_empty()),
        ("building-slots", legacy.building_slots.is_empty()),
        (
            "campus-building-directory",
            legacy.building_directory.is_empty(),
        ),
        (
            "building-suppressions",
            legacy.building_suppressions.is_empty(),
        ),
        ("detailed-building-state", false),
        ("foundation-styles", false),
    ] {
        report_entry(
            &mut report,
            subject,
            if is_empty {
                MigrationDisposition::Omitted
            } else {
                MigrationDisposition::Preserved
            },
            if is_empty {
                "The legacy project contained no records"
            } else {
                "Supported legacy records are retained in the schema-2 migration envelope"
            },
        );
    }

    let historical_artifacts = collect_historical_artifacts(legacy);
    report_entry(
        &mut report,
        "historical-generated-artifacts",
        if historical_artifacts.is_empty() {
            MigrationDisposition::Omitted
        } else {
            MigrationDisposition::Preserved
        },
        if historical_artifacts.is_empty() {
            "The legacy project referenced no generated artifacts"
        } else {
            "Legacy generated artifacts remain discoverable as compatibility-unverified history"
        },
    );
    report_entry(
        &mut report,
        "legacy-generated-completion",
        MigrationDisposition::Omitted,
        "Legacy generated artifacts cannot satisfy V1.1 completion",
    );

    let needs_reconfirmation = needs_reconfirmation
        .into_iter()
        .map(|(subject_id, reason)| NeedsReconfirmation { subject_id, reason })
        .collect();
    report
        .entries
        .sort_by(|left, right| left.subject.cmp(&right.subject));
    LegacyMigrationState {
        lineage,
        source_project,
        legacy_assertions,
        needs_reconfirmation,
        legacy_evidence,
        historical_artifacts,
        report,
    }
}

fn collect_historical_artifacts(legacy: &CampusProject) -> Vec<HistoricalLegacyArtifact> {
    let mut artifacts = Vec::new();
    if let Some(path) = &legacy.foundation_preview_path {
        push_artifact(&mut artifacts, "foundation-preview", path);
    }
    if let Some(path) = &legacy.detailed.generated_path {
        push_artifact(&mut artifacts, "detailed-generated-output", path);
    }
    for refinement in &legacy.detailed.refinements {
        if !refinement.generated_path.as_os_str().is_empty() {
            push_artifact(
                &mut artifacts,
                "detailed-refinement-output",
                &refinement.generated_path,
            );
        }
    }
    artifacts.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(left.legacy_path.cmp(&right.legacy_path))
    });
    artifacts
}

fn push_artifact(artifacts: &mut Vec<HistoricalLegacyArtifact>, kind: &str, path: &Path) {
    artifacts.push(HistoricalLegacyArtifact {
        kind: kind.into(),
        legacy_path: path.to_string_lossy().into_owned(),
        compatibility_status: "compatibility-unverified".into(),
        satisfies_v11_completion: false,
    });
}

fn report_entry(
    report: &mut Schema1MigrationReport,
    subject: &str,
    disposition: MigrationDisposition,
    reason: &str,
) {
    report.entries.push(MigrationReportEntry {
        subject: subject.into(),
        disposition,
        reason: reason.into(),
    });
}

fn source_format(value: &Value) -> Result<LegacySourceFormat, String> {
    if value.get("schemaVersion").and_then(Value::as_u64) == Some(1) {
        Ok(LegacySourceFormat::NativeSchema1)
    } else if value
        .pointer("/project/schemaVersion")
        .and_then(Value::as_str)
        == Some("1.0")
    {
        Ok(LegacySourceFormat::LegacyWebPortable)
    } else {
        Err("Input is not a supported schema-1 project".into())
    }
}

fn validate_legacy_paths(project: &CampusProject) -> Result<(), String> {
    for asset in &project.detailed.evidence_assets {
        let path = Path::new(&asset.relative_path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "Legacy evidence asset has an unsafe relative path: {}",
                asset.relative_path
            ));
        }
    }
    Ok(())
}

fn write_explicit_backup(path: &Path, source_bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        let existing = fs::read(path).map_err(|error| error.to_string())?;
        if existing != source_bytes {
            return Err("Existing schema-1 migration backup does not match the source".into());
        }
        return Ok(());
    }
    atomic_write_bytes(path, source_bytes)
}

fn rollback_migration(
    source_path: &Path,
    source_bytes: &[u8],
    index_path: &Path,
    old_index_bytes: Option<&[u8]>,
) {
    let _ = atomic_write_bytes(source_path, source_bytes);
    let _ = restore_optional_file(index_path, old_index_bytes);
}

fn sibling_path(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let file_name = file_name(path)?;
    Ok(path.with_file_name(format!("{file_name}.{suffix}")))
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "Managed project path requires a UTF-8 file name".into())
}
