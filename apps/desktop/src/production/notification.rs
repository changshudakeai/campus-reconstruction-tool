//! 通知中心呈现适配器：B7 通知记录转页面状态；故障操作状态与 B11 分离呈现。

use std::sync::Arc;

use localization::Localization;
use notification_center::NotificationCenter;

use super::toolbar;
use crate::presentation::{
    NavigationDecision, NotificationPageState, Presentation, PresentationAdapter, Progress, Screen,
    ToolbarPageState,
};
use crate::NoticeData;

#[cfg(test)]
use super::record_entry_call;

#[derive(Clone)]
pub(super) struct NotificationLabels {
    toolbar: ToolbarPageState,
    title: String,
    empty_list_text: String,
    archive_label: String,
    date_today: String,
    date_yesterday: String,
    importance_high_label: String,
    unread_marker: String,
    diagnostic_action_label: String,
}

impl NotificationLabels {
    pub(super) fn new(l10n: &Localization) -> Self {
        Self {
            toolbar: toolbar(l10n, true),
            title: l10n.t("notice.page_title"),
            empty_list_text: l10n.t("notice.empty_list"),
            archive_label: l10n.t("notice.archive_button"),
            date_today: l10n.t("notice.date_today"),
            date_yesterday: l10n.t("notice.date_yesterday"),
            importance_high_label: l10n.t("notice.importance_high"),
            unread_marker: l10n.t("notice.unread_marker"),
            diagnostic_action_label: l10n.t("notice.diagnostic_action"),
        }
    }
}

pub(super) struct DiagnosticFailureLabels {
    pub(super) source: String,
    pub(super) title: String,
    pub(super) panicked_body: String,
}

#[derive(Clone)]
pub(super) enum NotificationRequest {
    Open,
    Processing,
    Succeeded,
    Failed,
}

pub(crate) struct NotificationProductionAdapter {
    pub(crate) center: Arc<NotificationCenter>,
    pub(crate) labels: NotificationLabels,
}

impl PresentationAdapter<NotificationRequest, NotificationPageState>
    for NotificationProductionAdapter
{
    fn present(&mut self, request: NotificationRequest) -> Presentation<NotificationPageState> {
        #[cfg(test)]
        record_entry_call(7);
        if matches!(&request, NotificationRequest::Open) {
            self.center.mark_board_opened();
        }
        let notices = self
            .center
            .board_records()
            .into_iter()
            .map(|record| {
                let notification = record.notification();
                NoticeData {
                    id: notification.id.to_string().into(),
                    title: notification.title.clone().into(),
                    body: notification.body.clone().into(),
                    date: self.labels.date_today.clone().into(),
                    importance: if notification.level.is_error() {
                        "high".into()
                    } else {
                        "normal".into()
                    },
                    read: false,
                    has_diagnostic_action: record.has_diagnostic_action(),
                }
            })
            .collect();
        let page = NotificationPageState {
            toolbar: self.labels.toolbar.clone(),
            title: self.labels.title.clone(),
            empty_list_text: self.labels.empty_list_text.clone(),
            archive_label: self.labels.archive_label.clone(),
            date_today: self.labels.date_today.clone(),
            date_yesterday: self.labels.date_yesterday.clone(),
            importance_high_label: self.labels.importance_high_label.clone(),
            unread_marker: self.labels.unread_marker.clone(),
            diagnostic_action_label: self.labels.diagnostic_action_label.clone(),
            notices,
        };
        match request {
            NotificationRequest::Open => Presentation::ready(page)
                .with_navigation(NavigationDecision::Show(Screen::Notifications)),
            NotificationRequest::Processing => Presentation::processing(page, Progress::ZERO),
            NotificationRequest::Succeeded => Presentation::succeeded(page),
            NotificationRequest::Failed => Presentation::failed(page),
        }
    }
}
