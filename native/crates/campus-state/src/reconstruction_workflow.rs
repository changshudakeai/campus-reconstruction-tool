use crate::{CampusProject, DesktopMode};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructionWorkflowIntent {
    EnterFoundation,
    EnterDetailedBuilding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconstructionWorkflowError {
    DetailedBuildingRequiresReviewedSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailedBuildingHandoff {
    BlockedNoReviewedBuildingSlot,
    Ready { reviewed_building_slots: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampusReconstructionWorkflowProjection {
    pub mode: DesktopMode,
    pub detailed_building_handoff: DetailedBuildingHandoff,
}

impl fmt::Display for ReconstructionWorkflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DetailedBuildingRequiresReviewedSlot => {
                formatter.write_str("单栋精修需要至少一个 Reviewed Building Slot")
            }
        }
    }
}

pub struct CampusReconstructionWorkflow;

impl CampusReconstructionWorkflow {
    pub fn projection(project: &CampusProject) -> CampusReconstructionWorkflowProjection {
        let detailed_building_handoff = if project.building_slots.is_empty() {
            DetailedBuildingHandoff::BlockedNoReviewedBuildingSlot
        } else {
            DetailedBuildingHandoff::Ready {
                reviewed_building_slots: project.building_slots.len(),
            }
        };
        CampusReconstructionWorkflowProjection {
            mode: if project.mode == DesktopMode::Detailed
                && detailed_building_handoff
                    == DetailedBuildingHandoff::BlockedNoReviewedBuildingSlot
            {
                DesktopMode::Foundation
            } else {
                project.mode
            },
            detailed_building_handoff,
        }
    }

    pub fn apply(
        project: &mut CampusProject,
        intent: ReconstructionWorkflowIntent,
    ) -> Result<(), ReconstructionWorkflowError> {
        match intent {
            ReconstructionWorkflowIntent::EnterFoundation => {
                project.mode = DesktopMode::Foundation;
                Ok(())
            }
            ReconstructionWorkflowIntent::EnterDetailedBuilding => {
                if project.building_slots.is_empty() {
                    return Err(ReconstructionWorkflowError::DetailedBuildingRequiresReviewedSlot);
                }
                project.mode = DesktopMode::Detailed;
                Ok(())
            }
        }
    }
}
