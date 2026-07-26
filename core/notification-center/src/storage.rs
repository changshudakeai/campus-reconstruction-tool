//! 存储层抽象 + 内存表实现
//!
//! 暂不写 SQLite（T11 的地盘），v2.0.0 先用内存表，但保留 Storage trait
//! 接口方便 T11 完成后无缝迁移。
//!
//! 留存规则（ADR-0021）：最近 200 条或 30 天（先到为准）。

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use chrono::Utc;

use crate::message::Notification;

/// 公告栏最多留存的消息条数（ADR-0021 留存规则）。
pub const MAX_RETAINED_MESSAGES: usize = 200;

/// 消息最长留存天数（ADR-0021 留存规则）。
pub const MAX_RETAINED_DAYS: i64 = 30;

/// 存储 trait —— B7 与具体持久化方案之间的接缝。
///
/// 未来 T11（B2 数据持久化）就绪后，SQLite 实现只需实现此 trait 即可替换内存表。
pub trait Storage: Send + Sync {
    /// 添加一条新消息。
    fn append(&self, notification: Notification);

    /// 获取全部消息（倒序：最新的在前），不应用留存规则。
    fn all_notifications(&self) -> Vec<Notification>;

    /// 获取应用留存规则（200 条 / 30 天先到为准）后的消息，按时间倒序。
    fn snapshot(&self) -> Vec<Notification>;

    /// 删除指定 ID 的消息（仅供清理调度器使用；公告栏对用户只读）。
    fn remove_by_id(&self, id: &uuid::Uuid);
}

/// 内存表实现：`Arc<RwLock<VecDeque>>`，克隆即共享同一份数据。
#[derive(Debug, Default, Clone)]
pub struct InMemoryStorage {
    entries: Arc<RwLock<VecDeque<Notification>>>,
}

impl InMemoryStorage {
    /// 创建一个新的空内存表。
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for InMemoryStorage {
    fn append(&self, notification: Notification) {
        let mut store = self.entries.write().expect("通知内存表锁不可中毒");
        store.push_back(notification);
    }

    fn all_notifications(&self) -> Vec<Notification> {
        let store = self.entries.read().expect("通知内存表锁不可中毒");
        store.iter().rev().cloned().collect()
    }

    fn snapshot(&self) -> Vec<Notification> {
        let now = Utc::now();
        let mut valid: Vec<Notification> = self
            .entries
            .read()
            .expect("通知内存表锁不可中毒")
            .iter()
            .filter(|n| now.signed_duration_since(n.created_at).num_days() <= MAX_RETAINED_DAYS)
            .cloned()
            .collect();

        // 按时间倒序排序，最新的在前
        valid.sort_by_key(|n| std::cmp::Reverse(n.created_at));

        // 应用 200 条上限
        valid.truncate(MAX_RETAINED_MESSAGES);
        valid
    }

    fn remove_by_id(&self, id: &uuid::Uuid) {
        let mut store = self.entries.write().expect("通知内存表锁不可中毒");
        store.retain(|n| n.id != *id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::NotificationLevel;

    #[test]
    fn append_and_snapshot_newest_first() {
        let storage = InMemoryStorage::new();
        storage.append(Notification::info("test", "消息 1", "内容"));
        storage.append(Notification::warn("test", "消息 2", "内容"));

        let snapshot = storage.snapshot();
        assert_eq!(snapshot.len(), 2);
        // 最新在前
        assert_eq!(snapshot[0].title, "消息 2");
    }

    #[test]
    fn snapshot_drops_messages_older_than_30_days() {
        let storage = InMemoryStorage::new();

        // 一条超过 30 天的旧消息
        let old_msg = Notification {
            id: uuid::Uuid::new_v4(),
            level: NotificationLevel::Info,
            title: "旧消息".into(),
            body: "应该被清理".into(),
            source_tag: "test".into(),
            created_at: Utc::now() - chrono::Duration::days(31),
        };
        storage.append(old_msg);
        storage.append(Notification::info("test", "新消息", "内容"));

        let snapshot = storage.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].title, "新消息");
    }

    #[test]
    fn snapshot_caps_at_200_messages() {
        let storage = InMemoryStorage::new();
        for i in 0..250 {
            storage.append(Notification::info("test", format!("消息 {i}"), "内容"));
        }

        let snapshot = storage.snapshot();
        assert_eq!(snapshot.len(), MAX_RETAINED_MESSAGES);
        // 最新的那条应该在第一个
        assert_eq!(snapshot[0].title, "消息 249");
    }

    #[test]
    fn clone_shares_the_same_entries() {
        let storage = InMemoryStorage::new();
        let alias = storage.clone();
        storage.append(Notification::info("test", "消息", "内容"));
        assert_eq!(alias.all_notifications().len(), 1);
    }

    #[test]
    fn remove_by_id_deletes_only_that_message() {
        let storage = InMemoryStorage::new();
        let keep = Notification::info("test", "留下", "内容");
        let drop = Notification::info("test", "删掉", "内容");
        storage.append(keep.clone());
        storage.append(drop.clone());

        storage.remove_by_id(&drop.id);
        let rest = storage.all_notifications();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].id, keep.id);
    }
}
