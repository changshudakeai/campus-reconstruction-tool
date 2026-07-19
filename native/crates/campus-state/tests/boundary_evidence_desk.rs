use campus_state::{
    BoundaryCandidateAssessment, BoundaryCandidateDerivation, BoundaryCandidateValidity,
    BoundaryDiscoverySnapshot, BoundaryEdgeRef, BoundaryEvidenceAvailability, BoundaryEvidenceDesk,
    BoundaryInteractionMode, BoundaryRecoveryAction, BoundaryVertexRef, CampusProjectLibrary,
    CampusScope, InstallationId, PinnedBoundaryEvidence, ResultManifest, Schema2ProjectSession,
    SourceGeometry, V11ConstructionCapability,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn fixture_snapshot() -> BoundaryDiscoverySnapshot {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
    ))
    .unwrap();
    let manifest = serde_json::from_value::<ResultManifest>(serde_json::json!({
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
        "result_sha256": fixture["manifest"]["result_sha256"],
    }))
    .unwrap();
    let candidates = serde_json::from_value(fixture["candidates"].clone()).unwrap();
    let assessments = [
        (
            "boundary-osm-relation-100".to_string(),
            BoundaryCandidateAssessment {
                validity: BoundaryCandidateValidity::Valid,
                derivation: BoundaryCandidateDerivation {
                    source_records: vec!["relation/100".into()],
                    rule_versions: vec!["assembly-1.0.0".into()],
                    steps: vec!["assembled complete relation".into()],
                },
            },
        ),
        (
            "boundary-overture-land-200".to_string(),
            BoundaryCandidateAssessment {
                validity: BoundaryCandidateValidity::Invalid {
                    reasons: vec!["candidate relation is incomplete".into()],
                },
                derivation: BoundaryCandidateDerivation {
                    source_records: vec!["overture:land:200".into()],
                    rule_versions: vec!["assembly-1.0.0".into()],
                    steps: vec!["retained for diagnosis".into()],
                },
            },
        ),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    BoundaryDiscoverySnapshot {
        manifest,
        candidates,
        assessments,
    }
}

fn actor() -> InstallationId {
    InstallationId::new("boundary-desk-test").unwrap()
}

fn scope() -> CampusScope {
    CampusScope::new(
        "gaode:B00155J6JH",
        "East China Normal University Putuo Campus",
        [121.395, 31.202],
    )
    .unwrap()
}

#[test]
fn review_mode_blocks_changes_until_a_valid_candidate_and_handle_are_selected() {
    let mut desk = BoundaryEvidenceDesk::new(fixture_snapshot()).unwrap();

    assert_eq!(desk.mode(), BoundaryInteractionMode::Review);
    assert!(desk.projection().can_confirm);
    assert!(desk.move_selected_vertex([121.401, 31.221]).is_err());

    desk.select_candidate("boundary-overture-land-200").unwrap();
    let invalid = desk.projection();
    assert!(!invalid.can_adjust);
    assert!(!invalid.can_confirm);
    assert_eq!(
        invalid.confirmation_blocked_reason.as_deref(),
        Some("candidate relation is incomplete")
    );
    assert!(desk.enter_adjustment().is_err());

    desk.select_candidate("boundary-osm-relation-100").unwrap();
    desk.enter_adjustment().unwrap();
    assert_eq!(desk.mode(), BoundaryInteractionMode::Adjustment);
    assert!(desk.move_selected_vertex([121.401, 31.221]).is_err());

    desk.select_vertex(BoundaryVertexRef::outer(0, 1)).unwrap();
    desk.move_selected_vertex([121.411, 31.218]).unwrap();
    assert!(desk.projection().geometry_validity.valid);
}

#[test]
fn vertex_and_edge_changes_are_reversible_and_pin_the_edited_geometry() {
    let snapshot = fixture_snapshot();
    let original = snapshot.candidates[0].geometry.clone();
    let mut desk = BoundaryEvidenceDesk::new(snapshot).unwrap();
    desk.enter_adjustment().unwrap();

    desk.select_edge(BoundaryEdgeRef::outer(0, 1)).unwrap();
    let inserted = desk.insert_vertex_on_selected_edge().unwrap();
    assert_eq!(inserted, BoundaryVertexRef::outer(0, 2));
    assert_eq!(desk.projection().vertex_count, 5);

    desk.delete_selected_vertex().unwrap();
    assert_eq!(desk.projection().vertex_count, 4);
    desk.select_vertex(BoundaryVertexRef::outer(0, 1)).unwrap();
    desk.move_selected_vertex([121.411, 31.218]).unwrap();

    let edited = desk.working_geometry().unwrap().clone();
    assert_ne!(edited, original);
    desk.undo_adjustment().unwrap();
    assert_eq!(desk.working_geometry(), Some(&original));

    desk.select_vertex(BoundaryVertexRef::outer(0, 1)).unwrap();
    desk.move_selected_vertex([121.411, 31.218]).unwrap();
    desk.restore_candidate_original().unwrap();
    assert_eq!(desk.working_geometry(), Some(&original));

    desk.undo_adjustment().unwrap();
    let pinned: PinnedBoundaryEvidence = desk.to_pinned_evidence().unwrap();
    assert_eq!(
        pinned.confirmed_geometry.as_ref(),
        desk.working_geometry(),
        "confirmation retains the edited review geometry separately from source candidates"
    );
    assert_eq!(
        pinned.manifest.bundle.id, "cn-campus-2026-06",
        "confirmation pins the discovery Dataset Bundle"
    );
}

#[test]
fn unavailable_and_all_invalid_results_only_offer_retry_or_campus_return() {
    let unavailable = BoundaryEvidenceDesk::unavailable(
        "controlled service cancelled the boundary job",
        "Retry the same pinned boundary job",
    );
    let projection = unavailable.projection();
    assert_eq!(
        projection.availability,
        BoundaryEvidenceAvailability::Unavailable
    );
    assert_eq!(
        projection.recovery_actions,
        vec![
            BoundaryRecoveryAction::Retry,
            BoundaryRecoveryAction::ReturnToCampusTarget
        ]
    );
    assert!(!projection.can_confirm);
    assert!(!projection.can_adjust);
    assert!(unavailable.working_geometry().is_none());

    let mut snapshot = fixture_snapshot();
    for assessment in snapshot.assessments.values_mut() {
        assessment.validity = BoundaryCandidateValidity::Invalid {
            reasons: vec!["no complete closed education-area ring".into()],
        };
    }
    let all_invalid = BoundaryEvidenceDesk::new(snapshot).unwrap();
    assert_eq!(
        all_invalid.projection().availability,
        BoundaryEvidenceAvailability::AllInvalid
    );
    assert_eq!(
        all_invalid
            .projection()
            .confirmation_blocked_reason
            .as_deref(),
        Some("no complete closed education-area ring")
    );
    assert!(
        !matches!(
            all_invalid.working_geometry(),
            Some(SourceGeometry::Polygon(rings)) if rings.is_empty()
        ),
        "invalid evidence stays diagnosable and never turns into a blank drawing canvas"
    );
}

#[test]
fn local_geometry_validation_is_authoritative_for_candidate_adjustment() {
    let mut snapshot = fixture_snapshot();
    snapshot.candidates[0].geometry =
        SourceGeometry::LineString(vec![[121.395, 31.202], [121.405, 31.212]]);
    let mut desk = BoundaryEvidenceDesk::new(snapshot).unwrap();

    assert!(matches!(
        desk.candidate_validity("boundary-osm-relation-100"),
        BoundaryCandidateValidity::Invalid { .. }
    ));
    assert_eq!(
        desk.projection().availability,
        BoundaryEvidenceAvailability::AllInvalid
    );
    assert!(!desk.projection().can_adjust);
    assert!(desk.enter_adjustment().is_err());
}

#[test]
fn boundary_adjustments_and_restore_are_durable_semantic_project_operations() {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library = CampusProjectLibrary::open_for_construction(
        directory.path(),
        "gaode:B00155J6JH",
        &capability,
    )
    .unwrap();
    let project_id = library
        .create_project(scope(), "automatic boundary desk", actor())
        .unwrap()
        .id()
        .clone();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, &project_id).unwrap();
    session
        .apply_semantic_operation(
            &mut library,
            "load Boundary Discovery Snapshot",
            |project| project.begin_boundary_review(fixture_snapshot(), actor()),
        )
        .unwrap();
    let original = session
        .active()
        .unwrap()
        .boundary_review()
        .unwrap()
        .working_geometry()
        .unwrap()
        .clone();
    session
        .apply_semantic_operation(&mut library, "move boundary vertex 2", |project| {
            project.edit_boundary_review(actor(), |desk| {
                desk.enter_adjustment()?;
                desk.select_vertex(BoundaryVertexRef::outer(0, 1))?;
                desk.move_selected_vertex([121.411, 31.218])?;
                desk.leave_adjustment();
                Ok(())
            })
        })
        .unwrap();
    assert_ne!(
        session
            .active()
            .unwrap()
            .boundary_review()
            .unwrap()
            .working_geometry(),
        Some(&original)
    );

    session.undo(&mut library).unwrap();
    assert_eq!(
        session
            .active()
            .unwrap()
            .boundary_review()
            .unwrap()
            .working_geometry(),
        Some(&original)
    );

    session.redo(&mut library).unwrap();
    session
        .apply_semantic_operation(&mut library, "restore automatic boundary", |project| {
            project.edit_boundary_review(actor(), |desk| {
                desk.enter_adjustment()?;
                desk.restore_candidate_original()?;
                desk.leave_adjustment();
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(
        session
            .active()
            .unwrap()
            .boundary_review()
            .unwrap()
            .working_geometry(),
        Some(&original)
    );

    session.undo(&mut library).unwrap();
    session
        .apply_semantic_operation(
            &mut library,
            "persist edited boundary and queue five-category acquisition",
            |project| {
                let evidence = project
                    .boundary_review()
                    .ok_or("Boundary review disappeared")?
                    .to_pinned_evidence()?;
                project.confirm_boundary_and_queue_acquisition(
                    evidence,
                    "installation-42:boundary-baseline",
                    actor(),
                )
            },
        )
        .unwrap();
    let reopened = library.open_project(&project_id).unwrap();
    let evidence = reopened.boundary_evidence().unwrap();
    assert_ne!(evidence.confirmed_geometry(), Some(&original));
    assert_eq!(evidence.manifest.bundle.id, "cn-campus-2026-06");
    assert_eq!(
        reopened
            .pending_acquisition_start()
            .unwrap()
            .idempotency_key,
        "installation-42:boundary-baseline"
    );
    assert!(reopened.boundary_review().is_none());
}
