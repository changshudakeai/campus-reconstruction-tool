//! 第五步 3D 预览的入口级方法（T52）。
//!
//! 预览与导出互相独立：S1 只提交一次生成意图；预览页回传的统计/错误只留
//! 日志证据并记入会话观测，生成失败由导出入口的结构化错误负责呈现。

use crate::presentation::{ExportPresentationRequest, OperationState};
use crate::AppWindow;

use super::ProductionEntries;

impl ProductionEntries {
    /// 第五步“生成 3D 预览”：预览与导出互相独立，S1 只提交一次意图。
    pub(crate) fn start_preview(&mut self, window: &AppWindow) -> bool {
        self.supersede_diagnostic(window);
        let presentation = self.export.show(
            window,
            &self.center,
            ExportPresentationRequest::GeneratePreview,
        );
        matches!(presentation.operation(), OperationState::Processing { .. })
    }

    /// 预览页统计/错误：留日志证据并记入会话观测，不改写页面。
    pub(crate) fn handle_preview_ipc(&self, raw: &str) {
        crate::map_session::record_preview_ipc(raw);
        log::info!("map_session: 3D 预览页 IPC：{raw}");
    }
}
