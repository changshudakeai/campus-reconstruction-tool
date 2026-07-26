//! MCRebuild V2 —— B10 主题与动画模块
//!
//! 依据 ADR-0023(主题与动画——色卡机制 + 动效表 + Codex 手感基准):
//! - 色卡机制：代码只写颜色角色名禁写颜色号;切主题 = 换色卡
//! - 出厂两张色卡 (亮色 + 暗色),支持"跟随系统"
//! - "减少动画"开关一键全关
//! - 动效表集中定义(fast=0.2s,medium=0.5s,slow=1.0s)
//!
//! 本 crate 是基础模块 (B10),仅可依赖 B1 shared-domain-types.
//! 不得依赖任何功能模块或其他基础模块.

mod color_card;
mod motion_table;
mod slint_bridge;
mod system_detection;
mod theme_manager;

pub use color_card::*;
pub use motion_table::*;
pub use slint_bridge::*;
pub use system_detection::*;
pub use theme_manager::*;

/// 主题系统错误类型
#[derive(Debug, thiserror::Error)]
pub enum ThemingError {
    #[error("JSON 解析失败:{0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("颜色角色未定义:{0}")]
    ColorRoleNotFound(String),

    #[error("无效的颜色值:{value} (应为#RRGGBB 或 #AARRGGBB)")]
    InvalidColorValue { value: String },

    #[error("缺少必需的颜色角色:{0:?}")]
    MissingRequiredColors(Vec<ColorRole>),
}

/// 结果类型别名
pub type Result<T> = std::result::Result<T, ThemingError>;
