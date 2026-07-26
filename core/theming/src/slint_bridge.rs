//! MCRebuild V2 —— Slint 主题桥接模块
//!
//! 提供将 Rust 主题数据传递给 Slint UI 的接口.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// 从 Rust 传递到 Slint 的颜色映射 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlintColorPayload {
    #[serde(flatten)]
    pub colors: HashMap<String, String>,
}

impl SlintColorPayload {
    /// 从 ThemeManager 构建颜色映射
    pub fn from_color_card(card: &crate::ColorCard) -> Self {
        let mut colors = HashMap::new();
        for (role, hex) in card.colors.iter() {
            // 转换为 kebab-case 属性名
            let key = role.slint_property_name();
            colors.insert(key, hex.clone());
        }
        Self { colors }
    }
}

/// 从 Rust 传递到 Slint 的运动配置 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlintMotionPayload {
    pub reduce_motion: bool,
    pub speed_factor: f64,
    pub fast_duration: f64,
    pub medium_duration: f64,
    pub slow_duration: f64,
}

impl SlintMotionPayload {
    /// 从 ThemeManager 构建运动配置
    pub fn from_manager(manager: &crate::ThemeManager) -> Self {
        let table = manager.motion_table();
        Self {
            reduce_motion: manager.motion_settings().reduce_motion,
            speed_factor: manager.motion_settings().speed_factor,
            fast_duration: table.fast.duration,
            medium_duration: table.medium.duration,
            slow_duration: table.slow.duration,
        }
    }
}
