use campus_state::{
    decode_schema2_project, AcquisitionJobState, AcquisitionRequestIdentity, CampusProjectLibrary,
    CampusScope, FoundationAcquisitionCheckpoint, InstallationId, PinnedBoundaryEvidence,
    ProjectSaveStatus, RecoveryFaultPoint, ResultManifest, SaveFaultPoint, Schema2ProjectSession,
    SourceObservation, V11ConstructionCapability, VerifiedAcquisitionChunk,
};

fn scope() -> CampusScope {
    CampusScope::new(
        "gaode:B00155J6JH",
        "East China Normal University Putuo Campus",
        [121.395, 31.202],
    )
    .unwrap()
}

fn actor() -> InstallationId {
    InstallationId::new("durability-test").unwrap()
}

fn library(root: &std::path::Path) -> CampusProjectLibrary {
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    CampusProjectLibrary::open_for_construction(root, "gaode:B00155J6JH", &capability).unwrap()
}

fn boundary_evidence() -> PinnedBoundaryEvidence {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
    ))
    .unwrap();
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

fn partial_acquisition_checkpoint() -> FoundationAcquisitionCheckpoint {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .unwrap();
    let mut manifest: ResultManifest = serde_json::from_value(serde_json::json!({
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
    .unwrap();
    let observations: Vec<SourceObservation> =
        serde_json::from_value(fixture["observations"].clone()).unwrap();
    let canonical_ndjson = observations
        .iter()
        .map(|observation| serde_json::to_string(observation).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    manifest.chunks[0].uncompressed_bytes = canonical_ndjson.len() as u64;
    let mut checkpoint = FoundationAcquisitionCheckpoint::new(
        "acquisition-job-9",
        "1.0.0",
        manifest.bundle.clone(),
        boundary_evidence().manifest.result_sha256,
        AcquisitionRequestIdentity::new(
            "installation-42:project-7:foundation-baseline",
            "a".repeat(64),
        )
        .unwrap(),
        AcquisitionJobState::Partial,
        manifest.coverage_report.outcomes.clone(),
        None,
        30,
    )
    .unwrap();
    checkpoint.record_manifest(manifest.clone()).unwrap();
    checkpoint
        .record_verified_chunk(VerifiedAcquisitionChunk {
            descriptor: manifest.chunks[0].clone(),
            canonical_ndjson,
            observations,
        })
        .unwrap();
    checkpoint
}

#[test]
fn partial_live_acquisition_survives_save_close_reopen_and_keeps_verified_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let project_id = {
        let mut library = library(directory.path());
        let project = library
            .create_project(scope(), "resumable acquisition", actor())
            .unwrap();
        let project_id = project.id().clone();
        let mut session = Schema2ProjectSession::default();
        session.open_project(&library, &project_id).unwrap();
        session
            .apply_semantic_operation(&mut library, "confirm boundary", |project| {
                project.confirm_boundary(boundary_evidence(), actor())
            })
            .unwrap();
        session
            .apply_semantic_operation(
                &mut library,
                "checkpoint partial five-category acquisition",
                |project| {
                    project.record_acquisition_checkpoint(partial_acquisition_checkpoint(), actor())
                },
            )
            .unwrap();
        project_id
    };

    let reopened_library =
        CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();
    let reopened = reopened_library.open_project(&project_id).unwrap();
    let checkpoint = reopened.acquisition_checkpoint().unwrap();

    assert_eq!(checkpoint.job_id, "acquisition-job-9");
    assert_eq!(checkpoint.state, AcquisitionJobState::Partial);
    assert_eq!(checkpoint.retention_days, 30);
    assert_eq!(checkpoint.verified_chunks.len(), 1);
    assert_eq!(checkpoint.verified_chunks[0].observations.len(), 1);
    assert_eq!(
        checkpoint.verified_chunks[0].descriptor.stable_cursor,
        "v1:observations:0001:end"
    );
    assert!(checkpoint
        .outcomes
        .iter()
        .any(|outcome| outcome.status == campus_state::ProviderOutcomeStatus::Complete));
    assert!(checkpoint
        .outcomes
        .iter()
        .any(|outcome| outcome.status == campus_state::ProviderOutcomeStatus::Failed));
    let mut corrupt_identity = serde_json::to_value(&reopened).unwrap();
    corrupt_identity["foundation"]["acquisitionCheckpoint"]["request_identity"]
        ["idempotency_key"] = serde_json::json!("");
    assert!(
        decode_schema2_project(&serde_json::to_vec(&corrupt_identity).unwrap())
            .unwrap_err()
            .contains("stable idempotency key")
    );
    let mut missing_failure = serde_json::to_value(&reopened).unwrap();
    let failed_outcome = missing_failure["foundation"]["acquisitionCheckpoint"]["outcomes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|outcome| outcome["status"] == "failed")
        .unwrap();
    failed_outcome.as_object_mut().unwrap().remove("failure");
    assert!(
        decode_schema2_project(&serde_json::to_vec(&missing_failure).unwrap())
            .unwrap_err()
            .contains("structured failure")
    );
}

#[test]
fn semantic_save_points_persist_fifty_history_operations_and_redo() {
    let directory = tempfile::tempdir().unwrap();
    let mut library = library(directory.path());
    let project = library
        .create_project(scope(), "durable project", actor())
        .unwrap();
    let project_id = project.id().clone();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id).unwrap();

    for operation in 1..=55 {
        session
            .apply_semantic_operation(
                &mut library,
                format!("confirmed operation {operation}"),
                |project| project.mark_updated(actor()),
            )
            .unwrap();
    }

    assert_eq!(session.history().len(), 50);
    assert_eq!(session.history()[0].description(), "confirmed operation 6");
    assert!(matches!(
        session.save_status(),
        ProjectSaveStatus::Saved { .. }
    ));
    assert!(!session.is_dirty());

    session.undo(&mut library).unwrap();
    session.undo(&mut library).unwrap();
    assert!(session.can_redo());
    drop(session);

    let reopened_library =
        CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();
    let mut reopened = Schema2ProjectSession::default();
    reopened
        .open_project(&reopened_library, &project_id)
        .unwrap();
    assert_eq!(reopened.history().len(), 50);
    assert!(reopened.can_redo());
    assert_eq!(reopened.active().unwrap().workflow().project_revision(), 53);
    reopened
        .apply_semantic_operation(&mut library, "new branch operation", |project| {
            project.mark_updated(actor())
        })
        .unwrap();
    assert!(
        !reopened.can_redo(),
        "a new operation clears the redo branch"
    );

    library.inject_next_save_failure(SaveFaultPoint::BeforeProjectReplace);
    reopened.undo(&mut library).unwrap_err();
    assert_eq!(
        library
            .recovery_candidate(&project_id)
            .unwrap()
            .unwrap()
            .project_revision(),
        53,
        "a coherent undo recovery may have a lower revision than its confirmed save"
    );
}

#[test]
fn failed_save_blocks_context_change_and_preserves_recoverable_working_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut library = library(directory.path());
    let first = library
        .create_project(scope(), "first project", actor())
        .unwrap();
    let second = library
        .create_project(scope(), "second project", actor())
        .unwrap();
    let first_id = first.id().clone();
    let second_id = second.id().clone();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &first_id).unwrap();

    library.inject_next_save_failure(SaveFaultPoint::BeforeProjectReplace);
    let error = session
        .apply_semantic_operation(&mut library, "confirm boundary", |project| {
            project.mark_updated(actor())
        })
        .unwrap_err();
    assert!(error.contains("injected"));
    assert_eq!(session.active().unwrap().id(), &first_id);
    assert_eq!(session.active().unwrap().workflow().project_revision(), 1);
    assert!(session.is_dirty());
    assert!(matches!(
        session.save_status(),
        ProjectSaveStatus::Failed { .. }
    ));

    library.inject_next_save_failure(SaveFaultPoint::AfterStageValidation);
    assert!(session
        .switch_project(&mut library, &second_id)
        .unwrap_err()
        .contains("injected"));
    assert_eq!(session.active().unwrap().id(), &first_id);

    let recovery = library
        .recovery_candidate(&first_id)
        .unwrap()
        .expect("failed semantic save retains recovery");
    assert_eq!(recovery.project_revision(), 1);
    assert_eq!(
        library
            .open_project(&first_id)
            .unwrap()
            .workflow()
            .project_revision(),
        0
    );

    session.retry_save(&mut library).unwrap();
    session.switch_project(&mut library, &second_id).unwrap();
    assert_eq!(session.active().unwrap().id(), &second_id);
    assert!(library.recovery_candidate(&first_id).unwrap().is_none());
}

#[test]
fn recovery_and_previous_confirmed_save_are_distinct_and_recovery_is_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let mut library = library(directory.path());
    let project = library
        .create_project(scope(), "recoverable project", actor())
        .unwrap();
    let project_id = project.id().clone();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id).unwrap();
    session
        .apply_semantic_operation(&mut library, "first confirmed edit", |project| {
            project.mark_updated(actor())
        })
        .unwrap();

    let previous = library.previous_confirmed_project(&project_id).unwrap();
    assert_eq!(previous.workflow().project_revision(), 0);

    library.inject_next_save_failure(SaveFaultPoint::BeforeProjectReplace);
    session
        .apply_semantic_operation(&mut library, "interrupted edit", |project| {
            project.mark_updated(actor())
        })
        .unwrap_err();
    drop(session);

    let mut reopened = Schema2ProjectSession::default();
    reopened.open_project(&library, &project_id).unwrap();
    assert_eq!(reopened.active().unwrap().workflow().project_revision(), 1);
    assert_eq!(
        reopened
            .available_recovery(&library)
            .unwrap()
            .unwrap()
            .project_revision(),
        2
    );
    reopened.accept_recovery(&library).unwrap();
    assert_eq!(reopened.active().unwrap().workflow().project_revision(), 2);
    assert!(reopened.is_dirty());
    assert_eq!(
        library
            .open_project(&project_id)
            .unwrap()
            .workflow()
            .project_revision(),
        1,
        "acceptance does not overwrite the confirmed save"
    );
    reopened.request_save(&mut library).unwrap();
    assert!(library.recovery_candidate(&project_id).unwrap().is_none());

    let recovery_path = library.recovery_path(&project_id).unwrap();
    std::fs::write(&recovery_path, b"{\"partial\":true").unwrap();
    assert!(library.recovery_candidate(&project_id).is_err());
    assert_eq!(
        library
            .open_project(&project_id)
            .unwrap()
            .workflow()
            .project_revision(),
        2,
        "an incoherent recovery is never merged"
    );
}

#[test]
fn injected_recovery_faults_never_replace_confirmed_state_or_drop_recoverable_work() {
    let directory = tempfile::tempdir().unwrap();
    let mut library = library(directory.path());
    let project = library
        .create_project(scope(), "recovery fault project", actor())
        .unwrap();
    let project_id = project.id().clone();
    let confirmed_revision = project.workflow().project_revision();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id).unwrap();

    library.inject_next_recovery_failure(RecoveryFaultPoint::BeforeWrite);
    let error = session
        .apply_semantic_operation(&mut library, "unsaved recovery edit", |project| {
            project.mark_updated(actor())
        })
        .unwrap_err();
    assert!(error.contains("BeforeWrite"));
    assert_eq!(
        library
            .open_project(&project_id)
            .unwrap()
            .workflow()
            .project_revision(),
        confirmed_revision
    );
    assert!(session.is_dirty());
    assert!(library.recovery_candidate(&project_id).unwrap().is_none());

    library.inject_next_recovery_failure(RecoveryFaultPoint::AfterWrite);
    let error = session.retry_save(&mut library).unwrap_err();
    assert!(error.contains("AfterWrite"));
    assert!(library.recovery_candidate(&project_id).unwrap().is_some());

    library.inject_next_recovery_failure(RecoveryFaultPoint::BeforeRead);
    assert!(session
        .available_recovery(&library)
        .unwrap_err()
        .contains("BeforeRead"));
    assert!(library.recovery_candidate(&project_id).unwrap().is_some());

    library.inject_next_recovery_failure(RecoveryFaultPoint::BeforeDiscard);
    assert!(session
        .discard_recovery(&library)
        .unwrap_err()
        .contains("BeforeDiscard"));
    assert!(
        library.recovery_candidate(&project_id).unwrap().is_some(),
        "a failed discard must leave the coherent recovery candidate available"
    );
}

#[test]
fn every_injected_save_stage_keeps_the_previous_confirmed_project_valid() {
    for fault in SaveFaultPoint::ALL {
        let directory = tempfile::tempdir().unwrap();
        let mut library = library(directory.path());
        let project = library
            .create_project(scope(), format!("fault {fault:?}"), actor())
            .unwrap();
        let project_id = project.id().clone();
        let mut session = Schema2ProjectSession::default();
        session.open_project(&library, &project_id).unwrap();
        session
            .apply_semantic_operation(&mut library, "baseline operation", |project| {
                project.mark_updated(actor())
            })
            .unwrap();
        assert_eq!(
            library
                .previous_confirmed_project(&project_id)
                .unwrap()
                .workflow()
                .project_revision(),
            0
        );

        library.inject_next_save_failure(fault);
        assert!(session
            .apply_semantic_operation(&mut library, "faulted operation", |project| {
                project.mark_updated(actor())
            })
            .is_err());

        let confirmed = library.open_project(&project_id).unwrap();
        assert_eq!(
            confirmed.workflow().project_revision(),
            1,
            "{fault:?} must not report or expose a partial save"
        );
        assert_eq!(
            library
                .previous_confirmed_project(&project_id)
                .unwrap()
                .workflow()
                .project_revision(),
            0,
            "{fault:?} must not advance the retained rollback point"
        );
        assert_eq!(session.active().unwrap().workflow().project_revision(), 2);
        assert!(session.is_dirty());
    }
}

#[test]
fn process_interruption_at_every_save_stage_rolls_back_on_library_reopen() {
    for fault in SaveFaultPoint::ALL {
        let directory = tempfile::tempdir().unwrap();
        let mut library = library(directory.path());
        let project = library
            .create_project(scope(), format!("interruption {fault:?}"), actor())
            .unwrap();
        let project_id = project.id().clone();
        let mut session = Schema2ProjectSession::default();
        session.open_project(&library, &project_id).unwrap();
        session
            .apply_semantic_operation(&mut library, "baseline operation", |project| {
                project.mark_updated(actor())
            })
            .unwrap();

        library.inject_next_save_interruption(fault);
        session
            .apply_semantic_operation(&mut library, "interrupted operation", |project| {
                project.mark_updated(actor())
            })
            .unwrap_err();
        drop(session);
        drop(library);

        let reopened = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();
        assert_eq!(
            reopened
                .open_project(&project_id)
                .unwrap()
                .workflow()
                .project_revision(),
            1,
            "{fault:?} must roll back an interrupted multi-file save"
        );
        assert_eq!(
            reopened
                .previous_confirmed_project(&project_id)
                .unwrap()
                .workflow()
                .project_revision(),
            0,
            "{fault:?} must retain the prior rollback point"
        );
        assert_eq!(
            reopened
                .recovery_candidate(&project_id)
                .unwrap()
                .unwrap()
                .project_revision(),
            2,
            "{fault:?} must retain the coherent unsaved recovery candidate"
        );
    }
}
