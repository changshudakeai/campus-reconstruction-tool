use campus_state::{
    decode_schema2_project, CampusProjectLibrary, CampusScope, InstallationId,
    Schema2ProjectSession, V11CompatibilityProfile, V11ConstructionCapability, SCHEMA_2_VERSION,
};
use serde_json::json;

fn putuo_scope() -> CampusScope {
    CampusScope::new(
        "gaode:B00155J6JH",
        "华东师范大学普陀校区",
        [121.395, 31.202],
    )
    .unwrap()
}

fn construction_capability() -> V11ConstructionCapability {
    V11ConstructionCapability::request(true, Some("1")).unwrap()
}

#[test]
fn new_project_is_schema_2_with_immutable_identity_and_fixed_profile() {
    let directory = tempfile::tempdir().unwrap();
    let mut library = CampusProjectLibrary::open_for_construction(
        directory.path(),
        "gaode:B00155J6JH",
        &construction_capability(),
    )
    .unwrap();
    let actor = InstallationId::new("development-installation").unwrap();

    let created = library
        .create_project(putuo_scope(), "普陀校园重建", actor.clone())
        .unwrap();
    let original_id = created.id().clone();

    let other_campus =
        CampusScope::new("gaode:OTHER-CAMPUS", "Other Campus", [121.5, 31.3]).unwrap();
    assert!(library
        .create_project(other_campus, "Cross-campus project", actor.clone())
        .unwrap_err()
        .contains("does not match"));
    let mut cross_campus_copy = serde_json::to_value(&created).unwrap();
    cross_campus_copy["campusScope"]["targetId"] = json!("gaode:OTHER-CAMPUS");
    let cross_campus_copy =
        decode_schema2_project(&serde_json::to_vec(&cross_campus_copy).unwrap()).unwrap();
    assert!(library
        .save_project(&cross_campus_copy)
        .unwrap_err()
        .contains("does not match"));

    assert_eq!(created.schema_version(), SCHEMA_2_VERSION);
    assert_eq!(created.campus_scope().target_id(), "gaode:B00155J6JH");
    assert_eq!(created.audit().created_by(), &actor);
    assert_eq!(created.audit().updated_by(), &actor);
    assert_eq!(
        created.compatibility_profile(),
        &V11CompatibilityProfile::minecraft_java_26_1_2()
    );
    assert_eq!(
        created.compatibility_profile().preview_profile_id(),
        "minecraft-java-26.1.2-preview-v1"
    );
    assert_eq!(
        created.compatibility_profile().export_profile_id(),
        "minecraft-java-26.1.2-sponge-v3-v1"
    );
    assert_ne!(
        created.compatibility_profile().preview_profile_id(),
        created.compatibility_profile().export_profile_id()
    );
    assert_eq!(created.workflow().project_revision(), 0);

    library
        .rename_project(&original_id, "普陀校园重建（修订）", actor)
        .unwrap();
    let renamed = library.open_project(&original_id).unwrap();

    assert_eq!(renamed.id(), &original_id);
    assert_eq!(renamed.name(), "普陀校园重建（修订）");
    assert_eq!(renamed.schema_version(), SCHEMA_2_VERSION);
    assert_ne!(
        library
            .record(&original_id)
            .unwrap()
            .managed_relative_path(),
        renamed.name(),
        "the editable name must not be product or storage identity"
    );
}

#[test]
fn campus_scoped_record_can_find_save_close_and_reopen_a_project() {
    let directory = tempfile::tempdir().unwrap();
    let actor = InstallationId::new("development-installation").unwrap();
    let project_id = {
        let mut library = CampusProjectLibrary::open_for_construction(
            directory.path(),
            "gaode:B00155J6JH",
            &construction_capability(),
        )
        .unwrap();
        let mut project = library
            .create_project(putuo_scope(), "图书馆重建", actor.clone())
            .unwrap();
        project.mark_updated(actor).unwrap();
        library.save_project(&project).unwrap();
        let found = library.find_by_name("图书馆重建").unwrap();
        assert_eq!(found.project_id(), project.id());
        project.id().clone()
    };

    let library = CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();
    let reopened = library.open_project(&project_id).unwrap();

    assert_eq!(reopened.id(), &project_id);
    assert_eq!(reopened.workflow().project_revision(), 1);
    assert_eq!(
        reopened.compatibility_profile().minecraft_version(),
        "26.1.2"
    );
    assert_eq!(reopened.compatibility_profile().edition(), "Java Edition");
}

#[test]
fn round_trip_preserves_supported_optional_state_and_newer_schema_keeps_active_project() {
    let directory = tempfile::tempdir().unwrap();
    let actor = InstallationId::new("development-installation").unwrap();
    let mut library = CampusProjectLibrary::open_for_construction(
        directory.path(),
        "gaode:B00155J6JH",
        &construction_capability(),
    )
    .unwrap();
    let project = library
        .create_project(putuo_scope(), "可选状态测试", actor)
        .unwrap();
    let mut session = Schema2ProjectSession::default();
    session.open_project(&library, project.id()).unwrap();
    let mut supported = serde_json::to_value(&project).unwrap();
    supported["futureOptionalState"] = json!({"providerHint": "fixture-v2"});
    supported["audit"]["futureEditorKind"] = json!("fixture-operator");
    supported["workflow"]["futureOptionalTask"] = json!({"state": "pending"});
    supported["compatibilityProfile"]["futureCatalogDigest"] = json!("sha256:fixture");
    let supported_path = directory.path().join(
        library
            .record(project.id())
            .unwrap()
            .managed_relative_path(),
    );
    std::fs::write(
        &supported_path,
        serde_json::to_vec_pretty(&supported).unwrap(),
    )
    .unwrap();
    session.open_project(&library, project.id()).unwrap();
    library.save_project(session.active().unwrap()).unwrap();
    let round_trip: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&supported_path).unwrap()).unwrap();
    assert_eq!(
        round_trip["futureOptionalState"]["providerHint"],
        "fixture-v2"
    );
    assert_eq!(round_trip["audit"]["futureEditorKind"], "fixture-operator");
    assert_eq!(
        round_trip["workflow"]["futureOptionalTask"]["state"],
        "pending"
    );
    assert_eq!(
        round_trip["compatibilityProfile"]["futureCatalogDigest"],
        "sha256:fixture"
    );

    let mut newer = supported;
    newer["schemaVersion"] = json!(SCHEMA_2_VERSION + 1);
    std::fs::write(&supported_path, serde_json::to_vec_pretty(&newer).unwrap()).unwrap();
    let before_rejected_open = session.active().unwrap().id().clone();

    let error = session.open_project(&library, project.id()).unwrap_err();

    assert!(error.contains("newer than supported schema 2"));
    assert_eq!(session.active().unwrap().id(), &before_rejected_open);
}

#[test]
fn v11_construction_gate_is_internal_and_disabled_for_stable_builds() {
    assert!(campus_state::v11_construction_enabled(true, Some("1")));
    assert!(!campus_state::v11_construction_enabled(true, None));
    assert!(!campus_state::v11_construction_enabled(false, Some("1")));
    assert!(!campus_state::v11_construction_enabled(true, Some("true")));

    let directory = tempfile::tempdir().unwrap();
    let mut stable_library =
        CampusProjectLibrary::open(directory.path(), "gaode:B00155J6JH").unwrap();
    assert!(stable_library
        .create_project(
            putuo_scope(),
            "Stable path must not construct",
            InstallationId::new("stable-installation").unwrap(),
        )
        .unwrap_err()
        .contains("development gate"));
}
