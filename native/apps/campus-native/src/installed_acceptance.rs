use campus_services::acquisition::{
    AcquisitionClient, AcquisitionTransport, FaultInjectingTransport, InjectedAcquisitionFault,
    TransportError, TransportRequest, TransportResponse,
};
use campus_state::{
    CampusProjectLibrary, CampusScope, CampusTargetMatchApproval, FoundationCandidateDecision,
    FoundationCategory, FoundationResumePoint, InstallationId, MigrationFaultPoint,
    PinnedAcquisitionEvidence, PinnedBoundaryEvidence, PortableDestination,
    PortableTransferFaultPoint, ProviderOutcomeStatus, ResultManifest, SaveFaultPoint,
    Schema2Project, Schema2ProjectSession, SourceGeometry, SourceObservation,
    V11ConstructionCapability,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledAcceptanceReport {
    pub status: String,
    pub version: String,
    pub architecture: String,
    pub started_utc_unix_ms: u128,
    pub finished_utc_unix_ms: u128,
    pub reliability_cycles_required: u64,
    pub cases: Vec<InstalledAcceptanceCase>,
    pub release_blockers: Vec<ReleaseBlocker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledAcceptanceCase {
    pub case_id: String,
    pub category: String,
    pub mandatory: bool,
    pub status: String,
    pub input_digest_sha256: String,
    pub failure_point: String,
    pub expected_state: Value,
    pub actual_state: Value,
    pub project_summary_digest_sha256: String,
    pub event_ids: Vec<String>,
    pub result_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseBlocker {
    pub case_id: String,
    pub reason: String,
    pub event_ids: Vec<String>,
}

struct CaseOutcome {
    failure_point: String,
    expected: Value,
    actual: Value,
    summary: Value,
}

type CaseFn = fn(&Path, u64) -> Result<CaseOutcome, String>;

pub(crate) fn write_report(
    report_path: &Path,
    reliability_cycles: u64,
) -> Result<InstalledAcceptanceReport, String> {
    let parent = report_path
        .parent()
        .ok_or("installed evidence report has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if report_path.exists() {
        return Err("installed evidence report already exists".into());
    }
    let cases_root = parent.join("cases");
    std::fs::create_dir(&cases_root).map_err(|error| error.to_string())?;
    let started = unix_ms();
    let definitions: [(&str, &str, &str, CaseFn); 8] = [
        (
            "project-identity-and-campus-local-names",
            "durability",
            "project creation and name-conflict gates",
            project_identity_case,
        ),
        (
            "atomic-save-interruption-matrix",
            "durability",
            "every write/replace stage",
            atomic_save_case,
        ),
        (
            "recovery-and-fifty-operation-history",
            "durability",
            "accept/discard/corrupt recovery and restart",
            recovery_history_case,
        ),
        (
            "deterministic-resume-without-refresh",
            "resume",
            "earliest incomplete and completed export routes",
            resume_case,
        ),
        (
            "portable-project-transaction-matrix",
            "portability",
            "identity, campus, name, save, collision, and transfer gates",
            portability_case,
        ),
        (
            "populated-schema-one-migration-matrix",
            "migration",
            "managed and portable V1.0.1 fixtures",
            migration_case,
        ),
        (
            "hostile-input-and-failure-isolation",
            "migration",
            "corrupt, newer, unsafe, collision, and injected failures",
            hostile_input_case,
        ),
        (
            "helper-network-exit-and-cycle-reliability",
            "reliability",
            "helper termination, six acquisition faults, abnormal exit, and durable reopen cycles",
            reliability_case,
        ),
    ];
    let mut cases = Vec::new();
    let mut blockers = Vec::new();
    for (case_id, category, input, run) in definitions {
        let case_root = cases_root.join(case_id);
        std::fs::create_dir(&case_root).map_err(|error| error.to_string())?;
        let start_event = diagnostic_event(case_id, "start", "not-attempted");
        let result = run(&case_root, reliability_cycles);
        let (status, failure_point, expected, actual, summary, finish_event) = match result {
            Ok(outcome) => (
                "pass".to_string(),
                outcome.failure_point,
                outcome.expected,
                outcome.actual,
                outcome.summary,
                diagnostic_event(case_id, "pass", "work-preserved"),
            ),
            Err(error) => {
                let safe_error = safe_error_message(&error);
                let finish_event = diagnostic_event(case_id, "fail", "release-blocked");
                blockers.push(ReleaseBlocker {
                    case_id: case_id.into(),
                    reason: safe_error.clone(),
                    event_ids: vec![start_event.clone(), finish_event.clone()],
                });
                (
                    "fail".to_string(),
                    "case execution".into(),
                    json!({"result": "pass"}),
                    json!({"error": safe_error}),
                    json!({"result": "failed without claiming success"}),
                    finish_event,
                )
            }
        };
        let evidence_name = format!("cases/{case_id}/case-result.json");
        let evidence_payload = json!({
            "caseId": case_id,
            "input": input,
            "expectedState": expected.clone(),
            "actualState": actual.clone(),
            "projectSummary": summary.clone(),
        });
        let evidence_path = case_root.join("evidence.json");
        std::fs::write(
            &evidence_path,
            serde_json::to_vec_pretty(&evidence_payload).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let evidence_digest =
            sha256(&serde_json::to_vec(&evidence_payload).map_err(|error| error.to_string())?);
        let case = InstalledAcceptanceCase {
            case_id: case_id.into(),
            category: category.into(),
            mandatory: true,
            status,
            input_digest_sha256: evidence_digest,
            failure_point,
            expected_state: expected,
            actual_state: actual,
            project_summary_digest_sha256: sha256(
                &serde_json::to_vec(&summary).map_err(|error| error.to_string())?,
            ),
            event_ids: vec![start_event, finish_event],
            result_evidence: vec![format!("cases/{case_id}/evidence.json"), evidence_name],
        };
        std::fs::write(
            case_root.join("case-result.json"),
            serde_json::to_vec_pretty(&case).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        cases.push(case);
    }
    let report = InstalledAcceptanceReport {
        status: if blockers.is_empty() { "pass" } else { "fail" }.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        architecture: std::env::consts::ARCH.into(),
        started_utc_unix_ms: started,
        finished_utc_unix_ms: unix_ms(),
        reliability_cycles_required: reliability_cycles,
        cases,
        release_blockers: blockers,
    };
    std::fs::write(
        report_path,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(report)
}

fn capability() -> V11ConstructionCapability {
    V11ConstructionCapability::for_controlled_release()
}

fn actor() -> InstallationId {
    InstallationId::new("installed-acceptance").expect("fixed installation id")
}

fn scope(id: &str, name: &str, poi: &str) -> CampusScope {
    CampusScope::new(id, name, [121.4, 31.2])
        .and_then(|scope| scope.with_gaode_poi_id(poi))
        .expect("fixed campus scope")
}

fn library(root: &Path, target_id: &str) -> Result<CampusProjectLibrary, String> {
    CampusProjectLibrary::open_for_construction(root, target_id, &capability())
}

fn project_identity_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let campus = scope("putuo", "Putuo Campus", "B001");
    let mut putuo = library(&root.join("putuo"), "putuo")?;
    let first = putuo.create_project(campus.clone(), "Library", actor())?;
    let second = putuo.create_project(campus, "Library alternate", actor())?;
    if first.id() == second.id()
        || putuo
            .create_project(scope("putuo", "Putuo Campus", "B001"), "Library", actor())
            .is_ok()
    {
        return Err("campus-local project identity or unique-name gate failed".into());
    }
    let mut other = library(&root.join("other"), "minhang")?;
    let same_name = other.create_project(
        scope("minhang", "Minhang Campus", "B002"),
        "Library",
        actor(),
    )?;
    Ok(CaseOutcome {
        failure_point: "same-campus duplicate creation".into(),
        expected: json!({"putuoProjects": 2, "crossCampusSameName": true, "independentIds": true}),
        actual: json!({"putuoProjects": putuo.records().len(), "crossCampusSameName": same_name.name() == "Library", "independentIds": first.id() != second.id()}),
        summary: json!({"first": first.id().as_str(), "second": second.id().as_str(), "other": same_name.id().as_str()}),
    })
}

fn atomic_save_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let mut stages = Vec::new();
    for fault in SaveFaultPoint::ALL {
        let stage_root = root.join(format!("{fault:?}"));
        let mut store = library(&stage_root, "putuo")?;
        let project = store.create_project(
            scope("putuo", "Putuo Campus", "B001"),
            format!("save {fault:?}"),
            actor(),
        )?;
        let id = project.id().clone();
        let mut session = Schema2ProjectSession::default();
        session.open_project(&store, &id)?;
        session.apply_semantic_operation(&mut store, "baseline", |project| {
            project.mark_updated(actor())
        })?;
        store.inject_next_save_interruption(fault);
        if session
            .apply_semantic_operation(&mut store, "interrupted", |project| {
                project.mark_updated(actor())
            })
            .is_ok()
        {
            return Err(format!("{fault:?} reported false save success"));
        }
        drop(session);
        drop(store);
        let reopened = CampusProjectLibrary::open(&stage_root, "putuo")?;
        let confirmed = reopened.open_project(&id)?.workflow().project_revision();
        let recovery = reopened
            .recovery_candidate(&id)?
            .ok_or("missing coherent recovery")?
            .project_revision();
        if confirmed != 1 || recovery != 2 {
            return Err(format!("{fault:?} exposed partial state"));
        }
        stages.push(format!("{fault:?}"));
    }
    Ok(CaseOutcome {
        failure_point: "SaveFaultPoint::ALL".into(),
        expected: json!({"confirmedRevision": 1, "recoveryRevision": 2, "stages": 9}),
        actual: json!({"confirmedRevision": 1, "recoveryRevision": 2, "stages": stages.len()}),
        summary: json!({"stages": stages}),
    })
}

fn recovery_history_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let mut store = library(root, "putuo")?;
    let project =
        store.create_project(scope("putuo", "Putuo Campus", "B001"), "Recovery", actor())?;
    let id = project.id().clone();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&store, &id)?;
    for operation in 1..=55 {
        session.apply_semantic_operation(
            &mut store,
            format!("operation {operation}"),
            |project| project.mark_updated(actor()),
        )?;
    }
    if session.history().len() != 50 {
        return Err("history did not retain exactly 50 operations".into());
    }
    store.inject_next_save_failure(SaveFaultPoint::BeforeProjectReplace);
    let _ = session.apply_semantic_operation(&mut store, "recover me", |project| {
        project.mark_updated(actor())
    });
    drop(session);
    let mut reopened = Schema2ProjectSession::default();
    reopened.open_project(&store, &id)?;
    if reopened.history().len() != 50 {
        return Err("50-operation history did not persist across restart".into());
    }
    let confirmed_before = reopened
        .active()
        .ok_or("no active project")?
        .workflow()
        .project_revision();
    reopened.accept_recovery(&store)?;
    let recovered_revision = reopened
        .active()
        .ok_or("recovery not active")?
        .workflow()
        .project_revision();
    if recovered_revision <= confirmed_before || !reopened.is_dirty() {
        return Err("coherent recovery was not explicit working state".into());
    }
    reopened.request_save(&mut store)?;
    store.inject_next_save_failure(SaveFaultPoint::BeforeProjectReplace);
    let _ = reopened.apply_semantic_operation(&mut store, "discard me", |project| {
        project.mark_updated(actor())
    });
    reopened.discard_recovery(&store)?;
    if store.recovery_candidate(&id)?.is_some() {
        return Err("discard recovery did not restore the confirmed state".into());
    }
    let discarded_revision = reopened
        .active()
        .ok_or("discard removed the active project")?
        .workflow()
        .project_revision();
    let recovery_path = store.recovery_path(&id)?;
    std::fs::write(&recovery_path, b"{\"partial\":true").map_err(|error| error.to_string())?;
    if store.recovery_candidate(&id).is_ok() {
        return Err("corrupt recovery was accepted".into());
    }
    Ok(CaseOutcome {
        failure_point: "accept, discard, corrupt recovery, restart history".into(),
        expected: json!({"history": 50, "accepted": true, "discarded": true, "corruptRejected": true}),
        actual: json!({"history": 50, "accepted": recovered_revision > confirmed_before, "discarded": true, "discardedRevision": discarded_revision, "corruptRejected": true}),
        summary: json!({"projectId": id.as_str(), "confirmedBefore": confirmed_before, "recoveredRevision": recovered_revision}),
    })
}

fn resume_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let mut store = library(root, "putuo")?;
    let mut project =
        store.create_project(scope("putuo", "Putuo Campus", "B001"), "Resume", actor())?;
    if project.resume_point() != FoundationResumePoint::BoundaryReview {
        return Err("new project did not select earliest incomplete task".into());
    }
    let boundary = boundary_evidence()?;
    project.confirm_boundary(boundary, actor())?;
    if project.resume_point() != FoundationResumePoint::Acquisition {
        return Err("boundary completion did not advance to acquisition".into());
    }
    let complete_root = root.join("complete");
    let complete = completed_project(&complete_root)?;
    if complete.resume_point() != FoundationResumePoint::Complete {
        return Err("complete project did not open completion/export".into());
    }
    let snapshot_before = complete.acquisition_snapshot_identity().to_string();
    store.save_project(&project)?;
    let reopened = store.open_project(project.id())?;
    if reopened.resume_point() != FoundationResumePoint::Acquisition
        || reopened.acquisition_snapshot_identity() != project.acquisition_snapshot_identity()
    {
        return Err("reopen changed the resume point or refreshed a source implicitly".into());
    }
    let complete_store = CampusProjectLibrary::open(&complete_root, "putuo")?;
    let reopened_complete = complete_store.open_project(complete.id())?;
    let snapshot_after = reopened_complete
        .acquisition_snapshot_identity()
        .to_string();
    if snapshot_before.is_empty()
        || snapshot_before != snapshot_after
        || reopened_complete.resume_point() != FoundationResumePoint::Complete
    {
        return Err(
            "completed project reopen refreshed sources or changed its resume point".into(),
        );
    }
    Ok(CaseOutcome {
        failure_point: "reopen at each durable dependency boundary".into(),
        expected: json!({"new": "boundary_review", "afterBoundary": "acquisition", "complete": "complete", "implicitRefresh": false}),
        actual: json!({"new": "boundary_review", "afterBoundary": "acquisition", "complete": "complete", "implicitRefresh": snapshot_before != snapshot_after}),
        summary: serde_json::to_value(&complete).map_err(|error| error.to_string())?,
    })
}

fn portability_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let source_root = root.join("source");
    let target_root = root.join("target");
    let cross_root = root.join("cross");
    let transfer = root.join("portable.campus-project.json");
    let campus = scope("putuo", "Putuo Campus", "B001");
    let mut source = library(&source_root, "putuo")?;
    let project = source.create_project(campus.clone(), "Library", actor())?;
    let original_id = project.id().clone();
    let managed_path = source
        .record(&original_id)
        .ok_or("missing source record")?
        .managed_relative_path()
        .to_string();
    source.export_portable_project(&project, &transfer, PortableDestination::CreateNew)?;
    let source_bytes = std::fs::read(&transfer).map_err(|error| error.to_string())?;
    if project.id() != &original_id
        || source
            .record(&original_id)
            .ok_or("missing record")?
            .managed_relative_path()
            != managed_path
    {
        return Err("export changed active identity".into());
    }
    let mut target = library(&target_root, "putuo")?;
    target.create_project(campus.clone(), "Library", actor())?;
    let imported = target.import_portable_project(
        &transfer,
        campus,
        CampusTargetMatchApproval::AutomaticOnly,
        actor(),
    )?;
    if imported.id() == &original_id
        || imported.name() == "Library"
        || std::fs::read(&transfer).map_err(|error| error.to_string())? != source_bytes
    {
        return Err("same-campus import identity/name/source contract failed".into());
    }
    let mut cross = library(&cross_root, "minhang")?;
    let cross_scope = scope("minhang", "Minhang Campus", "B002");
    if cross
        .import_portable_project(
            &transfer,
            cross_scope.clone(),
            CampusTargetMatchApproval::AutomaticOnly,
            actor(),
        )
        .is_ok()
    {
        return Err("cross-campus import bypassed human gate".into());
    }
    let cross_imported = cross
        .import_portable_project(
            &transfer,
            cross_scope,
            CampusTargetMatchApproval::HumanConfirmed,
            actor(),
        )
        .map_err(|_| "confirmed cross-campus import failed")?;
    if cross_imported.id() == &original_id {
        return Err("cross-campus import reused the source Project ID".into());
    }
    let cross_reopened =
        CampusProjectLibrary::open(&cross_root, "minhang")?.open_project(cross_imported.id())?;
    if cross_reopened.id() != cross_imported.id() {
        return Err("confirmed cross-campus import failed".into());
    }
    Ok(CaseOutcome {
        failure_point: "export identity and same/cross-campus import gates".into(),
        expected: json!({"sourceIdentityPreserved": true, "newImportIdentity": true, "nameConflictResolved": true, "crossCampusConfirmed": true}),
        actual: json!({"sourceIdentityPreserved": project.id() == &original_id, "newImportIdentity": imported.id() != &original_id && cross_imported.id() != &original_id, "nameConflictResolved": imported.name() != "Library", "crossCampusConfirmed": cross.records().len() == 1, "crossCampusReopen": cross_reopened.id() == cross_imported.id()}),
        summary: json!({"source": original_id.as_str(), "imported": imported.id().as_str(), "crossImported": cross_imported.id().as_str(), "sourceDigest": sha256(&source_bytes)}),
    })
}

fn migration_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let native_bytes = populated_native_schema1_bytes()?;
    let managed_root = root.join("managed-library");
    std::fs::create_dir_all(&managed_root).map_err(|error| error.to_string())?;
    let native_path = managed_root.join("managed-v1.campus.json");
    std::fs::write(&native_path, &native_bytes).map_err(|error| error.to_string())?;
    let mut managed = CampusProjectLibrary::open(&managed_root, "gaode:B00155J6JH")?;
    let native = managed.migrate_managed_schema1_project(&native_path, actor())?;
    if std::fs::read(&native.backup_path).map_err(|error| error.to_string())? != native_bytes {
        return Err("managed migration backup changed".into());
    }
    let legacy_bytes = include_bytes!("../../../test-data/v1.0.1/legacy-web-portable-project.json");
    let legacy_path = root.join("legacy-web-portable.json");
    std::fs::write(&legacy_path, legacy_bytes).map_err(|error| error.to_string())?;
    let embedded = CampusProjectLibrary::portable_project_scope(&legacy_path)?;
    let embedded_target = embedded.target_id().to_string();
    let mut portable = library(&root.join("portable-library"), &embedded_target)?;
    let migrated = portable.import_portable_project(
        &legacy_path,
        embedded,
        CampusTargetMatchApproval::HumanConfirmed,
        actor(),
    )?;
    let migration = native
        .project
        .legacy_migration()?
        .ok_or("managed migration lineage missing")?;
    let original_value: Value =
        serde_json::from_slice(&native_bytes).map_err(|error| error.to_string())?;
    if migration.source_project != original_value {
        return Err("managed migration did not preserve the original project decisions".into());
    }
    let repeat_root = root.join("repeat-library");
    std::fs::create_dir_all(&repeat_root).map_err(|error| error.to_string())?;
    let repeat_path = repeat_root.join("managed-v1.campus.json");
    std::fs::write(&repeat_path, &native_bytes).map_err(|error| error.to_string())?;
    let mut repeat_library = CampusProjectLibrary::open(&repeat_root, "gaode:B00155J6JH")?;
    let repeat = repeat_library.migrate_managed_schema1_project(&repeat_path, actor())?;
    if migration.needs_reconfirmation.is_empty() || repeat.report != native.report {
        return Err("migration report or targeted reconfirmation missing".into());
    }
    Ok(CaseOutcome {
        failure_point: "managed and portable schema-1 transactional migration".into(),
        expected: json!({"managedBackup": true, "portableNewId": true, "deterministicReport": true, "targetedReconfirmation": true, "decisionsPreserved": true}),
        actual: json!({"managedBackup": native.backup_path.is_file(), "portableNewId": migrated.id() != native.project.id(), "deterministicReport": repeat.report == native.report, "targetedReconfirmation": !migration.needs_reconfirmation.is_empty(), "decisionsPreserved": migration.source_project == original_value}),
        summary: json!({"managedReport": native.report, "portableLegacy": migrated.legacy_migration()?.is_some(), "sourceDigest": sha256(&native_bytes)}),
    })
}

fn hostile_input_case(root: &Path, _reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let corrupt_root = root.join("corrupt-library");
    std::fs::create_dir_all(&corrupt_root).map_err(|error| error.to_string())?;
    let corrupt = corrupt_root.join("corrupt.json");
    std::fs::write(&corrupt, b"{not json").map_err(|error| error.to_string())?;
    let mut corrupt_library = CampusProjectLibrary::open(&corrupt_root, "putuo")?;
    if corrupt_library
        .migrate_managed_schema1_project(&corrupt, actor())
        .is_ok()
        || std::fs::read(&corrupt).map_err(|error| error.to_string())? != b"{not json"
    {
        return Err("corrupt migration changed source or succeeded".into());
    }
    let mut newer: Value = serde_json::from_slice(&populated_native_schema1_bytes()?)
        .map_err(|error| error.to_string())?;
    newer["schemaVersion"] = json!(3);
    let newer_root = root.join("newer-library");
    std::fs::create_dir_all(&newer_root).map_err(|error| error.to_string())?;
    let newer_path = newer_root.join("newer.json");
    std::fs::write(
        &newer_path,
        serde_json::to_vec_pretty(&newer).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut newer_library = CampusProjectLibrary::open(&newer_root, "putuo")?;
    if newer_library
        .migrate_managed_schema1_project(&newer_path, actor())
        .is_ok()
    {
        return Err("unsupported newer schema migrated".into());
    }
    let unsafe_root = root.join("unsafe-library");
    std::fs::create_dir_all(&unsafe_root).map_err(|error| error.to_string())?;
    let unsafe_path = unsafe_root.join("unsafe.json");
    let unsafe_bytes =
        include_bytes!("../../../test-data/v1.0.1/failures/unsafe-portable-path.json");
    std::fs::write(&unsafe_path, unsafe_bytes).map_err(|error| error.to_string())?;
    let mut unsafe_library = CampusProjectLibrary::open(&unsafe_root, "putuo")?;
    if unsafe_library
        .migrate_managed_schema1_project(&unsafe_path, actor())
        .is_ok()
        || std::fs::read(&unsafe_path).map_err(|error| error.to_string())? != unsafe_bytes
        || !unsafe_library.records().is_empty()
    {
        return Err("unsafe legacy path changed state or migrated".into());
    }
    let mut fault_count = 0;
    for fault in MigrationFaultPoint::ALL {
        let fault_root = root.join(format!("migration-{fault:?}"));
        std::fs::create_dir_all(&fault_root).map_err(|error| error.to_string())?;
        let source_path = fault_root.join("legacy.json");
        let bytes = populated_native_schema1_bytes()?;
        std::fs::write(&source_path, &bytes).map_err(|error| error.to_string())?;
        let mut store = CampusProjectLibrary::open(&fault_root, "gaode:B00155J6JH")?;
        store.inject_next_migration_failure(fault);
        if store
            .migrate_managed_schema1_project(&source_path, actor())
            .is_ok()
            || std::fs::read(&source_path).map_err(|error| error.to_string())? != bytes
            || !store.records().is_empty()
        {
            return Err(format!("{fault:?} leaked partial migration state"));
        }
        fault_count += 1;
    }
    let portable_root = root.join("portable-faults");
    let mut source = library(&portable_root.join("source"), "putuo")?;
    let project = source.create_project(scope("putuo", "Putuo", "B001"), "Portable", actor())?;
    let destination = portable_root.join("destination.json");
    std::fs::create_dir_all(&portable_root).map_err(|error| error.to_string())?;
    std::fs::write(&destination, b"existing").map_err(|error| error.to_string())?;
    for fault in [
        PortableTransferFaultPoint::ExportAfterStageWrite,
        PortableTransferFaultPoint::ExportAfterStageValidation,
    ] {
        source.inject_next_portable_failure(fault);
        if source
            .export_portable_project(
                &project,
                &destination,
                PortableDestination::ReplaceConfirmed,
            )
            .is_ok()
            || std::fs::read(&destination).map_err(|error| error.to_string())? != b"existing"
        {
            return Err(format!("{fault:?} changed collision destination"));
        }
    }
    let valid_portable = portable_root.join("valid.json");
    source.export_portable_project(&project, &valid_portable, PortableDestination::CreateNew)?;
    let valid_bytes = std::fs::read(&valid_portable).map_err(|error| error.to_string())?;
    let mut import_fault_count = 0;
    for fault in [
        PortableTransferFaultPoint::ImportAfterTemporaryCopy,
        PortableTransferFaultPoint::ImportAfterMigration,
        PortableTransferFaultPoint::ImportAfterProjectWrite,
        PortableTransferFaultPoint::ImportAfterIndexWrite,
    ] {
        let target_root = portable_root.join(format!("import-{fault:?}"));
        let mut target = library(&target_root, "putuo")?;
        target.inject_next_portable_failure(fault);
        if target
            .import_portable_project(
                &valid_portable,
                scope("putuo", "Putuo", "B001"),
                CampusTargetMatchApproval::AutomaticOnly,
                actor(),
            )
            .is_ok()
            || !target.records().is_empty()
            || !CampusProjectLibrary::open(&target_root, "putuo")?
                .records()
                .is_empty()
            || std::fs::read(&valid_portable).map_err(|error| error.to_string())? != valid_bytes
        {
            return Err(format!("{fault:?} leaked partial import state"));
        }
        import_fault_count += 1;
    }
    Ok(CaseOutcome {
        failure_point: "corrupt/newer input, migration matrix, portable collision matrix".into(),
        expected: json!({"corruptRejected": true, "newerRejected": true, "unsafePathRejected": true, "migrationFaults": 6, "importFaults": 4, "destinationUnchanged": true}),
        actual: json!({"corruptRejected": true, "newerRejected": true, "unsafePathRejected": true, "migrationFaults": fault_count, "importFaults": import_fault_count, "destinationUnchanged": std::fs::read(&destination).map_err(|error| error.to_string())? == b"existing"}),
        summary: json!({"migrationFaultCount": fault_count, "importFaultCount": import_fault_count, "destinationDigest": sha256(&std::fs::read(destination).map_err(|error| error.to_string())?)}),
    })
}

fn reliability_case(root: &Path, reliability_cycles: u64) -> Result<CaseOutcome, String> {
    let helper = helper_smoke()?;
    helper_termination_fault()?;
    let mut faults = Vec::new();
    for fault in [
        InjectedAcquisitionFault::NetworkLoss,
        InjectedAcquisitionFault::Timeout,
        InjectedAcquisitionFault::ServiceError,
        InjectedAcquisitionFault::Cancelled,
        InjectedAcquisitionFault::CorruptChunk,
        InjectedAcquisitionFault::AbnormalExit,
    ] {
        let client = AcquisitionClient::new(FaultInjectingTransport::new(EmptyTransport, 1, fault));
        if client.capabilities().is_ok() {
            return Err(format!("{fault:?} reported false acquisition success"));
        }
        faults.push(format!("{fault:?}"));
    }
    let reliability_root = root.join("reliability-cycles");
    let mut store = library(&reliability_root, "putuo")?;
    let project = store.create_project(
        scope("putuo", "Putuo", "B001"),
        "Reliability cycles",
        actor(),
    )?;
    let id = project.id().clone();
    let started = Instant::now();
    for cycle in 0..reliability_cycles {
        let mut session = Schema2ProjectSession::default();
        session.open_project(&store, &id)?;
        session.apply_semantic_operation(
            &mut store,
            format!("reliability cycle {cycle}"),
            |project| project.mark_updated(actor()),
        )?;
        drop(session);
        let reopened = CampusProjectLibrary::open(&reliability_root, "putuo")?;
        if reopened.open_project(&id)?.workflow().project_revision() != cycle + 1 {
            return Err("reliability reopen lost a confirmed operation".into());
        }
    }
    let elapsed_ms = started.elapsed().as_millis();
    Ok(CaseOutcome {
        failure_point: "installed helper lifecycle, network/service/chunk faults, abnormal exit, durable reopen cycles".into(),
        expected: json!({"helperStatus": "pass", "faults": 6, "minimumReliabilityCycles": reliability_cycles, "falseSuccess": false}),
        actual: json!({"helperStatus": helper["status"], "helperTermination": "recovered", "faults": faults.len(), "elapsedWallClockMs": elapsed_ms, "cycles": reliability_cycles, "falseSuccess": false}),
        summary: json!({"helper": helper, "faults": faults, "cycles": reliability_cycles, "elapsedWallClockMs": elapsed_ms, "finalRevision": store.open_project(&id)?.workflow().project_revision()}),
    })
}

#[cfg(not(test))]
fn helper_smoke() -> Result<Value, String> {
    super::run_candidate_helper_smoke()
}

#[cfg(not(test))]
fn helper_termination_fault() -> Result<(), String> {
    use campus_tool_protocol::{ToolCommand, ToolKind};

    let supervisor = super::DesktopToolProcessSupervisor::new();
    supervisor.inject_next_failure(super::desktop_tool_process::HelperFaultPoint::AfterSpawn);
    let result = supervisor.launch(
        "campus-map",
        ToolKind::Map,
        ToolCommand::Shutdown,
        |_event| async {},
    );
    match result {
        Err(error) if error.contains("AfterSpawn") => Ok(()),
        Err(error) => Err(format!(
            "helper termination returned unrelated failure: {error}"
        )),
        Ok(()) => Err("helper termination fault reported false success".into()),
    }
}

#[cfg(test)]
fn helper_termination_fault() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
fn helper_smoke() -> Result<Value, String> {
    Ok(json!({
        "status": "pass",
        "helpers": {"campusMap": "test-seam", "campusPreview": "test-seam"}
    }))
}

#[derive(Clone, Copy)]
struct EmptyTransport;

impl AcquisitionTransport for EmptyTransport {
    fn execute(&self, _request: TransportRequest) -> Result<TransportResponse, TransportError> {
        Ok(TransportResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: b"{}".to_vec(),
        })
    }
}

fn boundary_evidence() -> Result<PinnedBoundaryEvidence, String> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
    ))
    .map_err(|error| error.to_string())?;
    Ok(PinnedBoundaryEvidence {
        manifest: serde_json::from_value(json!({
            "contract_version": fixture["contract_version"], "bundle": fixture["bundle"],
            "coverage_report": fixture["coverage_report"],
            "licences": fixture["candidates"].as_array().ok_or("boundary candidates missing")?.iter().map(|candidate| candidate["licence"].clone()).collect::<Vec<_>>(),
            "chunks": fixture["manifest"]["chunks"], "result_sha256": fixture["manifest"]["result_sha256"]
        })).map_err(|error| error.to_string())?,
        candidates: serde_json::from_value(fixture["candidates"].clone()).map_err(|error| error.to_string())?,
        selected_candidate_id: "boundary-osm-relation-100".into(), confirmed_geometry: None,
        assessments: Default::default(),
    })
}

fn five_category_observations() -> Result<Vec<SourceObservation>, String> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/complex-building-review.json"
    ))
    .map_err(|error| error.to_string())?;
    let template: SourceObservation = serde_json::from_value(
        fixture["observations"]
            .as_array()
            .ok_or("observations missing")?
            .iter()
            .find(|observation| observation["id"] == "obs-campus-library-v2")
            .ok_or("template observation missing")?
            .clone(),
    )
    .map_err(|error| error.to_string())?;
    Ok(FoundationCategory::ALL
        .into_iter()
        .enumerate()
        .map(|(index, category)| {
            let mut observation = template.clone();
            observation.id = format!("observation-{category:?}-v1").to_ascii_lowercase();
            observation.category = category;
            observation.lineage.source_record_id =
                format!("stable/{category:?}").to_ascii_lowercase();
            observation.lineage.source_record_version = "1".into();
            observation.geometry_sha256 = format!("{:064x}", index + 1);
            observation.derivation.source_geometry_sha256 = observation.geometry_sha256.clone();
            observation.derivation.review_geometry_sha256 = format!("{:064x}", index + 101);
            observation.suggestions.clear();
            observation
        })
        .collect())
}

fn acquisition_evidence(
    observations: Vec<SourceObservation>,
    bundle_id: &str,
) -> Result<PinnedAcquisitionEvidence, String> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .map_err(|error| error.to_string())?;
    let mut manifest: ResultManifest = serde_json::from_value(json!({
        "contract_version": fixture["contract_version"], "bundle": fixture["bundle"],
        "coverage_report": fixture["coverage_report"],
        "licences": fixture["observations"].as_array().ok_or("fixture observations missing")?.iter().map(|observation| observation["licence"].clone()).collect::<Vec<_>>(),
        "chunks": fixture["manifest"]["chunks"], "result_sha256": "1".repeat(64)
    })).map_err(|error| error.to_string())?;
    manifest.bundle.id = bundle_id.into();
    manifest.bundle.osm_snapshot = format!("{bundle_id}-osm");
    manifest.bundle.overture_release = format!("{bundle_id}-overture");
    for outcome in &mut manifest.coverage_report.outcomes {
        outcome.status = ProviderOutcomeStatus::Complete;
        outcome.pagination_exhausted = true;
        outcome.relation_members_complete = true;
        outcome.gaps.clear();
        outcome.failure = None;
    }
    Ok(PinnedAcquisitionEvidence {
        manifest,
        observations,
    })
}

fn completed_project(root: &Path) -> Result<Schema2Project, String> {
    let mut store = library(root, "putuo")?;
    let mut project = store.create_project(scope("putuo", "Putuo", "B001"), "Complete", actor())?;
    let mut boundary = boundary_evidence()?;
    boundary.manifest.bundle.id = "installed-controlled".into();
    boundary.manifest.bundle.osm_snapshot = "installed-controlled-osm".into();
    boundary.manifest.bundle.overture_release = "installed-controlled-overture".into();
    boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.398, 31.208],
        [121.410, 31.208],
        [121.410, 31.220],
        [121.398, 31.220],
        [121.398, 31.208],
    ]]));
    project.confirm_boundary(boundary, actor())?;
    project.pin_acquisition(
        acquisition_evidence(five_category_observations()?, "installed-controlled")?,
        actor(),
    )?;
    for category in FoundationCategory::ALL {
        let ids = project
            .foundation_review_queue(category)?
            .items
            .into_iter()
            .map(|item| item.subject_id)
            .collect::<Vec<_>>();
        for id in ids {
            project.review_foundation_candidate(
                category,
                &id,
                FoundationCandidateDecision::Accept,
                actor(),
            )?;
        }
        project.complete_foundation_category(category, actor())?;
    }
    project.record_generation(32, 8, 32, 512, actor())?;
    project.record_export("e".repeat(64), 4096, "campus.foundation.json".into())?;
    store.save_project(&project)?;
    Ok(project)
}

fn populated_native_schema1_bytes() -> Result<Vec<u8>, String> {
    let mut value: Value =
        serde_json::from_slice(include_bytes!("../../../test-data/v1-demo.campus.json"))
            .map_err(|error| error.to_string())?;
    value["name"] = json!("V1.0.1 populated installed fixture");
    value["campusName"] = json!("ECNU Putuo Campus");
    value["campusTarget"] = json!({"poiId":"B00155J6JH","name":"ECNU Putuo Campus","gcj02":{"lng":121.406582,"lat":31.228318},"wgs84":{"lng":121.402037,"lat":31.230305},"acquisition":"installed-fixture"});
    value["candidates"][0]["sourceSnapshotId"] = json!("osm:installed-fixture");
    value["foundationSourceSnapshots"] = json!([{"id":"osm:installed-fixture","provider":"open_street_map","providerVersion":"fixture/1.0.1","status":"complete","southWest":{"lng":121.4059,"lat":31.2277},"northEast":{"lng":121.4072,"lat":31.2288},"acquiredAtUnixMs":1710000000000_u64,"candidates":[],"error":null}]);
    value["foundationReviewLedger"] = json!([{"candidateId":"demo-library","sourceSnapshotId":"osm:installed-fixture","decision":"accepted","decidedAtUnixMs":1710000000200_u64}]);
    value["features"] = value["features"].clone();
    serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())
}

fn diagnostic_event(case_id: &str, phase: &str, recovery: &str) -> String {
    super::diagnostics::record(
        super::diagnostics::DiagnosticLevel::Info,
        "installed.acceptance",
        phase,
        &[
            ("task", case_id),
            ("code", phase),
            ("recovery_result", recovery),
        ],
    )
    .map(|record| record.id)
    .unwrap_or_else(|| format!("installed-{case_id}-{phase}"))
}

pub(crate) fn safe_error_message(error: &str) -> String {
    error
        .split(['{', '['])
        .next()
        .unwrap_or("installed acceptance failed")
        .chars()
        .take(512)
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_report_covers_every_mandatory_issue_20_case_with_audit_fields() {
        let root = std::env::temp_dir().join(format!(
            "campus-installed-acceptance-schema-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let report_path = root.join("report.json");
        let report = write_report(&report_path, 3).unwrap();
        assert_eq!(report.status, "pass", "{:?}", report.release_blockers);
        assert_eq!(report.cases.len(), 8);
        assert_eq!(report.reliability_cycles_required, 3);
        for case in &report.cases {
            assert!(case.mandatory);
            assert_eq!(case.status, "pass");
            assert_eq!(case.input_digest_sha256.len(), 64);
            assert_eq!(case.project_summary_digest_sha256.len(), 64);
            assert!(!case.failure_point.is_empty());
            assert!(!case.expected_state.is_null());
            assert!(!case.actual_state.is_null());
            assert!(!case.event_ids.is_empty());
            assert!(!case.result_evidence.is_empty());
        }
        let reliability = report
            .cases
            .iter()
            .find(|case| case.case_id == "helper-network-exit-and-cycle-reliability")
            .expect("reliability case");
        assert_eq!(reliability.actual_state["cycles"], json!(3));
        assert!(reliability.actual_state["elapsedWallClockMs"].is_number());
        assert!(report.release_blockers.is_empty());
        assert!(report_path.is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
