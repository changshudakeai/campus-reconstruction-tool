//! B1 共享领域类型
//!
//! 全工程唯一的名词定义：校区、方案、候选、六类别、三态评审、封账等术语的
//! Rust 类型实现。**零内部依赖**——词汇的最底座。
//!
//! ## 模块组织
//!
//! - [id](crate::id): `CampusId`, `PlanId` —— 基于 Uuid v4 的唯一标识符
//! - [category](crate::category): `CandidateCategory` —— 六类别枚举
//! - [review](crate::review): `ReviewState` —— 三态评审枚举
//! - [boundary](crate::boundary): `Boundary`, `Orientation` —— 几何与方向
//! - [status](crate::status): `CollectionJobStatus` —— 采集任务状态

#![cfg_attr(not(test), warn(unreachable_pub))]

pub mod boundary;
pub mod category;
pub mod id;
pub mod review;
pub mod status;

// 重新导出公共类型，方便 crate 外使用
pub use boundary::{Boundary, Orientation};
pub use category::CandidateCategory;
pub use id::{CampusId, PlanId};
pub use review::ReviewState;
pub use status::CollectionJobStatus;
