//! B2 数据持久化核心
//!
//! SQLite 数据库唯一入口：迁移脚本、原始观测表（数据粮仓）、
//! 评审终态批量写回、校园级回收站。
//!
//! ## 架构边界（ADR-0002/0017）
//!
//! - 内部依赖仅 B1（共享领域类型），不依赖任何 F* 功能模块；
//! - rusqlite 只在本 crate 出现，其余模块经由本 crate 的公开 trait 使用存储；
//! - 铁律：原始观测数据（数据粮仓）**永不删除**——本 crate 不提供任何
//!   删除原始观测的 API，重复采集按内容指纹（digest）增量刷新。
//!
//! ## 公开接口
//!
//! - [`Database`]：数据库句柄，打开即自动迁移到最新 schema 版本；
//! - [`RawObservationsApi`]：原始观测写入与查询（缝 3，F4 采集当场落库）；
//! - [`ReviewDecisionsApi`]：评审终态批量写回（缝 4，封账时一次性原子事务）；
//! - [`TrashApi`]：回收站进站/恢复/到期清理/确认后永久删除（缝 2，F3 调用）；
//! - [`CampusCrudApi`] / [`PlanCrudApi`] / [`AppSettingsApi`]：校区/方案 CRUD 与
//!   "上次使用的校区"读写（缝 2，F3 方案管理调用）。

// B2 是全 workspace 唯一合法的 SQLite 直接使用者（clippy.toml 禁用表 + deny.toml
// bans 白名单双重执法，豁免按纪律留痕）。
#![allow(
    clippy::disallowed_types,
    reason = "B2 data-persistence 是 rusqlite::Connection 的唯一合法使用者（ADR-0002/0017 红线表）"
)]
#![cfg_attr(not(test), warn(unreachable_pub))]

mod candidate_projections;
mod database;
mod entities;
mod error;
mod migrations;
mod projects;
mod raw_observations;
mod review_decisions;
mod trash;

pub use candidate_projections::{
    CandidateBatch, CandidateBatchStatus, CandidateBatchSummary, CandidateEligibility,
    CandidateProjection, CandidateProjectionsApi, CandidateShape, CandidateValidation,
};
pub use database::Database;
pub use entities::{RawObservation, ReviewDecision, TrashItem, TRASH_RETENTION_DAYS};
pub use error::{Error, Result};
pub use migrations::LATEST_SCHEMA_VERSION;
pub use projects::{
    AppSettingKey, AppSettingsApi, CampusCrudApi, CampusEntity, PlanCrudApi, PlanEntity,
};
pub use raw_observations::RawObservationsApi;
pub use review_decisions::ReviewDecisionsApi;
pub use trash::TrashApi;
