//! ADR-0031：不透明故障操作与 B7 通知记录共用同一留存生命周期。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notification_center::{
    Notification, NotificationCenter, OpaqueNotificationAction, PresenterRegistry,
    MAX_RETAINED_DAYS, MAX_RETAINED_MESSAGES,
};

struct DropProbe(Arc<AtomicBool>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn cleanup_releases_expired_notification_action_without_board_query() {
    let center = NotificationCenter::new(PresenterRegistry::new());
    let released = Arc::new(AtomicBool::new(false));
    let probe = DropProbe(Arc::clone(&released));
    let mut notification = Notification::error("test", "过期", "带操作");
    notification.created_at = chrono::Utc::now() - chrono::Duration::days(MAX_RETAINED_DAYS + 1);
    center.publish_with_action(
        notification,
        OpaqueNotificationAction::new(move || {
            let _keep_capture_alive = &probe;
            Notification::info("test", "完成", "内容")
        }),
    );

    center.cleanup().run_once();

    assert!(
        released.load(Ordering::SeqCst),
        "正式清理入口必须同时释放过期通知捕获的操作"
    );
}

#[test]
fn cleanup_releases_action_evicted_by_capacity_without_board_query() {
    let center = NotificationCenter::new(PresenterRegistry::new());
    let released = Arc::new(AtomicBool::new(false));
    let probe = DropProbe(Arc::clone(&released));
    center.publish_with_action(
        Notification::error("test", "最旧通知", "带操作"),
        OpaqueNotificationAction::new(move || {
            let _keep_capture_alive = &probe;
            Notification::info("test", "完成", "内容")
        }),
    );
    for index in 0..MAX_RETAINED_MESSAGES {
        center.publish(Notification::info(
            "test",
            format!("更新通知 {index}"),
            "内容",
        ));
    }

    center.cleanup().run_once();

    assert!(
        released.load(Ordering::SeqCst),
        "容量淘汰通知时必须在同一轮释放关联操作"
    );
}
