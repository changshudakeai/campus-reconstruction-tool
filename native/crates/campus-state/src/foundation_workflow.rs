use crate::{CampusProject, CampusTargetEvidence, DetailedBuildingState, FoundationStep, GeoPoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationPhase {
    Scope,
    Review,
    Generate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundationMapTask {
    CampusSelection,
    CampusBoundary,
    FoundationReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundationWorkflowProjection {
    pub step: FoundationStep,
    pub phase: FoundationPhase,
    pub can_enter_review: bool,
    pub can_enter_generate: bool,
    pub map_task: Option<FoundationMapTask>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FoundationWorkflowIntent {
    SelectCampusTarget(CampusTargetEvidence),
    ConfirmCampusBoundary(Vec<GeoPoint>),
    SetCampusMetrics {
        orientation_degrees: f64,
        blocks_per_meter: f64,
    },
    EnterPhase(FoundationPhase),
    CompleteCurrentStep,
}

pub struct FoundationWorkflow;

impl FoundationWorkflow {
    pub fn projection(project: &CampusProject) -> FoundationWorkflowProjection {
        let phase = match project.foundation_step {
            FoundationStep::Campus | FoundationStep::Boundary | FoundationStep::Orientation => {
                FoundationPhase::Scope
            }
            FoundationStep::Building
            | FoundationStep::Road
            | FoundationStep::Water
            | FoundationStep::Vegetation
            | FoundationStep::Sports => FoundationPhase::Review,
            FoundationStep::Export => FoundationPhase::Generate,
        };
        FoundationWorkflowProjection {
            step: project.foundation_step,
            phase,
            can_enter_review: Self::scope_is_complete(project),
            can_enter_generate: Self::review_is_complete(project),
            map_task: match project.foundation_step {
                FoundationStep::Campus => Some(FoundationMapTask::CampusSelection),
                FoundationStep::Boundary => Some(FoundationMapTask::CampusBoundary),
                FoundationStep::Building
                | FoundationStep::Road
                | FoundationStep::Water
                | FoundationStep::Vegetation
                | FoundationStep::Sports => Some(FoundationMapTask::FoundationReview),
                FoundationStep::Orientation | FoundationStep::Export => None,
            },
        }
    }

    pub fn apply(
        project: &mut CampusProject,
        intent: FoundationWorkflowIntent,
    ) -> Result<(), String> {
        match intent {
            FoundationWorkflowIntent::SelectCampusTarget(target) => {
                Self::select_campus_target(project, target);
                Ok(())
            }
            FoundationWorkflowIntent::ConfirmCampusBoundary(points) => {
                Self::confirm_campus_boundary(project, points)
            }
            FoundationWorkflowIntent::SetCampusMetrics {
                orientation_degrees,
                blocks_per_meter,
            } => {
                if project.campus_target.is_none() || project.boundary.len() < 3 {
                    return Err("请先确认 Campus Target 与 Campus Boundary".into());
                }
                if !orientation_degrees.is_finite() || !blocks_per_meter.is_finite() {
                    return Err("Campus Orientation 与 Campus Scale 必须是有限数值".into());
                }
                project.orientation_degrees = orientation_degrees.clamp(-180.0, 180.0);
                project.blocks_per_meter = blocks_per_meter.clamp(0.25, 4.0);
                Ok(())
            }
            FoundationWorkflowIntent::EnterPhase(phase) => Self::enter_phase(project, phase),
            FoundationWorkflowIntent::CompleteCurrentStep => Self::complete_current_step(project),
        }
    }

    pub fn ensure_feature_discovery_allowed(project: &CampusProject) -> Result<(), String> {
        if !Self::scope_is_complete(project) {
            return Err(
                "必须先确认 Campus Target、Campus Boundary 与 Campus Orientation，才能发现地基要素"
                    .into(),
            );
        }
        if !matches!(
            project.foundation_step,
            FoundationStep::Building
                | FoundationStep::Road
                | FoundationStep::Water
                | FoundationStep::Vegetation
                | FoundationStep::Sports
        ) {
            return Err("当前 Foundation Workflow 任务不允许发现地基要素".into());
        }
        Ok(())
    }

    fn scope_is_complete(project: &CampusProject) -> bool {
        project.campus_target.is_some()
            && project.boundary.len() >= 3
            && project
                .completed_steps
                .contains(&FoundationStep::Orientation)
    }

    fn review_is_complete(project: &CampusProject) -> bool {
        project.completed_steps.contains(&FoundationStep::Sports)
            || project.foundation_step == FoundationStep::Export
    }

    fn select_campus_target(project: &mut CampusProject, target: CampusTargetEvidence) {
        let target_changed = project
            .campus_target
            .as_ref()
            .is_none_or(|current| current.poi_id != target.poi_id);
        if target_changed {
            project.boundary.clear();
            project.candidates.clear();
            project.foundation_source_snapshots.clear();
            project.foundation_review_ledger.clear();
            project.features.clear();
            project.building_slots.clear();
            project.building_directory.clear();
            project.building_suppressions.clear();
            project.foundation_preview_path = None;
            project.visual_capture_path = None;
            project.detailed = DetailedBuildingState::default();
            project.completed_steps = vec![FoundationStep::Campus];
        }
        project.campus_name = target.name.clone();
        project.map_view.center = target.gcj02;
        project.campus_target = Some(target);
        if project.foundation_step == FoundationStep::Campus {
            project.foundation_step = FoundationStep::Boundary;
        }
        Self::mark_complete(project, FoundationStep::Campus);
    }

    fn confirm_campus_boundary(
        project: &mut CampusProject,
        points: Vec<GeoPoint>,
    ) -> Result<(), String> {
        if project.campus_target.is_none() {
            return Err("请先确认 Campus Target".into());
        }
        if points.len() < 3
            || points
                .iter()
                .any(|point| !point.lng.is_finite() || !point.lat.is_finite())
        {
            return Err("Campus Boundary 至少需要三个有效节点".into());
        }
        project.boundary = points;
        Self::mark_complete(project, FoundationStep::Campus);
        Self::mark_complete(project, FoundationStep::Boundary);
        project.foundation_step = FoundationStep::Orientation;
        Ok(())
    }

    fn enter_phase(project: &mut CampusProject, phase: FoundationPhase) -> Result<(), String> {
        match phase {
            FoundationPhase::Scope => {
                project.foundation_step = FoundationStep::Campus;
                Ok(())
            }
            FoundationPhase::Review if Self::scope_is_complete(project) => {
                if !matches!(
                    project.foundation_step,
                    FoundationStep::Building
                        | FoundationStep::Road
                        | FoundationStep::Water
                        | FoundationStep::Vegetation
                        | FoundationStep::Sports
                ) {
                    project.foundation_step = FoundationStep::Building;
                }
                Ok(())
            }
            FoundationPhase::Generate if Self::review_is_complete(project) => {
                project.foundation_step = FoundationStep::Export;
                Ok(())
            }
            FoundationPhase::Review => {
                Err("请先完成 Campus Target、Campus Boundary 与 Campus Orientation".into())
            }
            FoundationPhase::Generate => Err("请先完成 Foundation Feature Review".into()),
        }
    }

    fn complete_current_step(project: &mut CampusProject) -> Result<(), String> {
        match project.foundation_step {
            FoundationStep::Campus => Err("请在高德中确认 Campus Target".into()),
            FoundationStep::Boundary => Err("请在高德中确认 Campus Boundary".into()),
            FoundationStep::Orientation => {
                if project.campus_target.is_none() || project.boundary.len() < 3 {
                    return Err("Campus Target 或 Campus Boundary 尚未确认".into());
                }
                Self::mark_complete(project, FoundationStep::Orientation);
                project.foundation_step = FoundationStep::Building;
                Ok(())
            }
            FoundationStep::Building
            | FoundationStep::Road
            | FoundationStep::Water
            | FoundationStep::Vegetation
            | FoundationStep::Sports => {
                let completed = project.foundation_step;
                Self::mark_complete(project, completed);
                project.foundation_step = completed.next();
                Ok(())
            }
            FoundationStep::Export => {
                Self::mark_complete(project, FoundationStep::Export);
                Ok(())
            }
        }
    }

    fn mark_complete(project: &mut CampusProject, step: FoundationStep) {
        if !project.completed_steps.contains(&step) {
            project.completed_steps.push(step);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> CampusTargetEvidence {
        CampusTargetEvidence {
            poi_id: "campus-a".into(),
            name: "校区 A".into(),
            gcj02: GeoPoint {
                lng: 121.4,
                lat: 31.2,
            },
            wgs84: GeoPoint {
                lng: 121.395,
                lat: 31.202,
            },
            acquisition: "gaode_poi_search".into(),
        }
    }

    fn boundary() -> Vec<GeoPoint> {
        vec![
            GeoPoint { lng: 1.0, lat: 1.0 },
            GeoPoint { lng: 2.0, lat: 1.0 },
            GeoPoint { lng: 2.0, lat: 2.0 },
        ]
    }

    #[test]
    fn workflow_rejects_review_until_scope_is_complete() {
        let mut project = CampusProject::new("test", "campus");
        assert!(FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::EnterPhase(FoundationPhase::Review)
        )
        .is_err());
        assert!(FoundationWorkflow::ensure_feature_discovery_allowed(&project).is_err());

        FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::SelectCampusTarget(target()),
        )
        .unwrap();
        FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::ConfirmCampusBoundary(boundary()),
        )
        .unwrap();
        FoundationWorkflow::apply(&mut project, FoundationWorkflowIntent::CompleteCurrentStep)
            .unwrap();

        assert_eq!(project.foundation_step, FoundationStep::Building);
        assert!(FoundationWorkflow::ensure_feature_discovery_allowed(&project).is_ok());
    }

    #[test]
    fn map_task_is_derived_from_the_workflow_step() {
        let mut project = CampusProject::new("test", "campus");
        assert_eq!(
            FoundationWorkflow::projection(&project).map_task,
            Some(FoundationMapTask::CampusSelection)
        );
        FoundationWorkflow::apply(
            &mut project,
            FoundationWorkflowIntent::SelectCampusTarget(target()),
        )
        .unwrap();
        assert_eq!(
            FoundationWorkflow::projection(&project).map_task,
            Some(FoundationMapTask::CampusBoundary)
        );
    }
}
