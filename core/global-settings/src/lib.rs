//! F1 应用全局设置
//!
//! 语言与 Minecraft 游戏版本的**应用级**全局设置（ADR-0004：所有校区、所有
//! 方案统一按全局版本导出，不提供方案级覆盖），以及老用户着陆流程所需的
//! "上次使用的校区"读写（ADR-0006）。
//!
//! ## 模块组织
//!
//! - [settings](crate::settings)：`GlobalSettings` / `FirstRunSetup` / `SettingsManager`
//!   —— 设置读写与首次设置向导（页面兼任知情告知）
//! - [landing](crate::landing)：`LandingCampus` —— 上次使用的校区视图
//!   （校区已被删除时返回 `None`，调用方退回校区选择页）
//! - [error](crate::error)：带类型错误
//!
//! ## 架构边界（ADR-0017）
//!
//! 内部依赖仅 B1（共享领域类型）与 B2（数据持久化）；存储只经 B2 公开的
//! `AppSettingsApi` / `CampusCrudApi` trait，本 crate 不触碰 SQL。

#![cfg_attr(not(test), warn(unreachable_pub))]

mod error;
mod landing;
mod settings;

pub use error::{Error, Result};
pub use landing::LandingCampus;
pub use settings::{
    FirstRunSetup, GlobalSettings, SettingsManager, DEFAULT_LANGUAGE, DEFAULT_MINECRAFT_VERSION,
    SUPPORTED_LANGUAGES, SUPPORTED_MINECRAFT_VERSIONS, VERSION_NOTICE_TEXT,
};
