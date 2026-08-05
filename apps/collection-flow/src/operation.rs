//! 后台采集操作句柄（export-flow 的 Start/Poll/过期模式模板）。
//!
//! [`CollectionFlow::start`] 在返回前冻结不可变输入并派生后台 worker；
//! UI 只轮询 [`Self::try_complete`]，取消/切换方案使生命周期过期后，
//! 旧结果不得被拉回（返回 [`CollectionError::Expired`]，无伪成功）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use data_acquisition::CollectionProgressView;

use crate::error::{CollectionError, Result};
use crate::view::CollectionOutcome;

/// 后台采集操作；UI 读取真实进度并轮询终态。
pub struct CollectionOperation {
    result: mpsc::Receiver<CollectionOutcome>,
    lifecycle: Arc<AtomicU64>,
    generation: u64,
}

impl CollectionOperation {
    pub(crate) fn new(
        result: mpsc::Receiver<CollectionOutcome>,
        lifecycle: Arc<AtomicU64>,
        generation: u64,
    ) -> Self {
        Self {
            result,
            lifecycle,
            generation,
        }
    }

    /// 当前进度视图（进行中为拉取态；终态由 [`Self::try_complete`] 决定页面）。
    pub fn progress_view(&self) -> CollectionProgressView {
        CollectionProgressView::fetching()
    }

    /// 非阻塞取得后台终态；没有终态时返回 `None`。
    ///
    /// 取消/切换方案（生命周期过期）后，即使 worker 已完成也不得交付
    /// 旧结果——按 export-flow 模板返回 [`CollectionError::Expired`]。
    pub fn try_complete(&mut self) -> Option<Result<CollectionOutcome>> {
        let expired = self.lifecycle.load(Ordering::SeqCst) != self.generation;
        match self.result.try_recv() {
            Ok(_) if expired => Some(Err(CollectionError::Expired)),
            Ok(outcome) => Some(Ok(outcome)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(CollectionError::BackgroundTask)),
        }
    }
}
