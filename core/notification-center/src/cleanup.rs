//! 自动清理调度器（ADR-0021 留存规则的执行者）
//!
//! 后台线程每 5 分钟扫描一次消息存储，把超出留存规则
//! （最近 200 条或 30 天先到为准）的消息移除；清理不需要用户确认。
//!
//! 实现说明：等待用 `mpsc::recv_timeout`（可即时被 stop 打断），
//! 不用 `thread::sleep`（clippy 禁用表：阻塞睡眠违反无卡顿铁律）。

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use log::debug;

use crate::storage::Storage;

/// 清理扫描间隔：每 5 分钟一次。
pub const CLEANUP_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// 后台清理线程的控制柄（停止信号 + join 柄）。
struct Worker {
    stop_tx: Sender<()>,
    handle: JoinHandle<()>,
}

/// 自动清理调度器 —— 定期把过期/超量消息移出存储。
pub struct CleanupScheduler {
    storage: Arc<dyn Storage>,
    worker: Mutex<Option<Worker>>,
}

impl CleanupScheduler {
    /// 创建调度器（不自动启动，调用 [`start`](Self::start) 开始定时扫描）。
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            worker: Mutex::new(None),
        }
    }

    /// 启动后台清理线程，按 [`CLEANUP_INTERVAL`]（5 分钟）定时扫描。
    ///
    /// 已在运行时调用是无操作。
    pub fn start(&self) {
        self.start_with_interval(CLEANUP_INTERVAL);
    }

    /// 以自定义间隔启动（测试用短间隔；生产走 [`start`](Self::start)）。
    pub fn start_with_interval(&self, interval: Duration) {
        let mut guard = self.worker.lock().expect("调度器状态锁不可中毒");
        if guard.is_some() {
            debug!("清理调度器已在运行，忽略重复启动");
            return;
        }

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let storage = Arc::clone(&self.storage);
        let handle = std::thread::spawn(move || loop {
            // recv_timeout 既是定时器也是停止信号接收器：
            // 收到消息或通道关闭 → 退出；超时 → 执行一轮清理。
            match stop_rx.recv_timeout(interval) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => run_cleanup(storage.as_ref()),
            }
        });
        *guard = Some(Worker { stop_tx, handle });
    }

    /// 停止后台清理线程并等待其退出（未启动时是无操作）。
    pub fn stop(&self) {
        let worker = self.worker.lock().expect("调度器状态锁不可中毒").take();
        if let Some(worker) = worker {
            // 发送失败说明线程已退出，直接 join 即可。
            let _ = worker.stop_tx.send(());
            let _ = worker.handle.join();
            debug!("清理调度器已停止");
        }
    }

    /// 是否有后台清理线程在运行。
    pub fn is_running(&self) -> bool {
        self.worker.lock().expect("调度器状态锁不可中毒").is_some()
    }

    /// 立即执行一轮清理（不依赖后台线程；测试与手动触发用）。
    pub fn run_once(&self) {
        run_cleanup(self.storage.as_ref());
    }
}

impl Drop for CleanupScheduler {
    /// 调度器销毁时顺带停掉后台线程，避免悬挂。
    fn drop(&mut self) {
        self.stop();
    }
}

/// 单轮清理：把不在留存快照（200 条 / 30 天）内的消息移出存储。
fn run_cleanup(storage: &dyn Storage) {
    let all = storage.all_notifications();
    let keep: std::collections::HashSet<uuid::Uuid> =
        storage.snapshot().iter().map(|n| n.id).collect();

    let mut cleaned = 0usize;
    for stale in all.iter().filter(|n| !keep.contains(&n.id)) {
        storage.remove_by_id(&stale.id);
        cleaned += 1;
    }

    if cleaned > 0 {
        debug!("清理调度器：移除 {cleaned} 条过期/超量消息");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Notification;
    use crate::storage::InMemoryStorage;

    fn expired_message(title: &str) -> Notification {
        let mut n = Notification::info("test", title, "超过 30 天");
        n.created_at = chrono::Utc::now() - chrono::Duration::days(31);
        n
    }

    #[test]
    fn run_once_removes_expired_messages() {
        let storage = InMemoryStorage::new();
        storage.append(expired_message("旧消息"));
        storage.append(Notification::info("test", "新消息", "今天发的"));
        assert_eq!(storage.all_notifications().len(), 2);

        let scheduler = CleanupScheduler::new(Arc::new(storage.clone()));
        scheduler.run_once();

        let rest = storage.all_notifications();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].title, "新消息");
    }

    #[test]
    fn run_once_keeps_fresh_messages() {
        let storage = InMemoryStorage::new();
        for i in 0..10 {
            storage.append(Notification::info("test", format!("消息 {i}"), "内容"));
        }

        let scheduler = CleanupScheduler::new(Arc::new(storage.clone()));
        scheduler.run_once();
        assert_eq!(storage.all_notifications().len(), 10);
    }

    #[test]
    fn run_once_on_empty_storage_is_noop() {
        let storage = InMemoryStorage::new();
        let scheduler = CleanupScheduler::new(Arc::new(storage.clone()));
        scheduler.run_once();
        assert!(storage.all_notifications().is_empty());
    }

    #[test]
    fn background_worker_cleans_on_interval() {
        let storage = InMemoryStorage::new();
        storage.append(expired_message("旧消息"));

        let scheduler = CleanupScheduler::new(Arc::new(storage.clone()));
        scheduler.start_with_interval(Duration::from_millis(20));
        assert!(scheduler.is_running());

        // 轮询等待后台线程完成至少一轮清理（上限 2 秒，避免偶发慢机器误报）。
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if storage.all_notifications().is_empty() {
                break;
            }
            std::thread::yield_now();
        }
        scheduler.stop();

        assert!(storage.all_notifications().is_empty());
        assert!(!scheduler.is_running());
    }

    #[test]
    fn start_twice_is_noop_and_stop_joins() {
        let storage = InMemoryStorage::new();
        let scheduler = CleanupScheduler::new(Arc::new(storage));
        scheduler.start_with_interval(Duration::from_millis(50));
        scheduler.start_with_interval(Duration::from_millis(50));
        assert!(scheduler.is_running());
        scheduler.stop();
        assert!(!scheduler.is_running());
        // 重复 stop 是无操作
        scheduler.stop();
    }
}
