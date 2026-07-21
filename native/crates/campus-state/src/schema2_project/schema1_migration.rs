use super::*;
use crate::{decode_schema1_project, CampusProject, ReviewDecision};

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
    pub reason: ReconfirmationReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReconfirmationReason {
    MissingLegacyCampusTargetEvidence,
    LegacyBoundaryRequiresControlledEvidence,
    MissingLegacySubject,
    ContradictoryLegacyDecision,
    UnsupportedLegacyDecision,
    MissingSourceSnapshot,
    ManualOrScreenshotDerivedGeometry,
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
    pub kind: HistoricalArtifactKind,
    pub legacy_path: String,
    pub compatibility_status: LegacyArtifactCompatibility,
    pub satisfies_v11_completion: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalArtifactKind {
    FoundationPreview,
    DetailedGeneratedOutput,
    DetailedRefinementOutput,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LegacyArtifactCompatibility {
    CompatibilityUnverified,
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

impl LegacyMigrationState {
    pub fn decode_preserved_project(&self) -> Result<CampusProject, String> {
        let bytes = serde_json::to_vec(&self.source_project).map_err(|error| error.to_string())?;
        decode_schema1_project(&bytes)
    }
}

#[derive(Debug)]
pub struct Schema1MigrationOutcome {
    pub project: Schema2Project,
    pub backup_path: PathBuf,
    pub report: Schema1MigrationReport,
}

pub(super) fn portable_schema1_scope(bytes: &[u8]) -> Result<CampusScope, String> {
    let legacy = decode_schema1_project(bytes)?;
    if let Some(target) = legacy.campus_target {
        let mut scope = CampusScope::new(
            format!("legacy-gaode:{}", target.poi_id),
            target.name,
            [target.wgs84.lng, target.wgs84.lat],
        )?;
        if !target.poi_id.trim().is_empty() {
            scope = scope.with_gaode_poi_id(target.poi_id)?;
        }
        Ok(scope)
    } else {
        CampusScope::new(
            "legacy-unmatched-campus",
            legacy.campus_name,
            [legacy.map_view.center.lng, legacy.map_view.center.lat],
        )
    }
}

pub(super) fn migrate_portable_schema1_copy(
    bytes: &[u8],
    selected_scope: CampusScope,
    actor: InstallationId,
) -> Result<Schema2Project, String> {
    let mut source_project: Value =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let source_format = source_format(&source_project)?;
    validate_legacy_paths(&source_project)?;
    super::portable_project::strip_non_portable_state(&mut source_project);
    let legacy = decode_schema1_project(bytes)?;
    let lineage = LegacyMigrationLineage {
        source_schema_version: legacy.schema_version,
        source_format,
        source_file_name: "portable-project-source.campus.json".into(),
        backup_file_name: "immutable-temporary-copy.campus.json".into(),
    };
    let migration = build_legacy_migration_state(&legacy, &source_project, lineage);
    let mut candidate = build_schema2_candidate(selected_scope.target_id(), &legacy, actor)?;
    candidate.campus_scope = selected_scope;
    candidate.optional_state.insert(
        LEGACY_MIGRATION_STATE_KEY.into(),
        serde_json::to_value(migration).map_err(|error| error.to_string())?,
    );
    validate_schema2_project(&candidate)?;
    Ok(candidate)
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
        validate_legacy_paths(&source_value)?;
        self.ensure_name_available(&legacy_project.name, None)?;

        let lineage = LegacyMigrationLineage {
            source_schema_version: legacy_project.schema_version,
            source_format,
            source_file_name: file_name(source_path)?,
            backup_file_name: file_name(&backup_path)?,
        };
        let migration =
            build_legacy_migration_state(&legacy_project, &source_value, lineage.clone());
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
            remove_file_if_present(&stage_path);
            return Err(rollback_error(
                error,
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            ));
        }
        if let Err(error) = atomic_write_json(&index_path, &next_index) {
            remove_file_if_present(&stage_path);
            return Err(rollback_error(
                error,
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            ));
        }
        if let Err(error) = self.fail_migration_at(MigrationFaultPoint::AfterIndexReplace) {
            remove_file_if_present(&stage_path);
            return Err(rollback_error(
                error,
                source_path,
                &original_bytes,
                &index_path,
                old_index_bytes.as_deref(),
            ));
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
    candidate.retained_detailed = legacy.detailed.clone();
    candidate.retained_detailed.generated_path = None;
    candidate.retained_detailed_measurements = legacy
        .building_slots
        .iter()
        .map(|slot| {
            (
                slot.id.clone(),
                DetailedBuildingMeasurements {
                    height_m: slot.height_m,
                    floors: slot.floors,
                    roof_shape: slot.roof_shape.clone(),
                },
            )
        })
        .collect();
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
    source_project: &Value,
    lineage: LegacyMigrationLineage,
) -> LegacyMigrationState {
    let mut report = Schema1MigrationReport::default();
    let mut needs_reconfirmation = BTreeMap::<String, ReconfirmationReason>::new();
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
            ReconfirmationReason::MissingLegacyCampusTargetEvidence,
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
            ReconfirmationReason::LegacyBoundaryRequiresControlledEvidence,
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
    let snapshot_by_id = raw_foundation_source_snapshots(source_project)
        .filter_map(|snapshot| {
            Some((
                snapshot.get("id")?.as_str()?.to_owned(),
                snapshot.get("status").and_then(Value::as_str) == Some("complete"),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut ledger_subjects = BTreeMap::<String, ()>::new();
    for entry in raw_foundation_review_entries(source_project) {
        let Some(candidate_id) = entry.get("candidateId").and_then(Value::as_str) else {
            continue;
        };
        let Some(decision) = entry
            .get("decision")
            .cloned()
            .and_then(|value| serde_json::from_value::<ReviewDecision>(value).ok())
        else {
            continue;
        };
        ledger_subjects.insert(candidate_id.to_owned(), ());
        let source_snapshot_id = entry
            .get("sourceSnapshotId")
            .and_then(Value::as_str)
            .or_else(|| raw_candidate_source_snapshot_id(source_project, candidate_id));
        let source_is_complete = source_snapshot_id
            .and_then(|id| snapshot_by_id.get(id))
            .copied()
            .unwrap_or(false);

        let Some(candidate) = candidate_by_id.get(candidate_id) else {
            let suppression_subject = format!("suppression:{candidate_id}");
            if legacy
                .building_suppressions
                .iter()
                .any(|suppression| suppression.source_id == candidate_id)
            {
                if decision != ReviewDecision::Rejected {
                    needs_reconfirmation.insert(
                        suppression_subject,
                        ReconfirmationReason::ContradictoryLegacyDecision,
                    );
                } else if source_is_complete {
                    legacy_assertions.push(LegacyAssertion {
                        subject_id: suppression_subject,
                        decision,
                        source_snapshot_id: source_snapshot_id.map(str::to_owned),
                        lineage: lineage.clone(),
                    });
                } else {
                    needs_reconfirmation.insert(
                        suppression_subject,
                        ReconfirmationReason::MissingSourceSnapshot,
                    );
                }
            } else {
                needs_reconfirmation.insert(
                    format!("candidate:{candidate_id}"),
                    ReconfirmationReason::MissingLegacySubject,
                );
            }
            continue;
        };
        let subject_id = format!("candidate:{candidate_id}");
        if candidate.review != decision {
            needs_reconfirmation.insert(
                subject_id,
                ReconfirmationReason::ContradictoryLegacyDecision,
            );
            continue;
        }
        if source_is_complete {
            legacy_assertions.push(LegacyAssertion {
                subject_id,
                decision,
                source_snapshot_id: source_snapshot_id.map(str::to_owned),
                lineage: lineage.clone(),
            });
        } else {
            needs_reconfirmation.insert(subject_id, ReconfirmationReason::MissingSourceSnapshot);
        }
    }
    if let Some(reviews) = raw_portable_reviews(source_project) {
        for (candidate_id, raw_decision) in reviews {
            ledger_subjects.insert(candidate_id.clone(), ());
            let subject_id = format!("candidate:{candidate_id}");
            let Ok(decision) = serde_json::from_value::<ReviewDecision>(raw_decision.clone())
            else {
                needs_reconfirmation
                    .insert(subject_id, ReconfirmationReason::UnsupportedLegacyDecision);
                continue;
            };
            let Some(candidate) = candidate_by_id.get(candidate_id.as_str()) else {
                needs_reconfirmation.insert(subject_id, ReconfirmationReason::MissingLegacySubject);
                continue;
            };
            if candidate.review != decision {
                needs_reconfirmation.insert(
                    subject_id,
                    ReconfirmationReason::ContradictoryLegacyDecision,
                );
                continue;
            }
            let source_snapshot_id = raw_candidate_source_snapshot_id(source_project, candidate_id);
            let source_is_complete = source_snapshot_id
                .and_then(|id| snapshot_by_id.get(id))
                .copied()
                .unwrap_or(false);
            if source_is_complete {
                legacy_assertions.push(LegacyAssertion {
                    subject_id,
                    decision,
                    source_snapshot_id: source_snapshot_id.map(str::to_owned),
                    lineage: lineage.clone(),
                });
            } else {
                needs_reconfirmation
                    .insert(subject_id, ReconfirmationReason::MissingSourceSnapshot);
            }
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
            ReconfirmationReason::MissingSourceSnapshot,
        );
    }
    for suppression in &legacy.building_suppressions {
        let subject_id = format!("suppression:{}", suppression.source_id);
        if !legacy_assertions
            .iter()
            .any(|assertion| assertion.subject_id == subject_id)
            && !needs_reconfirmation.contains_key(&subject_id)
        {
            needs_reconfirmation.insert(subject_id, ReconfirmationReason::MissingSourceSnapshot);
        }
    }
    legacy_assertions.sort_by(|left, right| left.subject_id.cmp(&right.subject_id));
    report_entry(
        &mut report,
        "review-decisions",
        if needs_reconfirmation
            .keys()
            .any(|subject| subject.starts_with("candidate:") || subject.starts_with("suppression:"))
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
                ReconfirmationReason::ManualOrScreenshotDerivedGeometry,
            );
            legacy_evidence.push(LegacyEvidence {
                subject_id,
                kind: LegacyEvidenceKind::ManualGeometry,
                lineage: lineage.clone(),
            });
        }
    }
    for snapshot in raw_foundation_source_snapshots(source_project) {
        if snapshot.get("provider").and_then(Value::as_str) == Some("visual_feature_provider") {
            let Some(snapshot_id) = snapshot.get("id").and_then(Value::as_str) else {
                continue;
            };
            let subject_id = format!("source-snapshot:{snapshot_id}");
            needs_reconfirmation.insert(
                subject_id.clone(),
                ReconfirmationReason::ManualOrScreenshotDerivedGeometry,
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
            ReconfirmationReason::ManualOrScreenshotDerivedGeometry,
        );
        legacy_evidence.push(LegacyEvidence {
            subject_id,
            kind: LegacyEvidenceKind::ScreenshotDerivedGeometry,
            lineage: lineage.clone(),
        });
    }
    legacy_evidence.sort_by(|left, right| left.subject_id.cmp(&right.subject_id));
    append_record_level_report(&mut report, legacy, source_project, &needs_reconfirmation);

    let raw_root = raw_native_or_web_root(source_project);
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
        (
            "detailed-building-state",
            raw_root.get("detailed").is_none(),
        ),
        (
            "foundation-styles",
            raw_root.get("foundationStylePack").is_none()
                && raw_root
                    .pointer("/foundation/foundationStylePack")
                    .is_none(),
        ),
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
    for (index, artifact) in historical_artifacts.iter().enumerate() {
        report_entry(
            &mut report,
            &format!(
                "historical-artifact:{}:record-{}",
                historical_artifact_label(artifact.kind),
                index + 1
            ),
            MigrationDisposition::Preserved,
            "The generated artifact reference is retained as compatibility-unverified history",
        );
    }
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
        source_project: source_project.clone(),
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
        push_artifact(
            &mut artifacts,
            HistoricalArtifactKind::FoundationPreview,
            path,
        );
    }
    if let Some(path) = &legacy.detailed.generated_path {
        push_artifact(
            &mut artifacts,
            HistoricalArtifactKind::DetailedGeneratedOutput,
            path,
        );
    }
    for refinement in &legacy.detailed.refinements {
        if !refinement.generated_path.as_os_str().is_empty() {
            push_artifact(
                &mut artifacts,
                HistoricalArtifactKind::DetailedRefinementOutput,
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

fn append_record_level_report(
    report: &mut Schema1MigrationReport,
    legacy: &CampusProject,
    source_project: &Value,
    needs_reconfirmation: &BTreeMap<String, ReconfirmationReason>,
) {
    for (index, snapshot) in raw_foundation_source_snapshots(source_project).enumerate() {
        let id = snapshot
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("raw-record-{}", index + 1));
        let has_stable_id = snapshot.get("id").and_then(Value::as_str).is_some();
        report_entry(
            report,
            &format!("source-snapshot:{id}"),
            if has_stable_id {
                MigrationDisposition::Preserved
            } else {
                MigrationDisposition::Quarantined
            },
            if has_stable_id {
                "The provider outcome and its legacy lineage are retained verbatim"
            } else {
                "The raw source snapshot is retained but lacks a stable identifier"
            },
        );
    }
    for (index, entry) in raw_foundation_review_entries(source_project).enumerate() {
        let candidate_id = entry
            .get("candidateId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("raw-record-{}", index + 1));
        let reconfirmation_reason =
            review_reconfirmation_reason(needs_reconfirmation, &candidate_id);
        report_entry(
            report,
            &format!("review-ledger:{candidate_id}:record-{}", index + 1),
            if reconfirmation_reason.is_some() {
                MigrationDisposition::Quarantined
            } else {
                MigrationDisposition::Transformed
            },
            reconfirmation_reason
                .map(reconfirmation_reason_report)
                .unwrap_or("The legacy review record becomes a lineage-bearing assertion"),
        );
    }
    if let Some(reviews) = raw_portable_reviews(source_project) {
        for candidate_id in reviews.keys() {
            let reconfirmation_reason =
                review_reconfirmation_reason(needs_reconfirmation, candidate_id);
            report_entry(
                report,
                &format!("review-ledger:{candidate_id}"),
                if reconfirmation_reason.is_some() {
                    MigrationDisposition::Quarantined
                } else {
                    MigrationDisposition::Transformed
                },
                reconfirmation_reason
                    .map(reconfirmation_reason_report)
                    .unwrap_or("The portable review decision becomes a lineage-bearing assertion"),
            );
        }
    }
    for candidate in &legacy.candidates {
        let subject = format!("candidate:{}", candidate.id);
        report_entry(
            report,
            &subject,
            if needs_reconfirmation.contains_key(&subject) {
                MigrationDisposition::Quarantined
            } else {
                MigrationDisposition::Transformed
            },
            if needs_reconfirmation.contains_key(&subject) {
                "The candidate is retained but its decision requires targeted reconfirmation"
            } else {
                "The candidate decision is retained as a lineage-bearing legacy assertion"
            },
        );
    }
    for feature in &legacy.features {
        let subject = format!("feature:{}", feature.id);
        report_entry(
            report,
            &subject,
            if needs_reconfirmation.contains_key(&subject) {
                MigrationDisposition::Quarantined
            } else {
                MigrationDisposition::Preserved
            },
            if needs_reconfirmation.contains_key(&subject) {
                "The geometry is retained as legacy evidence and requires reconfirmation"
            } else {
                "The source-linked legacy feature is retained for schema-2 review"
            },
        );
    }
    for slot in &legacy.building_slots {
        report_entry(
            report,
            &format!("building-slot:{}", slot.id),
            MigrationDisposition::Preserved,
            "The Building Slot and observed massing fields are retained",
        );
    }
    for record in &legacy.building_directory {
        report_entry(
            report,
            &format!("campus-building-directory:{}", record.source_id),
            MigrationDisposition::Preserved,
            "The reviewed campus name is retained with its legacy record",
        );
    }
    for suppression in &legacy.building_suppressions {
        let subject = format!("suppression:{}", suppression.source_id);
        let requires_reconfirmation = needs_reconfirmation.contains_key(&subject);
        report_entry(
            report,
            &subject,
            if requires_reconfirmation {
                MigrationDisposition::Quarantined
            } else {
                MigrationDisposition::Transformed
            },
            if requires_reconfirmation {
                "The suppression is retained but requires targeted reconfirmation"
            } else {
                "The rejected decision and complete source dependency become a legacy assertion"
            },
        );
    }
    append_undecoded_record_report(report, legacy, source_project);
    for (collection, records) in raw_detailed_collections(source_project) {
        for (index, record) in records.iter().enumerate() {
            let record_id = record
                .get("id")
                .or_else(|| record.get("slotId"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("record-{}", index + 1));
            report_entry(
                report,
                &format!("detailed-{collection}:{record_id}"),
                MigrationDisposition::Preserved,
                "The supported Detailed record is retained in the migration envelope",
            );
        }
    }
}

fn append_undecoded_record_report(
    report: &mut Schema1MigrationReport,
    legacy: &CampusProject,
    source_project: &Value,
) {
    for (index, record) in raw_candidate_records(source_project).enumerate() {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("raw-record-{}", index + 1));
        if legacy.candidates.iter().any(|candidate| candidate.id == id) {
            continue;
        }
        report_entry(
            report,
            &format!("candidate:{id}"),
            MigrationDisposition::Omitted,
            "The raw legacy candidate could not be decoded into a supported candidate record",
        );
    }
    for (index, record) in raw_feature_records(source_project).enumerate() {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("raw-record-{}", index + 1));
        if legacy.features.iter().any(|feature| feature.id == id) {
            continue;
        }
        report_entry(
            report,
            &format!("feature:{id}"),
            MigrationDisposition::Omitted,
            "The raw legacy feature could not be decoded into a supported feature record",
        );
    }
}

fn review_reconfirmation_reason<'a>(
    needs_reconfirmation: &'a BTreeMap<String, ReconfirmationReason>,
    candidate_id: &str,
) -> Option<&'a ReconfirmationReason> {
    needs_reconfirmation
        .get(&format!("candidate:{candidate_id}"))
        .or_else(|| needs_reconfirmation.get(&format!("suppression:{candidate_id}")))
}

fn reconfirmation_reason_report(reason: &ReconfirmationReason) -> &'static str {
    match reason {
        ReconfirmationReason::MissingLegacySubject => {
            "The review record is quarantined because its legacy subject is missing"
        }
        ReconfirmationReason::ContradictoryLegacyDecision => {
            "The review record is quarantined because it contradicts the retained subject state"
        }
        ReconfirmationReason::UnsupportedLegacyDecision => {
            "The review record is quarantined because its legacy decision is unsupported"
        }
        ReconfirmationReason::MissingSourceSnapshot => {
            "The review record is quarantined because it lacks a complete source snapshot"
        }
        ReconfirmationReason::MissingLegacyCampusTargetEvidence
        | ReconfirmationReason::LegacyBoundaryRequiresControlledEvidence
        | ReconfirmationReason::ManualOrScreenshotDerivedGeometry => {
            "The review record is quarantined for targeted reconfirmation"
        }
    }
}

fn push_artifact(
    artifacts: &mut Vec<HistoricalLegacyArtifact>,
    kind: HistoricalArtifactKind,
    path: &Path,
) {
    artifacts.push(HistoricalLegacyArtifact {
        kind,
        legacy_path: path.to_string_lossy().into_owned(),
        compatibility_status: LegacyArtifactCompatibility::CompatibilityUnverified,
        satisfies_v11_completion: false,
    });
}

fn historical_artifact_label(kind: HistoricalArtifactKind) -> &'static str {
    match kind {
        HistoricalArtifactKind::FoundationPreview => "foundation-preview",
        HistoricalArtifactKind::DetailedGeneratedOutput => "detailed-generated-output",
        HistoricalArtifactKind::DetailedRefinementOutput => "detailed-refinement-output",
    }
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

fn validate_legacy_paths(source_project: &Value) -> Result<(), String> {
    for asset in raw_detailed_evidence_assets(source_project) {
        let Some(relative_path) = asset.get("relativePath").and_then(Value::as_str) else {
            continue;
        };
        let path = Path::new(relative_path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "Legacy evidence asset has an unsafe relative path: {}",
                relative_path
            ));
        }
    }
    Ok(())
}

fn raw_native_or_web_root(source_project: &Value) -> &Value {
    source_project.get("project").unwrap_or(source_project)
}

fn raw_foundation_source_snapshots(source_project: &Value) -> impl Iterator<Item = &Value> {
    raw_native_or_web_root(source_project)
        .get("foundationSourceSnapshots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn raw_foundation_review_entries(source_project: &Value) -> impl Iterator<Item = &Value> {
    raw_native_or_web_root(source_project)
        .get("foundationReviewLedger")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn raw_portable_reviews(source_project: &Value) -> Option<&serde_json::Map<String, Value>> {
    raw_native_or_web_root(source_project)
        .pointer("/foundation/reviews")
        .and_then(Value::as_object)
}

fn raw_candidate_source_snapshot_id<'a>(
    source_project: &'a Value,
    candidate_id: &str,
) -> Option<&'a str> {
    let root = raw_native_or_web_root(source_project);
    let candidates = root
        .get("candidates")
        .or_else(|| root.pointer("/foundation/candidates"))?
        .as_array()?;
    candidates
        .iter()
        .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(candidate_id))?
        .get("sourceSnapshotId")
        .and_then(Value::as_str)
}

fn raw_candidate_records(source_project: &Value) -> impl Iterator<Item = &Value> {
    let root = raw_native_or_web_root(source_project);
    root.get("candidates")
        .or_else(|| root.pointer("/foundation/candidates"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn raw_feature_records(source_project: &Value) -> impl Iterator<Item = &Value> {
    let root = raw_native_or_web_root(source_project);
    root.get("features")
        .or_else(|| root.pointer("/foundation/manualFeatures"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn raw_detailed_evidence_assets(source_project: &Value) -> impl Iterator<Item = &Value> {
    raw_native_or_web_root(source_project)
        .pointer("/detailed/evidenceAssets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn raw_detailed_collections(source_project: &Value) -> Vec<(&'static str, &Vec<Value>)> {
    let Some(detailed) = raw_native_or_web_root(source_project)
        .get("detailed")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    [
        ("refinement", "refinements"),
        ("semantic-feature", "semanticFeatures"),
        ("external-model", "externalModels"),
        ("source-conflict", "sourceConflicts"),
        ("evidence-asset", "evidenceAssets"),
        ("function-classification", "functionClassifications"),
        ("template-proposal", "templateProposals"),
        ("selected-template", "selectedTemplates"),
        ("facade-draft", "facadeDrafts"),
    ]
    .into_iter()
    .filter_map(|(label, key)| {
        detailed
            .get(key)
            .and_then(Value::as_array)
            .map(|records| (label, records))
    })
    .collect()
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
) -> Result<(), String> {
    atomic_write_bytes(source_path, source_bytes)?;
    restore_optional_file(index_path, old_index_bytes)
}

fn rollback_error(
    migration_error: String,
    source_path: &Path,
    source_bytes: &[u8],
    index_path: &Path,
    old_index_bytes: Option<&[u8]>,
) -> String {
    match rollback_migration(source_path, source_bytes, index_path, old_index_bytes) {
        Ok(()) => migration_error,
        Err(rollback_error) => {
            format!("{migration_error}; migration rollback failed: {rollback_error}")
        }
    }
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
