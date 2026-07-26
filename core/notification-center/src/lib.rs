//! B7 通知中心：弹窗铁律 + 公告栏留底（ADR-0021）
//!
//! 全应用消息分发中枢，按三级规矩分派：
//! - [`error`]：要紧错误（影响数据质量/卡住流程）——**模态弹窗**，
//!   阻塞调用线程直到用户确认，禁止横幅；
//! - [`warn`]：普通提示——toast 浮动几秒自动消失；
//! - [`info`]：小毛病——不打扰，仅留底进公告栏。
//!
//! 三级消息一律留底（全应用一本账），留存最近 200 条或 30 天先到为准。
//!
//! ## 架构边界（ADR-0017/0021）
//!
//! - B7 是基础模块：零内部依赖，不依赖功能层/壳（下不依上）；
//! - UI 呈现走 [`Presenter`] 接缝：壳实现弹窗/toast/铃铛的 Slint 声明
//!   （slint 只准壳依赖），B7 只负责分派、留底、清理与未读计数；
//! - 持久化暂用内存表 [`InMemoryStorage`]（SQLite 属 B2/T11 地盘），
//!   [`Storage`] trait 是未来迁移的接缝；
//! - 文案：B7 不产文案，标题/正文由调用方以文本键解析后的成品递入
//!   （窗口契约章：错误分派结论 = 呈现方式 + 文本 + 来源标签）。

#![cfg_attr(not(test), warn(unreachable_pub))]

pub mod center;
pub mod cleanup;
pub mod level;
pub mod message;
pub mod presenter;
pub mod storage;

// 重新导出公共类型，方便 crate 外使用
pub use center::NotificationCenter;
pub use cleanup::{CleanupScheduler, CLEANUP_INTERVAL};
pub use level::NotificationLevel;
pub use message::Notification;
pub use presenter::{DummyPresenter, Presenter, PresenterRegistry};
pub use storage::{InMemoryStorage, Storage, MAX_RETAINED_DAYS, MAX_RETAINED_MESSAGES};

/// 要紧错误：模态弹窗（阻塞直到用户确认）+ 留底。
///
/// 弹窗铁律（ADR-0021）：影响采集数据质量、或导致后续操作无法继续的错误
/// 必须走本函数，禁止降级为 toast/横幅。
pub fn error(source_tag: impl Into<String>, title: impl Into<String>, body: impl Into<String>) {
    dispatch(Notification::error(source_tag, title, body));
}

/// 普通提示：toast 浮动几秒自动消失 + 留底。
pub fn warn(source_tag: impl Into<String>, title: impl Into<String>, body: impl Into<String>) {
    dispatch(Notification::warn(source_tag, title, body));
}

/// 小毛病：不打扰用户，仅留底进公告栏。
pub fn info(source_tag: impl Into<String>, title: impl Into<String>, body: impl Into<String>) {
    dispatch(Notification::info(source_tag, title, body));
}

/// 统一分发入口：全局单例未初始化时降级写日志，消息不静默丢弃。
fn dispatch(notification: Notification) {
    match NotificationCenter::global() {
        Some(center) => center.publish(notification),
        None => log::error!(
            "通知中心未初始化，消息未送达：[{}] {} — {}",
            notification.source_tag,
            notification.title,
            notification.body
        ),
    }
}
