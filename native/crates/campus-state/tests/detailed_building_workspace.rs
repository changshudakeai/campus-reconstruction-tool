use std::path::PathBuf;

use campus_state::{
    BuildingSlot, CampusProject, DetailedBuildingWorkspace, DetailedBuildingWorkspaceTask,
    FacadeReconstructionDraft,
};

fn project_with_reviewed_building() -> CampusProject {
    let mut project = CampusProject::new("Putuo Campus", "East China Normal University");
    project.building_slots.push(BuildingSlot {
        id: "building-1".into(),
        name: "Teaching Building".into(),
        footprint: Vec::new(),
        height_m: Some(18.0),
        floors: Some(5),
        roof_shape: Some("flat".into()),
        refined: false,
    });
    project
}

#[test]
fn detailed_building_workspace_is_blocked_without_a_reviewed_slot() {
    let project = CampusProject::new("Putuo Campus", "East China Normal University");

    let projection = DetailedBuildingWorkspace::projection(&project);

    assert_eq!(
        projection.task,
        DetailedBuildingWorkspaceTask::BlockedNoReviewedBuildingSlot
    );
}

#[test]
fn detailed_building_workspace_starts_by_selecting_a_building() {
    let project = project_with_reviewed_building();

    assert_eq!(
        DetailedBuildingWorkspace::projection(&project).task,
        DetailedBuildingWorkspaceTask::SelectBuilding
    );
}

#[test]
fn selecting_a_building_advances_to_evidence_and_template() {
    let mut project = project_with_reviewed_building();
    project.detailed.selected_slot_id = Some("building-1".into());

    assert_eq!(
        DetailedBuildingWorkspace::projection(&project).task,
        DetailedBuildingWorkspaceTask::ChooseEvidenceOrTemplate
    );
}

#[test]
fn a_facade_draft_advances_to_rule_review() {
    let mut project = project_with_reviewed_building();
    project.detailed.selected_slot_id = Some("building-1".into());
    project
        .detailed
        .facade_drafts
        .push(FacadeReconstructionDraft {
            id: "draft-1".into(),
            slot_id: "building-1".into(),
            model_version: "template-v1".into(),
            confidence: 80,
            rules: Vec::new(),
            evidence_ids: Vec::new(),
        });

    assert_eq!(
        DetailedBuildingWorkspace::projection(&project).task,
        DetailedBuildingWorkspaceTask::ReviewFacadeRules
    );
}

#[test]
fn a_generated_preview_advances_to_preview_review() {
    let mut project = project_with_reviewed_building();
    project.detailed.selected_slot_id = Some("building-1".into());
    project
        .detailed
        .facade_drafts
        .push(FacadeReconstructionDraft {
            id: "draft-1".into(),
            slot_id: "building-1".into(),
            model_version: "template-v1".into(),
            confidence: 80,
            rules: Vec::new(),
            evidence_ids: Vec::new(),
        });
    project.detailed.generated_path = Some(PathBuf::from("preview.schem"));

    assert_eq!(
        DetailedBuildingWorkspace::projection(&project).task,
        DetailedBuildingWorkspaceTask::ReviewPreview
    );
}
