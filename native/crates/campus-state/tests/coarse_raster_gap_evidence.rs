use campus_state::{
    AcquisitionRequestIdentity, CampusProjectLibrary, CampusScope, CandidateEvidenceAssessment,
    CoarseRasterAlgorithmProfile, CoarseRasterClip, CoarseRasterDecision, CoarseRasterExclusion,
    CoarseRasterExclusionReason, CoarseRasterGrid, CoarseRasterObservation, CoarseRasterRunOutcome,
    CoarseRasterSource, CoarseRasterSubject, CoarseRasterSupplementRun,
    EvidenceAssessmentDimension, FoundationCategory, InstallationId, LicenceRecord,
    PinnedAcquisitionEvidence, PinnedBoundaryEvidence, ResultChunk, ResultManifest, ServiceFailure,
    SourceGeometry, SourceObservation, V11ConstructionCapability,
    COARSE_RASTER_APPROXIMATE_COVERAGE_WARNING, COARSE_RASTER_FINISHING_WARNING,
    COARSE_RASTER_PIXEL_EDGE_WARNING,
};
use std::collections::BTreeMap;

fn algorithm_profile() -> CoarseRasterAlgorithmProfile {
    CoarseRasterAlgorithmProfile {
        algorithm_version: "coarse-gap-water-v1.0.0".into(),
        vectorization_version: "gdal-polygonize-3.11+simplify-v1".into(),
        thresholds: BTreeMap::from([
            ("minimum_probability".into(), 0.72),
            ("maximum_cloud_fraction".into(), 0.08),
        ]),
        minimum_component_pixels: 24,
        simplification_tolerance_metres: 10.0,
    }
}

fn bundle_with_profile(fixture: &serde_json::Value) -> serde_json::Value {
    let mut bundle = fixture["bundle"].clone();
    bundle.as_object_mut().unwrap().insert(
        "coarse_raster_profiles".into(),
        serde_json::to_value(BTreeMap::from([(
            algorithm_profile().algorithm_version.clone(),
            algorithm_profile(),
        )]))
        .unwrap(),
    );
    bundle
}
fn actor() -> InstallationId {
    InstallationId::new("coarse-raster-gap-evidence").unwrap()
}

fn boundary_evidence() -> PinnedBoundaryEvidence {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/boundary-discovery-snapshot.json"
    ))
    .unwrap();
    PinnedBoundaryEvidence {
        manifest: serde_json::from_value(serde_json::json!({
            "contract_version": fixture["contract_version"],
            "bundle": bundle_with_profile(&fixture),
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

fn acquisition_evidence() -> PinnedAcquisitionEvidence {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/canonical-acquisition.json"
    ))
    .unwrap();
    let mut observations =
        serde_json::from_value::<Vec<SourceObservation>>(fixture["observations"].clone()).unwrap();
    let mut structured_water = observations[0].clone();
    structured_water.id = "obs-water-88".into();
    structured_water.category = FoundationCategory::Water;
    structured_water.lineage.source_record_id = "way/88".into();
    structured_water.geometry = SourceGeometry::Polygon(vec![vec![
        [121.400, 31.220],
        [121.401, 31.220],
        [121.401, 31.221],
        [121.400, 31.221],
        [121.400, 31.220],
    ]]);
    observations.push(structured_water);
    PinnedAcquisitionEvidence {
        manifest: serde_json::from_value::<ResultManifest>(serde_json::json!({
            "contract_version": fixture["contract_version"],
            "bundle": bundle_with_profile(&fixture),
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

fn project() -> campus_state::Schema2Project {
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "coarse raster evidence",
            actor(),
        )
        .unwrap();
    project
        .confirm_boundary(boundary_evidence(), actor())
        .unwrap();
    project
        .pin_acquisition(acquisition_evidence(), actor())
        .unwrap();
    project
}

fn water_gap_id(project: &campus_state::Schema2Project) -> String {
    project
        .foundation_review_queue(FoundationCategory::Water)
        .unwrap()
        .known_gaps[0]
        .id
        .clone()
}

fn observation(project: &campus_state::Schema2Project) -> CoarseRasterObservation {
    let evidence = project.pinned_evidence().unwrap();
    let gap_id = water_gap_id(project);
    CoarseRasterObservation {
        id: "raster-water-east-v1".into(),
        category: FoundationCategory::Water,
        subject: CoarseRasterSubject::Water,
        linked_gap_id: gap_id.clone(),
        dataset_bundle_id: evidence.acquisition.manifest.bundle.id.clone(),
        source: CoarseRasterSource {
            provider: "sentinel-2-l2a".into(),
            dataset_version: "S2B_MSIL2A_20260701T022549".into(),
            observed_at: "2026-07-01T02:25:49Z".into(),
            native_resolution_metres: 10.0,
            class_label: "surface-water".into(),
            source_chunk_id: "raster-chunk-water-east".into(),
            source_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
            licence: LicenceRecord {
                identifier: "copernicus-sentinel-data-terms".into(),
                url: "https://dataspace.copernicus.eu/terms-and-conditions".into(),
                attribution: "Contains modified Copernicus Sentinel data 2026".into(),
                dataset_release: "sentinel-2-l2a-2026-07-01".into(),
                acquired_at: "2026-07-02T08:00:00Z".into(),
                upstream_obligations: vec!["retain attribution".into()],
            },
        },
        grid: CoarseRasterGrid {
            crs: "EPSG:32651".into(),
            affine_transform: [10.0, 0.0, 340000.0, 0.0, -10.0, 3455000.0],
            cloud_handling: "SCL cloud/shadow classes excluded".into(),
            nodata_handling: "nodata excluded before connected components".into(),
        },

        algorithm: algorithm_profile(),

        clip: CoarseRasterClip {
            boundary_result_sha256: evidence.boundary.manifest.result_sha256.clone(),
            linked_gap_id: gap_id,
            gap_tile_id: "31/121/1".into(),
            gap_geometry: SourceGeometry::Polygon(vec![vec![
                [121.397, 31.218],
                [121.410, 31.218],
                [121.410, 31.230],
                [121.397, 31.230],
                [121.397, 31.218],
            ]]),
            clipped_to_boundary_and_gap: true,
        },
        pre_clip_geometry: SourceGeometry::Polygon(vec![vec![
            [121.400, 31.220],
            [121.408, 31.220],
            [121.408, 31.226],
            [121.400, 31.226],
            [121.400, 31.220],
        ]]),
        approximate_geometry: SourceGeometry::Polygon(vec![vec![
            [121.404, 31.222],
            [121.408, 31.222],
            [121.408, 31.226],
            [121.404, 31.226],
            [121.404, 31.222],
        ]]),
        input_cell_count: 40,
        retained_cell_count: 32,
        component_cell_counts: vec![32],
        structured_conflict_observation_ids: vec![
            "obs-osm-relation-42".into(),
            "obs-water-88".into(),
        ],
        exclusions: vec![CoarseRasterExclusion {
            reason: CoarseRasterExclusionReason::StructuredGeometryPriority,
            excluded_cell_count: 8,
            structured_observation_ids: vec!["obs-osm-relation-42".into(), "obs-water-88".into()],
            explanation: "Explicit OSM water geometry retains priority".into(),
        }],
        assessment: CandidateEvidenceAssessment {
            geometry: EvidenceAssessmentDimension {
                status: "approximate".into(),
                reason: "32 connected 10 m cells survive clipping".into(),
            },
            semantics: EvidenceAssessmentDimension {
                status: "supported".into(),
                reason: "surface-water threshold is bundle-pinned".into(),
            },
            entity_match: EvidenceAssessmentDimension {
                status: "not_applicable".into(),
                reason: "coarse coverage does not assert a precise entity".into(),
            },
            name_match: EvidenceAssessmentDimension {
                status: "not_applicable".into(),
                reason: "raster coverage carries no naming claim".into(),
            },
            priority: "structured_geometry_first".into(),
        },
        derived_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
    }
}

fn proposed_run(project: &campus_state::Schema2Project) -> CoarseRasterSupplementRun {
    let observation = observation(project);
    let mut manifest = project
        .pinned_evidence()
        .unwrap()
        .acquisition
        .manifest
        .clone();
    manifest.chunks = vec![ResultChunk {
        id: observation.source.source_chunk_id.clone(),
        stable_cursor: "v1:raster:water-east:end".into(),
        content_type: "application/x-ndjson".into(),
        content_encoding: "gzip".into(),
        sha256: observation.source.source_sha256.clone(),
        uncompressed_bytes: 4096,
    }];
    manifest.licences = vec![observation.source.licence.clone()];
    manifest.result_sha256 =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into();
    CoarseRasterSupplementRun {
        id: "raster-run-water-east-v1".into(),
        category: FoundationCategory::Water,
        linked_gap_id: observation.linked_gap_id.clone(),
        dataset_bundle_id: observation.dataset_bundle_id.clone(),
        requested_at: "2026-07-02T08:00:00Z".into(),
        job_id: "foundation-job-15-raster-water".into(),
        contract_version: manifest.contract_version.clone(),
        request_identity: AcquisitionRequestIdentity::new(
            "foundation-job-15-raster-water/request-1",
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .unwrap(),
        retention_days: 30,
        manifest: Some(manifest),
        outcome: CoarseRasterRunOutcome::Proposals {
            observations: vec![observation],
        },
    }
}

#[test]
fn supplementation_requires_a_current_relevant_structured_gap_and_coarse_subject() {
    let mut project = project();
    let mut run = proposed_run(&project);
    run.linked_gap_id = "gap:water:invented".into();
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut run.outcome {
        observations[0].linked_gap_id = run.linked_gap_id.clone();
        observations[0].clip.linked_gap_id = run.linked_gap_id.clone();
    }
    assert!(project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err()
        .contains("Known Feature Gap"));

    for forbidden in [
        CoarseRasterSubject::Building,
        CoarseRasterSubject::Circulation,
        CoarseRasterSubject::SportsFacility,
        CoarseRasterSubject::IndividualTree,
        CoarseRasterSubject::NarrowBank,
        CoarseRasterSubject::SubResolutionDetail,
    ] {
        let mut run = proposed_run(&project);
        if let CoarseRasterRunOutcome::Proposals { observations } = &mut run.outcome {
            observations[0].subject = forbidden;
        }
        assert!(project
            .record_coarse_raster_supplement(run, actor())
            .unwrap_err()
            .contains("large contiguous water, vegetation, or land-cover"));
    }
}

#[test]
fn structured_geometry_priority_requires_traceable_cell_exclusion() {
    let mut project = project();
    let mut run = proposed_run(&project);
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut run.outcome {
        observations[0].exclusions.clear();
        observations[0].retained_cell_count = observations[0].input_cell_count;
        observations[0].component_cell_counts = vec![observations[0].input_cell_count];
    }

    let error = project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err();
    assert!(error.contains("structured geometry priority"));
}

#[test]
fn acceptance_persists_but_neither_resolves_the_gap_nor_enters_generation() {
    let mut project = project();
    let gap_id = water_gap_id(&project);
    project
        .record_coarse_raster_supplement(proposed_run(&project), actor())
        .unwrap();
    project
        .review_coarse_raster_observation(
            FoundationCategory::Water,
            "raster-water-east-v1",
            CoarseRasterDecision::Accepted,
            actor(),
        )
        .unwrap();

    let evidence = project.coarse_raster_evidence(FoundationCategory::Water);
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        project.coarse_raster_decision(FoundationCategory::Water, &evidence[0].id),
        CoarseRasterDecision::Accepted
    );
    assert_eq!(
        project
            .foundation_review_queue(FoundationCategory::Water)
            .unwrap()
            .ledger_sequence,
        1
    );
    assert_eq!(
        project
            .foundation_review_queue(FoundationCategory::Water)
            .unwrap()
            .known_gaps
            .iter()
            .find(|gap| gap.id == gap_id)
            .unwrap()
            .status,
        campus_state::KnownFeatureGapStatus::Open
    );
    assert!(!project
        .reviewed_features_for_completed_category(FoundationCategory::Water)
        .unwrap_or_default()
        .iter()
        .any(|feature| feature.id == "raster-water-east-v1"));

    let persisted_runs = serde_json::to_string(project.coarse_raster_runs()).unwrap();
    assert!(!persisted_runs.contains("reviewHistory"));
    assert!(!persisted_runs.contains("\"decision\""));
}

#[test]
fn a_coarse_candidate_revision_invalidates_its_old_ledger_decision() {
    let mut project = project();
    project
        .record_coarse_raster_supplement(proposed_run(&project), actor())
        .unwrap();
    project
        .review_coarse_raster_observation(
            FoundationCategory::Water,
            "raster-water-east-v1",
            CoarseRasterDecision::Accepted,
            actor(),
        )
        .unwrap();

    let mut persisted = serde_json::to_value(&project).unwrap();
    let derived_sha256 = persisted
        .pointer("/foundation/coarseRasterRuns/0/outcome/observations/0/derivedSha256")
        .and_then(serde_json::Value::as_str)
        .expect("coarse candidate digest is persisted");
    assert_eq!(derived_sha256.len(), 64);
    *persisted
        .pointer_mut("/foundation/coarseRasterRuns/0/outcome/observations/0/derivedSha256")
        .unwrap() = serde_json::Value::String("f".repeat(64));

    let restored = campus_state::decode_schema2_project(&serde_json::to_vec(&persisted).unwrap())
        .expect("a valid candidate revision remains decodable");
    assert_eq!(
        restored.coarse_raster_decision(FoundationCategory::Water, "raster-water-east-v1"),
        CoarseRasterDecision::Unresolved
    );
}

#[test]
fn reject_and_leave_unresolved_actions_are_append_only_and_round_trip() {
    let mut project = project();
    project
        .record_coarse_raster_supplement(proposed_run(&project), actor())
        .unwrap();
    project
        .review_coarse_raster_observation(
            FoundationCategory::Water,
            "raster-water-east-v1",
            CoarseRasterDecision::Rejected {
                reason: "cloud edge dominates the component".into(),
            },
            actor(),
        )
        .unwrap();
    project
        .review_coarse_raster_observation(
            FoundationCategory::Water,
            "raster-water-east-v1",
            CoarseRasterDecision::Unresolved,
            actor(),
        )
        .unwrap();

    let json = serde_json::to_vec(&project).unwrap();
    let restored = campus_state::decode_schema2_project(&json).unwrap();
    let observation = restored.coarse_raster_evidence(FoundationCategory::Water)[0];
    assert_eq!(
        restored.coarse_raster_decision(FoundationCategory::Water, &observation.id),
        CoarseRasterDecision::Unresolved
    );
    assert_eq!(
        restored
            .foundation_review_queue(FoundationCategory::Water)
            .unwrap()
            .ledger_sequence,
        2
    );
    assert_eq!(restored.coarse_raster_runs(), project.coarse_raster_runs());
}

#[test]
fn algorithm_thresholds_are_pinned_per_bundle_and_version() {
    let mut project = project();
    project
        .record_coarse_raster_supplement(proposed_run(&project), actor())
        .unwrap();
    let mut changed = proposed_run(&project);
    changed.id = "raster-run-water-east-v2".into();
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut changed.outcome {
        observations[0].id = "raster-water-east-v2".into();
        observations[0]
            .algorithm
            .thresholds
            .insert("minimum_probability".into(), 0.61);
    }
    assert!(project
        .record_coarse_raster_supplement(changed, actor())
        .unwrap_err()
        .contains("exactly match the profile pinned by the Dataset Bundle"));
}

#[test]
fn provider_failure_and_unusable_coverage_are_explicit_non_success_outcomes() {
    let mut project = project();
    let gap_id = water_gap_id(&project);
    let bundle_id = project
        .pinned_evidence()
        .unwrap()
        .acquisition
        .manifest
        .bundle
        .id
        .clone();
    for (id, outcome) in [
        (
            "raster-run-provider-failed",
            CoarseRasterRunOutcome::ProviderFailure {
                failure: ServiceFailure {
                    code: "sentinel-timeout".into(),
                    scope: "water/31-121-1".into(),
                    retryable: true,
                    explanation: "Sentinel-2 provider timed out".into(),
                    suggested_action: "Retry this coarse supplement".into(),
                },
            },
        ),
        (
            "raster-run-unusable",
            CoarseRasterRunOutcome::UnusableCoverage {
                failure: ServiceFailure {
                    code: "cloud-obscured".into(),
                    scope: "water/31-121-1".into(),
                    retryable: false,
                    explanation: "All eligible cells are cloud or nodata".into(),
                    suggested_action: "Leave the Known Feature Gap unresolved".into(),
                },
            },
        ),
    ] {
        let mut run = proposed_run(&project);
        run.id = id.into();
        run.linked_gap_id = gap_id.clone();
        run.dataset_bundle_id = bundle_id.clone();
        run.outcome = outcome;
        project
            .record_coarse_raster_supplement(run, actor())
            .unwrap();
    }
    assert_eq!(project.coarse_raster_runs().len(), 2);
    assert!(matches!(
        project.coarse_raster_runs()[0].outcome,
        CoarseRasterRunOutcome::ProviderFailure { .. }
    ));
    assert!(matches!(
        project.coarse_raster_runs()[1].outcome,
        CoarseRasterRunOutcome::UnusableCoverage { .. }
    ));
    let restored =
        campus_state::decode_schema2_project(&serde_json::to_vec(&project).unwrap()).unwrap();
    assert_eq!(restored.coarse_raster_runs(), project.coarse_raster_runs());
}

#[test]
fn empty_success_and_unclipped_surfaces_cannot_hide_an_unusable_result() {
    let mut project = project();
    let mut empty = proposed_run(&project);
    empty.outcome = CoarseRasterRunOutcome::Proposals {
        observations: Vec::new(),
    };
    assert!(project
        .record_coarse_raster_supplement(empty, actor())
        .unwrap_err()
        .contains("cannot be empty"));

    let mut unclipped = proposed_run(&project);
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut unclipped.outcome {
        observations[0].clip.clipped_to_boundary_and_gap = false;
    }
    assert!(project
        .record_coarse_raster_supplement(unclipped, actor())
        .unwrap_err()
        .contains("boundary/gap intersection"));
}

#[test]
fn raster_runs_are_never_started_for_building_circulation_or_sports_gaps() {
    let mut project = project();
    let mut run = proposed_run(&project);
    run.category = FoundationCategory::Building;
    assert!(project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err()
        .contains("only for water, vegetation, or land-cover gaps"));
}
#[test]
fn stale_gap_evidence_cannot_receive_a_current_review_decision() {
    let mut project = project();
    let gap_id = water_gap_id(&project);
    project
        .record_coarse_raster_supplement(proposed_run(&project), actor())
        .unwrap();
    project
        .resolve_feature_gap(
            FoundationCategory::Water,
            &gap_id,
            vec!["obs-water-88".into()],
            actor(),
        )
        .unwrap();
    assert!(project
        .review_coarse_raster_observation(
            FoundationCategory::Water,
            "raster-water-east-v1",
            CoarseRasterDecision::Accepted,
            actor(),
        )
        .unwrap_err()
        .contains("stale or outside the current review basis"));
}
#[test]
fn retained_surface_must_be_valid_and_inside_the_confirmed_boundary() {
    let mut project = project();
    let mut run = proposed_run(&project);
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut run.outcome {
        observations[0].approximate_geometry = SourceGeometry::Polygon(vec![vec![
            [121.500, 31.300],
            [121.501, 31.300],
            [121.501, 31.301],
            [121.500, 31.301],
            [121.500, 31.300],
        ]]);
        observations[0].clip.gap_geometry = SourceGeometry::Polygon(vec![vec![
            [121.300, 31.100],
            [121.600, 31.100],
            [121.600, 31.400],
            [121.300, 31.400],
            [121.300, 31.100],
        ]]);
    }
    assert!(project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err()
        .contains("leaves the confirmed Campus Boundary"));
}

#[test]
fn actual_structured_intersections_cannot_be_hidden_by_caller_metadata() {
    let mut project = project();
    let mut run = proposed_run(&project);
    if let CoarseRasterRunOutcome::Proposals { observations } = &mut run.outcome {
        observations[0].structured_conflict_observation_ids.clear();
        observations[0].exclusions = vec![CoarseRasterExclusion {
            reason: CoarseRasterExclusionReason::OutsideBoundaryOrGap,
            excluded_cell_count: 8,
            structured_observation_ids: Vec::new(),
            explanation: "caller falsely attributes all clipped cells to the boundary".into(),
        }];
    }
    assert!(project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err()
        .contains("detected, declared, and excluded exactly"));
}

#[test]
fn proposal_manifest_must_declare_the_source_licence() {
    let mut project = project();
    let mut run = proposed_run(&project);
    run.manifest.as_mut().unwrap().licences.clear();
    assert!(project
        .record_coarse_raster_supplement(run, actor())
        .unwrap_err()
        .contains("manifest licence and run identity"));
}
#[test]
fn comparison_copy_is_fixed_and_explicit_about_approximation() {
    assert_eq!(
        COARSE_RASTER_APPROXIMATE_COVERAGE_WARNING,
        "Approximate coverage only; this is not a precise feature boundary."
    );
    assert!(COARSE_RASTER_PIXEL_EDGE_WARNING.contains("at least one source pixel"));
    assert!(COARSE_RASTER_FINISHING_WARNING.contains("Minecraft/Axiom"));
}
