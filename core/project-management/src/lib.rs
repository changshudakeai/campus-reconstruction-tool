//! F3 项目方案管理模块
//!
//! 校区/方案的增删改查、方案边界会话、卡片三件套、复制方案、回收站集成。
//! 作为 ViewModel 层调用 B2 (data-persistence)，自身不写任何 SQL。
//!
//! ## 公开接口
//!
//! - `CampusViewModel`: 校区 CRUD ViewModel（列表、创建）
//! - `PlanViewModel`: 方案 CRUD ViewModel（列表、创建、改名、复制、删除到回收站）
//! - `AppSettingsViewModel`: 上次使用的校区读写（app_settings 表）
//! - `TrashViewModel`: 回收站（恢复查询、确认后永久删除、到期清理框架）
//! - `PlanBoundarySession`: 方案级边界获取、刷新、确认与会话复用

mod boundary_session;
mod entities;
mod error;
mod view_models;

pub use boundary_session::*;
pub use entities::*;
pub use error::*;
pub use view_models::*;
