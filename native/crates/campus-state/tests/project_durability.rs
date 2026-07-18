use campus_state::{
    CampusProjectLibrary, CampusScope, InstallationId, ProjectSaveStatus, SaveFaultPoint,
    Schema2ProjectSession, V11ConstructionCapability,
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

        library.inject_next_save_failure(fault);
        assert!(session
            .apply_semantic_operation(&mut library, "faulted operation", |project| {
                project.mark_updated(actor())
            })
            .is_err());

        let confirmed = library.open_project(&project_id).unwrap();
        assert_eq!(
            confirmed.workflow().project_revision(),
            0,
            "{fault:?} must not report or expose a partial save"
        );
        assert_eq!(session.active().unwrap().workflow().project_revision(), 1);
        assert!(session.is_dirty());
    }
}
