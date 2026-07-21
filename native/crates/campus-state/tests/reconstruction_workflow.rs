use campus_state::{
    BuildingSlot, CampusProject, CampusReconstructionWorkflow, DesktopApplicationState,
    DesktopMode, DetailedBuildingHandoff, ReconstructionWorkflowIntent,
};

#[test]
fn detailed_building_mode_requires_a_reviewed_building_slot() {
    let mut project = CampusProject::new("Putuo Campus", "华东师范大学普陀校区");

    let result = CampusReconstructionWorkflow::apply(
        &mut project,
        ReconstructionWorkflowIntent::EnterDetailedBuilding,
    );

    assert_eq!(project.mode, DesktopMode::Foundation);
    assert_eq!(
        result.unwrap_err().to_string(),
        "单栋精修需要至少一个 Reviewed Building Slot"
    );
}

#[test]
fn reviewed_building_slot_allows_detailed_mode_and_foundation_remains_returnable() {
    let mut state = DesktopApplicationState::default();
    state.new_project("Putuo Campus", "华东师范大学普陀校区");
    state.mutate_project(|project| {
        project.building_slots.push(BuildingSlot {
            id: "library".into(),
            name: "图书馆".into(),
            footprint: Vec::new(),
            height_m: Some(24.0),
            floors: Some(5),
            roof_shape: Some("flat".into()),
            refined: false,
        });
    });

    state
        .apply_reconstruction_intent(ReconstructionWorkflowIntent::EnterDetailedBuilding)
        .unwrap();
    assert_eq!(state.project.as_ref().unwrap().mode, DesktopMode::Detailed);

    state
        .apply_reconstruction_intent(ReconstructionWorkflowIntent::EnterFoundation)
        .unwrap();
    assert_eq!(
        state.project.as_ref().unwrap().mode,
        DesktopMode::Foundation
    );
}

#[test]
fn rejected_detailed_handoff_does_not_create_undo_or_dirty_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.campus.json");
    let mut state = DesktopApplicationState::default();
    state.new_project("Putuo Campus", "华东师范大学普陀校区");
    state.save_to(path).unwrap();

    let result =
        state.apply_reconstruction_intent(ReconstructionWorkflowIntent::EnterDetailedBuilding);

    assert!(result.is_err());
    assert_eq!(
        state.project.as_ref().unwrap().mode,
        DesktopMode::Foundation
    );
    assert!(!state.can_undo());
    assert!(!state.dirty);
}

#[test]
fn project_workbench_exposes_a_blocked_detailed_building_handoff() {
    let project = CampusProject::new("Putuo Campus", "华东师范大学普陀校区");

    let projection = CampusReconstructionWorkflow::projection(&project);

    assert_eq!(
        projection.detailed_building_handoff,
        DetailedBuildingHandoff::BlockedNoReviewedBuildingSlot
    );
}

#[test]
fn project_workbench_recovers_a_legacy_detailed_mode_without_slots() {
    let mut project = CampusProject::new("Putuo Campus", "华东师范大学普陀校区");
    project.mode = DesktopMode::Detailed;

    let projection = CampusReconstructionWorkflow::projection(&project);

    assert_eq!(projection.mode, DesktopMode::Foundation);
    assert_eq!(
        projection.detailed_building_handoff,
        DetailedBuildingHandoff::BlockedNoReviewedBuildingSlot
    );
}
