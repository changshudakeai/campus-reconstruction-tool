//! 历史栈返回工作区的会话恢复呈现（本工单 C.11）。
//!
//! 返回工作区复用当前内存会话（同一方案/步骤/未保存边界点），不重新打开
//! 方案；地图按当前步骤重建（①②交互地图、③④⑤让位显示，评审地图由评审
//! 入口在 Open 时装载）。跨重启恢复由“工作现场恢复”工单负责，不在本文件。

use crate::presentation::{NavigationDecision, Presentation, Screen, WorkspacePageState};
use gaode_client::BoundaryEditPageConfig;

use super::workspace_adapter::WorkspaceProductionAdapter;
use super::workspace_boundary::polygon_coordinates;

impl WorkspaceProductionAdapter {
    /// 历史栈返回工作区：复用当前内存会话（同一方案/步骤/未保存边界点），
    /// 不重新打开方案；地图按当前步骤重建。
    pub(super) fn resume(&mut self) -> Presentation<WorkspacePageState> {
        let step = self.context.session.borrow().active_step;
        self.rebuild_map_for_step(step);
        Presentation::ready(self.context.page())
            .with_navigation(NavigationDecision::Show(Screen::Workspace))
    }

    /// 按当前步骤重建工作区地图（与 [`super::workspace_adapter::WorkspaceProductionAdapter`]
    /// 的导航路径同一纪律）。
    fn rebuild_map_for_step(&mut self, step: i32) {
        let Some((keys, anchor)) = self.context.map_credentials() else {
            return;
        };
        if keys.0.is_empty() {
            self.context.session.borrow_mut().map_available = false;
            return;
        }
        match step {
            // 边界/朝向/采集/导出：边界页或朝向页 WebView 按步骤重建；
            // 已是对应页面时保持不变（幂等）。
            0 | 2 | 4 => {
                let plan_id = self
                    .context
                    .active_plan_id()
                    .unwrap_or_else(|| "__adopted_workspace__".to_owned());
                if crate::map_session::present(
                    self.context.window.clone(),
                    crate::map_session::MapDisplayIntent::Boundary {
                        plan_id,
                        api_key: keys.0,
                        security_key: keys.1,
                        anchor,
                    },
                ) {
                    self.context.session.borrow_mut().map_available = false;
                    self.context.mark_map_loading();
                }
            }
            1 => {
                let existing_boundary = self
                    .context
                    .export_flow
                    .boundary_view()
                    .as_ref()
                    .and_then(polygon_coordinates);
                let config = BoundaryEditPageConfig::new(&keys.0, &keys.1)
                    .with_anchor(anchor.0, anchor.1)
                    .with_orientation_mode(true)
                    .with_existing_boundary(existing_boundary);
                let plan_id = self
                    .context
                    .active_plan_id()
                    .unwrap_or_else(|| "__adopted_workspace__".to_owned());
                crate::map_session::present(
                    self.context.window.clone(),
                    crate::map_session::MapDisplayIntent::Orientation { plan_id, config },
                );
                self.context.session.borrow_mut().map_available = false;
                self.context.mark_map_loading();
            }
            // 步骤 ④ 评审：地图由评审入口（ReviewRequest::Open）装载。
            _ => {}
        }
    }
}
