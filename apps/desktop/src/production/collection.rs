//! 采集呈现适配器：S1 只转发一次完整意图，并呈现 A1 返回的页面状态与通知。

use std::sync::Arc;

use super::workspace_boundary::WorkspaceProductionContext;
use crate::presentation::{
    CollectionPageState, CollectionRequest, NavigationDecision, NotificationFact, Presentation,
    PresentationAdapter, Progress, Screen,
};

#[cfg(test)]
use super::record_entry_call;

pub(super) const COLLECTION_CATEGORY_KEYS: [&str; 6] = [
    "collection.category_building",
    "collection.category_road",
    "collection.category_water",
    "collection.category_vegetation",
    "collection.category_sports",
    "collection.category_other",
];

/// 采集呈现适配器：S1 只转发一次完整意图，并呈现 A1 返回的页面状态与通知。
pub(crate) struct CollectionProductionAdapter {
    pub(crate) context: WorkspaceProductionContext,
    pub(crate) flow: Arc<collection_flow::CollectionFlow>,
    pub(crate) operation: Option<collection_flow::CollectionOperation>,
}

impl CollectionProductionAdapter {
    fn page_state(&self, view: &collection_flow::CollectionPageView) -> CollectionPageState {
        let injector = self.context.injector();
        let injector = injector.borrow();
        let l10n = injector.l10n();
        let statuses = match view.status {
            collection_flow::CollectionStatus::Pending
            | collection_flow::CollectionStatus::Failed => vec![l10n.t("common.pending"); 6],
            collection_flow::CollectionStatus::Fetching => {
                vec![l10n.t("collection.progress_fetching"); 6]
            }
            collection_flow::CollectionStatus::Completed => view
                .progress
                .categories
                .iter()
                .map(|category| category.collected.to_string())
                .collect(),
        };
        let progress_label = match view.status {
            collection_flow::CollectionStatus::Pending
            | collection_flow::CollectionStatus::Failed => l10n.t("collection.progress_title"),
            collection_flow::CollectionStatus::Fetching => l10n.t("collection.progress_fetching"),
            collection_flow::CollectionStatus::Completed => l10n.t_with_args(
                "collection.progress_done",
                serde_json::json!({ "count": view.progress.collected_total }),
            ),
        };
        let report_body = view.report.as_ref().map_or_else(String::new, |report| {
            let mut lines = report.category_lines.clone();
            lines.extend(report.candidate_lines.clone());
            lines.extend(report.issue_lines.clone());
            if let Some(no_issues) = &report.no_issues_line {
                lines.push(no_issues.clone());
            }
            lines.join("\n")
        });
        let category_labels = COLLECTION_CATEGORY_KEYS
            .iter()
            .map(|key| l10n.t(key))
            .collect();
        // T31：候选采集源已切 OSM/Overpass（高德点位不再作为候选几何来源）
        let source_label = l10n.t("collection.source_osm");
        let collect_label = l10n.t("collection.collect_button");
        let category_skip_label = l10n.t("collection.skippable");
        let report_entry_label = l10n.t("audit.report_entry");
        drop(injector);
        CollectionPageState {
            workspace: self.context.page(),
            source_label,
            collect_label,
            progress_label,
            category_labels,
            category_statuses: statuses,
            category_skip_label,
            diff_summary: view.diff_summary.clone().unwrap_or_default(),
            report_entry_label,
            report_body,
        }
    }

    /// Start 同步错误（无后台操作）与后台失败共用的失败呈现：A1 已汇总
    /// 页面状态与通知事实，S1 只转发给 B7 并绘制。
    fn start_failure(
        &self,
        error: &collection_flow::CollectionError,
    ) -> Presentation<CollectionPageState> {
        let failure = self.flow.failure_view(error);
        let mut presentation = Presentation::failed(self.page_state(&failure.page));
        if let Some(notification) = failure.notification {
            presentation = presentation.with_notification(NotificationFact::new(notification));
        }
        presentation
    }
}

impl PresentationAdapter<CollectionRequest, CollectionPageState> for CollectionProductionAdapter {
    fn present(&mut self, request: CollectionRequest) -> Presentation<CollectionPageState> {
        #[cfg(test)]
        record_entry_call(3);
        match request {
            CollectionRequest::Open => Presentation::ready(self.page_state(&self.flow.page_view()))
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            CollectionRequest::Start => match self.flow.start() {
                Ok(operation) => {
                    self.operation = Some(operation);
                    Presentation::processing(
                        self.page_state(&self.flow.page_view()),
                        Progress::ZERO,
                    )
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
                }
                Err(error) => self
                    .start_failure(&error)
                    .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            },
            CollectionRequest::Poll => self.poll(),
            CollectionRequest::ShowReport => {
                let mut view = self.flow.page_view();
                if let Some(report) = self.flow.report_view() {
                    view.report = Some(report);
                }
                Presentation::ready(self.page_state(&view))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            CollectionRequest::Abandon => {
                self.flow.leave();
                self.operation = None;
                Presentation::ready(self.page_state(&self.flow.page_view()))
            }
        }
    }
}

impl CollectionProductionAdapter {
    fn poll(&mut self) -> Presentation<CollectionPageState> {
        let Some(mut operation) = self.operation.take() else {
            return Presentation::ready(self.page_state(&self.flow.page_view()))
                .with_navigation(NavigationDecision::Show(Screen::Workspace));
        };
        match operation.try_complete() {
            Some(Ok(collection_flow::CollectionOutcome::Succeeded(summary))) => {
                let mut presentation = Presentation::succeeded(self.page_state(&summary.page));
                if let Some(notification) = summary.notification {
                    presentation =
                        presentation.with_notification(NotificationFact::new(notification));
                }
                presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Ok(collection_flow::CollectionOutcome::Failed(failure))) => {
                let mut presentation = Presentation::failed(self.page_state(&failure.page));
                if let Some(notification) = failure.notification {
                    presentation =
                        presentation.with_notification(NotificationFact::new(notification));
                }
                presentation.with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Err(collection_flow::CollectionError::Expired)) => {
                // 取消/切换方案后旧结果不得拉回：回到当前页面状态。
                Presentation::ready(self.page_state(&self.flow.page_view()))
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
            Some(Err(error)) => self
                .start_failure(&error)
                .with_navigation(NavigationDecision::Show(Screen::Workspace)),
            None => {
                self.operation = Some(operation);
                Presentation::processing(self.page_state(&self.flow.page_view()), Progress::ZERO)
                    .with_navigation(NavigationDecision::Show(Screen::Workspace))
            }
        }
    }
}
