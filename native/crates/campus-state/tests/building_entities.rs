use campus_state::{
    decode_schema2_project, BuildingBoundaryDecision, BuildingEntityDecision,
    BuildingEntityReviewLedger, BuildingEvidenceDescriptor, BuildingGenerationBasis,
    BuildingNameAssignmentMode, BuildingNameEvidence, CampusProjectLibrary, CampusScope,
    FoundationCategory, FoundationReviewDisposition, InstallationId, PinnedAcquisitionEvidence,
    PinnedBoundaryEvidence, ResultManifest, SourceGeometry, SourceObservation,
    V11ConstructionCapability,
};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
struct ComplexBuildingFixture {
    observations: Vec<SourceObservation>,
    building_evidence: Vec<BuildingEvidenceDescriptor>,
    name_evidence: Vec<BuildingNameEvidence>,
}

fn fixture() -> ComplexBuildingFixture {
    serde_json::from_str(include_str!(
        "../../../../contracts/acquisition/v1/fixtures/complex-building-review.json"
    ))
    .unwrap()
}

fn actor() -> InstallationId {
    InstallationId::new("building-review-acceptance").unwrap()
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

#[test]
fn typed_source_evidence_round_trips_without_flattening_or_duplicate_delivery() {
    let fixture = fixture();
    let geometry_types = fixture
        .observations
        .iter()
        .map(|observation| observation.geometry.type_name())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        geometry_types,
        BTreeSet::from([
            "Point",
            "MultiPoint",
            "LineString",
            "MultiLineString",
            "Polygon",
            "MultiPolygon",
        ])
    );
    let multipolygon = fixture
        .observations
        .iter()
        .find(|observation| observation.id == "obs-campus-library-v1")
        .unwrap();
    let SourceGeometry::MultiPolygon(polygons) = &multipolygon.geometry else {
        panic!("fixture must retain the complex source geometry");
    };
    assert_eq!(polygons.len(), 2, "disconnected components stay separate");
    assert_eq!(
        polygons[0].len(),
        2,
        "the courtyard remains an interior ring"
    );
    assert_ne!(
        multipolygon.geometry, multipolygon.review_geometry_proposal,
        "source and WGS-84 review geometry remain distinct"
    );
    assert!(!multipolygon.original_properties.is_empty());
    assert!(!multipolygon.lineage.upstream_records.is_empty());
    assert!(!multipolygon.derivation.steps.is_empty());
    assert!(!multipolygon.attribute_provenance.is_empty());

    let ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();
    let entity = ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap();
    assert_eq!(
        entity.evidence_ids,
        vec![
            "obs-campus-library-v1",
            "obs-campus-library-v2",
            "obs-campus-library-overlap",
            "obs-campus-library-part",
        ],
        "an identical replay collapses while changed, overlapping, and part evidence remains"
    );
    assert_eq!(
        ledger.duplicate_deliveries()["obs-campus-library-v1"],
        vec!["obs-campus-library-v1-replay"]
    );
    assert!(entity
        .unresolved_overlap_groups
        .contains(&"library-conflict".into()));

    let round_trip: BuildingEntityReviewLedger =
        serde_json::from_slice(&serde_json::to_vec(&ledger).unwrap()).unwrap();
    assert_eq!(round_trip, ledger);
}

#[test]
fn reversible_entity_review_resolves_primary_parts_boundary_and_names_after_conflation() {
    let fixture = fixture();
    let mut ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();

    ledger
        .record(
            BuildingEntityDecision::SetPrimary {
                entity_id: "building:campus-library".into(),
                observation_id: "obs-campus-library-overlap".into(),
            },
            &fixture.observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::SetBoundary {
                entity_id: "building:campus-library".into(),
                decision: BuildingBoundaryDecision::RetainWhole,
            },
            &fixture.observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::AssignName {
                entity_id: "building:campus-library".into(),
                name_evidence_id: "name-library-exclusive".into(),
                mode: BuildingNameAssignmentMode::Automatic,
            },
            &fixture.observations,
        )
        .unwrap();
    let automatically_reused = ledger.record(
        BuildingEntityDecision::AssignName {
            entity_id: "building:annex".into(),
            name_evidence_id: "name-library-reused-poi".into(),
            mode: BuildingNameAssignmentMode::Automatic,
        },
        &fixture.observations,
    );
    assert!(automatically_reused
        .unwrap_err()
        .contains("at most one Building Entity"));
    let ambiguous = ledger.record(
        BuildingEntityDecision::AssignName {
            entity_id: "building:annex".into(),
            name_evidence_id: "name-campus-ambiguous".into(),
            mode: BuildingNameAssignmentMode::Automatic,
        },
        &fixture.observations,
    );
    assert!(ambiguous.unwrap_err().contains("exclusive building-level"));

    ledger
        .record(
            BuildingEntityDecision::Split {
                entity_id: "building:campus-library".into(),
                outputs: vec![
                    campus_state::BuildingEntitySplit {
                        entity_id: "building:campus-library-main".into(),
                        evidence_ids: vec![
                            "obs-campus-library-v1".into(),
                            "obs-campus-library-v2".into(),
                            "obs-campus-library-overlap".into(),
                        ],
                        primary_observation_id: "obs-campus-library-overlap".into(),
                        part_observation_ids: Vec::new(),
                    },
                    campus_state::BuildingEntitySplit {
                        entity_id: "building:campus-library-wing".into(),
                        evidence_ids: vec!["obs-campus-library-part".into()],
                        primary_observation_id: "obs-campus-library-part".into(),
                        part_observation_ids: vec!["obs-campus-library-part".into()],
                    },
                ],
            },
            &fixture.observations,
        )
        .unwrap();
    let split_entities = ledger.entities();
    assert!(split_entities
        .iter()
        .filter(|entity| entity.split_from.as_deref() == Some("building:campus-library"))
        .all(|entity| entity.display_name.is_none()));

    ledger.revoke_last().unwrap();
    let restored = ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap();
    assert_eq!(
        restored.display_name.as_deref(),
        Some("Putuo Campus Library")
    );
    assert_eq!(
        restored.primary_observation_id,
        "obs-campus-library-overlap"
    );
    ledger.revoke_last().unwrap();
    assert!(ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap()
        .display_name
        .is_none());
    assert!(ledger.entries().iter().all(|entry| {
        entry.recorded_at_unix_ms > 0 && !entry.basis.observation_geometry_sha256.is_empty()
    }));
}

#[test]
fn reviewed_entity_projection_keeps_sources_immutable_and_emits_distinct_generation_geometry() {
    let fixture = fixture();
    let original_observations = fixture.observations.clone();
    let mut ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();
    ledger
        .record(
            BuildingEntityDecision::SetBoundary {
                entity_id: "building:campus-library".into(),
                decision: BuildingBoundaryDecision::RetainWhole,
            },
            &fixture.observations,
        )
        .unwrap();
    assert!(ledger
        .reviewed_entities(
            &fixture.observations,
            BuildingGenerationBasis {
                origin_wgs84: [121.3998, 31.2098],
                orientation_degrees: 17.5,
                blocks_per_meter: 1.25,
                rule_version: "building-generation-geometry/v1".into(),
            },
        )
        .unwrap_err()
        .contains("geometry conflict"));
    ledger
        .record(
            BuildingEntityDecision::SetPrimary {
                entity_id: "building:campus-library".into(),
                observation_id: "obs-campus-library-v1".into(),
            },
            &fixture.observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::SetBoundary {
                entity_id: "building:annex".into(),
                decision: BuildingBoundaryDecision::Exclude,
            },
            &fixture.observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::AssignName {
                entity_id: "building:campus-library".into(),
                name_evidence_id: "name-library-exclusive".into(),
                mode: BuildingNameAssignmentMode::Automatic,
            },
            &fixture.observations,
        )
        .unwrap();

    let reviewed = ledger
        .reviewed_entities(
            &fixture.observations,
            BuildingGenerationBasis {
                origin_wgs84: [121.3998, 31.2098],
                orientation_degrees: 17.5,
                blocks_per_meter: 1.25,
                rule_version: "building-generation-geometry/v1".into(),
            },
        )
        .unwrap();

    assert_eq!(reviewed.len(), 1);
    assert_eq!(reviewed[0].id, "building:campus-library");
    assert_eq!(
        reviewed[0].review_geometry,
        fixture
            .observations
            .iter()
            .find(|observation| observation.id == "obs-campus-library-v1")
            .unwrap()
            .review_geometry_proposal,
        "retain-whole never clips a straddling Building"
    );
    assert_eq!(
        reviewed[0].generation_geometry.source_entity_id,
        "building:campus-library"
    );
    assert_ne!(
        reviewed[0].generation_geometry.geometry, reviewed[0].review_geometry,
        "Generation Geometry is project-space, not relabelled WGS-84 review evidence"
    );
    assert_eq!(
        reviewed[0].generation_geometry.basis.orientation_degrees,
        17.5
    );
    assert_eq!(fixture.observations, original_observations);
    assert_eq!(reviewed[0].source_observations.len(), 4);
}

#[test]
fn separate_and_merge_decisions_are_explicit_and_reversible_without_geometry_union() {
    let mut fixture = fixture();
    fixture
        .building_evidence
        .iter_mut()
        .find(|descriptor| descriptor.observation_id == "obs-annex")
        .unwrap()
        .overlap_group = Some("library-conflict".into());
    let mut ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();
    ledger
        .record(
            BuildingEntityDecision::KeepSeparate {
                entity_ids: vec!["building:campus-library".into(), "building:annex".into()],
            },
            &fixture.observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::Merge {
                entity_ids: vec!["building:campus-library".into(), "building:annex".into()],
                merged_entity_id: "building:library-complex".into(),
                primary_observation_id: "obs-campus-library-v1".into(),
                part_observation_ids: vec!["obs-campus-library-part".into()],
            },
            &fixture.observations,
        )
        .unwrap();
    let merged = ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:library-complex")
        .unwrap();
    assert_eq!(
        merged.merged_from,
        vec!["building:campus-library", "building:annex"]
    );
    assert_eq!(merged.primary_observation_id, "obs-campus-library-v1");
    assert!(merged.evidence_ids.contains(&"obs-annex".into()));
    assert!(
        merged.display_name.is_none(),
        "merge never copies a source or entity name implicitly"
    );

    ledger.revoke_last().unwrap();
    assert!(ledger
        .entities()
        .iter()
        .any(|entity| entity.id == "building:campus-library"));
    assert!(ledger
        .entities()
        .iter()
        .any(|entity| entity.id == "building:annex"));
}

#[test]
fn deduplication_keeps_any_materially_changed_evidence() {
    let mut fixture = fixture();
    let replay = fixture
        .observations
        .iter_mut()
        .find(|observation| observation.id == "obs-campus-library-v1-replay")
        .unwrap();
    replay.lineage.acquired_at = "2026-07-18T10:00:00Z".into();
    replay.licence.acquired_at = "2026-07-18T10:00:01Z".into();
    replay.time_semantics.acquired_at = "2026-07-18T10:00:02Z".into();
    let mut changed = fixture
        .observations
        .iter()
        .find(|observation| observation.id == "obs-campus-library-v1-replay")
        .unwrap()
        .clone();
    changed.id = "obs-campus-library-crs-change".into();
    changed.coordinate_semantics.axis_order = "latitude,longitude".into();
    fixture.observations.push(changed);
    let mut descriptor = fixture
        .building_evidence
        .iter()
        .find(|descriptor| descriptor.observation_id == "obs-campus-library-v1-replay")
        .unwrap()
        .clone();
    descriptor.observation_id = "obs-campus-library-crs-change".into();
    fixture.building_evidence.push(descriptor);

    let ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();

    assert_eq!(
        ledger.duplicate_deliveries()["obs-campus-library-v1"],
        vec!["obs-campus-library-v1-replay"],
        "fetch timestamps do not turn an unchanged delivery into new source evidence"
    );
    assert!(ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap()
        .evidence_ids
        .contains(&"obs-campus-library-crs-change".into()));
}

#[test]
fn missing_boundary_metadata_and_ambiguous_names_require_explicit_review() {
    let mut fixture = fixture();
    let mut observation = fixture.observations.remove(
        fixture
            .observations
            .iter()
            .position(|item| item.id == "obs-annex")
            .unwrap(),
    );
    observation.suggestions.clear();
    let observations = vec![observation];
    let mut ledger =
        BuildingEntityReviewLedger::from_observations(&observations, Vec::new()).unwrap();
    let entity_id = ledger.entities()[0].id.clone();
    assert_eq!(
        ledger.entities()[0].boundary_decision,
        BuildingBoundaryDecision::Pending,
        "missing boundary evidence must never invent an inside/retain decision"
    );
    ledger
        .record(
            BuildingEntityDecision::SetBoundary {
                entity_id: entity_id.clone(),
                decision: BuildingBoundaryDecision::RetainWhole,
            },
            &observations,
        )
        .unwrap();
    ledger
        .record(
            BuildingEntityDecision::LeaveUnnamed {
                entity_id,
                reason: "Only campus-level or ambiguous POIs are available".into(),
            },
            &observations,
        )
        .unwrap();
    let reviewed = ledger
        .reviewed_entities(
            &observations,
            BuildingGenerationBasis {
                origin_wgs84: [121.4, 31.21],
                orientation_degrees: 0.0,
                blocks_per_meter: 1.0,
                rule_version: "building-generation/v1".into(),
            },
        )
        .unwrap();
    assert_eq!(reviewed[0].display_name, None);
}

#[test]
fn conflict_decisions_clear_only_the_overlap_groups_they_resolve() {
    let mut fixture = fixture();
    for descriptor in &mut fixture.building_evidence {
        descriptor.overlap_group = match descriptor.observation_id.as_str() {
            "obs-campus-library-v1" | "obs-campus-library-v1-replay" | "obs-campus-library-v2" => {
                Some("library-refresh".into())
            }
            "obs-campus-library-overlap" | "obs-annex" => Some("library-annex".into()),
            _ => descriptor.overlap_group.clone(),
        };
    }
    let mut ledger = BuildingEntityReviewLedger::new(
        &fixture.observations,
        fixture.building_evidence,
        fixture.name_evidence,
    )
    .unwrap();
    ledger
        .record(
            BuildingEntityDecision::SetPrimary {
                entity_id: "building:campus-library".into(),
                observation_id: "obs-campus-library-v2".into(),
            },
            &fixture.observations,
        )
        .unwrap();
    let library = ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap();
    assert_eq!(
        library.unresolved_overlap_groups,
        vec!["library-annex"],
        "selecting a primary resolves only that observation's overlap group"
    );
    ledger
        .record(
            BuildingEntityDecision::KeepSeparate {
                entity_ids: vec!["building:campus-library".into(), "building:annex".into()],
            },
            &fixture.observations,
        )
        .unwrap();
    assert!(ledger
        .entities()
        .iter()
        .filter(|entity| {
            entity.id == "building:campus-library" || entity.id == "building:annex"
        })
        .all(|entity| entity.unresolved_overlap_groups.is_empty()));
}

#[test]
fn acquisition_refresh_appends_changed_evidence_without_replacing_review_history() {
    let fixture = fixture();
    let initial_observations = fixture
        .observations
        .iter()
        .filter(|observation| {
            !matches!(
                observation.id.as_str(),
                "obs-campus-library-v1-replay"
                    | "obs-campus-library-v2"
                    | "obs-campus-library-overlap"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "refresh acceptance",
            actor(),
        )
        .unwrap();
    project
        .confirm_boundary(boundary_evidence(), actor())
        .unwrap();
    project
        .pin_acquisition(acquisition_evidence(initial_observations), actor())
        .unwrap();
    project
        .initialize_building_entity_review(fixture.name_evidence.clone(), actor())
        .unwrap();
    for decision in [
        BuildingEntityDecision::SetBoundary {
            entity_id: "building:campus-library".into(),
            decision: BuildingBoundaryDecision::RetainWhole,
        },
        BuildingEntityDecision::AssignName {
            entity_id: "building:campus-library".into(),
            name_evidence_id: "name-library-exclusive".into(),
            mode: BuildingNameAssignmentMode::Automatic,
        },
    ] {
        project
            .record_building_entity_decision(decision, actor())
            .unwrap();
    }
    let mut refresh = acquisition_evidence(fixture.observations.clone());
    refresh.manifest.result_sha256 = "sha256:building-refresh-v2".into();
    project.pin_acquisition(refresh, actor()).unwrap();

    let ledger = project.building_entity_review();
    let library_entity = ledger
        .entities()
        .iter()
        .find(|entity| entity.id == "building:campus-library")
        .unwrap();
    assert_eq!(
        library_entity.display_name.as_deref(),
        Some("Putuo Campus Library")
    );
    assert!(library_entity
        .evidence_ids
        .contains(&"obs-campus-library-v2".into()));
    assert_eq!(
        library_entity.unresolved_overlap_groups,
        vec!["library-conflict"],
        "changed geometry reopens the relevant conflict on the stable entity"
    );
    assert!(matches!(
        ledger.entries().last().unwrap().decision,
        BuildingEntityDecision::RefreshEvidence { .. }
    ));
    project
        .record_building_entity_decision(
            BuildingEntityDecision::SetPrimary {
                entity_id: "building:campus-library".into(),
                observation_id: "obs-campus-library-v2".into(),
            },
            actor(),
        )
        .unwrap();
    let project_id = project.id().clone();
    library.save_project(&project).unwrap();
    let reopened = library.open_project(&project_id).unwrap();
    assert_eq!(
        reopened.building_entity_review().entries().len(),
        4,
        "refresh and the follow-up decision survive persistence as append-only history"
    );
}

#[test]
fn schema2_project_persists_entity_review_and_generation_consumes_its_projection() {
    let fixture = fixture();
    let directory = tempfile::tempdir().unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let mut library =
        CampusProjectLibrary::open_for_construction(directory.path(), "campus:putuo", &capability)
            .unwrap();
    let mut project = library
        .create_project(
            CampusScope::new("campus:putuo", "Putuo Campus", [121.4, 31.21]).unwrap(),
            "complex building acceptance",
            actor(),
        )
        .unwrap();
    project
        .confirm_boundary(boundary_evidence(), actor())
        .unwrap();
    project
        .pin_acquisition(acquisition_evidence(fixture.observations.clone()), actor())
        .unwrap();
    project
        .initialize_building_entity_review(fixture.name_evidence, actor())
        .unwrap();
    assert!(project
        .initialize_building_entity_review(Vec::new(), actor())
        .unwrap_err()
        .contains("already initialized"));
    for decision in [
        BuildingEntityDecision::SetPrimary {
            entity_id: "building:campus-library".into(),
            observation_id: "obs-campus-library-overlap".into(),
        },
        BuildingEntityDecision::SetBoundary {
            entity_id: "building:campus-library".into(),
            decision: BuildingBoundaryDecision::RetainWhole,
        },
        BuildingEntityDecision::SetBoundary {
            entity_id: "building:annex".into(),
            decision: BuildingBoundaryDecision::Exclude,
        },
        BuildingEntityDecision::AssignName {
            entity_id: "building:campus-library".into(),
            name_evidence_id: "name-library-exclusive".into(),
            mode: BuildingNameAssignmentMode::Automatic,
        },
    ] {
        project
            .record_building_entity_decision(decision, actor())
            .unwrap();
    }
    project
        .revoke_last_building_entity_decision(actor())
        .unwrap();
    project
        .revoke_last_building_entity_decision(actor())
        .unwrap();
    for decision in [
        BuildingEntityDecision::SetBoundary {
            entity_id: "building:annex".into(),
            decision: BuildingBoundaryDecision::Exclude,
        },
        BuildingEntityDecision::AssignName {
            entity_id: "building:campus-library".into(),
            name_evidence_id: "name-library-exclusive".into(),
            mode: BuildingNameAssignmentMode::Automatic,
        },
    ] {
        project
            .record_building_entity_decision(decision, actor())
            .unwrap();
    }
    assert!(project
        .complete_foundation_review(
            FoundationCategory::Building,
            FoundationReviewDisposition::SelectedEvidence {
                evidence_ids: vec!["obs-campus-library-v1".into()],
            },
            actor(),
        )
        .unwrap_err()
        .contains("Building Entities"));
    project
        .complete_foundation_review(
            FoundationCategory::Building,
            FoundationReviewDisposition::ReviewedBuildingEntities {
                entity_ids: vec!["building:campus-library".into()],
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_review(
            FoundationCategory::Circulation,
            FoundationReviewDisposition::SelectedEvidence {
                evidence_ids: vec!["obs-path".into()],
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_review(
            FoundationCategory::Water,
            FoundationReviewDisposition::SelectedEvidence {
                evidence_ids: vec!["obs-water-lines".into()],
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_review(
            FoundationCategory::Vegetation,
            FoundationReviewDisposition::SelectedEvidence {
                evidence_ids: vec!["obs-tree-point".into(), "obs-tree-cluster".into()],
            },
            actor(),
        )
        .unwrap();
    project
        .complete_foundation_review(
            FoundationCategory::Sports,
            FoundationReviewDisposition::KnownGap {
                reasons: vec!["provider cancelled after retaining diagnostics".into()],
            },
            actor(),
        )
        .unwrap();

    let projection = project.reviewed_projection().unwrap();
    assert_eq!(projection.building_entities.len(), 1);
    assert_eq!(
        projection.building_entities[0].id,
        "building:campus-library"
    );
    assert!(projection
        .selected_features
        .iter()
        .any(|feature| feature.id == "building:campus-library"));
    assert!(projection
        .selected_features
        .iter()
        .any(|feature| { feature.id == "building:campus-library:part:obs-campus-library-part" }));
    assert!(projection
        .selected_features
        .iter()
        .all(|feature| feature.id != "obs-campus-library-v1"));
    project.record_generation(64, 8, 64, 512, actor()).unwrap();
    project
        .record_export(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into(),
            4096,
            "complex-building.foundation-manifest.json".into(),
        )
        .unwrap();
    let export = project.exported_output().unwrap();
    assert_eq!(export.building_provenance, projection.building_entities);
    assert_ne!(
        export.building_provenance[0].source_observations[0].geometry,
        export.building_provenance[0].review_geometry
    );
    assert_eq!(
        export.building_provenance[0]
            .generation_geometry
            .source_entity_id,
        "building:campus-library"
    );

    library.save_project(&project).unwrap();
    let reopened = library.open_project(project.id()).unwrap();
    assert_eq!(reopened.building_entity_review().entries().len(), 8);
    assert_eq!(
        reopened.exported_output().unwrap().building_provenance,
        projection.building_entities
    );
    let decoded = decode_schema2_project(&serde_json::to_vec(&reopened).unwrap()).unwrap();
    assert_eq!(
        decoded.reviewed_projection().unwrap().building_entities,
        projection.building_entities
    );
    let mut tampered = serde_json::to_value(&reopened).unwrap();
    tampered["foundation"]["buildingReview"]["entities"][0]["primary_observation_id"] =
        serde_json::json!("obs-annex");
    assert!(
        decode_schema2_project(&serde_json::to_vec(&tampered).unwrap())
            .unwrap_err()
            .contains("deterministic Building Entity projection")
    );
}
