//! 弹窗铁律集成测试（ADR-0021）
//!
//! 验收点：调用 notify::error(...) → 弹窗出现且**阻塞**后续操作；
//! warn 走 toast、info 只留底；三级消息一律进公告栏一本账。
//!
//! 本文件是独立测试进程，安全使用全局单例 init；
//! 三个等级在同一个 #[test] 里顺序验证，避免测试间共享全局态的干扰。

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notification_center::{Notification, NotificationCenter, Presenter, PresenterRegistry};

/// 模拟用户 100ms 后才点"知道了"的模态弹窗 Presenter，记录全部事件顺序。
struct BlockingDialogPresenter {
    events: Arc<Mutex<Vec<String>>>,
    dialog_hold: Duration,
}

impl BlockingDialogPresenter {
    fn record(&self, event: impl Into<String>) {
        self.events
            .lock()
            .expect("事件记录锁不可中毒")
            .push(event.into());
    }
}

impl Presenter for BlockingDialogPresenter {
    fn show_error_dialog(&self, notification: &Notification) {
        self.record(format!("dialog-open:{}", notification.title));
        // 用 recv_timeout 当定时器（clippy 禁 thread::sleep），
        // 模拟用户过了一会儿才点掉模态弹窗。
        let (_tx, rx) = mpsc::channel::<()>();
        let _ = rx.recv_timeout(self.dialog_hold);
        self.record(format!("dialog-closed:{}", notification.title));
    }

    fn show_toast(&self, notification: &Notification) {
        self.record(format!("toast:{}", notification.title));
    }

    fn update_unread_count(&self, count: usize) {
        self.record(format!("unread:{count}"));
    }
}

#[test]
fn error_popup_blocks_warn_toasts_info_stays_quiet() {
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let dialog_hold = Duration::from_millis(100);

    let registry = PresenterRegistry::new();
    registry.set_presenter(Arc::new(BlockingDialogPresenter {
        events: events.clone(),
        dialog_hold,
    }));
    let center = NotificationCenter::init(registry);

    // ── 弹窗铁律：error() 阻塞调用线程直到弹窗被点掉 ──────────────
    let start = Instant::now();
    notification_center::error("方案 2", "导出失败", "磁盘写入被拒绝");
    let elapsed = start.elapsed();
    events
        .lock()
        .expect("事件记录锁不可中毒")
        .push("after-error-returned".into());

    // 阻塞证据 1：error() 的耗时 ≥ 弹窗停留时间
    assert!(
        elapsed >= dialog_hold,
        "error() 应阻塞到弹窗关闭（耗时 {elapsed:?} < 停留 {dialog_hold:?}）"
    );

    // 阻塞证据 2：事件顺序 = 弹窗打开 → 弹窗关闭 → error() 返回后的代码
    {
        let log = events.lock().expect("事件记录锁不可中毒");
        let index_of = |needle: &str| {
            log.iter()
                .position(|event| event == needle)
                .unwrap_or_else(|| panic!("缺少事件 {needle}，实际：{log:?}"))
        };
        let open = index_of("dialog-open:导出失败");
        let close = index_of("dialog-closed:导出失败");
        let resumed = index_of("after-error-returned");
        assert!(open < close && close < resumed, "顺序错误：{log:?}");

        // 铁律反面：要紧错误绝不能以 toast 呈现
        assert!(
            !log.iter().any(|event| event == "toast:导出失败"),
            "Error 级消息被降级成了 toast：{log:?}"
        );
    }

    // ── warn()：toast 呈现，不弹窗 ────────────────────────────────
    notification_center::warn("应用", "自动保存成功", "刚才的修改已存好");
    {
        let log = events.lock().expect("事件记录锁不可中毒");
        assert!(log.iter().any(|event| event == "toast:自动保存成功"));
        assert!(!log.iter().any(|event| event == "dialog-open:自动保存成功"));
    }

    // ── info()：不弹不飘，只留底 ─────────────────────────────────
    notification_center::info("方案 2", "一张图未加载", "不影响使用");
    {
        let log = events.lock().expect("事件记录锁不可中毒");
        assert!(!log.iter().any(|event| event.contains("一张图未加载")));
    }

    // ── 公告栏一本账：三级消息全部留底，带来源标签，最新在前 ──────
    let board = center.board_snapshot();
    assert_eq!(board.len(), 3);
    assert_eq!(board[0].title, "一张图未加载");
    assert_eq!(board[0].source_tag, "方案 2");
    assert_eq!(board[2].title, "导出失败");

    // ── 铃铛未读数：发了 3 条 = 3，点开公告栏即清零 ───────────────
    assert_eq!(center.unread_count(), 3);
    center.mark_board_opened();
    assert_eq!(center.unread_count(), 0);
    assert!(events
        .lock()
        .expect("事件记录锁不可中毒")
        .iter()
        .any(|event| event == "unread:0"));

    // ── init 幂等：重复初始化返回同一个实例 ──────────────────────
    let again = NotificationCenter::init(PresenterRegistry::new());
    assert_eq!(again.board_snapshot().len(), 3);
}
