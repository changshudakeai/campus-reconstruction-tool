//! B7 Presenter 的 Slint 壳实现（弹窗铁律 ADR-0021，T19B-2"装喇叭"）。
//
// [`ShellPresenter`] 把 B7 的分派结论变成看得见的界面：Error 级点亮
// 主窗口最顶层的模态遮罩（`error-dialog-*` 属性），遮罩吞掉全部输入，
// 用户点"知道了"前界面上什么都做不了——"点掉才能继续"的用户语义。
//
// ## 阻塞语义（诚实声明）
//
// - **非 UI 线程调用**（后台采集/导出线程）：经 `invoke_from_event_loop`
//   点亮遮罩后，真正阻塞调用线程直到用户点"知道了"（channel 等待）。
// - **UI 线程调用**（Slint 回调内报错）：点亮遮罩后立即返回。Slint 公共
//   API 不支持嵌套事件循环（官方路线图中的 Modal Windows 特性尚未落地），
//   在 UI 线程字面阻塞会死锁——遮罩已保证用户无法绕过弹窗操作界面，
//   模态语义由遮罩层承担。
//
// Warn 级 toast 与铃铛未读角标由本壳直接呈现（ADR-0021）：toast 短暂弹出
// 自动消失（main.slint Timer），未读数随 B7 分派增减；消息仍由 B7 留底。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use notification_center::{
    Notification, NotificationActionOutcome, OpaqueNotificationAction, Presenter,
};
use slint::{ComponentHandle, Weak};

use crate::AppWindow;

/// 在后台执行 B7 保存的不透明故障操作，并只把完成事件送回 UI 线程。
#[derive(Clone, Default)]
pub(crate) struct DiagnosticActionRunner {
    state: Arc<ActionRunnerState>,
}

#[derive(Default)]
struct ActionRunnerState {
    next_generation: AtomicU64,
    latest_generation: AtomicU64,
    completed: Mutex<VecDeque<(u64, Option<NotificationActionOutcome>)>>,
}

/// 一次已经回到 UI 事件循环的后台操作结果。
pub(crate) struct CompletedDiagnosticAction {
    is_latest: bool,
    outcome: Option<NotificationActionOutcome>,
}

impl CompletedDiagnosticAction {
    pub(crate) fn into_parts(self) -> (bool, Option<NotificationActionOutcome>) {
        (self.is_latest, self.outcome)
    }
}

impl DiagnosticActionRunner {
    /// 记录一个后来发生的页面操作，使仍在后台运行的旧结果不能覆盖它。
    pub(crate) fn invalidate(&self) {
        let generation = self.state.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state
            .latest_generation
            .store(generation, Ordering::SeqCst);
    }

    /// 立即登记处理中状态并把功能模块拥有的用例移交后台线程。
    pub(crate) fn start(&self, action: OpaqueNotificationAction, window: Weak<AppWindow>) {
        let generation = self.state.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.state
            .latest_generation
            .store(generation, Ordering::SeqCst);
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action.invoke())).ok();
            state
                .completed
                .lock()
                .expect("故障操作完成队列锁不可中毒")
                .push_back((generation, outcome));
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = window.upgrade() {
                    window.invoke_diagnostic_actions_completed();
                }
            });
        });
    }

    /// 仅在 UI 事件循环中取回已完成结果；较旧任务仍入 B7，但不覆盖最新状态。
    pub(crate) fn drain(&self) -> Vec<CompletedDiagnosticAction> {
        let latest = self.state.latest_generation.load(Ordering::SeqCst);
        self.state
            .completed
            .lock()
            .expect("故障操作完成队列锁不可中毒")
            .drain(..)
            .map(|(generation, outcome)| CompletedDiagnosticAction {
                is_latest: generation == latest,
                outcome,
            })
            .collect()
    }
}

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
                window.set_error_dialog_diagnostic_action_visible(false);
                // T36：关闭错误弹窗后清除采集“重试”按钮
                window.set_error_dialog_retry_visible(false);
                // T34：弹窗遮挡统一机制——错误弹窗关闭后按当前步骤模式
                // （边界页 vs 朝向页）恢复地图，不得恢复错页。
                crate::map_session::uncover_after_modal();
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
        // T36：新错误弹窗默认不带采集“重试”（由采集失败路径单独点亮）
        window.set_error_dialog_retry_visible(false);
        window.set_error_dialog_title(notification.title.clone().into());
        window.set_error_dialog_source(notification.source_tag.clone().into());
        window.set_error_dialog_body(notification.body.clone().into());
        window.set_error_dialog_notification_id(notification.id.to_string().into());
        let has_diagnostic_action =
            notification_center::NotificationCenter::global().is_some_and(|center| {
                center
                    .diagnostic_action(&notification.id.to_string())
                    .is_some()
            });
        window.set_error_dialog_diagnostic_action_visible(has_diagnostic_action);
        window.set_error_dialog_visible(true);
        // T34：地图 WebView 是原生子窗口，会渲染在 Slint 模态遮罩之上，
        // 让错误弹窗（及其拦截输入的遮罩）不可见；弹窗前先隐藏地图。
        crate::map_session::cover_for_modal();
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

    fn show_toast(&self, notification: &Notification) {
        if std::thread::current().id() == self.ui_thread {
            if let Some(window) = self.window.upgrade() {
                Self::present_toast(&window, notification);
            }
            return;
        }
        let weak = self.window.clone();
        let notification = notification.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                Self::present_toast(&window, &notification);
            }
        });
    }

    fn update_unread_count(&self, count: usize) {
        if std::thread::current().id() == self.ui_thread {
            if let Some(window) = self.window.upgrade() {
                window.set_notice_unread_count(count as i32);
            }
            return;
        }
        let weak = self.window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = weak.upgrade() {
                window.set_notice_unread_count(count as i32);
            }
        });
    }
}

impl ShellPresenter {
    /// 把普通提示填进 toast 浮层并点亮（仅 UI 线程调用；Timer 自动消失）。
    fn present_toast(window: &AppWindow, notification: &Notification) {
        window.set_toast_title(notification.title.clone().into());
        window.set_toast_body(notification.body.clone().into());
        window.set_toast_visible(true);
    }
}

// 注意：本模块的行为测试在 tests/ui_bindings.rs（需要真 AppWindow，
// 而 Slint 平台只能在一个线程初始化一次，单元测试并行创窗口必炸）。

// ────────────────────────────────────────────────────────────────────────────
// 回调错误统一出口（原 dispatch.rs，合并到呈现模块：错误出口与弹窗呈现同属 B7）
// ────────────────────────────────────────────────────────────────────────────
// T19B-1 —— 回调错误统一出口（壳内零业务逻辑的错误处理纪律）。
//
// Slint 回调闭包内调用 VM 方法返回的 `Result` 错误一律递到这里，
// 按弹窗铁律（ADR-0021）经 B7 `error()` 模态弹窗 + 公告栏留底；
// 壳不自行判断错误轻重、不静默吞错。B7 全局单例由
// [`crate::run_dev`] 启动时初始化；Slint Presenter（弹窗/toast 声明）
// 随后续 T19B 工单注册，注册前消息照常留底不丢。

use localization::Localization;

/// 把回调错误分派给 B7（模态弹窗 + 公告栏留底）。
///
/// `error` 是带类型错误的显示形式；来源标签与标题走 B6 文本键
/// （`app.source_tag` / `dialog.error_title`），错误详情原样透传
/// （不隐藏、不吞异常，ADR-0025 错误码转换行约束）。
pub fn report_callback_error(l10n: &Localization, error: &dyn std::fmt::Display) {
    notification_center::error(
        l10n.t("app.source_tag"),
        l10n.t("dialog.error_title"),
        error.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use localization::Language;
    use notification_center::{NotificationCenter, PresenterRegistry};

    use super::*;

    #[test]
    fn callback_error_reaches_b7_board() {
        // init 幂等：测试进程内首次调用即建立全局一本账
        let center = NotificationCenter::init(PresenterRegistry::new());
        let l10n = Localization::new(Language::ZhCn).expect("加载 zh-CN 资源");

        let error_text = "演示错误：数据库暂不可用";
        report_callback_error(&l10n, &error_text);

        assert!(
            center.board_snapshot().iter().any(|n| n.body == error_text),
            "回调错误应留底进 B7 公告栏"
        );
    }
}
