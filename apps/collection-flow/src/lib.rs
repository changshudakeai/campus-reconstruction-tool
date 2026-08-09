//! A1 collection-flow —— 可选候选采集完整用例（ADR-0039/0040）。
//!
//! 应用流程模块：位于 S1 薄壳与功能/基础模块之间，拥有"开始采集"和
//! "查看采集报告"两个完整用户操作的深模块接口。接口后协调 F4 采集、
//! B2 原始观测落库与候选投影原子发布、B14 点线面验证、F7 覆盖体检，
//! 返回已决定的页面状态、进度、导航结果和通知事实。
//!
//! - 候选数据按 ADR-0040 分为**永久原始观测**（数据粮仓，只写不删）与
//!   **可重建候选投影**（批次原子发布）；只有原始观测已保存、候选投影
//!   完整发布且采集报告完成后才解锁评审入口。
//! - 采集失败只暂停本次候选采集，不取消已确认边界的基础导出资格。
//! - 生产 adapter 与测试 adapter 只放在 [`CollectionFlow`] 内部 seam，
//!   测试通过外部接口观察行为，不越过接口检查 F4/B2/B14/F7 内部顺序。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod error;
mod flow;
mod input;
mod operation;
mod view;

pub use error::{CollectionError, Result};
pub use flow::{CollectionFlow, CollectionRunLimits};
pub use operation::CollectionOperation;
pub use view::{
    CollectionFailure, CollectionFailureView, CollectionOutcome, CollectionPageView,
    CollectionReportView, CollectionStatus, CollectionSummary,
};
