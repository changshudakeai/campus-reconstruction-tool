//! 组合根的机械接线文件：UI 回调绑定、确认/输入弹窗调度与轮询定时器。
//!
//! 本文件与 `mod.rs`、`navigation.rs` 同属组合根模块：构造创建与全部 UI 回调
//! 绑定仍由组合根拥有，只按“机械接线”横向落盘，不按设置/方案/工作区/导航等
//! 入口拆分；运行期业务编排仍不进入 S1（ADR-0037/0039）。

use std::cell::RefCell;
use std::rc::Rc;

use gaode_client::IpcMessage;
use slint::ComponentHandle;

use crate::presentation::{
    CollectionRequest, ExportPresentationRequest, OperationState, ReviewRequest, Screen,
    SettingsRequest, TrashRequest, WorkspaceRequest,
};
use crate::AppWindow;

use super::{
    navigation::NavigationStrategy, CampusPlanRequest, PendingConfirmation, PendingInput,
    ProductionEntries,
};

impl ProductionEntries {
    /// 用户确认后执行对应的待确认操作；返回是否进入"校区搜索处理中"
    /// （调用方据此拉起搜索轮询定时器；其余确认操作返回 false）。
    pub(crate) fn confirm_pending_action(&mut self, window: &AppWindow) -> bool {
        let Some(pending) = self.pending_confirmation.take() else {
            return false;
        };
        match pending {
            PendingConfirmation::ClearGaodeKeys => {
                self.settings
                    .show(window, &self.center, SettingsRequest::ConfirmClearKeys);
            }
            PendingConfirmation::DeletePlan { plan_id } => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmDeletePlan { plan_id },
                );
            }
            PendingConfirmation::PurgePlan { trash_id } => {
                self.trash.show(
                    window,
                    &self.center,
                    TrashRequest::ConfirmPurge { trash_id },
                );
            }
            PendingConfirmation::ClearTrash => {
                self.trash
                    .show(window, &self.center, TrashRequest::ConfirmClearAll);
            }
            PendingConfirmation::GoToSettings => {
                crate::map_session::hide();
                // 从工作区边界页经确认进入设置：记录来源以便返回。
                let before = window.get_active_screen();
                self.show_settings(window);
                self.record_forward_navigation(window, before);
            }
            PendingConfirmation::DiscardBoundaryDraft { step } => {
                crate::map_session::discard_boundary_draft();
                self.handle_workspace_step_clicked(window, step);
            }
            PendingConfirmation::LeaveWorkspace { pending } => {
                // 确认离开：按导航策略路由（放弃交付上下文由 apply_leave_route 执行）。
                self.apply_leave_route(
                    window,
                    window.get_active_screen(),
                    NavigationStrategy::route_confirm(pending),
                );
            }
            PendingConfirmation::OrientationRecalc => {
                self.workspace
                    .show(window, &self.center, WorkspaceRequest::ConfirmOrientation);
            }
            PendingConfirmation::ReviewBatchReject => {
                self.review
                    .show(window, &self.center, ReviewRequest::ConfirmPending);
            }
            PendingConfirmation::ReviewApplySuggestions => {
                self.review
                    .show(window, &self.center, ReviewRequest::ConfirmSuggestionApply);
            }
            PendingConfirmation::ConfirmCampusSelection { poi_id } => {
                let before = window.get_active_screen();
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmSelectCampus { poi_id },
                );
                self.record_forward_navigation(window, before);
            }
            PendingConfirmation::RetryCampusSearch { query } => {
                window.set_campus_search_text(query.into());
                return self.request_campus_search(window);
            }
        }
        // 其余确认操作不进入校区搜索轮询
        false
    }

    pub(crate) fn cancel_pending_action(&mut self, window: &AppWindow) {
        let pending = self.pending_confirmation.take();
        if matches!(pending, Some(PendingConfirmation::LeaveWorkspace { .. })) {
            // 取消离开：导航策略给出停留路由（不导航、不放弃交付上下文）。
            self.apply_leave_route(
                window,
                window.get_active_screen(),
                NavigationStrategy::route_cancel(),
            );
        }
        if matches!(pending, Some(PendingConfirmation::ReviewBatchReject)) {
            self.review
                .show(window, &self.center, ReviewRequest::CancelPending);
            return;
        }
        if matches!(pending, Some(PendingConfirmation::ReviewApplySuggestions)) {
            self.review
                .show(window, &self.center, ReviewRequest::CancelSuggestionApply);
            return;
        }
        if matches!(
            pending,
            Some(PendingConfirmation::ConfirmCampusSelection { .. })
        ) {
            self.campus_plan
                .show(window, &self.center, CampusPlanRequest::CancelSelectCampus);
            return;
        }
        if matches!(pending, Some(PendingConfirmation::RetryCampusSearch { .. })) {
            // 取消：停留校区选择页，不创建校区、不能绕过搜索（ADR-0008 第 9 条）
            self.show_campus_select(window);
            return;
        }
        self.workspace
            .show(window, &self.center, WorkspaceRequest::CancelConfirmation);
    }

    /// 用户确认输入窗后执行对应的方案操作；返回是否消费了本次输入确认。
    pub(crate) fn confirm_pending_input(&mut self, window: &AppWindow) -> bool {
        let Some(pending) = self.pending_input.take() else {
            return false;
        };
        let name = window.get_input_dialog_text().to_string();
        match pending {
            PendingInput::CreatePlan => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmCreatePlan { name },
                );
            }
            PendingInput::RenamePlan { plan_id } => {
                self.campus_plan.show(
                    window,
                    &self.center,
                    CampusPlanRequest::ConfirmRenamePlan { plan_id, name },
                );
            }
        }
        true
    }

    pub(crate) fn cancel_pending_input(&mut self, window: &AppWindow) {
        self.pending_input = None;
        window.set_input_dialog_visible(false);
        window.set_input_dialog_text("".into());
        window.set_input_dialog_mode(0);
    }

    fn poll_export(&mut self, window: &AppWindow) -> bool {
        let presentation = self
            .export
            .show(window, &self.center, ExportPresentationRequest::Poll);
        !matches!(presentation.operation(), OperationState::Processing { .. })
    }

    fn poll_collection(&mut self, window: &AppWindow) -> bool {
        let presentation = self
            .collection
            .show(window, &self.center, CollectionRequest::Poll);
        // T36：采集失败必须弹错误对话框并给“重试”
        let failed = matches!(presentation.operation(), OperationState::Failed);
        window.set_error_dialog_retry_visible(failed);
        window.set_error_dialog_retry_label(self.collection_retry_label.clone().into());
        !matches!(presentation.operation(), OperationState::Processing { .. })
    }

    fn start_export_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().export_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let completed = entries.borrow_mut().poll_export(&window);
                if completed {
                    entries.borrow_mut().export_poll_timer.stop();
                }
            },
        );
    }

    fn start_collection_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().collection_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let completed = entries.borrow_mut().poll_collection(&window);
                if completed {
                    entries.borrow_mut().collection_poll_timer.stop();
                }
            },
        );
    }

    /// D-3：校区搜索后台轮询（20ms 采样，终态即停；失败终态同样停表，
    /// 由确认弹窗"重试/取消"接管）。
    pub(crate) fn start_campus_search_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().campus_search_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let center = entries.borrow().center.clone();
                let presentation = entries.borrow_mut().campus_plan.show(
                    &window,
                    &center,
                    CampusPlanRequest::PollSearch,
                );
                if !matches!(presentation.operation(), OperationState::Processing { .. }) {
                    entries.borrow_mut().campus_search_poll_timer.stop();
                }
            },
        );
    }

    /// T31：Rust 侧 OSM 边界自动获取后台轮询（20ms 采样，终态即停）。
    fn start_boundary_fetch_polling(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak_entries = Rc::downgrade(entries);
        let weak_window = window.as_weak();
        entries.borrow_mut().boundary_fetch_poll_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(20),
            move || {
                let Some(entries) = weak_entries.upgrade() else {
                    return;
                };
                let Some(window) = weak_window.upgrade() else {
                    return;
                };
                let center = entries.borrow().center.clone();
                let presentation = entries.borrow_mut().workspace.show(
                    &window,
                    &center,
                    WorkspaceRequest::PollBoundaryFetch,
                );
                if !matches!(presentation.operation(), OperationState::Processing { .. }) {
                    entries.borrow_mut().boundary_fetch_poll_timer.stop();
                }
            },
        );
    }

    pub(crate) fn bind_actions(entries: &Rc<RefCell<Self>>, window: &AppWindow) {
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_wizard_continue_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().complete_first_run(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_save_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().save_settings(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_save_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().save_keys(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_test_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().test_gaode_connection(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_gaode_clear_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_clear_keys(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_replay_tutorial_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().replay_tutorial(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_back_clicked(move || {
            if let Some(window) = weak.upgrade() {
                // 旧“设置返回”并入统一返回按钮：按历史栈返回。
                shared.borrow_mut().go_back(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_notice_diagnostic_action_clicked(move |notification_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .start_diagnostic_action(&window, &notification_id);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_error_dialog_diagnostic_action_clicked(move |notification_id| {
            let Some(window) = weak.upgrade() else { return };
            window.invoke_error_dialog_dismissed();
            shared
                .borrow_mut()
                .start_diagnostic_action(&window, &notification_id);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_diagnostic_actions_completed(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().finish_diagnostic_actions(&window);
        });

        // ── S1-04：校区搜索与最近记录 ────────────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_search_requested(move || {
            if let Some(window) = weak.upgrade() {
                let processing = shared.borrow_mut().request_campus_search(&window);
                if processing {
                    ProductionEntries::start_campus_search_polling(&shared, &window);
                }
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_campus_clicked(move |campus_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .select_campus(&window, campus_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_remove_recent_clicked(move |campus_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .remove_recent_campus(&window, campus_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_campus_select_settings_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .navigate_forward(&window, Screen::Settings);
            }
        });

        // ── S1-04：方案列表 CRUD ────────────────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_create_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_create_plan(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_rename_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_rename_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_duplicate_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .duplicate_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_delete_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_delete_plan(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_back_clicked(move || {
            if let Some(window) = weak.upgrade() {
                // 旧“返回校区选择”并入统一返回按钮：按历史栈返回。
                shared.borrow_mut().go_back(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_restore_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .restore_trash_item(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_purge_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .request_purge_trash_item(&window, plan_id.to_string());
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_purge_all_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().request_clear_trash(&window);
            }
        });

        // ── 输入窗（方案新建/改名，S1-04）──────────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_input_dialog_confirmed(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().confirm_pending_input(&window);
                // T34：弹窗遮挡统一机制——输入窗关闭后按当前步骤模式恢复地图
                crate::map_session::uncover_after_modal();
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_input_dialog_cancelled(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().cancel_pending_input(&window);
                // T34：弹窗遮挡统一机制——输入窗取消后按当前步骤模式恢复地图
                crate::map_session::uncover_after_modal();
            }
        });
        // ── S1-05：工作区导航、边界与朝向门控（统一经工作区功能入口）────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_plan_list_card_clicked(move |plan_id| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .open_workspace_plan_from_plan_list(&window, &plan_id);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_step_clicked(move |step| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_step_clicked(&window, step);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_back_to_plan_list_clicked(move || {
            if let Some(window) = weak.upgrade() {
                // 旧“返回方案列表”语义并入统一返回按钮：按历史栈返回。
                shared.borrow_mut().go_back(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_canvas_clicked(move |x, y| {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_boundary_canvas_click(&window, x, y);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_undo_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_undo(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_delete_selected_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_delete_selected(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_confirm_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_confirm(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_reset_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_boundary_reset(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_boundary_refresh_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let processing = shared.borrow_mut().handle_boundary_refresh(&window);
                if processing {
                    ProductionEntries::start_boundary_fetch_polling(&shared, &window);
                }
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_submit_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_orientation_submit(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_reset_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_orientation_reset(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_orientation_confirm_two_points_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_orientation_confirm_two_points(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_drawer_toggle_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().handle_workspace_drawer_toggle(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_start_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let started = shared.borrow_mut().start_collection(&window);
                if started {
                    ProductionEntries::start_collection_polling(&shared, &window);
                }
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_report_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().show_collection_report(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_collection_cancel_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().cancel_collection(&window);
            }
        });

        // T36：采集失败错误弹窗点“重试”→ 隐藏弹窗并重新发起同一开始意图
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_error_dialog_retry_clicked(move || {
            if let Some(window) = weak.upgrade() {
                window.set_error_dialog_visible(false);
                window.set_error_dialog_retry_visible(false);
                let started = shared.borrow_mut().start_collection(&window);
                crate::map_session::uncover_after_modal();
                if started {
                    ProductionEntries::start_collection_polling(&shared, &window);
                }
            }
        });

        // 鈹€鈹€ M3 璇勫椤靛洖璋冿細S1 鍙浆鍙戠敤鎴锋搷浣滅粰 F5 鍏ュ彛 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_category_clicked(move |index| {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_set_category(&window, index);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_page_prev_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_page_prev(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_page_next_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_page_next(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_state_clicked(move |candidate_id, state| {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_set_state(
                &window,
                candidate_id.to_string(),
                state.to_string(),
            );
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_highlight_clicked(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_highlight(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_locate_clicked(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_locate(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_card_selection_toggled(move |candidate_id| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_toggle_selected(&window, candidate_id.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_select_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_select_all(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_deselect_all_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_deselect_all(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_bulk_state_clicked(move |state| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_bulk_state(&window, state.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_pause_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_pause(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_resume_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_resume(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_seal_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_seal(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_suggestion_filter_clicked(move |index| {
            let Some(window) = weak.upgrade() else { return };
            shared
                .borrow_mut()
                .review_toggle_suggestion_filter(&window, index);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_apply_suggestions_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_apply_suggestions(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_review_undo_suggestions_clicked(move || {
            let Some(window) = weak.upgrade() else { return };
            shared.borrow_mut().review_undo_suggestions(&window);
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_export_start_clicked(move || {
            if let Some(window) = weak.upgrade() {
                let processing = shared.borrow_mut().start_export(&window);
                if processing {
                    ProductionEntries::start_export_polling(&shared, &window);
                }
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_tutorial_dismiss_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_tutorial_dismiss(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_tutorial_skip_all_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .handle_workspace_tutorial_skip_all(&window);
            }
        });

        // Slint 契约测试的消息注入也进入地图会话，不绕过结构化路由。
        window.on_workspace_map_ipc(move |message| {
            crate::map_session::dispatch_contract_ipc(message.to_string());
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_workspace_map_status_changed(move |available| {
            if let Some(window) = weak.upgrade() {
                crate::map_session::set_contract_available(available);
                shared.borrow_mut().handle_map_status(&window, available);
            }
        });
        // 地图会话是唯一 WebView 事件入口：代际/页面路由在此之前已收口。
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        let event_handler = Rc::new(move |event: crate::map_session::MapEvent| {
            if let Some(window) = weak.upgrade() {
                let starts_boundary_fetch = matches!(
                    &event,
                    crate::map_session::MapEvent::Workspace {
                        scene: crate::map_session::MapScene::Boundary,
                        message: IpcMessage::MapReady,
                        ..
                    }
                );
                shared.borrow_mut().handle_map_event(&window, event);
                if starts_boundary_fetch
                    && window.get_workspace_active_step() == 0
                    && !crate::map_session::has_boundary_draft()
                {
                    ProductionEntries::start_boundary_fetch_polling(&shared, &window);
                }
            }
        });
        let weak = window.as_weak();
        let availability_handler = Rc::new(
            move |_scene: crate::map_session::MapScene, available: bool| {
                if let Some(window) = weak.upgrade() {
                    window.invoke_workspace_map_status_changed(available);
                }
            },
        );
        crate::map_session::register_handlers(event_handler, availability_handler);

        // ── 右上角工具栏（S1-05：离开工作区先经功能入口判定）──────────────
        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_toolbar_back_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared.borrow_mut().go_back(&window);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_notice_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Notifications);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_switch_campus_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::CampusSelect);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_trash_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Trash);
            }
        });

        let weak = window.as_weak();
        let shared = Rc::clone(entries);
        window.on_settings_toolbar_button_clicked(move || {
            if let Some(window) = weak.upgrade() {
                shared
                    .borrow_mut()
                    .leave_workspace_then(&window, Screen::Settings);
            }
        });
    }
}
