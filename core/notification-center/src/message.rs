//! 通知数据结构
//!
//! 单条通知的完整信息：等级、标题、正文、来源标签、创建时间。
//! 公告栏只读可复制（ADR-0021）：[`Notification::clipboard_text`]
//! 是复制按钮取文字的唯一出口。

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::level::NotificationLevel;
use crate::storage::MAX_RETAINED_DAYS;

/// 功能模块执行不透明故障操作后给出的明确结果。
#[derive(Debug, Clone)]
pub enum NotificationActionOutcome {
    /// 用例成功，并给出交给 B7 的完成通知。
    Succeeded(Notification),
    /// 用例失败，并给出交给 B7 的错误通知。
    Failed(Notification),
}

impl NotificationActionOutcome {
    /// 建立成功结果。
    pub fn succeeded(notification: Notification) -> Self {
        Self::Succeeded(notification)
    }

    /// 建立失败结果。
    pub fn failed(notification: Notification) -> Self {
        Self::Failed(notification)
    }

    /// 是否由功能模块明确报告失败。
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// 取出交给 B7 的原始结果通知。
    pub fn into_notification(self) -> Notification {
        match self {
            Self::Succeeded(notification) | Self::Failed(notification) => notification,
        }
    }
}

impl From<Notification> for NotificationActionOutcome {
    fn from(notification: Notification) -> Self {
        Self::Succeeded(notification)
    }
}

/// 由产生故障的功能模块拥有、其他模块只能保存和触发的不透明操作。
#[derive(Clone)]
pub struct OpaqueNotificationAction {
    executor: Arc<dyn Fn() -> NotificationActionOutcome + Send + Sync>,
}

impl OpaqueNotificationAction {
    /// 把功能模块拥有的完整故障资料用例封装为不透明操作。
    pub fn new<Outcome>(executor: impl Fn() -> Outcome + Send + Sync + 'static) -> Self
    where
        Outcome: Into<NotificationActionOutcome>,
    {
        Self {
            executor: Arc::new(move || executor().into()),
        }
    }

    /// 将操作原样交回创建它的功能模块执行。
    pub fn invoke(&self) -> NotificationActionOutcome {
        (self.executor)()
    }
}

impl fmt::Debug for OpaqueNotificationAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueNotificationAction(..)")
    }
}

/// 单条通知 —— 一次提醒的完整记录（只读，入账后不再修改）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// 唯一标识（UUID v4）。
    pub id: Uuid,
    /// 消息等级（决定呈现方式：弹窗 / toast / 仅留底）。
    pub level: NotificationLevel,
    /// 标题（调用方以文本键解析后的成品文字）。
    pub title: String,
    /// 正文（详细说明，可多行）。
    pub body: String,
    /// 来源标签（如"方案 2：导出失败"、"应用：设置已保存"），跨方案不混淆。
    pub source_tag: String,
    /// 创建时间（UTC 时间戳，展示时由壳转本地时区）。
    pub created_at: DateTime<Utc>,
}

/// B7 留存的一条通知及其可选不透明操作。
#[derive(Debug, Clone)]
pub struct NotificationRecord {
    notification: Notification,
    diagnostic_action: Option<OpaqueNotificationAction>,
}

impl NotificationRecord {
    /// 建立一条没有故障资料操作的通知记录。
    pub fn new(notification: Notification) -> Self {
        Self {
            notification,
            diagnostic_action: None,
        }
    }

    /// 附加功能模块拥有的不透明故障资料操作。
    pub fn with_diagnostic_action(mut self, action: OpaqueNotificationAction) -> Self {
        self.diagnostic_action = Some(action);
        self
    }

    /// 用户通知内容。
    pub fn notification(&self) -> &Notification {
        &self.notification
    }

    /// 是否提供故障资料操作。
    pub fn has_diagnostic_action(&self) -> bool {
        self.diagnostic_action.is_some()
    }

    /// 不透明故障资料操作。
    pub fn diagnostic_action(&self) -> Option<&OpaqueNotificationAction> {
        self.diagnostic_action.as_ref()
    }
}

impl Notification {
    /// 创建一条 Error 级通知（要紧错误，模态弹窗）。
    pub fn error(
        source_tag: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self::new(NotificationLevel::Error, source_tag, title, body)
    }

    /// 创建一条 Warn 级通知（普通提示，toast）。
    pub fn warn(
        source_tag: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self::new(NotificationLevel::Warn, source_tag, title, body)
    }

    /// 创建一条 Info 级通知（小毛病，仅留底）。
    pub fn info(
        source_tag: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self::new(NotificationLevel::Info, source_tag, title, body)
    }

    /// 统一构造入口：生成 ID 与创建时间。
    fn new(
        level: NotificationLevel,
        source_tag: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            level,
            title: title.into(),
            body: body.into(),
            source_tag: source_tag.into(),
            created_at: Utc::now(),
        }
    }

    /// 是否已超过留存天数（30 天，自动清理判据）。
    pub fn is_expired(&self) -> bool {
        Utc::now().signed_duration_since(self.created_at).num_days() > MAX_RETAINED_DAYS
    }

    /// 公告栏"复制"按钮取的整段文字（来源标签 + 时间 + 标题 + 正文）。
    pub fn clipboard_text(&self) -> String {
        format!(
            "[{}] {} — {}\n{}",
            self.source_tag,
            self.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            self.title,
            self.body
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_matching_level() {
        assert!(Notification::error("test", "标题", "内容").level.is_error());
        assert!(Notification::warn("test", "标题", "内容").level.is_warn());
        assert!(Notification::info("test", "标题", "内容").level.is_info());
    }

    #[test]
    fn fields_are_preserved() {
        let n = Notification::error("方案 2：导出失败", "导出失败", "磁盘写入被拒绝");
        assert_eq!(n.source_tag, "方案 2：导出失败");
        assert_eq!(n.title, "导出失败");
        assert_eq!(n.body, "磁盘写入被拒绝");
    }

    #[test]
    fn message_older_than_30_days_is_expired() {
        let mut n = Notification::info("test", "旧消息", "该清理了");
        n.created_at = Utc::now() - chrono::Duration::days(MAX_RETAINED_DAYS + 1);
        assert!(n.is_expired());
    }

    #[test]
    fn fresh_message_is_not_expired() {
        let n = Notification::info("test", "新消息", "今天发的");
        assert!(!n.is_expired());
    }

    #[test]
    fn clipboard_text_contains_all_parts() {
        let n = Notification::warn("应用：设置", "设置已保存", "语言切换为中文");
        let text = n.clipboard_text();
        assert!(text.contains("应用：设置"));
        assert!(text.contains("设置已保存"));
        assert!(text.contains("语言切换为中文"));
    }

    #[test]
    fn serde_roundtrip_preserves_message() {
        let n = Notification::error("test", "标题", "内容");
        let json = serde_json::to_string(&n).expect("序列化成功");
        let back: Notification = serde_json::from_str(&json).expect("反序列化成功");
        assert_eq!(n, back);
    }
}
