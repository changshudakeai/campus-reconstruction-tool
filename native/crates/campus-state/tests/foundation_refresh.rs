use campus_state::{
    AcquisitionSuggestion, BoundaryRefreshClassification, CampusBoundaryRelationship,
    CampusProjectLibrary, CampusScope, ChangedReviewDependencies, FoundationCandidateDecision,
    FoundationCategory, FoundationResumePoint, InstallationId, KnownFeatureGapHistoryAction,
    KnownFeatureGapStatus, ObservationRefreshClassification, PinnedAcquisitionEvidence,
    PinnedBoundaryEvidence, ProviderOutcomeStatus, ResultManifest, SourceGeometry,
    SourceObservation, V11ConstructionCapability,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ObservationFixture {
    observations: Vec<SourceObservation>,
}

fn actor() -> InstallationId {
    InstallationId::new("foundation-refresh-acceptance").unwrap()
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

fn five_category_observations() -> Vec<SourceObservation> {
    let fixture: ObservationFixture = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/complex-building-review.json"
    ))
    .unwrap();
    let template = fixture
        .observations
        .into_iter()
        .find(|observation| observation.id == "obs-campus-library-v2")
        .unwrap();
    FoundationCategory::ALL
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
        .collect()
}

fn acquisition_evidence(
    observations: Vec<SourceObservation>,
    bundle_id: &str,
    result_digit: char,
) -> PinnedAcquisitionEvidence {
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
        "result_sha256": result_digit.to_string().repeat(64)
    }))
    .unwrap();
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
    PinnedAcquisitionEvidence {
        manifest,
        observations,
    }
}

fn canonical_acquisition_evidence() -> PinnedAcquisitionEvidence {
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
        observations: serde_json::from_value(fixture["observations"].clone()).unwrap(),
    }
}

fn completed_project() -> campus_state::Schema2Project {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "dependency-local refresh",
            actor(),
        )
        .unwrap();
    let mut boundary = boundary_evidence();
    boundary.manifest.bundle.id = "2026-06-controlled".into();
    boundary.manifest.bundle.osm_snapshot = "2026-06-controlled-osm".into();
    boundary.manifest.bundle.overture_release = "2026-06-controlled-overture".into();
    boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.398, 31.208],
        [121.410, 31.208],
        [121.410, 31.220],
        [121.398, 31.220],
        [121.398, 31.208],
    ]]));
    project.confirm_boundary(boundary, actor()).unwrap();
    project
        .pin_acquisition(
            acquisition_evidence(five_category_observations(), "2026-06-controlled", '1'),
            actor(),
        )
        .unwrap();
    for category in FoundationCategory::ALL {
        let ids = project
            .foundation_review_queue(category)
            .unwrap()
            .items
            .into_iter()
            .map(|item| item.subject_id)
            .collect::<Vec<_>>();
        for id in ids {
            project
                .review_foundation_candidate(
                    category,
                    &id,
                    FoundationCandidateDecision::Accept,
                    actor(),
                )
                .unwrap();
        }
        project
            .complete_foundation_category(category, actor())
            .unwrap();
    }
    project.record_generation(32, 8, 32, 512, actor()).unwrap();
    project
        .record_export("e".repeat(64), 4096, "campus.foundation.json".into())
        .unwrap();
    project
}

#[test]
fn explicit_unchanged_refresh_carries_review_and_current_formal_output_forward() {
    let mut project = completed_project();
    let revision_before = project.workflow().project_revision();
    let mut observations = five_category_observations();
    let water = observations
        .iter_mut()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap();
    water.id = "observation-water-v2".into();
    water.lineage.source_record_version = "2".into();

    let difference = project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '2'),
            None,
            actor(),
        )
        .unwrap();

    assert!(difference
        .observations
        .iter()
        .all(|change| { change.classification == ObservationRefreshClassification::Unchanged }));
    assert_eq!(project.resume_point(), FoundationResumePoint::Complete);
    assert!(project.generated_output().is_some());
    assert!(project.exported_output().is_some());
    assert!(!project
        .generated_output()
        .unwrap()
        .dependency_basis
        .subjects
        .is_empty());
    assert!(!project
        .exported_output()
        .unwrap()
        .dependency_basis
        .subjects
        .is_empty());
    assert!(!project
        .exported_output()
        .unwrap()
        .dependency_basis
        .assembly_rules
        .is_empty());
    assert!(project
        .foundation_review()
        .operations()
        .iter()
        .all(|operation| !operation.basis.dependencies.subjects.is_empty()));
    let carried_water = project
        .foundation_review()
        .operations()
        .iter()
        .rev()
        .find(|operation| {
            matches!(
                &operation.action,
                campus_state::FoundationReviewAction::Candidate { subject_id, .. }
                    if subject_id == "observation-water-v2"
            )
        })
        .unwrap();
    assert!(carried_water.carried_from_sequence.is_some());
    assert!(carried_water
        .after
        .candidate_dispositions
        .contains_key("observation-water-v2"));
    assert!(project.stale_generated_outputs().is_empty());
    assert!(project.stale_exported_outputs().is_empty());
    assert!(project.workflow().project_revision() > revision_before);
    project
        .record_export(
            "a".repeat(64),
            8192,
            "current-after-refresh.foundation.json".into(),
        )
        .expect("an unchanged refresh keeps the generated dependency basis formal");
}

#[test]
fn one_geometry_change_reopens_only_its_category_and_retains_stale_outputs() {
    let mut project = completed_project();
    let mut observations = five_category_observations();
    let water = observations
        .iter_mut()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap();
    water.id = "observation-water-v2".into();
    water.lineage.source_record_version = "2".into();
    water.geometry_sha256 = "a".repeat(64);
    water.derivation.source_geometry_sha256 = water.geometry_sha256.clone();
    water.derivation.review_geometry_sha256 = "b".repeat(64);

    let difference = project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '3'),
            None,
            actor(),
        )
        .unwrap();

    let water_change = difference
        .observations
        .iter()
        .find(|change| {
            change
                .upstream_source_record_identity
                .ends_with("stable/water")
        })
        .unwrap();
    assert_eq!(
        water_change.classification,
        ObservationRefreshClassification::Changed
    );
    assert!(water_change.changed_dependencies.geometry);
    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Water)
    );
    for category in [
        FoundationCategory::Building,
        FoundationCategory::Circulation,
    ] {
        assert!(
            project
                .foundation_review_queue(category)
                .unwrap()
                .progress
                .complete
        );
    }
    assert!(project.generated_output().is_none());
    assert!(project.exported_output().is_none());
    assert_eq!(project.stale_generated_outputs().len(), 1);
    assert_eq!(project.stale_exported_outputs().len(), 1);
    assert!(project
        .record_export("f".repeat(64), 1024, "stale.foundation.json".into())
        .unwrap_err()
        .contains("current reviewed projection"));
    let reopened: campus_state::Schema2Project =
        serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
    assert_eq!(reopened.stale_generated_outputs().len(), 1);
    assert_eq!(reopened.stale_exported_outputs().len(), 1);
    assert_eq!(
        reopened.acquisition_refresh_history()[0]
            .difference
            .as_ref()
            .unwrap()
            .current_bundle_id,
        "2026-07-controlled"
    );
}

#[test]
fn changing_rejected_evidence_reopens_review_without_staling_selected_output() {
    let mut project = completed_project();
    let mut observations = five_category_observations();
    let mut rejected = observations
        .iter()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap()
        .clone();
    rejected.id = "observation-water-rejected-v1".into();
    rejected.lineage.source_record_id = "source/water-rejected".into();
    rejected.geometry_sha256 = "7".repeat(64);
    rejected.derivation.source_geometry_sha256 = rejected.geometry_sha256.clone();
    observations.push(rejected.clone());
    project
        .apply_foundation_refresh(
            acquisition_evidence(observations.clone(), "2026-07-controlled", '7'),
            None,
            actor(),
        )
        .unwrap();
    project
        .review_foundation_candidate(
            FoundationCategory::Water,
            &rejected.id,
            FoundationCandidateDecision::Reject {
                reason: "not part of the reviewed campus model".into(),
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_category(FoundationCategory::Water, actor())
        .unwrap();
    project.record_generation(32, 8, 32, 512, actor()).unwrap();
    project
        .record_export("9".repeat(64), 4096, "selected-only.foundation.json".into())
        .unwrap();
    let stale_generated_before = project.stale_generated_outputs().len();
    let stale_exported_before = project.stale_exported_outputs().len();

    observations
        .iter_mut()
        .find(|observation| observation.id == rejected.id)
        .unwrap()
        .original_properties
        .insert("surface".into(), serde_json::json!("updated"));
    project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-08-controlled", '8'),
            None,
            actor(),
        )
        .unwrap();

    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Water)
    );
    assert!(project.generated_output().is_some());
    assert!(project.exported_output().is_some());
    assert_eq!(
        project.stale_generated_outputs().len(),
        stale_generated_before
    );
    assert_eq!(
        project.stale_exported_outputs().len(),
        stale_exported_before
    );
}

#[test]
fn withdrawn_confirmed_evidence_keeps_prior_lineage_for_review() {
    let mut project = completed_project();
    let observations = five_category_observations()
        .into_iter()
        .filter(|observation| observation.category != FoundationCategory::Sports)
        .collect();

    let difference = project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '4'),
            None,
            actor(),
        )
        .unwrap();

    let withdrawn = difference
        .observations
        .iter()
        .find(|change| change.classification == ObservationRefreshClassification::Withdrawn)
        .unwrap();
    assert!(withdrawn
        .upstream_source_record_identity
        .ends_with("stable/sports"));
    let retained = project
        .withdrawn_refresh_evidence(&withdrawn.upstream_source_record_identity)
        .unwrap();
    assert_eq!(retained.category, FoundationCategory::Sports);
    assert_eq!(retained.lineage.source_record_id, "stable/sports");
    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Sports)
    );
}

#[test]
fn withdrawing_one_subject_does_not_block_carrying_a_later_subject_operation() {
    let mut project = completed_project();
    let mut observations = five_category_observations();
    let mut second_water = observations
        .iter()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap()
        .clone();
    second_water.id = "observation-water-second-v1".into();
    second_water.lineage.source_record_id = "source/water-second".into();
    second_water.geometry_sha256 = "b".repeat(64);
    second_water.derivation.source_geometry_sha256 = second_water.geometry_sha256.clone();
    observations.push(second_water.clone());
    project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", 'b'),
            None,
            actor(),
        )
        .unwrap();
    project
        .review_foundation_candidate(
            FoundationCategory::Water,
            &second_water.id,
            FoundationCandidateDecision::Accept,
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_category(FoundationCategory::Water, actor())
        .unwrap();

    let observations = five_category_observations()
        .into_iter()
        .filter(|observation| observation.category != FoundationCategory::Water)
        .chain(std::iter::once(second_water.clone()))
        .collect();
    let difference = project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-08-controlled", 'c'),
            None,
            actor(),
        )
        .expect("a withdrawal must not make unrelated cumulative review state unremappable");

    assert!(difference.observations.iter().any(|change| {
        change.classification == ObservationRefreshClassification::Withdrawn
            && change.category == FoundationCategory::Water
    }));
    let queue = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    assert!(matches!(
        queue
            .items
            .iter()
            .find(|item| item.subject_id == second_water.id)
            .unwrap()
            .disposition,
        campus_state::CandidateReviewDisposition::Accepted
    ));
}

fn assert_water_dependency_change(
    mutate_observation: impl FnOnce(&mut SourceObservation),
    expected: impl FnOnce(&ChangedReviewDependencies) -> bool,
) {
    let mut project = completed_project();
    let mut observations = five_category_observations();
    let water = observations
        .iter_mut()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap();
    water.id = "observation-water-v2".into();
    water.lineage.source_record_version = "2".into();
    mutate_observation(water);

    let difference = project
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '5'),
            None,
            actor(),
        )
        .unwrap();

    let change = difference
        .observations
        .iter()
        .find(|change| {
            change
                .upstream_source_record_identity
                .ends_with("stable/water")
        })
        .unwrap();
    assert_eq!(
        change.classification,
        ObservationRefreshClassification::Changed
    );
    assert!(expected(&change.changed_dependencies));
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
}

#[test]
fn grouping_naming_attribute_containment_licence_and_rule_changes_are_local() {
    assert_water_dependency_change(
        |observation| {
            observation.suggestions.push(AcquisitionSuggestion {
                kind: "entity-grouping".into(),
                rule_version: "grouping-v1".into(),
                reason: "new grouping evidence".into(),
                building_entity_id: Some("water-group".into()),
                building_role: None,
                boundary_relationship: None,
                overlap_group: Some("water-overlap".into()),
            });
        },
        |changes| changes.grouping,
    );
    assert_water_dependency_change(
        |observation| {
            observation
                .original_properties
                .insert("name".into(), serde_json::json!("Renamed Pond"));
        },
        |changes| changes.naming,
    );
    assert_water_dependency_change(
        |observation| {
            observation
                .original_properties
                .insert("surface".into(), serde_json::json!("reinforced-concrete"));
        },
        |changes| changes.attribute,
    );
    assert_water_dependency_change(
        |observation| {
            observation.suggestions.push(AcquisitionSuggestion {
                kind: "boundary-containment".into(),
                rule_version: "containment-v1".into(),
                reason: "relationship changed".into(),
                building_entity_id: None,
                building_role: None,
                boundary_relationship: Some(CampusBoundaryRelationship::Straddling),
                overlap_group: None,
            });
        },
        |changes| changes.containment,
    );
    assert_water_dependency_change(
        |observation| {
            observation.licence.attribution = "Updated attribution".into();
        },
        |changes| changes.licence,
    );
    assert_water_dependency_change(
        |observation| {
            observation.derivation.rule_version = "review-geometry-v2".into();
        },
        |changes| changes.rule_version,
    );
}

#[test]
fn coverage_change_reopens_only_the_affected_category() {
    let mut project = completed_project();
    let mut refresh = acquisition_evidence(five_category_observations(), "2026-07-controlled", '6');
    refresh
        .manifest
        .coverage_report
        .outcomes
        .iter_mut()
        .find(|outcome| outcome.category == FoundationCategory::Water)
        .unwrap()
        .raw_count += 1;

    let difference = project
        .apply_foundation_refresh(refresh, None, actor())
        .unwrap();

    assert_eq!(difference.coverage.len(), 1);
    assert_eq!(difference.coverage[0].category, FoundationCategory::Water);
    assert!(difference.observations.iter().any(|change| {
        change.category == FoundationCategory::Water
            && change.classification == ObservationRefreshClassification::CoverageChanged
            && change.changed_dependencies.coverage
    }));
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
}

#[test]
fn assembly_rule_change_reopens_building_without_reopening_other_categories() {
    let mut project = completed_project();
    let mut refresh = acquisition_evidence(five_category_observations(), "2026-07-controlled", '6');
    refresh.manifest.bundle.assembly_rules = "building-assembly-v2".into();

    project
        .apply_foundation_refresh(refresh, None, actor())
        .unwrap();

    assert_eq!(
        project.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Building)
    );
    for category in [
        FoundationCategory::Circulation,
        FoundationCategory::Water,
        FoundationCategory::Vegetation,
        FoundationCategory::Sports,
    ] {
        assert!(
            project
                .foundation_review_queue(category)
                .unwrap()
                .progress
                .complete,
            "{category:?} must not depend on Building assembly rules"
        );
    }
}

#[test]
fn boundary_expansion_reviews_only_added_area_and_shrink_reassesses_removed_evidence() {
    let mut expanded = completed_project();
    let mut observations = five_category_observations();
    let mut added = observations
        .iter()
        .find(|observation| observation.category == FoundationCategory::Water)
        .unwrap()
        .clone();
    added.id = "observation-water-added-v1".into();
    added.lineage.source_record_id = "stable/water-added".into();
    added.geometry_sha256 = "c".repeat(64);
    observations.push(added);
    let mut expanded_boundary = boundary_evidence();
    expanded_boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.390, 31.200],
        [121.420, 31.200],
        [121.420, 31.230],
        [121.390, 31.230],
        [121.390, 31.200],
    ]]));
    let expansion = expanded
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '7'),
            Some(expanded_boundary),
            actor(),
        )
        .unwrap();
    assert_eq!(expansion.boundary, BoundaryRefreshClassification::Expanded);
    assert_eq!(
        expanded.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Water)
    );
    assert!(
        expanded
            .foundation_review_queue(FoundationCategory::Building)
            .unwrap()
            .progress
            .complete,
        "expansion must preserve categories with no added-area evidence"
    );

    let mut shrunk = completed_project();
    let observations = five_category_observations()
        .into_iter()
        .filter(|observation| observation.category != FoundationCategory::Sports)
        .collect();
    let mut shrunk_boundary = boundary_evidence();
    shrunk_boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.399, 31.209],
        [121.405, 31.209],
        [121.405, 31.215],
        [121.399, 31.215],
        [121.399, 31.209],
    ]]));
    let shrink = shrunk
        .apply_foundation_refresh(
            acquisition_evidence(observations, "2026-07-controlled", '8'),
            Some(shrunk_boundary),
            actor(),
        )
        .unwrap();
    assert_eq!(shrink.boundary, BoundaryRefreshClassification::Shrunk);
    assert_eq!(
        shrunk.resume_point(),
        FoundationResumePoint::Review(FoundationCategory::Sports)
    );
    assert!(
        shrunk
            .foundation_review_queue(FoundationCategory::Vegetation)
            .unwrap()
            .progress
            .complete
    );

    let mut shifted = completed_project();
    let mut shifted_boundary = boundary_evidence();
    shifted_boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.3981, 31.208],
        [121.4101, 31.208],
        [121.4101, 31.220],
        [121.3981, 31.220],
        [121.3981, 31.208],
    ]]));
    let relationship_change = shifted
        .apply_foundation_refresh(
            acquisition_evidence(five_category_observations(), "2026-07-controlled", 'a'),
            Some(shifted_boundary),
            actor(),
        )
        .unwrap();
    assert_eq!(
        relationship_change.boundary,
        BoundaryRefreshClassification::RelationshipChanged
    );
    assert_eq!(
        shifted.resume_point(),
        FoundationResumePoint::Generation,
        "moving the boundary must preserve review while making boundary-dependent output stale"
    );
    assert!(FoundationCategory::ALL.into_iter().all(|category| shifted
        .foundation_review_queue(category)
        .unwrap()
        .progress
        .complete));
    assert!(shifted.generated_output().is_none());
    assert_eq!(shifted.stale_generated_outputs().len(), 1);

    let mut concave = completed_project();
    let mut concave_boundary = boundary_evidence();
    concave_boundary.confirmed_geometry = Some(SourceGeometry::Polygon(vec![vec![
        [121.397, 31.207],
        [121.411, 31.207],
        [121.411, 31.221],
        [121.405, 31.221],
        [121.405, 31.214],
        [121.403, 31.214],
        [121.403, 31.221],
        [121.397, 31.221],
        [121.397, 31.207],
    ]]));
    let concave_change = concave
        .apply_foundation_refresh(
            acquisition_evidence(five_category_observations(), "2026-07-controlled", 'd'),
            Some(concave_boundary),
            actor(),
        )
        .unwrap();
    assert_eq!(
        concave_change.boundary,
        BoundaryRefreshClassification::RelationshipChanged,
        "an outward envelope with an inward notch is not a pure expansion"
    );
}

#[test]
fn opening_a_saved_project_is_read_only_and_never_refreshes_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "open without refresh",
            actor(),
        )
        .unwrap();
    let mut boundary = boundary_evidence();
    boundary.manifest.bundle.id = "2026-06-controlled".into();
    project.confirm_boundary(boundary, actor()).unwrap();
    project
        .pin_acquisition(
            acquisition_evidence(five_category_observations(), "2026-06-controlled", '9'),
            actor(),
        )
        .unwrap();
    library.save_project(&project).unwrap();
    let revision = project.workflow().project_revision();
    let result = project
        .pinned_evidence()
        .unwrap()
        .acquisition
        .manifest
        .result_sha256
        .clone();

    let reopened = library.open_project(project.id()).unwrap();

    assert_eq!(reopened.workflow().project_revision(), revision);
    assert_eq!(
        reopened
            .pinned_evidence()
            .unwrap()
            .acquisition
            .manifest
            .result_sha256,
        result
    );
    assert!(reopened.acquisition_refresh_history().is_empty());
}

#[test]
fn gap_resolution_and_reopening_append_evidence_linked_history_across_refreshes() {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "gap refresh history",
            actor(),
        )
        .unwrap();
    let initial = canonical_acquisition_evidence();
    let mut boundary = boundary_evidence();
    boundary.manifest.bundle = initial.manifest.bundle.clone();
    project.confirm_boundary(boundary, actor()).unwrap();
    project.pin_acquisition(initial.clone(), actor()).unwrap();
    let gap_id = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap()
        .known_gaps[0]
        .id
        .clone();
    project
        .acknowledge_feature_gap(FoundationCategory::Water, &gap_id, actor())
        .unwrap();

    let mut refreshed = initial;
    refreshed.manifest.bundle.id = "bundle-v1-gap-resolved".into();
    refreshed.manifest.bundle.osm_snapshot = "osm-gap-resolved".into();
    refreshed.manifest.result_sha256 = "d".repeat(64);
    for outcome in refreshed
        .manifest
        .coverage_report
        .outcomes
        .iter_mut()
        .filter(|outcome| outcome.category == FoundationCategory::Water)
    {
        outcome.status = ProviderOutcomeStatus::Complete;
        outcome.pagination_exhausted = true;
        outcome.relation_members_complete = true;
        outcome.gaps.clear();
        outcome.failure = None;
    }
    let mut resolving_evidence = refreshed.observations[0].clone();
    resolving_evidence.id = "observation-water-gap-resolution-v1".into();
    resolving_evidence.category = FoundationCategory::Water;
    resolving_evidence.lineage.source_record_id = "stable/water-gap-resolution".into();
    resolving_evidence.geometry_sha256 = "e".repeat(64);
    refreshed.observations.push(resolving_evidence.clone());
    project
        .apply_foundation_refresh(refreshed, None, actor())
        .unwrap();

    project
        .resolve_feature_gap(
            FoundationCategory::Water,
            &gap_id,
            vec![resolving_evidence.id.clone()],
            actor(),
        )
        .unwrap();
    let history = project.known_feature_gap_history(FoundationCategory::Water, &gap_id);
    assert!(matches!(
        history.as_slice(),
        [
            KnownFeatureGapHistoryAction::Observed,
            KnownFeatureGapHistoryAction::Acknowledged { .. },
            KnownFeatureGapHistoryAction::Resolved { .. }
        ]
    ));
    assert_eq!(
        project
            .foundation_review_queue(FoundationCategory::Water)
            .unwrap()
            .known_gaps
            .iter()
            .find(|gap| gap.id == gap_id)
            .unwrap()
            .status,
        KnownFeatureGapStatus::Resolved
    );

    project
        .reopen_feature_gap(FoundationCategory::Water, &gap_id, actor())
        .unwrap();
    let reopened = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    let reopened_gap = reopened
        .known_gaps
        .iter()
        .find(|gap| gap.id == gap_id)
        .unwrap();
    assert_eq!(reopened_gap.status, KnownFeatureGapStatus::Open);
    assert!(matches!(
        reopened_gap.history.last().unwrap(),
        KnownFeatureGapHistoryAction::Reopened { .. }
    ));

    project
        .resolve_feature_gap(
            FoundationCategory::Water,
            &gap_id,
            vec![resolving_evidence.id.clone()],
            actor(),
        )
        .unwrap();
    let mut second_refresh = project.pinned_evidence().unwrap().acquisition.clone();
    second_refresh.manifest.bundle.id = "bundle-v1-gap-evidence-changed".into();
    second_refresh.manifest.bundle.osm_snapshot = "osm-gap-evidence-changed".into();
    second_refresh.manifest.result_sha256 = "f".repeat(64);
    second_refresh
        .observations
        .iter_mut()
        .find(|observation| observation.id == resolving_evidence.id)
        .unwrap()
        .original_properties
        .insert("surface".into(), serde_json::json!("updated"));

    project
        .apply_foundation_refresh(second_refresh, None, actor())
        .unwrap();

    let automatically_reopened = project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap();
    let automatically_reopened_gap = automatically_reopened
        .known_gaps
        .iter()
        .find(|gap| gap.id == gap_id)
        .unwrap();
    assert_eq!(
        automatically_reopened_gap.status,
        KnownFeatureGapStatus::Open
    );
    assert!(matches!(
        automatically_reopened_gap.history.last().unwrap(),
        KnownFeatureGapHistoryAction::Reopened { .. }
    ));
}
