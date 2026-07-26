//! 通知中心核心 —— 全局单例：分派、留底、未读计数、清理调度
//!
//! 壳启动时 [`NotificationCenter::init`] 一次；功能模块通过 crate 根的
//! [`error`](crate::error) / [`warn`](crate::warn) / [`info`](crate::info)
//! 提交分派结论，B7 负责呈现方式与历史存储（窗口契约缝 1）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use once_cell::sync::OnceCell;

use crate::cleanup::CleanupScheduler;
use crate::level::NotificationLevel;
use crate::message::Notification;
use crate::presenter::PresenterRegistry;
use crate::storage::Storage;

/// 全局单例槽（进程内只初始化一次）。
static INSTANCE: OnceCell<Arc<NotificationCenter>> = OnceCell::new();

/// 通知中心 —— 全应用一本账的消息中枢。
pub struct NotificationCenter {
    registry: PresenterRegistry,
    scheduler: CleanupScheduler,
    unread: AtomicUsize,
}

impl NotificationCenter {
    /// 创建一个独立实例（不注册为全局单例；测试用）。
    pub fn new(registry: PresenterRegistry) -> Self {
        let storage: Arc<dyn Storage> = Arc::new(registry.storage().clone());
        Self {
            registry,
            scheduler: CleanupScheduler::new(storage),
            unread: AtomicUsize::new(0),
        }
    }

    /// 初始化全局单例并启动自动清理调度器（每 5 分钟一轮）。
    ///
    /// 幂等：重复调用返回首次初始化的实例（后续传入的 registry 被忽略）。
    pub fn init(registry: PresenterRegistry) -> Arc<Self> {
        INSTANCE
            .get_or_init(|| {
                let center = Arc::new(Self::new(registry));
                center.scheduler.start();
                center
            })
            .clone()
    }

    /// 取全局单例（未初始化返回 None）。
    pub fn global() -> Option<Arc<Self>> {
        INSTANCE.get().cloned()
    }

    /// 发布一条通知：留底 + 未读数 +1 + 按等级分派呈现。
    ///
    /// Error 级会阻塞当前线程直到用户在模态弹窗上确认（弹窗铁律）。
    pub fn publish(&self, notification: Notification) {
        // 一律留底（全应用一本账）
        self.registry.storage().append(notification.clone());

        // 铃铛未读数 +1
        let unread = self.unread.fetch_add(1, Ordering::SeqCst) + 1;
        self.registry.update_unread_count(unread);

        // 三级分派（弹窗铁律：Error 必须模态弹窗，禁止降级）
        match notification.level {
            NotificationLevel::Error => self.registry.show_error(&notification),
            NotificationLevel::Warn => self.registry.show_toast(&notification),
            NotificationLevel::Info => {
                log::info!("[{}] {}", notification.source_tag, notification.title);
            }
        }
    }

    /// 公告栏消息列表（留存规则内，按时间倒序）。
    pub fn board_snapshot(&self) -> Vec<Notification> {
        self.registry.storage().snapshot()
    }

    /// 公告栏被点开：未读数清零并刷新角标（ADR-0021"点开即清零"）。
    pub fn mark_board_opened(&self) {
        self.unread.store(0, Ordering::SeqCst);
        self.registry.update_unread_count(0);
    }

    /// 当前未读数。
    pub fn unread_count(&self) -> usize {
        self.unread.load(Ordering::SeqCst)
    }

    /// Presenter 注册器（壳注册/注销 UI 实现的入口）。
    pub fn registry(&self) -> &PresenterRegistry {
        &self.registry
    }

    /// 清理调度器（手动触发一轮清理走 `cleanup().run_once()`）。
    pub fn cleanup(&self) -> &CleanupScheduler {
        &self.scheduler
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presenter::DummyPresenter;

    // 注意：全局单例 init 的行为放在集成测试（tests/popup_rule.rs）里验证，
    // 单元测试一律用独立实例 NotificationCenter::new，避免测试间共享全局状态。

    fn center_with_dummy() -> NotificationCenter {
        let registry = PresenterRegistry::new();
        registry.set_presenter(Arc::new(DummyPresenter));
        NotificationCenter::new(registry)
    }

    #[test]
    fn publish_appends_to_board() {
        let center = center_with_dummy();
        center.publish(Notification::error("方案 2", "导出失败", "磁盘写入被拒绝"));
        center.publish(Notification::info("应用", "设置已保存", "语言切换为中文"));

        let board = center.board_snapshot();
        assert_eq!(board.len(), 2);
        // 倒序：最新的在前
        assert_eq!(board[0].title, "设置已保存");
    }

    #[test]
    fn unread_count_increments_and_resets_on_open() {
        let center = center_with_dummy();
        center.publish(Notification::warn("test", "提示 1", "内容"));
        center.publish(Notification::warn("test", "提示 2", "内容"));
        assert_eq!(center.unread_count(), 2);

        center.mark_board_opened();
        assert_eq!(center.unread_count(), 0);
    }

    #[test]
    fn publish_without_presenter_still_records() {
        // 无界面环境：消息不弹不飘，但必须留底（一本账不缺页）
        let center = NotificationCenter::new(PresenterRegistry::new());
        center.publish(Notification::error("test", "标题", "内容"));
        assert_eq!(center.board_snapshot().len(), 1);
    }

    #[test]
    fn cleanup_run_once_via_accessor() {
        let center = center_with_dummy();
        center.publish(Notification::info("test", "新消息", "内容"));
        center.cleanup().run_once();
        assert_eq!(center.board_snapshot().len(), 1);
    }
}
