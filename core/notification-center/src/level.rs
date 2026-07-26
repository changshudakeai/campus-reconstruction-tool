//! 通知等级枚举（三级分派，ADR-0021）
//!
//! - **Error**：要紧错误（影响数据质量/卡住流程）——模态弹窗，禁止横幅；
//! - **Warn**：普通提示——toast 浮动几秒自动消失；
//! - **Info**：小毛病——不打扰，仅留底进公告栏。

use serde::{Deserialize, Serialize};

/// 通知等级 —— 决定呈现方式（弹窗 / toast / 仅留底）。
///
/// `#[non_exhaustive]`：下游被编译器强制写兜底分支（实施决定·架构与模块）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NotificationLevel {
    /// 要紧错误：必须模态弹窗确认（弹窗铁律）。
    /// 例：采集中断网、高德接口拒绝、导出失败、数据写入失败、体检疑点。
    Error,
    /// 普通提示：toast 自动消失 + 留底。
    /// 例：采集完成、自动保存成功、导出完成。
    Warn,
    /// 小毛病：不打扰，仅留底。
    /// 例：某张图未加载。
    Info,
}

impl NotificationLevel {
    /// 稳定的英文标识（日志、序列化外键用）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }

    /// 是否要紧错误（需要模态弹窗确认）。
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// 是否普通提示（toast 呈现）。
    pub fn is_warn(&self) -> bool {
        matches!(self, Self::Warn)
    }

    /// 是否仅留底（不打扰）。
    pub fn is_info(&self) -> bool {
        matches!(self, Self::Info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_level_is_modal() {
        assert!(NotificationLevel::Error.is_error());
        assert!(!NotificationLevel::Error.is_warn());
        assert!(!NotificationLevel::Error.is_info());
    }

    #[test]
    fn warn_level_is_toast() {
        assert!(!NotificationLevel::Warn.is_error());
        assert!(NotificationLevel::Warn.is_warn());
        assert!(!NotificationLevel::Warn.is_info());
    }

    #[test]
    fn info_level_is_quiet() {
        assert!(!NotificationLevel::Info.is_error());
        assert!(!NotificationLevel::Info.is_warn());
        assert!(NotificationLevel::Info.is_info());
    }

    #[test]
    fn as_str_is_stable() {
        assert_eq!(NotificationLevel::Error.as_str(), "error");
        assert_eq!(NotificationLevel::Warn.as_str(), "warn");
        assert_eq!(NotificationLevel::Info.as_str(), "info");
    }
}
