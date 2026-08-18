//! 全局导航策略：集中拥有历史栈、stackable 规则、返回/离开/确认/取消路由。
//!
//! 输入是“当前屏幕 + 返回/离开意图 + 工作区入口的离开安全判定”，输出是结构化
//! 转移结果（显示目标页 / 需要确认 / 停留，以及离开时是否放弃交付上下文）。
//! 离开是否安全仍由 `workspace_leave.rs`（工作区入口侧）判定，本模块只消费结论，
//! 不重判业务条件；本模块不持有功能入口引用，也不做运行期业务编排（ADR-0037/0039）。

use crate::presentation::{NavigationDecision, OperationState, Screen};

/// 工作区入口已判定的离开安全结论（由 workspace_leave 经呈现层转交）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaveSafety {
    /// 可以直接离开；携带功能入口实际决定显示的目标页。
    Allowed(Screen),
    /// 需要用户确认后才离开；携带等待确认的目标页。
    NeedsConfirmation(Screen),
    /// 必须停留当前页（如边界地图正在处理）。
    Blocked,
}

impl LeaveSafety {
    /// 消费工作区入口的离开判定：`workspace_adapter` 已把 workspace_leave 的
    /// 结论翻译为 `NavigationDecision + OperationState`，这里只做结构性转换。
    pub(super) fn from_workspace_presentation(
        navigation: NavigationDecision,
        operation: &OperationState,
        target: Screen,
    ) -> Self {
        match navigation {
            NavigationDecision::Show(screen) => Self::Allowed(screen),
            NavigationDecision::Blocked => Self::Blocked,
            NavigationDecision::Stay if operation == &OperationState::NeedsConfirmation => {
                Self::NeedsConfirmation(target)
            }
            NavigationDecision::Stay => Self::Blocked,
        }
    }
}

/// 等待用户确认的离开意图（确认后按它路由，取消则停留）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingLeave {
    pub(super) target: Screen,
    pub(super) from_back: bool,
}

/// 一次返回/离开意图的结构化转移结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaveRoute {
    /// 显示目标页；`from_back` 区分历史栈返回与正向跳转；
    /// `abandon_delivery_context` 标记离开工作区时要放弃当前交付上下文。
    Navigate {
        target: Screen,
        from_back: bool,
        abandon_delivery_context: bool,
    },
    /// 需要确认：挂起确认弹窗，确认后按 `PendingLeave` 路由。
    Confirm { target: Screen, from_back: bool },
    /// 停留当前页（取消离开、边界地图处理中、栈空且无回退目标）。
    Stay,
}

/// 全局导航策略：只持有呈现层导航状态与路由规则，不持有功能入口。
#[derive(Debug, Default)]
pub(super) struct NavigationStrategy {
    /// 页面导航历史栈（“从哪儿进、从哪儿出”）：栈顶即当前页的返回目标。
    back_stack: Vec<Screen>,
}

impl NavigationStrategy {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// 正向跳转：屏幕实际切换后记录来源页（stackable 规则由本模块集中判定）。
    pub(super) fn record_forward(&mut self, before: i32, after: i32) {
        if before == after {
            return;
        }
        let (Some(from), Some(to)) = (Screen::from_index(before), Screen::from_index(after)) else {
            return;
        };
        if Self::stackable(from) && Self::stackable(to) && from != to {
            self.back_stack.push(from);
        }
    }

    /// 返回目标：栈顶即“进入当前页时的上一页”；工作区栈空时回落方案列表；
    /// 其余页面栈空时无返回目标（返回按钮隐藏且点击无动作）。
    pub(super) fn back_target(&self, current: i32) -> Option<Screen> {
        match self.back_stack.last().copied() {
            Some(target) => Some(target),
            // 工作区从零进入（如启动“工作现场恢复”）没有历史栈条目，但仍
            // 必须能返回当前校区方案列表（原“返回方案列表”按钮语义，s1_13）。
            None if Screen::from_index(current) == Some(Screen::Workspace) => {
                Some(Screen::PlanList)
            }
            None => None,
        }
    }

    /// 工具栏返回可见性：工作区总是显示（栈空时回落方案列表）；其余页面仅
    /// 在有上一页时显示（无上一页不显示返回，验收 C.13）。
    pub(super) fn back_visible(&self, current: i32) -> bool {
        !self.back_stack.is_empty() || Screen::from_index(current) == Some(Screen::Workspace)
    }

    /// 弹出栈顶返回“进入当前页时的上一页”；栈空时回落 `fallback`。
    pub(super) fn pop_or_fallback(&mut self, fallback: Screen) -> Screen {
        self.back_stack.pop().unwrap_or(fallback)
    }

    /// 启动着陆/首启完成等“从零进入”路径：清空历史栈，不显示返回按钮。
    pub(super) fn clear(&mut self) {
        self.back_stack.clear();
    }

    /// 可进入历史栈的页面（首启向导不参与；校区选择作为入口页参与）。
    pub(super) fn stackable(screen: Screen) -> bool {
        !matches!(screen, Screen::FirstRunSetup)
    }

    /// 返回/离开路由：当前屏幕 + 意图 + 工作区入口的离开安全判定 →
    /// 结构化转移结果。离开工作区（无论直接允许还是确认后）都会标记放弃
    /// 交付上下文；非工作区页面跳转不放弃任何交付上下文。
    pub(super) fn route_leave(
        &self,
        current: i32,
        from_back: bool,
        safety: LeaveSafety,
    ) -> LeaveRoute {
        match safety {
            LeaveSafety::Allowed(target) => LeaveRoute::Navigate {
                target,
                from_back,
                abandon_delivery_context: Screen::from_index(current) == Some(Screen::Workspace),
            },
            LeaveSafety::NeedsConfirmation(target) => LeaveRoute::Confirm { target, from_back },
            LeaveSafety::Blocked => LeaveRoute::Stay,
        }
    }

    /// 确认离开：离开工作区会使当前交付 generation 过期（ADR-0042 §6），
    /// 旧 worker 的结果不得交给新页面。
    pub(super) fn route_confirm(pending: PendingLeave) -> LeaveRoute {
        LeaveRoute::Navigate {
            target: pending.target,
            from_back: pending.from_back,
            abandon_delivery_context: true,
        }
    }

    /// 取消离开：留在原步骤，进行中的采集/导出继续（产品行为 3）。
    pub(super) fn route_cancel() -> LeaveRoute {
        LeaveRoute::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_leave() -> LeaveSafety {
        LeaveSafety::Allowed(Screen::Notifications)
    }

    #[test]
    fn forward_records_stackable_source_onto_history() {
        let mut nav = NavigationStrategy::new();
        // 方案列表 → 工作区 → 通知中心 → 回收站：逐层压栈。
        nav.record_forward(2, 4);
        nav.record_forward(4, 5);
        nav.record_forward(5, 6);

        assert_eq!(nav.back_target(6), Some(Screen::Notifications));
        assert!(nav.back_visible(6));
        assert_eq!(nav.pop_or_fallback(Screen::PlanList), Screen::Notifications);
        assert_eq!(nav.back_target(5), Some(Screen::Workspace));
        assert_eq!(nav.pop_or_fallback(Screen::PlanList), Screen::Workspace);
        assert_eq!(nav.back_target(4), Some(Screen::PlanList));
        assert_eq!(nav.pop_or_fallback(Screen::PlanList), Screen::PlanList);
        assert!(nav.back_stack.is_empty());
    }

    #[test]
    fn stackable_skips_first_run_and_self_navigation() {
        let mut nav = NavigationStrategy::new();
        // 首启向导不参与历史栈（完成首启进入校区选择不产生返回）。
        nav.record_forward(0, 1);
        assert!(nav.back_stack.is_empty());

        // 未发生屏幕切换 / 相同页面来回不压栈。
        nav.record_forward(2, 2);
        assert!(nav.back_stack.is_empty());
        nav.record_forward(2, 1);
        nav.record_forward(1, 2);
        assert_eq!(nav.back_target(2), Some(Screen::CampusSelect));
    }

    #[test]
    fn workspace_with_empty_stack_falls_back_to_plan_list() {
        let mut nav = NavigationStrategy::new();
        assert_eq!(nav.back_target(4), Some(Screen::PlanList));
        assert!(nav.back_visible(4), "工作区从零进入仍显示返回按钮");

        // 其余页面栈空：无返回目标、返回按钮隐藏。
        assert_eq!(nav.back_target(1), None);
        assert!(!nav.back_visible(1));
        assert_eq!(nav.back_target(2), None);
        assert!(!nav.back_visible(2));

        // 清空历史栈后工作区仍回落方案列表。
        nav.record_forward(2, 4);
        nav.clear();
        assert!(nav.back_stack.is_empty());
        assert_eq!(nav.back_target(4), Some(Screen::PlanList));
    }

    #[test]
    fn route_distinguishes_back_from_forward_and_abandon_flag() {
        let nav = NavigationStrategy::new();

        // 正向跳转离开工作区：压栈语义由 from_back=false 表达，并放弃交付上下文。
        assert_eq!(
            nav.route_leave(4, false, workspace_leave()),
            LeaveRoute::Navigate {
                target: Screen::Notifications,
                from_back: false,
                abandon_delivery_context: true,
            }
        );
        // 历史栈返回：from_back=true，非工作区不放弃交付上下文。
        assert_eq!(
            nav.route_leave(2, true, LeaveSafety::Allowed(Screen::Notifications)),
            LeaveRoute::Navigate {
                target: Screen::Notifications,
                from_back: true,
                abandon_delivery_context: false,
            }
        );
        // 工作区返回同样放弃交付上下文（离开后旧结果不得回写）。
        assert_eq!(
            nav.route_leave(4, true, LeaveSafety::Allowed(Screen::PlanList)),
            LeaveRoute::Navigate {
                target: Screen::PlanList,
                from_back: true,
                abandon_delivery_context: true,
            }
        );
    }

    #[test]
    fn route_confirm_navigates_with_abandon_and_cancel_stays() {
        let pending = PendingLeave {
            target: Screen::PlanList,
            from_back: true,
        };
        assert_eq!(
            NavigationStrategy::route_confirm(pending),
            LeaveRoute::Navigate {
                target: Screen::PlanList,
                from_back: true,
                abandon_delivery_context: true,
            }
        );
        assert_eq!(NavigationStrategy::route_cancel(), LeaveRoute::Stay);
    }

    #[test]
    fn route_needs_confirmation_or_blocked_stays_until_decided() {
        let nav = NavigationStrategy::new();

        // 运行中操作/未保存边界：先确认，不直接放弃交付上下文。
        assert_eq!(
            nav.route_leave(4, false, LeaveSafety::NeedsConfirmation(Screen::Trash)),
            LeaveRoute::Confirm {
                target: Screen::Trash,
                from_back: false,
            }
        );
        // 边界地图处理中：停留。
        assert_eq!(
            nav.route_leave(4, true, LeaveSafety::Blocked),
            LeaveRoute::Stay
        );
    }

    #[test]
    fn workspace_presentation_is_consumed_without_redeciding() {
        let target = Screen::Notifications;
        assert_eq!(
            LeaveSafety::from_workspace_presentation(
                NavigationDecision::Show(Screen::Notifications),
                &OperationState::Ready,
                target,
            ),
            LeaveSafety::Allowed(Screen::Notifications)
        );
        assert_eq!(
            LeaveSafety::from_workspace_presentation(
                NavigationDecision::Blocked,
                &OperationState::Ready,
                target,
            ),
            LeaveSafety::Blocked
        );
        assert_eq!(
            LeaveSafety::from_workspace_presentation(
                NavigationDecision::Stay,
                &OperationState::NeedsConfirmation,
                target,
            ),
            LeaveSafety::NeedsConfirmation(target)
        );
        assert_eq!(
            LeaveSafety::from_workspace_presentation(
                NavigationDecision::Stay,
                &OperationState::Ready,
                target,
            ),
            LeaveSafety::Blocked
        );
    }
}
