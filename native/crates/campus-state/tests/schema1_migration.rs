use campus_state::{
    decode_schema1_project, CampusProjectLibrary, CampusScope, InstallationId, LegacyEvidenceKind,
    MigrationDisposition, MigrationFaultPoint, ReviewDecision, Schema2ProjectSession,
    V11ConstructionCapability, SCHEMA_2_VERSION,
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

fn copy_fixture(root: &Path, relative: &str) -> (PathBuf, Vec<u8>) {
    let bytes = std::fs::read(test_data(relative)).unwrap();
    let path = root.join("managed-v1-project.campus.json");
    std::fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

#[test]
fn schema1_compatibility_decoder_is_read_only_and_rejects_other_schemas() {
    let bytes = std::fs::read(test_data("v1-demo.campus.json")).unwrap();
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
    let (legacy_path, original_bytes) = copy_fixture(directory.path(), "v1-demo.campus.json");
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
    assert_eq!(migration.historical_artifacts.len(), 2);
    assert!(migration
        .historical_artifacts
        .iter()
        .all(|artifact| !artifact.satisfies_v11_completion));
    assert!(migration.report.entries.iter().any(|entry| {
        entry.subject == "legacy-generated-completion"
            && entry.disposition == MigrationDisposition::Omitted
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
    let (legacy_path, _) =
        copy_fixture(directory.path(), "v1.0.1/legacy-web-portable-project.json");
    let mut library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();

    let outcome = library
        .migrate_managed_schema1_project(&legacy_path, actor())
        .unwrap();
    let migration = outcome.project.legacy_migration().unwrap().unwrap();

    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "candidate:legacy-library" && item.reason == "missing-source-snapshot"
    }));
    assert!(migration.needs_reconfirmation.iter().any(|item| {
        item.subject_id == "feature:legacy-water"
            && item.reason == "manual-or-screenshot-derived-geometry"
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
    let (legacy_path, original_bytes) = copy_fixture(directory.path(), "v1-demo.campus.json");
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
            && item.reason == "contradictory-legacy-decision"
    }));
    assert!(!migration
        .legacy_assertions
        .iter()
        .any(|assertion| assertion.subject_id == "candidate:demo-library"));
}

#[test]
fn invalid_inputs_keep_an_explicit_backup_and_do_not_register_partial_state() {
    for fixture in [
        "v1.0.1/failures/corrupt-project.json",
        "v1.0.1/failures/truncated-project.json",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (legacy_path, original_bytes) = copy_fixture(directory.path(), fixture);
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
    let (legacy_path, original_bytes) = copy_fixture(directory.path(), "v1-demo.campus.json");
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
        let (legacy_path, original_bytes) = copy_fixture(directory.path(), "v1-demo.campus.json");
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
