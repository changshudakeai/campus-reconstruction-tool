//! 公开 API 快照测试（执法清单 2.5）
//!
//! 检查所有公开类型可实例化、关键行为可调用；
//! 完整 API 面记录在 tests/snapshots/public-api.txt，增删现形于 PR diff。

use std::sync::Arc;
use std::time::Duration;

use notification_center::{
    CleanupScheduler, DummyPresenter, InMemoryStorage, Notification, NotificationCenter,
    NotificationLevel, PresenterRegistry, Storage, CLEANUP_INTERVAL, MAX_RETAINED_DAYS,
    MAX_RETAINED_MESSAGES,
};

#[test]
fn public_api_types_exist() {
    // NotificationLevel：三级分派枚举 + 判别方法
    assert!(NotificationLevel::Error.is_error());
    assert!(NotificationLevel::Warn.is_warn());
    assert!(NotificationLevel::Info.is_info());
    assert_eq!(NotificationLevel::Error.as_str(), "error");

    // Notification：三个等级构造器 + 过期判断 + 复制文字
    let n = Notification::error("方案 2", "导出失败", "磁盘写入被拒绝");
    assert!(n.level.is_error());
    assert!(!n.is_expired());
    assert!(n.clipboard_text().contains("导出失败"));

    // 留存规则常量（ADR-0021：200 条或 30 天先到为准）
    assert_eq!(MAX_RETAINED_MESSAGES, 200);
    assert_eq!(MAX_RETAINED_DAYS, 30);
    assert_eq!(CLEANUP_INTERVAL, Duration::from_secs(300));

    // Storage trait + InMemoryStorage 内存表
    let storage = InMemoryStorage::new();
    storage.append(n.clone());
    assert_eq!(storage.all_notifications().len(), 1);
    assert_eq!(storage.snapshot().len(), 1);
    storage.remove_by_id(&n.id);
    assert!(storage.all_notifications().is_empty());

    // PresenterRegistry：注册/注销壳的 UI 实现
    let registry = PresenterRegistry::new();
    registry.set_presenter(Arc::new(DummyPresenter));
    registry.clear_presenter();

    // NotificationCenter 独立实例：发布 / 公告栏 / 未读数 / 清理
    let center = NotificationCenter::new(registry);
    center.publish(Notification::warn("应用", "设置已保存", "语言切换为中文"));
    assert_eq!(center.board_snapshot().len(), 1);
    assert_eq!(center.unread_count(), 1);
    center.mark_board_opened();
    assert_eq!(center.unread_count(), 0);
    center.cleanup().run_once();
    assert!(!center.cleanup().is_running());
    let _registry_ref = center.registry();

    // CleanupScheduler 可独立构造（后台线程行为见 src/cleanup.rs 单元测试）
    let scheduler = CleanupScheduler::new(Arc::new(InMemoryStorage::new()));
    scheduler.run_once();

    // 全局单例入口存在（本测试不初始化全局态，行为见 tests/popup_rule.rs）
    assert!(NotificationCenter::global().is_none());
}
