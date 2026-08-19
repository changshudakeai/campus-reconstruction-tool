//! 导出呈现适配器：S1 只提交一次开始意图并呈现 A2 export-flow 返回的页面状态与通知。
//!
//! T52：第五步额外拥有“生成 3D 预览”这一独立呈现意图。预览与导出互相独立：
//! 预览生成失败只呈现错误，绝不影响导出按钮与导出流程。

use std::sync::Arc;

use export_flow::{BlockPreviewOperation, BoundaryExportFlow, Error as ExportError};
use notification_center::{Notification, NotificationActionOutcome, OpaqueNotificationAction};
use shared_domain_types::CandidateCategory;

use super::workspace_boundary::WorkspaceProductionContext;
use super::workspace_leave::WorkspaceOperation;
use crate::presentation::{
    ExportPageState, ExportPresentationRequest, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, Progress, Screen,
};

#[cfg(test)]
use super::record_entry_call;

pub(crate) struct ExportProductionAdapter {
    pub(crate) context: WorkspaceProductionContext,
    pub(crate) flow: Arc<BoundaryExportFlow>,
    pub(crate) operation: Option<export_flow::BoundaryExportOperation>,
    /// 在途 3D 预览生成操作（与导出操作互不占用）。
    pub(crate) preview_operation: Option<BlockPreviewOperation>,
    /// 第五步预览状态文案（未生成 / 生成中 / 已生成 / 失败）。
    pub(crate) preview_status: String,
    /// 是否已有可交互的预览内容（复位/缩放按钮可用）。
    pub(crate) preview_has_content: bool,
    /// 是否正在生成预览（生成按钮禁用）。
    pub(crate) preview_generating: bool,
}

impl ExportProductionAdapter {
    fn page_with_status(&self, title_key: &str, subtitle: impl Into<String>) -> ExportPageState {
        let injector = self.context.injector();
        let l10n = injector.borrow();
        let mut workspace = self.context.page();
        workspace.placeholder_title = l10n.l10n().t(title_key);
        workspace.placeholder_subtitle = subtitle.into();
        ExportPageState {
            workspace,
            preview_generate_label: l10n.l10n().t("export.preview_generate_button"),
            preview_status: self.preview_status.clone(),
            preview_reset_label: l10n.l10n().t("export.preview_reset_button"),
            preview_zoom_in_label: l10n.l10n().t("export.preview_zoom_in_button"),
            preview_zoom_out_label: l10n.l10n().t("export.preview_zoom_out_button"),
            preview_controls_hint: l10n.l10n().t("export.preview_controls_hint"),
            preview_has_content: self.preview_has_content,
            preview_generating: self.preview_generating,
        }
    }

    /// 导出页副标题：存在已封账保留候选时显示增强导出内容提示，否则显示
    /// 边界直出说明（S1 只呈现 A2 返回的结构化计数，不判断业务条件）。
    fn export_subtitle(&self) -> String {
        let injector = self.context.injector();
        let l10n = injector.borrow();
        let hint = self.flow.enhanced_hint().ok().flatten();
        if let Some(hint) = hint {
            let categories = hint
                .keep_by_category
                .iter()
                .map(|(category, count)| {
                    format!(
                        "{} {count}",
                        l10n.l10n().t(export_category_label_key(*category))
                    )
                })
                .collect::<Vec<_>>()
                .join("、");
            l10n.l10n().t_with_array(
                "export.enhanced_summary",
                &[
                    &hint.keep_total.to_string(),
                    &categories,
                    &hint.pending_count.to_string(),
                    &hint.remove_count.to_string(),
                ],
            )
        } else {
            l10n.l10n().t("export.boundary_only_summary")
        }
    }

    fn failure_presentation(&self, error: &ExportError) -> Presentation<ExportPageState> {
        let (body, action) = {
            let injector = self.context.injector();
            let l10n = injector.borrow();
            let category = l10n.l10n().t(export_error_category_key(error));
            let body = l10n
                .l10n()
                .t_with_array("export.failure_user_message", &[&category]);
            let diagnostic_source = l10n.l10n().t("app.source_tag");
            let diagnostic_title = l10n.l10n().t("notice.diagnostic_action");
            let diagnostic_detail = export_diagnostic_detail(error);
            let action = OpaqueNotificationAction::new(move || {
                NotificationActionOutcome::succeeded(Notification::info(
                    diagnostic_source.clone(),
                    diagnostic_title.clone(),
                    diagnostic_detail.clone(),
                ))
            });
            (body, action)
        };
        let injector = self.context.injector();
        let l10n = injector.borrow();
        let notification = NotificationFact::new(Notification::error(
            l10n.l10n().t("app.source_tag"),
            l10n.l10n().t("dialog.error_title"),
            body.clone(),
        ))
        .with_diagnostic_action(action);
        Presentation::failed(self.page_with_status("error.export_failed", body))
            .with_notification(notification)
    }

    /// 预览失败呈现：明确错误弹窗 + 抽屉状态，但不影响导出。
    fn preview_failure_presentation(
        &mut self,
        error: &ExportError,
    ) -> Presentation<ExportPageState> {
        self.preview_generating = false;
        let (body, notification) = {
            let injector = self.context.injector();
            let l10n = injector.borrow();
            let category = l10n.l10n().t(export_error_category_key(error));
            let body = l10n
                .l10n()
                .t_with_array("export.preview_failed_body", &[&category]);
            let diagnostic_source = l10n.l10n().t("app.source_tag");
            let diagnostic_title = l10n.l10n().t("notice.diagnostic_action");
            let diagnostic_detail = export_diagnostic_detail(error);
            let action = OpaqueNotificationAction::new(move || {
                NotificationActionOutcome::succeeded(Notification::info(
                    diagnostic_source.clone(),
                    diagnostic_title.clone(),
                    diagnostic_detail.clone(),
                ))
            });
            let notification = NotificationFact::new(Notification::error(
                l10n.l10n().t("app.source_tag"),
                l10n.l10n().t("export.preview_failed_title"),
                body.clone(),
            ))
            .with_diagnostic_action(action);
            (body, notification)
        };
        self.preview_status = body;
        Presentation::failed(self.page_with_status("export.confirm_title", self.export_subtitle()))
            .with_notification(notification)
    }
}

impl PresentationAdapter<ExportPresentationRequest, ExportPageState> for ExportProductionAdapter {
    fn present(&mut self, request: ExportPresentationRequest) -> Presentation<ExportPageState> {
        #[cfg(test)]
        record_entry_call(6);
        match request {
            ExportPresentationRequest::Open => {
                let mut presentation = if let Some(operation) = &self.preview_operation {
                    let progress = operation.progress_view();
                    Presentation::processing(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                } else {
                    Presentation::ready(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                    )
                };
                presentation =
                    presentation.with_navigation(NavigationDecision::Show(Screen::Workspace));
                presentation
            }
            ExportPresentationRequest::Start => match self.flow.start() {
                Ok(operation) => {
                    self.context.operation_started(WorkspaceOperation::Export);
                    let progress = operation.progress_view();
                    self.operation = Some(operation);
                    Presentation::processing(
                        self.page_with_status(progress.stage_key, self.export_subtitle()),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                }
                Err(error) => self.failure_presentation(&error),
            }
            .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            ExportPresentationRequest::GeneratePreview => {
                if let Some(operation) = &self.preview_operation {
                    let progress = operation.progress_view();
                    return Presentation::processing(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
                }
                match self.flow.start_preview() {
                    Ok(operation) => {
                        let progress = operation.progress_view();
                        self.preview_operation = Some(operation);
                        self.preview_generating = true;
                        {
                            let injector = self.context.injector();
                            let l10n = injector.borrow();
                            self.preview_status = l10n.l10n().t("export.preview_generating");
                        }
                        Presentation::processing(
                            self.page_with_status("export.confirm_title", self.export_subtitle()),
                            Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                        )
                    }
                    Err(error) => self.preview_failure_presentation(&error),
                }
                .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ExportPresentationRequest::PreviewPoll => {
                let Some(operation) = self.preview_operation.as_mut() else {
                    return Presentation::ready(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                if let Some(result) = operation.try_complete() {
                    self.preview_operation = None;
                    self.preview_generating = false;
                    match result {
                        Ok(payload) => {
                            self.preview_has_content = true;
                            crate::map_session::remember_preview_payload(payload.json.clone());
                            let mut presentation = {
                                let injector = self.context.injector();
                                let l10n = injector.borrow();
                                self.preview_status = l10n.l10n().t_with_array(
                                    "export.preview_ready",
                                    &[&payload.block_count.to_string()],
                                );
                                Presentation::succeeded(self.page_with_status(
                                    "export.confirm_title",
                                    self.export_subtitle(),
                                ))
                            };
                            if payload.simplified {
                                let injector = self.context.injector();
                                let l10n = injector.borrow();
                                presentation = presentation.with_notification(
                                    NotificationFact::new(Notification::warn(
                                        l10n.l10n().t("app.source_tag"),
                                        l10n.l10n().t("export.preview_simplified_title"),
                                        l10n.l10n().t("export.preview_simplified_notice"),
                                    )),
                                );
                            }
                            presentation
                        }
                        Err(error) => self.preview_failure_presentation(&error),
                    }
                } else {
                    let progress = operation.progress_view();
                    Presentation::processing(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                }
                .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ExportPresentationRequest::Poll => {
                let Some(operation) = self.operation.as_mut() else {
                    return Presentation::ready(
                        self.page_with_status("export.confirm_title", self.export_subtitle()),
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace));
                };
                if let Some(result) = operation.try_complete() {
                    self.operation = None;
                    self.context.operation_finished(WorkspaceOperation::Export);
                    match result {
                        Ok(result) => {
                            let injector = self.context.injector();
                            let l10n = injector.borrow();
                            let dimensions = result.schematic_dimensions;
                            let subtitle = l10n.l10n().t_with_array(
                                "export.done_with_dimensions",
                                &[
                                    &result.schematic_path.display().to_string(),
                                    &dimensions[0].to_string(),
                                    &dimensions[1].to_string(),
                                    &dimensions[2].to_string(),
                                ],
                            );
                            let mut presentation = Presentation::succeeded(
                                self.page_with_status("export.done", subtitle),
                            );
                            if let Some(detail) = result.cleanup_warning {
                                let source = l10n.l10n().t("app.source_tag");
                                let title = l10n.l10n().t("export.done");
                                let warning_body = l10n.l10n().t("export.cleanup_warning");
                                let diagnostic_title = l10n.l10n().t("notice.diagnostic_action");
                                let diagnostic_source = source.clone();
                                let action = OpaqueNotificationAction::new(move || {
                                    NotificationActionOutcome::succeeded(Notification::info(
                                        diagnostic_source.clone(),
                                        diagnostic_title.clone(),
                                        detail.clone(),
                                    ))
                                });
                                presentation = presentation.with_notification(
                                    NotificationFact::new(Notification::warn(
                                        source,
                                        title,
                                        warning_body,
                                    ))
                                    .with_diagnostic_action(action),
                                );
                            }
                            presentation
                        }
                        Err(error) => self.failure_presentation(&error),
                    }
                } else {
                    let progress = operation.progress_view();
                    Presentation::processing(
                        self.page_with_status(progress.stage_key, self.export_subtitle()),
                        Progress::try_from(progress.percent as u8).unwrap_or(Progress::ZERO),
                    )
                }
                .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            ExportPresentationRequest::Abandon => {
                self.flow.leave();
                self.operation = None;
                self.preview_operation = None;
                self.preview_generating = false;
                self.context.operation_finished(WorkspaceOperation::Export);
                Presentation::ready(
                    self.page_with_status("export.confirm_title", self.export_subtitle()),
                )
            }
        }
    }
}

fn export_diagnostic_detail(error: &ExportError) -> String {
    error.to_string()
}

fn export_error_category_key(error: &ExportError) -> &'static str {
    match error {
        ExportError::Boundary(_) => "error.export_boundary_failed",
        ExportError::SettingsRead(_) => "error.export_settings_failed",
        ExportError::Version(_) => "error.export_version_failed",
        ExportError::Generation(_) => "error.export_generation_failed",
        ExportError::ManifestWrite(_) => "error.export_manifest_write_failed",
        ExportError::SchematicWrite(_) => "error.export_schematic_write_failed",
        ExportError::ArtifactWrite(_) => "error.export_artifact_write_failed",
        ExportError::ArtifactRecovery(_) => "error.export_recovery_failed",
        ExportError::BackgroundTask => "error.export_background_failed",
        ExportError::Preview(_) => "error.export_preview_failed",
        _ => "error.export_failed",
    }
}

/// 类别 → 显示名文本键（collection 命名空间既有键，与 F5 同源）。
fn export_category_label_key(category: CandidateCategory) -> &'static str {
    match category {
        CandidateCategory::Building => "collection.category_building",
        CandidateCategory::Road => "collection.category_road",
        CandidateCategory::Water => "collection.category_water",
        CandidateCategory::Vegetation => "collection.category_vegetation",
        CandidateCategory::Sports => "collection.category_sports",
        CandidateCategory::Other => "collection.category_other",
        _ => "collection.category_other",
    }
}
