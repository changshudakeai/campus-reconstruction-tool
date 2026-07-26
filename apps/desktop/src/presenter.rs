//! B7 Presenter 的 Slint 壳实现（弹窗铁律 ADR-0021，T19B-2"装喇叭"）。
//!
//! [`ShellPresenter`] 把 B7 的分派结论变成看得见的界面：Error 级点亮
//! 主窗口最顶层的模态遮罩（`error-dialog-*` 属性），遮罩吞掉全部输入，
//! 用户点"知道了"前界面上什么都做不了——"点掉才能继续"的用户语义。
//!
//! ## 阻塞语义（诚实声明）
//!
//! - **非 UI 线程调用**（后台采集/导出线程）：经 `invoke_from_event_loop`
//!   点亮遮罩后，真正阻塞调用线程直到用户点"知道了"（channel 等待）。
//! - **UI 线程调用**（Slint 回调内报错）：点亮遮罩后立即返回。Slint 公共
//!   API 不支持嵌套事件循环（官方路线图中的 Modal Windows 特性尚未落地），
//!   在 UI 线程字面阻塞会死锁——遮罩已保证用户无法绕过弹窗操作界面，
//!   模态语义由遮罩层承担。
//!
//! Warn 级 toast 与铃铛角标的 Slint 呈现随公告栏工单（T19B 后续）接线，
//! 消息与未读数已由 B7 留底/维护，不丢。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use notification_center::{Notification, Presenter};
use slint::{ComponentHandle, Weak};

use crate::AppWindow;

/// B7 [`Presenter`] 的 Slint 壳实现（经 `PresenterRegistry::set_presenter` 注册）。
pub struct ShellPresenter {
    /// 主窗口弱引用（`Weak` 可跨线程持有，`upgrade` 仅在 UI 线程成功）
    window: Weak<AppWindow>,
    /// UI 线程 id（[`ShellPresenter::install`] 必须在 UI 线程调用）
    ui_thread: ThreadId,
    /// 等待"知道了"的后台调用者（点掉弹窗时全部唤醒）
    pending_acks: Arc<Mutex<Vec<Sender<()>>>>,
}

impl ShellPresenter {
    /// 在 UI 线程创建 Presenter 并接线"知道了"回调。
    ///
    /// 返回值交给 `NotificationCenter` 的 `PresenterRegistry::set_presenter`
    /// 注册；注册前的消息由 B7 照常留底。
    pub fn install(window: &AppWindow) -> Arc<Self> {
        let presenter = Arc::new(Self {
            window: window.as_weak(),
            ui_thread: std::thread::current().id(),
            pending_acks: Arc::new(Mutex::new(Vec::new())),
        });

        let weak = window.as_weak();
        let acks = Arc::clone(&presenter.pending_acks);
        window.on_error_dialog_dismissed(move || {
            if let Some(window) = weak.upgrade() {
                window.set_error_dialog_visible(false);
            }
            // 唤醒全部等待中的后台调用者（弹窗已被用户确认）
            for ack in acks.lock().expect("弹窗确认队列锁不可中毒").drain(..) {
                let _ = ack.send(());
            }
        });

        presenter
    }

    /// 把通知内容填进主窗口的弹窗属性并点亮遮罩（仅 UI 线程调用）。
    fn present(window: &AppWindow, notification: &Notification) {
        window.set_error_dialog_title(notification.title.clone().into());
        window.set_error_dialog_source(notification.source_tag.clone().into());
        window.set_error_dialog_body(notification.body.clone().into());
        window.set_error_dialog_visible(true);
    }
}

impl Presenter for ShellPresenter {
    fn show_error_dialog(&self, notification: &Notification) {
        if std::thread::current().id() == self.ui_thread {
            // UI 线程：点亮遮罩即返回（阻塞语义见模块文档"诚实声明"节）
            if let Some(window) = self.window.upgrade() {
                Self::present(&window, notification);
            }
            return;
        }

        // 非 UI 线程：登记等待名单 → 经事件循环点亮遮罩 → 阻塞到用户确认
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_acks
            .lock()
            .expect("弹窗确认队列锁不可中毒")
            .push(tx);
        let weak = self.window.clone();
        let notification = notification.clone();
        let dispatched = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                Self::present(&window, &notification);
            }
        });
        if dispatched.is_err() {
            // 事件循环不可用（无界面环境）：消息已由 B7 留底，不再阻塞。
            // 自己的 tx 留在名单里无害（rx 已 drop，dismiss 时 send 静默失败）。
            return;
        }
        let _ = rx.recv();
    }

    fn show_toast(&self, _notification: &Notification) {
        // Warn 级 toast 的 Slint 呈现归公告栏工单（T19B 后续）；消息已留底
    }

    fn update_unread_count(&self, _count: usize) {
        // 铃铛角标随公告栏界面（T19B 后续）接线；未读数已由 B7 维护
    }
}

// 注意：本模块的行为测试在 tests/ui_bindings.rs（需要真 AppWindow，
// 而 Slint 平台只能在一个线程初始化一次，单元测试并行创窗口必炸）。
