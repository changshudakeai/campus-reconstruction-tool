//! F5 候选审核工作台
//!
//! 左卡片列表 + 中间大地图 + 右信息面板的三栏布局；所有候选初始为"待定"，
//! 逐个或批量裁决为保留/剔除；点错状态直接改点另一个状态（状态本身就是后悔药）。
//!
//! ## 缝 4 契约（无卡顿铁律，ADR-0016/0022）
//!
//! - 进入评审台时 [`ReviewWorkbench::load`] 向 B2 **一次性读入**候选集到内存；
//! - 评审期间不发生任何写库——本类型在读入与封账之间根本不持有数据库句柄；
//! - 导出确认后 [`ReviewWorkbench::seal`] 把最终三态一次性批量写回 B2，
//!   写回失败则封账不生效（评审状态保持可改）。
//!
//! ## B8 接口预留（ADR-0022 验收标准）
//!
//! 评审状态变更以明确的"状态变更操作"[`StateChange`] 形式实现（而非散落的
//! 直接赋值），将来补建撤销重做的命令历史层时无须翻修既有代码。
//!
//! ## 文案纪律（ADR-0005）
//!
//! 本 crate 不硬编码任何用户可见中文，只产出 B6 文本键（`review.*` 等），
//! 由 UI 层经 `localization::t()` 解析。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod candidate;
mod command;
mod error;
mod session;
mod view_models;
mod workbench;

pub use candidate::{Candidate, CandidateKey};
pub use command::{
    CommandOutcome, ConfirmationRequest, StateChange, BATCH_REMOVE_CONFIRM_THRESHOLD,
};
pub use error::{Error, Result};
pub use view_models::{
    text_keys, CandidateCardView, CategoryTabView, ExportSummary, InfoPanelView, MapObjectView,
    WorkbenchView,
};
pub use workbench::ReviewWorkbench;
