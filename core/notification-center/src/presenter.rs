//! Presenter 接缝 —— 壳实现的 UI 呈现接口（窗口契约缝 1）
//!
//! B7 只做分派、留底、清理与未读计数；弹窗/toast/铃铛的 Slint 声明
//! 由壳实现本模块的 [`Presenter`] trait 接入（slint 只准壳依赖）。
//!
//! ## 弹窗铁律的接缝要求
//!
//! [`Presenter::show_error_dialog`] **必须模态阻塞**：不返回，调用线程就
//! 不能继续——这是"点掉才能继续"在代码层的表达。

use std::sync::{Arc, RwLock};

use crate::message::Notification;
use crate::storage::InMemoryStorage;

/// UI 呈现接口 —— 壳实现此 trait，B7 通过它把分派结论变成看得见的界面。
pub trait Presenter: Send + Sync {
    /// 显示 Error 级通知的**模态弹窗**。
    ///
    /// 铁律：本方法必须阻塞到用户点"知道了"才返回；实现禁止改用横幅/toast。
    fn show_error_dialog(&self, notification: &Notification);

    /// 显示 Warn 级通知的 toast（右上角浮动几秒后自动消失）。
    ///
    /// 不得阻塞调用线程；消失由壳的定时器负责。
    fn show_toast(&self, notification: &Notification);

    /// 更新铃铛图标上的未读数角标（0 = 不显示角标）。
    fn update_unread_count(&self, count: usize);
}

/// 空操作 Presenter：无界面环境（测试、无头运行）的占位实现。
#[derive(Debug, Default, Clone, Copy)]
pub struct DummyPresenter;

impl Presenter for DummyPresenter {
    fn show_error_dialog(&self, _notification: &Notification) {}

    fn show_toast(&self, _notification: &Notification) {}

    fn update_unread_count(&self, _count: usize) {}
}

/// Presenter 注册器：持有可替换的 Presenter 与消息内存表。
///
/// 克隆共享同一份注册状态与存储（内部全是引用计数指针）。
#[derive(Clone, Default)]
pub struct PresenterRegistry {
    presenter: Arc<RwLock<Option<Arc<dyn Presenter>>>>,
    storage: InMemoryStorage,
}

impl PresenterRegistry {
    /// 创建一个新的注册器（未注册 Presenter、空存储）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册 Presenter（壳启动时调用；重复注册以最后一次为准）。
    pub fn set_presenter(&self, presenter: Arc<dyn Presenter>) {
        let mut guard = self.presenter.write().expect("Presenter 注册锁不可中毒");
        *guard = Some(presenter);
    }

    /// 注销 Presenter（回到无界面状态）。
    pub fn clear_presenter(&self) {
        let mut guard = self.presenter.write().expect("Presenter 注册锁不可中毒");
        *guard = None;
    }

    /// 弹出错误模态弹窗（未注册 Presenter 时无操作，消息仍已留底）。
    pub fn show_error(&self, notification: &Notification) {
        if let Some(presenter) = self.current_presenter() {
            presenter.show_error_dialog(notification);
        }
    }

    /// 弹出 toast（未注册 Presenter 时无操作，消息仍已留底）。
    pub fn show_toast(&self, notification: &Notification) {
        if let Some(presenter) = self.current_presenter() {
            presenter.show_toast(notification);
        }
    }

    /// 刷新铃铛未读数角标。
    pub fn update_unread_count(&self, count: usize) {
        if let Some(presenter) = self.current_presenter() {
            presenter.update_unread_count(count);
        }
    }

    /// 消息存储（内存表）。
    pub fn storage(&self) -> &InMemoryStorage {
        &self.storage
    }

    /// 取当前注册的 Presenter（克隆 Arc，避免持锁调用 UI 回调）。
    fn current_presenter(&self) -> Option<Arc<dyn Presenter>> {
        self.presenter
            .read()
            .expect("Presenter 注册锁不可中毒")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 计数 Presenter：记录每类回调被调用的次数。
    #[derive(Debug, Default)]
    struct CountingPresenter {
        dialogs: AtomicUsize,
        toasts: AtomicUsize,
    }

    impl Presenter for CountingPresenter {
        fn show_error_dialog(&self, _notification: &Notification) {
            self.dialogs.fetch_add(1, Ordering::SeqCst);
        }

        fn show_toast(&self, _notification: &Notification) {
            self.toasts.fetch_add(1, Ordering::SeqCst);
        }

        fn update_unread_count(&self, _count: usize) {}
    }

    #[test]
    fn calls_without_presenter_are_noops() {
        let registry = PresenterRegistry::new();
        let n = Notification::error("test", "标题", "内容");
        registry.show_error(&n);
        registry.show_toast(&n);
        registry.update_unread_count(1);
    }

    #[test]
    fn registered_presenter_receives_calls() {
        let registry = PresenterRegistry::new();
        let counting = Arc::new(CountingPresenter::default());
        registry.set_presenter(counting.clone());

        let n = Notification::error("test", "标题", "内容");
        registry.show_error(&n);
        registry.show_toast(&n);

        assert_eq!(counting.dialogs.load(Ordering::SeqCst), 1);
        assert_eq!(counting.toasts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clear_presenter_stops_delivery() {
        let registry = PresenterRegistry::new();
        let counting = Arc::new(CountingPresenter::default());
        registry.set_presenter(counting.clone());
        registry.clear_presenter();

        registry.show_error(&Notification::error("test", "标题", "内容"));
        assert_eq!(counting.dialogs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn clones_share_registration_and_storage() {
        let registry = PresenterRegistry::new();
        let alias = registry.clone();
        alias.storage().append(Notification::info("test", "消息", "内容"));
        assert_eq!(registry.storage().all_notifications().len(), 1);
    }
}
