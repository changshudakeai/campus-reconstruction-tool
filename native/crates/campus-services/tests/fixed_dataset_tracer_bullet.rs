use campus_services::acquisition::tracer_bullet::{
    run_fixed_dataset_tracer_bullet, FixedDatasetTracerRequest,
};
use campus_services::acquisition::{fixture_transport::FixtureTransport, AcquisitionClient};
use campus_state::{
    CampusProjectLibrary, CampusScope, FoundationCategory, FoundationResumePoint,
    FoundationReviewDisposition, InstallationId, V11ConstructionCapability,
};

#[test]
fn fixed_dataset_runs_from_confirmed_campus_to_revision_matched_exports() {
    let workspace = tempfile::tempdir().unwrap();
    let library_root = workspace.path().join("library");
    let output_root = workspace.path().join("output");
    let campus_scope = CampusScope::new(
        "gaode:B00155J6JH",
        "East China Normal University Putuo Campus",
        [121.395, 31.202],
    )
    .unwrap();
    let capability = V11ConstructionCapability::request(true, Some("1")).unwrap();
    let client = AcquisitionClient::new(FixtureTransport::canonical().unwrap());

    let report = run_fixed_dataset_tracer_bullet(FixedDatasetTracerRequest {
        library_root: &library_root,
        output_root: &output_root,
        campus_scope: campus_scope.clone(),
        project_name: "V1.1 fixed dataset tracer",
        actor: InstallationId::new("acceptance-test").unwrap(),
        construction_capability: &capability,
        acquisition_client: &client,
    })
    .unwrap();

    assert!(report.schematic_path.exists());
    assert!(report.schematic_bytes > 0);
    assert!(report.manifest_path.exists());
    assert_eq!(
        report.resume_after_reopen,
        FoundationResumePoint::Review(FoundationCategory::Building)
    );

    let library =
        CampusProjectLibrary::open(&library_root, campus_scope.target_id().to_string()).unwrap();
    let reopened = library.open_project(&report.project_id).unwrap();
    assert_eq!(reopened.resume_point(), FoundationResumePoint::Complete);
    assert_eq!(
        reopened.pinned_evidence().unwrap().boundary.bundle_id,
        "cn-campus-2026-06"
    );
    assert_eq!(
        reopened
            .pinned_evidence()
            .unwrap()
            .boundary
            .candidates
            .len(),
        2
    );
    assert_eq!(
        reopened
            .pinned_evidence()
            .unwrap()
            .acquisition
            .observation_count,
        1
    );
    assert_eq!(
        reopened
            .pinned_evidence()
            .unwrap()
            .acquisition
            .coverage_outcomes
            .len(),
        5
    );
    assert_eq!(
        reopened.pinned_evidence().unwrap().acquisition.observations[0].source_record_id,
        "relation/42"
    );
    assert_eq!(
        reopened
            .foundation_review()
            .disposition(FoundationCategory::Building)
            .unwrap(),
        &FoundationReviewDisposition::SelectedEvidence {
            evidence_ids: vec!["obs-osm-relation-42".into()]
        }
    );
    assert!(FoundationCategory::ALL
        .into_iter()
        .all(|category| { reopened.foundation_review().disposition(category).is_some() }));
    assert_eq!(
        reopened.generated_output().unwrap().project_revision,
        reopened.workflow().project_revision()
    );
    assert_eq!(
        reopened.exported_output().unwrap().project_revision,
        reopened.workflow().project_revision()
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["projectId"], report.project_id.as_str());
    assert_eq!(
        manifest["projectRevision"],
        reopened.workflow().project_revision()
    );
    assert_eq!(
        manifest["compatibilityProfileId"],
        "minecraft-java-26.1.2-axiom-v1"
    );
    assert_eq!(manifest["schematic"]["bytes"], report.schematic_bytes);
    assert_eq!(
        manifest["schematic"]["sha256"],
        reopened.exported_output().unwrap().schematic_sha256
    );
}
