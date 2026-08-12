//! 离开工作区完整用例：依据工作区状态与运行中操作决定导航、确认或阻断。
//!
//! S1 只提交离开目标；采集/导出呈现入口在操作生命周期变化时更新本入口，
//! 不允许用 UI 轮询 timer 推导业务安全性。

use std::cell::Cell;
use std::rc::Rc;

use crate::presentation::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceOperation {
    Collection,
    Export,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LeaveWorkspaceIntent {
    pub(super) target: Screen,
    pub(super) map_processing: bool,
    pub(super) active_step: i32,
    pub(super) boundary_unsaved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaveConfirmationReason {
    UnsavedBoundary,
    OperationRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaveBlockedReason {
    BoundaryMapProcessing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LeaveWorkspaceDecision {
    Allow { target: Screen },
    NeedsConfirmation(LeaveConfirmationReason),
    Blocked(LeaveBlockedReason),
}

#[derive(Clone, Default)]
pub(super) struct LeaveWorkspaceUseCase {
    active_operations: Rc<Cell<u8>>,
}

impl LeaveWorkspaceUseCase {
    const COLLECTION_ACTIVE: u8 = 1;
    const EXPORT_ACTIVE: u8 = 2;

    pub(super) fn operation_started(&self, operation: WorkspaceOperation) {
        self.active_operations
            .set(self.active_operations.get() | Self::operation_flag(operation));
    }

    pub(super) fn operation_finished(&self, operation: WorkspaceOperation) {
        self.active_operations
            .set(self.active_operations.get() & !Self::operation_flag(operation));
    }

    pub(super) fn decide(&self, intent: LeaveWorkspaceIntent) -> LeaveWorkspaceDecision {
        if intent.map_processing && intent.active_step == 0 {
            return LeaveWorkspaceDecision::Blocked(LeaveBlockedReason::BoundaryMapProcessing);
        }
        if intent.boundary_unsaved {
            return LeaveWorkspaceDecision::NeedsConfirmation(
                LeaveConfirmationReason::UnsavedBoundary,
            );
        }
        if self.active_operations.get() != 0 {
            return LeaveWorkspaceDecision::NeedsConfirmation(
                LeaveConfirmationReason::OperationRunning,
            );
        }
        LeaveWorkspaceDecision::Allow {
            target: intent.target,
        }
    }

    const fn operation_flag(operation: WorkspaceOperation) -> u8 {
        match operation {
            WorkspaceOperation::Collection => Self::COLLECTION_ACTIVE,
            WorkspaceOperation::Export => Self::EXPORT_ACTIVE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_list_intent() -> LeaveWorkspaceIntent {
        LeaveWorkspaceIntent {
            target: Screen::PlanList,
            map_processing: false,
            active_step: 2,
            boundary_unsaved: false,
        }
    }

    #[test]
    fn collection_operation_requires_confirmation_until_finished() {
        let use_case = LeaveWorkspaceUseCase::default();
        use_case.operation_started(WorkspaceOperation::Collection);

        assert_eq!(
            use_case.decide(plan_list_intent()),
            LeaveWorkspaceDecision::NeedsConfirmation(LeaveConfirmationReason::OperationRunning)
        );

        use_case.operation_finished(WorkspaceOperation::Collection);
        assert_eq!(
            use_case.decide(plan_list_intent()),
            LeaveWorkspaceDecision::Allow {
                target: Screen::PlanList
            }
        );
    }

    #[test]
    fn export_operation_requires_confirmation_until_finished() {
        let use_case = LeaveWorkspaceUseCase::default();
        use_case.operation_started(WorkspaceOperation::Export);

        assert_eq!(
            use_case.decide(plan_list_intent()),
            LeaveWorkspaceDecision::NeedsConfirmation(LeaveConfirmationReason::OperationRunning)
        );

        use_case.operation_finished(WorkspaceOperation::Export);
        assert_eq!(
            use_case.decide(plan_list_intent()),
            LeaveWorkspaceDecision::Allow {
                target: Screen::PlanList
            }
        );
    }
}
