//! F9 导出控制台
//!
//! 窗口契约缝 5 与缝 6 的居中层：
//! - **缝 5**：接收 [`ExportRequest`]（保留项集合 + 类别汇总 + 待定计数）；
//!   产出确认弹窗纯数据视图 [`ExportConfirmDialogView`]（类别汇总表格 /
//!   封账后果文字 / 待定项报数 / 取消与确认按钮）；
//! - 用户确认后经 [`SealGate`] 封账（评审终态批量写回由 F5 `seal()` 落实）；
//! - **缝 6**：[`pipeline`] 把 B18 的 `BlockModel` 适配成 B4 的 `VoxelModel`
//!   并落成 .schem（两基础模块互不依赖，F9 居中适配）；
//! - **非阻塞进度条**：[`ProgressTracker`] 多线程共享，UI 轮询
//!   [`ExportProgressView`]（右上角浮动提示 + 百分比数字）；
//! - 导出失败回滚：[`ExportConsole::fail_and_rollback`] 释放封账
//!   （评审恢复可改）并按弹窗铁律走 B7 `error()` 模态弹窗；
//! - 导出成功跳转：[`ExportConsole::complete_export`] 产出跳转目标
//!   （导出完成页）并走 B7 `warn()` toast。
//!
//! # 架构边界（ADR-0017）
//!
//! - F9 依赖 B1/B4/B6/B7/B18；横向零依赖其他 F* 功能模块
//!   （F5 评审台只在 dev-dependencies，供集成测试验证回滚语义）；
//! - slint 只准壳依赖：本 crate 只产出纯数据 ViewModel，弹窗/进度条的
//!   Slint 声明由壳绑定；
//! - 文案外置（ADR-0005）：只引用 B6 文本键（`export.*` 等既有键），
//!   禁止硬编码用户可见文字；
//! - 弹窗铁律（ADR-0021）：导出失败属"卡住流程"级——B7 `error()` 模态弹窗，
//!   禁止横幅；导出完成属普通提示——B7 `warn()` toast。
//!
//! # 封账语义（ADR-0022）
//!
//! - 确认即封账：评审决定不可再改（F5 `seal()` 后 `submit()` 返回
//!   `AlreadySealed`）；
//! - 封账失败不生效：`SealGate::seal()` 返回 Err 时评审保持可改，
//!   不出现"账封了但没存上"的半截状态；
//! - 导出失败回滚：`SealGate::release()` 使封账失效，重新进台评审可改。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod console;
mod data;
mod error;
mod pipeline;
mod progress;
mod seal_gate;
mod views;

pub use console::ExportConsole;
pub use data::{ExportRequest, ExportStage, ExportSummary};
pub use error::{Error, Result};
pub use pipeline::{adapt_to_voxel_model, export_schematic};
pub use progress::ProgressTracker;
pub use seal_gate::{MockSealGate, SealGate};
pub use views::{
    text_keys, ExportConfirmDialogView, ExportProgressView, NavigationTarget, SummaryRowView,
};

/// 带 Mock 门控的导出控制台别名（测试与文档示例用）
pub type MockExportConsole = ExportConsole<MockSealGate>;
