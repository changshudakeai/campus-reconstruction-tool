use campus_state::{
    DesktopApplicationState, FoundationSourceSnapshot, FoundationSourceStatus,
    FoundationStylePreset, ReviewDecision,
};
use serde_json::Value;
use std::path::PathBuf;

fn test_data(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-data")
        .join(relative)
}

#[test]
fn native_schema_1_fixture_contains_the_frozen_v1_0_1_field_set() {
    let path = test_data("v1-demo.campus.json");
    let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let root = value.as_object().unwrap();
    for field in [
        "schemaVersion",
        "name",
        "campusName",
        "campusTarget",
        "mode",
        "foundationStep",
        "completedSteps",
        "boundary",
        "orientationDegrees",
        "blocksPerMeter",
        "mapView",
        "candidates",
        "foundationSourceSnapshots",
        "foundationReviewLedger",
        "features",
        "buildingSlots",
        "buildingDirectory",
        "buildingSuppressions",
        "foundationStylePreset",
        "foundationStylePack",
        "foundationPreviewPath",
        "visualCapturePath",
        "detailed",
    ] {
        assert!(
            root.contains_key(field),
            "native fixture is missing {field}"
        );
    }
    let detailed = root["detailed"].as_object().unwrap();
    for field in [
        "selectedSlotId",
        "stylePreset",
        "wallBlock",
        "windowDensity",
        "wallDepth",
        "generatedPath",
        "refinements",
        "semanticFeatures",
        "externalModels",
        "sourceConflicts",
        "evidenceAssets",
        "functionClassifications",
        "templateProposals",
        "selectedTemplates",
        "facadeDrafts",
    ] {
        assert!(
            detailed.contains_key(field),
            "detailed fixture is missing {field}"
        );
    }

    let mut state = DesktopApplicationState::default();
    state.open(path).unwrap();
    let project = state.project.unwrap();
    assert_eq!(project.schema_version, 1);
    assert_eq!(
        project.detailed.selected_slot_id.as_deref(),
        Some("demo-library")
    );
    assert!(!project.foundation_source_snapshots.is_empty());
    assert!(!project.detailed.facade_drafts.is_empty());
    assert!(project.detailed.generated_path.is_none());
}

#[test]
fn legacy_web_portable_fixture_preserves_supported_v1_fields() {
    let mut state = DesktopApplicationState::default();
    state
        .open(test_data("v1.0.1/legacy-web-portable-project.json"))
        .unwrap();
    let project = state.project.unwrap();

    assert_eq!(project.name, "Legacy web portable fixture");
    assert_eq!(project.campus_name, "ECNU Putuo Campus");
    assert_eq!(project.orientation_degrees, 18.0);
    assert_eq!(project.blocks_per_meter, 1.5);
    assert_eq!(
        project.foundation_style_preset,
        FoundationStylePreset::HistoricRedBrick
    );
    assert!(project
        .candidates
        .iter()
        .any(|candidate| candidate.id == "legacy-library"
            && candidate.review == ReviewDecision::Accepted));
    assert!(project
        .features
        .iter()
        .any(|feature| feature.id == "legacy-water"));
    assert!(project
        .building_suppressions
        .iter()
        .any(|record| record.source_id == "legacy-shed"));
}

#[test]
fn versioned_regression_contract_names_every_v1_seam() {
    let contract: Value = serde_json::from_slice(
        &std::fs::read(test_data("v1.0.1/regression-contract.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(contract["baseline"], "1.0.1");
    for seam in [
        "foundation",
        "detailed",
        "provider",
        "coordinate",
        "generator",
        "schematic",
        "helperProcess",
        "deploymentContract",
    ] {
        assert!(
            contract.get(seam).is_some(),
            "regression contract is missing {seam}"
        );
    }
    assert_eq!(contract["foundation"]["minimumFeatureCount"], 1);
    assert_eq!(contract["detailed"]["selectedSlotId"], "demo-library");
    assert_eq!(contract["provider"]["failedSnapshotStatus"], "failed");
    assert_eq!(contract["coordinate"]["gcj02"][0], 121.406582);
    assert_eq!(contract["generator"]["seed"], 42);
    assert_eq!(contract["schematic"]["version"], 3);
    assert_eq!(contract["helperProcess"]["protocolVersion"], 5);
    assert_eq!(contract["deploymentContract"]["healthPath"], "/health");
    assert_eq!(
        contract["deploymentContract"]["queryPath"],
        "/overture/buildings"
    );
}

#[test]
fn frozen_failure_inputs_are_classified_without_secrets_or_machine_paths() {
    let failures = test_data("v1.0.1/failures");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&failures).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("C:\\Users\\"));
        assert!(!text.to_ascii_lowercase().contains("api_key"));
        assert!(!text.to_ascii_lowercase().contains("securityjscode"));
        names.push(path.file_name().unwrap().to_string_lossy().into_owned());
    }
    names.sort();
    assert_eq!(
        names,
        [
            "corrupt-project.json",
            "injected-provider-failure.json",
            "partial-write-project.json",
            "truncated-project.json",
            "unsafe-portable-path.json",
        ]
    );

    for file in [
        "corrupt-project.json",
        "partial-write-project.json",
        "truncated-project.json",
    ] {
        let mut state = DesktopApplicationState::default();
        assert!(
            state.open(failures.join(file)).is_err(),
            "{file} must remain invalid"
        );
    }
    let unsafe_fixture: Value =
        serde_json::from_slice(&std::fs::read(failures.join("unsafe-portable-path.json")).unwrap())
            .unwrap();
    assert_eq!(
        unsafe_fixture.pointer("/detailed/evidenceAssets/0/relativePath"),
        Some(&Value::String("../../outside.txt".into()))
    );
    let mut unsafe_state = DesktopApplicationState::default();
    unsafe_state
        .open(failures.join("unsafe-portable-path.json"))
        .unwrap();
    let portable_output = tempfile::tempdir().unwrap();
    let portable_destination = portable_output.path().join("portable.campus.json");
    assert!(unsafe_state
        .save_as_portable(&portable_destination)
        .is_err());

    let injected: FoundationSourceSnapshot = serde_json::from_slice(
        &std::fs::read(failures.join("injected-provider-failure.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(injected.status, FoundationSourceStatus::Failed);
    assert_eq!(
        injected.error.as_deref(),
        Some("fixture-injected provider timeout")
    );
}
