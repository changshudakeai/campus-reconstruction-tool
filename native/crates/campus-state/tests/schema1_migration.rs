use campus_state::{
    decode_schema1_project, CampusProjectLibrary, CampusScope, InstallationId, LegacyEvidenceKind,
    MigrationDisposition, MigrationFaultPoint, ReconfirmationReason, ReviewDecision,
    Schema2ProjectSession, V11ConstructionCapability, SCHEMA_2_VERSION,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn test_data(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-data")
        .join(relative)
}

fn actor() -> InstallationId {
    InstallationId::new("migration-test-installation").unwrap()
}

fn scope() -> CampusScope {
    CampusScope::new(
        "gaode:B00155J6JH",
        "East China Normal University Putuo Campus",
        [121.395, 31.202],
    )
    .unwrap()
}

fn construction_capability() -> V11ConstructionCapability {
    V11ConstructionCapability::request(true, Some("1")).unwrap()
}

fn write_managed_fixture(root: &Path, bytes: Vec<u8>) -> (PathBuf, Vec<u8>) {
    let path = root.join("managed-v1-project.campus.json");
    std::fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

fn populated_native_schema1_bytes() -> Vec<u8> {
    let mut value: Value =
        serde_json::from_slice(&std::fs::read(test_data("v1-demo.campus.json")).unwrap()).unwrap();
    value["name"] = json!("V1.0.1 baseline project");
    value["campusName"] = json!("ECNU Putuo Campus");
    value["campusTarget"] = json!({
        "poiId": "fixture-poi-putuo",
        "name": "ECNU Putuo Campus",
        "gcj02": {"lng": 121.406582, "lat": 31.228318},
        "wgs84": {"lng": 121.402037, "lat": 31.230305},
        "acquisition": "v1.0.1-baseline-fixture"
    });
    value["candidates"][0]["sourceSnapshotId"] = json!("osm:v1-baseline");
    value["foundationSourceSnapshots"] = json!([{
        "id": "osm:v1-baseline",
        "provider": "open_street_map",
        "providerVersion": "fixture/2026-07-18",
        "status": "complete",
        "southWest": {"lng": 121.4059, "lat": 31.2277},
        "northEast": {"lng": 121.4072, "lat": 31.2288},
        "acquiredAtUnixMs": 1710000000000_u64,
        "candidates": [],
        "error": null
    }]);
    value["foundationReviewLedger"] = json!([{
        "candidateId": "demo-library",
        "sourceSnapshotId": "osm:v1-baseline",
        "decision": "accepted",
        "decidedAtUnixMs": 1710000000200_u64
    }]);
    value["buildingDirectory"] = json!([{
        "sourceId": "demo-library",
        "name": "Fixture Library",
        "updatedAtUnixMs": 1710000000300_u64
    }]);
    value["buildingSuppressions"] = json!([{
        "sourceId": "fixture-rejected-shed",
        "reason": "outside campus",
        "suppressedAtUnixMs": 1710000000400_u64
    }]);
    value["detailed"]["facadeDrafts"] = json!([{
        "id": "facade-1",
        "slotId": "demo-library",
        "modelVersion": "fixture-v1",
        "confidence": 90,
        "rules": [],
        "evidenceIds": []
    }]);
    serde_json::to_vec_pretty(&value).unwrap()
}

fn legacy_web_schema1_bytes() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "project": {
            "schemaVersion": "1.0",
            "name": "Legacy web portable fixture",
            "campus": {"canonicalName": "ECNU Putuo Campus"},
            "foundation": {
                "orientationDegrees": 18.0,
                "foundationStyle": {"blocksPerMeter": 1.5},
                "boundaryDraft": {"points": [
                    {"lng": 121.40, "lat": 31.23},
                    {"lng": 121.41, "lat": 31.23},
                    {"lng": 121.41, "lat": 31.22}
                ]},
                "reviews": {
                    "legacy-library": "accepted",
                    "legacy-road": "rejected"
                },
                "candidates": [{
                    "id": "legacy-library",
                    "name": "Legacy Library",
                    "kind": "building",
                    "source": "OSM fixture",
                    "confidence": "high",
                    "geometry": {"points": [
                        {"lng": 121.405, "lat": 31.228},
                        {"lng": 121.406, "lat": 31.228},
                        {"lng": 121.406, "lat": 31.227}
                    ]}
                }],
                "manualFeatures": [{
                    "id": "legacy-water",
                    "name": "Legacy Pond",
                    "kind": "water",
                    "geometry": {"points": [
                        {"lng": 121.405, "lat": 31.228},
                        {"lng": 121.4052, "lat": 31.228},
                        {"lng": 121.4052, "lat": 31.2278}
                    ]},
                    "block": "water"
                }]
            }
        }
    }))
    .unwrap()
}

#[test]
fn schema1_compatibility_decoder_is_read_only_and_rejects_other_schemas() {
    let bytes = populated_native_schema1_bytes();
    let before = bytes.clone();

    let project = decode_schema1_project(&bytes).unwrap();

    assert_eq!(project.schema_version, 1);
    assert_eq!(bytes, before, "decoding must not modify the source bytes");

    let mut newer: Value = serde_json::from_slice(&bytes).unwrap();
    newer["schemaVersion"] = json!(SCHEMA_2_VERSION + 1);
    assert!(decode_schema1_project(&serde_json::to_vec(&newer).unwrap())
        .unwrap_err()
        .contains("newer than supported schema 1"));
}

#[test]
fn populated_managed_schema1_project_migrates_from_backup_without_losing_supported_work() {
    let directory = tempfile::tempdir().unwrap();
    let (legacy_path, original_bytes) =
        write_managed_fixture(directory.path(), populated_native_schema1_bytes());
    let mut legacy_value: Value = serde_json::from_slice(&original_bytes).unwrap();
    legacy_value["foundationPreviewPath"] = json!("artifacts/foundation-preview.png");
    legacy_value["detailed"]["generatedPath"] = json!("artifacts/detailed-output.schem");
    let original_bytes = serde_json::to_vec_pretty(&legacy_value).unwrap();
    std::fs::write(&legacy_path, &original_bytes).unwrap();
    let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

    let outcome = library
        .migrate_managed_schema1_project(&legacy_path, actor())
        .unwrap();

    assert_eq!(std::fs::read(&outcome.backup_path).unwrap(), original_bytes);
    assert_eq!(outcome.project.schema_version(), SCHEMA_2_VERSION);
    assert_eq!(
        outcome.project.campus_scope().target_id(),
        "gaode:B00155J6JH"
    );
    assert_eq!(outcome.project.name(), "V1.0.1 baseline project");
    assert_eq!(
        outcome.project.generation_settings().orientation_degrees,
        12.0
    );
    assert_eq!(outcome.project.generation_settings().blocks_per_meter, 1.0);
    assert!(outcome.project.generated_output().is_none());
    assert!(outcome.project.exported_output().is_none());

    let migration = outcome.project.legacy_migration().unwrap().unwrap();
    let preserved_project = migration.decode_preserved_project().unwrap();
    assert_eq!(preserved_project.name, "V1.0.1 baseline project");
    assert_eq!(preserved_project.building_slots[0].id, "demo-library");
    assert_eq!(
        migration.source_project["campusTarget"]["poiId"],
        "fixture-poi-putuo"
    );
    assert_eq!(
        migration.source_project["features"][0]["id"],
        "accepted-demo-library"
    );
    assert_eq!(
        migration.source_project["buildingSlots"][0]["id"],
        "demo-library"
    );
    assert_eq!(
        migration.source_project["buildingDirectory"][0]["name"],
        "Fixture Library"
    );
    assert_eq!(
        migration.source_project["buildingSuppressions"][0]["sourceId"],
        "fixture-rejected-shed"
    );
    assert_eq!(
        migration.source_project["detailed"]["facadeDrafts"][0]["id"],
        "facade-1"
    );
    assert!(migration
        .legacy_assertions
        .iter()
        .any(|assertion| assertion.subject_id == "candidate:demo-library"
            && assertion.decision == ReviewDecision::Accepted
            && assertion.source_snapshot_id.as_deref() == Some("osm:v1-baseline")));
    assert!(migration
        .needs_reconfirmation
        .iter()
        .any(|item| item.subject_id == "boundary:campus"));
    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "suppression:fixture-rejected-shed"
            && item.reason == ReconfirmationReason::MissingSourceSnapshot
    }));
    assert!(!migration
        .legacy_assertions
        .iter()
        .any(|assertion| assertion.subject_id == "suppression:fixture-rejected-shed"));
    assert_eq!(migration.historical_artifacts.len(), 2);
    assert!(migration
        .historical_artifacts
        .iter()
        .all(|artifact| !artifact.satisfies_v11_completion));
    assert!(migration.report.entries.iter().any(|entry| {
        entry.subject == "legacy-generated-completion"
            && entry.disposition == MigrationDisposition::Omitted
    }));
    assert!(migration.report.entries.iter().any(|entry| {
        entry.subject == "candidate:demo-library"
            && entry.disposition == MigrationDisposition::Transformed
    }));
    assert!(migration.report.entries.iter().any(|entry| {
        entry.subject == "detailed-facade-draft:facade-1"
            && entry.disposition == MigrationDisposition::Preserved
    }));

    let record = library.find_by_name("V1.0.1 baseline project").unwrap();
    assert_eq!(record.project_id(), outcome.project.id());
    assert_eq!(
        directory.path().join(record.managed_relative_path()),
        legacy_path
    );
    let reopened = library.open_project(outcome.project.id()).unwrap();
    assert_eq!(
        reopened.legacy_migration().unwrap().unwrap(),
        migration,
        "migration lineage and report must persist in schema 2"
    );
}

#[test]
fn uncertain_and_manual_legacy_subjects_are_targeted_for_reconfirmation() {
    let directory = tempfile::tempdir().unwrap();
    let (legacy_path, _) = write_managed_fixture(directory.path(), legacy_web_schema1_bytes());
    let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

    let outcome = library
        .migrate_managed_schema1_project(&legacy_path, actor())
        .unwrap();
    let migration = outcome.project.legacy_migration().unwrap().unwrap();

    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "candidate:legacy-library"
            && item.reason == ReconfirmationReason::MissingSourceSnapshot
    }));
    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "feature:legacy-water"
            && item.reason == ReconfirmationReason::ManualOrScreenshotDerivedGeometry
    }));
    assert!(migration.legacy_evidence.iter().any(|evidence| {
        evidence.subject_id == "feature:legacy-water"
            && evidence.kind == LegacyEvidenceKind::ManualGeometry
    }));
    assert!(migration
        .report
        .entries
        .windows(2)
        .all(|pair| pair[0].subject <= pair[1].subject));
}

#[test]
fn contradictory_legacy_decision_is_quarantined_without_becoming_an_assertion() {
    let directory = tempfile::tempdir().unwrap();
    let (legacy_path, original_bytes) =
        write_managed_fixture(directory.path(), populated_native_schema1_bytes());
    let mut contradictory: Value = serde_json::from_slice(&original_bytes).unwrap();
    contradictory["foundationReviewLedger"][0]["decision"] = json!("rejected");
    std::fs::write(
        &legacy_path,
        serde_json::to_vec_pretty(&contradictory).unwrap(),
    )
    .unwrap();
    let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

    let outcome = library
        .migrate_managed_schema1_project(&legacy_path, actor())
        .unwrap();
    let migration = outcome.project.legacy_migration().unwrap().unwrap();

    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "candidate:demo-library"
            && item.reason == ReconfirmationReason::ContradictoryLegacyDecision
    }));
    assert!(!migration
        .legacy_assertions
        .iter()
        .any(|assertion| assertion.subject_id == "candidate:demo-library"));
}

#[test]
fn invalid_inputs_keep_an_explicit_backup_and_do_not_register_partial_state() {
    for original_bytes in [
        b"{ this is deliberately not valid JSON }".to_vec(),
        br#"{"schemaVersion":1,"name":"partial write","campusName":"Fixture Campus","mode":"foundation","candidates":["#.to_vec(),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (legacy_path, original_bytes) =
            write_managed_fixture(directory.path(), original_bytes);
        let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

        let error = library
            .migrate_managed_schema1_project(&legacy_path, actor())
            .unwrap_err();

        assert!(!error.is_empty());
        assert_eq!(std::fs::read(&legacy_path).unwrap(), original_bytes);
        let backup_path = CampusProjectLibrary::schema1_backup_path(&legacy_path).unwrap();
        assert_eq!(std::fs::read(backup_path).unwrap(), original_bytes);
        assert!(library.find_by_name("partial write").is_err());
        assert!(!directory.path().join("library-index.json").exists());
    }

    let directory = tempfile::tempdir().unwrap();
    let (legacy_path, original_bytes) =
        write_managed_fixture(directory.path(), populated_native_schema1_bytes());
    let mut newer: Value = serde_json::from_slice(&original_bytes).unwrap();
    newer["schemaVersion"] = json!(SCHEMA_2_VERSION + 1);
    let newer_bytes = serde_json::to_vec_pretty(&newer).unwrap();
    std::fs::write(&legacy_path, &newer_bytes).unwrap();
    let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

    assert!(library
        .migrate_managed_schema1_project(&legacy_path, actor())
        .unwrap_err()
        .contains("newer than supported schema 1"));
    assert_eq!(std::fs::read(&legacy_path).unwrap(), newer_bytes);
    assert!(library.find_by_name("V1.0.1 baseline project").is_err());
}

#[test]
fn every_injected_migration_failure_keeps_source_backup_active_project_and_library_unchanged() {
    for fault in MigrationFaultPoint::ALL {
        let directory = tempfile::tempdir().unwrap();
        let mut library = CampusProjectLibrary::open_for_construction(
            directory.path(),
            "gaode:B00155J6JH",
            &construction_capability(),
        )
        .unwrap();
        let active_project = library
            .create_project(scope(), "active schema 2 project", actor())
            .unwrap();
        let mut session = Schema2ProjectSession::default();
        session.open_project(&library, active_project.id()).unwrap();
        let (legacy_path, original_bytes) =
            write_managed_fixture(directory.path(), populated_native_schema1_bytes());
        let index_before = std::fs::read(directory.path().join("library-index.json")).unwrap();
        let active_id = session.active().unwrap().id().clone();
        library.inject_next_migration_failure(fault);

        let error = library
            .migrate_managed_schema1_project(&legacy_path, actor())
            .unwrap_err();

        assert!(error.contains("injected migration failure"));
        assert_eq!(std::fs::read(&legacy_path).unwrap(), original_bytes);
        assert_eq!(
            std::fs::read(CampusProjectLibrary::schema1_backup_path(&legacy_path).unwrap())
                .unwrap(),
            original_bytes
        );
        assert_eq!(
            std::fs::read(directory.path().join("library-index.json")).unwrap(),
            index_before
        );
        assert!(library.find_by_name("V1.0.1 baseline project").is_err());
        assert_eq!(session.active().unwrap().id(), &active_id);
        assert_eq!(
            library.open_project(&active_id).unwrap().id(),
            &active_id,
            "an unrelated active project remains openable"
        );
    }
}
