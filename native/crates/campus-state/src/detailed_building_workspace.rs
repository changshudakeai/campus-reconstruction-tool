use crate::CampusProject;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailedBuildingWorkspaceTask {
    BlockedNoReviewedBuildingSlot,
    SelectBuilding,
    ChooseEvidenceOrTemplate,
    ReviewFacadeRules,
    ReviewPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetailedBuildingWorkspaceProjection {
    pub task: DetailedBuildingWorkspaceTask,
}

pub struct DetailedBuildingWorkspace;

impl DetailedBuildingWorkspace {
    pub fn projection(project: &CampusProject) -> DetailedBuildingWorkspaceProjection {
        let selected_slot_id = project
            .detailed
            .selected_slot_id
            .as_deref()
            .filter(|selected_id| {
                project
                    .building_slots
                    .iter()
                    .any(|slot| slot.id == *selected_id)
            });
        let task = if project.building_slots.is_empty() {
            DetailedBuildingWorkspaceTask::BlockedNoReviewedBuildingSlot
        } else if selected_slot_id.is_none() {
            DetailedBuildingWorkspaceTask::SelectBuilding
        } else if project.detailed.generated_path.is_some() {
            DetailedBuildingWorkspaceTask::ReviewPreview
        } else if project
            .detailed
            .facade_drafts
            .iter()
            .any(|draft| Some(draft.slot_id.as_str()) == selected_slot_id)
        {
            DetailedBuildingWorkspaceTask::ReviewFacadeRules
        } else {
            DetailedBuildingWorkspaceTask::ChooseEvidenceOrTemplate
        };

        DetailedBuildingWorkspaceProjection { task }
    }
}
