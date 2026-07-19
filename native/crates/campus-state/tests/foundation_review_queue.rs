use campus_state::{
    AcquisitionSuggestion, CampusProjectLibrary, CampusScope, CandidateReviewDisposition,
    FoundationBatchDecision, FoundationBatchReview, FoundationCandidateDecision,
    FoundationCategory, FoundationResumePoint, InstallationId, PinnedAcquisitionEvidence,
    PinnedBoundaryEvidence, ResultManifest, ReviewConflictResolution, SourceGeometry,
    SourceObservation, V11ConstructionCapability,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ReviewFixture {
    observations: Vec<SourceObservation>,
}

fn actor() -> InstallationId {
    InstallationId::new("foundation-review-queue-acceptance").unwrap()
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

fn acquisition_evidence(observations: Vec<SourceObservation>) -> PinnedAcquisitionEvidence {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .unwrap();
    PinnedAcquisitionEvidence {
        manifest: serde_json::from_value::<ResultManifest>(serde_json::json!({
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
        .unwrap(),
        observations,
    }
}

fn review_observations() -> Vec<SourceObservation> {
    let mut fixture: ReviewFixture = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/complex-building-review.json"
    ))
    .unwrap();
    let mut sports_container = fixture
        .observations
        .iter()
        .find(|item| item.id == "obs-campus-library-v2")
        .unwrap()
        .clone();
    sports_container.id = "obs-sports-container".into();
    sports_container.category = FoundationCategory::Sports;
    sports_container.original_properties = [("leisure".into(), serde_json::json!("sports_centre"))]
        .into_iter()
        .collect();
    sports_container.suggestions = vec![AcquisitionSuggestion {
        kind: "containment".into(),
        rule_version: "sports-containment-v1".into(),
        reason: "The outer sports campus is a container, not a filled pitch".into(),
        building_entity_id: None,
        building_role: None,
        boundary_relationship: None,
        overlap_group: Some("sports-containment".into()),
    }];
    let mut sports_track = sports_container.clone();
    sports_track.id = "obs-running-track".into();
    sports_track.original_properties = [("leisure".into(), serde_json::json!("track"))]
        .into_iter()
        .collect();
    let mut sports_hall = sports_container.clone();
    sports_hall.id = "obs-sports-hall".into();
    sports_hall.category = FoundationCategory::Building;
    sports_hall.original_properties = [("building".into(), serde_json::json!("sports_hall"))]
        .into_iter()
        .collect();
    fixture
        .observations
        .retain(|item| !item.id.ends_with("-replay"));
    let circulation = fixture
        .observations
        .iter_mut()
        .find(|item| item.id == "obs-path")
        .unwrap();
    circulation.review_geometry_proposal =
        SourceGeometry::LineString(vec![[121.4, 31.209], [121.403, 31.209]]);
    circulation.derivation.review_geometry_sha256 =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
    circulation
        .derivation
        .steps
        .push("clip_continuous_feature_to_confirmed_boundary".into());
    fixture
        .observations
        .extend([sports_container, sports_track, sports_hall]);
    fixture.observations
}

fn canonical_observations() -> Vec<SourceObservation> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .unwrap();
    serde_json::from_value(fixture["observations"].clone()).unwrap()
}

fn project_with(observations: Vec<SourceObservation>) -> campus_state::Schema2Project {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "five-category review",
            actor(),
        )
        .unwrap();
    project
        .confirm_boundary(boundary_evidence(), actor())
        .unwrap();
    project
        .pin_acquisition(acquisition_evidence(observations), actor())
        .unwrap();
    project
}

#[test]
fn five_category_queue_exposes_independent_progress_evidence_and_provider_state() {
    let project = project_with(review_observations());

    let queues =
        FoundationCategory::ALL.map(|category| project.foundation_review_queue(category).unwrap());

    assert_eq!(queues.len(), 5);
    assert!(queues.iter().all(|queue| queue.progress.total > 0));
    assert!(queues
        .iter()
        .all(|queue| !queue.provider_outcomes.is_empty()));
    assert!(queues.iter().all(|queue| !queue.progress.complete));
    assert!(queues.iter().all(|queue| {
        queue.items.iter().all(|item| {
            !item.source_summary.is_empty()
                && !item.lineage_summary.is_empty()
                && !item.provenance_summary.is_empty()
                && item.assessment.geometry.reason.len() > 3
                && item.assessment.semantics.reason.len() > 3
        })
    }));

    let circulation = &queues[1].items[0];
    assert_eq!(circulation.geometry_form.as_str(), "centreline");
    assert_eq!(circulation.subtype.as_deref(), Some("footway"));
    assert_eq!(
        circulation.source_geometry,
        SourceGeometry::LineString(vec![[121.399, 31.209], [121.404, 31.209]])
    );
    assert_ne!(
        circulation.source_geometry, circulation.review_geometry,
        "traceable clipping must not replace source geometry"
    );
    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Building)
    );
}

#[test]
fn exact_subject_batch_is_atomic_and_never_completes_the_category() {
    let mut project = project_with(review_observations());
    let queue = project
        .foundation_review_queue(FoundationCategory::Vegetation)
        .unwrap();
    let subjects = queue
        .items
        .iter()
        .map(|item| item.subject_id.clone())
        .collect::<Vec<_>>();
    let ledger_len = project.foundation_review().operations().len();

    let mut stale_subjects = subjects.clone();
    stale_subjects.push("withdrawn-observation".into());
    let error = project
        .batch_review_foundation(
            FoundationBatchReview {
                category: FoundationCategory::Vegetation,
                exact_subject_ids: stale_subjects,
                expected_basis: queue.basis.clone(),
                expected_ledger_sequence: queue.ledger_sequence,
                decision: FoundationBatchDecision::Accept,
            },
            actor(),
        )
        .unwrap_err();
    assert!(error.contains("exact subject set"));
    assert_eq!(project.foundation_review().operations().len(), ledger_len);

    project
        .batch_review_foundation(
            FoundationBatchReview {
                category: FoundationCategory::Vegetation,
                exact_subject_ids: subjects,
                expected_basis: queue.basis,
                expected_ledger_sequence: queue.ledger_sequence,
                decision: FoundationBatchDecision::Accept,
            },
            actor(),
        )
        .unwrap();
    let reviewed = project
        .foundation_review_queue(FoundationCategory::Vegetation)
        .unwrap();
    assert_eq!(reviewed.progress.pending, 0);
    assert!(!reviewed.progress.complete);
    assert_eq!(
        project.foundation_review().operations().len(),
        ledger_len + 1
    );
    let operation = project.foundation_review().operations().last().unwrap();
    assert!(operation
        .before
        .candidate_dispositions
        .values()
        .all(|state| matches!(state, CandidateReviewDisposition::Pending)));
    assert!(operation
        .after
        .candidate_dispositions
        .values()
        .all(|state| matches!(state, CandidateReviewDisposition::Accepted)));
    assert!(!operation.after.category_complete);
}

#[test]
fn deferred_observation_requires_an_acknowledged_gap_and_never_projects() {
    let original = review_observations();
    let mut project = project_with(original.clone());
    let water_queue = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    let gap_id = water_queue.known_gaps[0].id.clone();
    let water_id = water_queue.items[0].subject_id.clone();

    let error = project
        .review_foundation_candidate(
            FoundationCategory::Water,
            &water_id,
            FoundationCandidateDecision::Defer {
                structured_reason: "relation membership is incomplete".into(),
                acknowledged_gap_id: gap_id.clone(),
            },
            actor(),
        )
        .unwrap_err();
    assert!(error.contains("acknowledged Known Feature Gap"));

    project
        .acknowledge_feature_gap(FoundationCategory::Water, &gap_id, actor())
        .unwrap();
    project
        .review_foundation_candidate(
            FoundationCategory::Water,
            &water_id,
            FoundationCandidateDecision::Defer {
                structured_reason: "relation membership is incomplete".into(),
                acknowledged_gap_id: gap_id,
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_category(FoundationCategory::Water, actor())
        .unwrap();

    let queue = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    assert!(queue.progress.complete);
    assert!(matches!(
        queue.items[0].disposition,
        CandidateReviewDisposition::Deferred { .. }
    ));
    assert_eq!(
        project.pinned_evidence().unwrap().acquisition.observations,
        original
    );
    assert!(project
        .reviewed_features_for_completed_category(FoundationCategory::Water)
        .unwrap()
        .is_empty());
}

#[test]
fn conflicts_and_sports_containment_block_completion_until_resolved() {
    let mut project = project_with(review_observations());
    for item in project
        .foundation_review_queue(FoundationCategory::Sports)
        .unwrap()
        .items
    {
        project
            .review_foundation_candidate(
                FoundationCategory::Sports,
                &item.subject_id,
                FoundationCandidateDecision::Accept,
                actor(),
            )
            .unwrap();
    }
    let conflict_id = "suggestion:sports:sports-containment";
    assert!(project
        .foundation_review_queue(FoundationCategory::Sports)
        .unwrap()
        .conflicts
        .iter()
        .any(|conflict| conflict.id == conflict_id));
    assert!(project
        .complete_foundation_category(FoundationCategory::Sports, actor())
        .unwrap_err()
        .contains("unresolved conflict"));

    project
        .resolve_foundation_review_conflict(
            FoundationCategory::Sports,
            conflict_id,
            ReviewConflictResolution::Containment {
                container_id: "obs-sports-container".into(),
                member_id: "obs-running-track".into(),
                container_generates_surface: false,
            },
            actor(),
        )
        .unwrap();
    let gap_ids = project
        .foundation_review_queue(FoundationCategory::Sports)
        .unwrap()
        .known_gaps
        .into_iter()
        .map(|gap| gap.id)
        .collect::<Vec<_>>();
    for gap_id in gap_ids {
        project
            .acknowledge_feature_gap(FoundationCategory::Sports, &gap_id, actor())
            .unwrap();
    }
    project
        .complete_foundation_category(FoundationCategory::Sports, actor())
        .unwrap();
    let selected = project
        .reviewed_features_for_completed_category(FoundationCategory::Sports)
        .unwrap();
    assert!(!selected
        .iter()
        .any(|feature| feature.id == "obs-sports-container"));
    assert!(selected
        .iter()
        .any(|feature| feature.id == "obs-running-track"));
}

#[test]
fn reopening_chooses_the_earliest_incomplete_category_while_inspection_is_read_only() {
    let mut project = project_with(canonical_observations());
    project
        .review_foundation_candidate(
            FoundationCategory::Building,
            "obs-osm-relation-42",
            FoundationCandidateDecision::Accept,
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_category(FoundationCategory::Building, actor())
        .unwrap();
    project
        .complete_foundation_category(FoundationCategory::Circulation, actor())
        .unwrap();
    for category in [
        FoundationCategory::Water,
        FoundationCategory::Vegetation,
        FoundationCategory::Sports,
    ] {
        let gap_id = project
            .foundation_review_queue(category)
            .unwrap()
            .known_gaps[0]
            .id
            .clone();
        project
            .acknowledge_feature_gap(category, &gap_id, actor())
            .unwrap();
        project
            .complete_foundation_category(category, actor())
            .unwrap();
    }
    assert_eq!(project.resume_point(), FoundationResumePoint::Generation);

    let revision_before_inspection = project.workflow().project_revision();
    let inspected = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    assert!(inspected.progress.complete);
    assert_eq!(
        project.workflow().project_revision(),
        revision_before_inspection,
        "viewing a completed category must not reopen or save it"
    );

    let water_gap_id = inspected.known_gaps[0].id.clone();
    project
        .reopen_feature_gap(FoundationCategory::Water, &water_gap_id, actor())
        .unwrap();
    assert!(
        !project
            .foundation_review()
            .operations()
            .last()
            .unwrap()
            .after
            .category_complete
    );
    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Water)
    );
    assert!(
        project
            .foundation_review_queue(FoundationCategory::Building)
            .unwrap()
            .progress
            .complete
    );
    assert!(
        project
            .foundation_review_queue(FoundationCategory::Circulation)
            .unwrap()
            .progress
            .complete
    );
}

#[test]
fn building_queue_routes_through_stable_entities_and_acknowledged_name_gaps() {
    let mut project = project_with(review_observations());
    project
        .initialize_building_entity_review(Vec::new(), actor())
        .unwrap();
    let primary_ids = project
        .building_entity_review()
        .entities()
        .iter()
        .map(|entity| entity.primary_observation_id.clone())
        .collect::<Vec<_>>();
    for subject_id in &primary_ids {
        project
            .review_foundation_candidate(
                FoundationCategory::Building,
                subject_id,
                FoundationCandidateDecision::Accept,
                actor(),
            )
            .unwrap();
    }
    if primary_ids.len() > 1 {
        assert!(project
            .revoke_foundation_candidate_review(
                FoundationCategory::Building,
                &primary_ids[0],
                actor(),
            )
            .unwrap_err()
            .contains("not the latest reversible"));
    }
    let gap_ids = project
        .foundation_review_queue(FoundationCategory::Building)
        .unwrap()
        .known_gaps
        .into_iter()
        .map(|gap| gap.id)
        .collect::<Vec<_>>();
    assert!(!gap_ids.is_empty());
    for gap_id in gap_ids {
        project
            .acknowledge_feature_gap(FoundationCategory::Building, &gap_id, actor())
            .unwrap();
    }
    let queue = project
        .foundation_review_queue(FoundationCategory::Building)
        .unwrap();
    assert_eq!(queue.progress.pending, 0);
    project
        .complete_foundation_category(FoundationCategory::Building, actor())
        .unwrap();
    assert!(
        project
            .foundation_review_queue(FoundationCategory::Building)
            .unwrap()
            .progress
            .complete
    );
}

#[test]
fn changed_review_basis_drops_stale_decisions_and_incomplete_outcomes_always_create_gaps() {
    let observations = review_observations();
    let mut project = project_with(observations.clone());
    let water_id = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap()
        .items[0]
        .subject_id
        .clone();
    project
        .review_foundation_candidate(
            FoundationCategory::Water,
            &water_id,
            FoundationCandidateDecision::Accept,
            actor(),
        )
        .unwrap();

    let mut refreshed = acquisition_evidence(observations);
    refreshed.manifest.result_sha256 =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    let sports_outcome = refreshed
        .manifest
        .coverage_report
        .outcomes
        .iter_mut()
        .find(|outcome| outcome.category == FoundationCategory::Sports)
        .unwrap();
    sports_outcome.gaps.clear();
    project.pin_acquisition(refreshed, actor()).unwrap();

    assert!(matches!(
        project
            .foundation_review_queue(FoundationCategory::Water)
            .unwrap()
            .items[0]
            .disposition,
        CandidateReviewDisposition::Pending
    ));
    assert_eq!(
        project
            .foundation_review_queue(FoundationCategory::Sports)
            .unwrap()
            .known_gaps
            .len(),
        1,
        "cancelled/failed/partial outcomes must create a blocker even without explicit gap strings"
    );
}

#[test]
fn reviewed_model_projects_evidence_backed_conflict_resolutions() {
    let mut observations = review_observations();
    observations
        .iter_mut()
        .for_each(|observation| observation.suggestions.clear());
    let mut supporting_water = observations
        .iter()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap()
        .clone();
    supporting_water.id = "obs-water-supporting-name".into();
    supporting_water.lineage.source_record_id = "way/water-supporting-name".into();
    supporting_water.geometry_sha256 =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into();
    observations.push(supporting_water);
    let mut project = project_with(observations);
    let water_id = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap()
        .items[0]
        .subject_id
        .clone();
    project
        .declare_foundation_review_conflict(
            campus_state::FoundationReviewConflict {
                id: "water-name".into(),
                category: FoundationCategory::Water,
                subject_ids: vec![water_id.clone(), "obs-water-supporting-name".into()],
                kind: campus_state::FoundationReviewConflictKind::Naming,
                explanation: "Two retained labels require a reviewed name".into(),
            },
            actor(),
        )
        .unwrap();
    project
        .resolve_foundation_review_conflict(
            FoundationCategory::Water,
            "water-name",
            ReviewConflictResolution::Naming {
                subject_id: water_id.clone(),
                display_name: "East Pond".into(),
                evidence_ids: vec![water_id.clone()],
            },
            actor(),
        )
        .unwrap();

    for category in FoundationCategory::ALL {
        let queue = project.foundation_review_queue(category).unwrap();
        for item in queue.items {
            if matches!(item.disposition, CandidateReviewDisposition::Pending) {
                project
                    .review_foundation_candidate(
                        category,
                        &item.subject_id,
                        FoundationCandidateDecision::Accept,
                        actor(),
                    )
                    .unwrap();
            }
        }
        let gaps = project
            .foundation_review_queue(category)
            .unwrap()
            .known_gaps;
        for gap in gaps.into_iter().filter(|gap| !gap.acknowledged) {
            project
                .acknowledge_feature_gap(category, &gap.id, actor())
                .unwrap();
        }
        project
            .complete_foundation_category(category, actor())
            .unwrap();
    }

    let projection = project.reviewed_projection().unwrap();
    let resolution = projection
        .reviewed_feature_resolutions
        .iter()
        .find(|resolution| resolution.subject_id == water_id)
        .unwrap();
    assert_eq!(resolution.display_name.as_deref(), Some("East Pond"));
    assert_eq!(resolution.name_evidence_ids, vec![water_id]);
}

#[test]
fn initialized_building_batch_records_one_exact_set_ledger_group() {
    let mut project = project_with(review_observations());
    project
        .initialize_building_entity_review(Vec::new(), actor())
        .unwrap();
    let queue = project
        .foundation_review_queue(FoundationCategory::Building)
        .unwrap();
    let exact_subject_ids = queue
        .items
        .iter()
        .map(|item| item.subject_id.clone())
        .collect::<Vec<_>>();
    let operation_count = project.foundation_review().operations().len();
    project
        .batch_review_foundation(
            FoundationBatchReview {
                category: FoundationCategory::Building,
                exact_subject_ids: exact_subject_ids.clone(),
                expected_basis: queue.basis,
                expected_ledger_sequence: queue.ledger_sequence,
                decision: FoundationBatchDecision::Accept,
            },
            actor(),
        )
        .unwrap();
    assert_eq!(
        project.foundation_review().operations().len(),
        operation_count + 1
    );
    let group = project.foundation_review().operations().last().unwrap();
    assert_eq!(group.subjects, exact_subject_ids);
    assert!(matches!(
        group.action,
        campus_state::FoundationReviewAction::Batch {
            decision: FoundationBatchDecision::Accept
        }
    ));
    assert!(group.explanation.as_deref().unwrap().contains("exact-set"));
}

#[test]
fn building_candidate_revoke_restores_the_whole_atomic_entity_action() {
    let mut project = project_with(review_observations());
    project
        .initialize_building_entity_review(Vec::new(), actor())
        .unwrap();
    let before = project.building_entity_review().entities()[0].clone();
    let subject_id = before.primary_observation_id.clone();
    let entry_count = project.building_entity_review().entries().len();
    project
        .review_foundation_candidate(
            FoundationCategory::Building,
            &subject_id,
            FoundationCandidateDecision::Accept,
            actor(),
        )
        .unwrap();
    assert_eq!(
        project.building_entity_review().entries().len(),
        entry_count + 1,
        "one queue action must be one entity-ledger entry"
    );
    project
        .revoke_foundation_candidate_review(FoundationCategory::Building, &subject_id, actor())
        .unwrap();
    assert_eq!(
        project
            .building_entity_review()
            .entities()
            .iter()
            .find(|entity| entity.id == before.id)
            .unwrap(),
        &before
    );
}
