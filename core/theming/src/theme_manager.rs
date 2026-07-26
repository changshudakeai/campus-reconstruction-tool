//! MCRebuild V2 —— 主题管理器模块
//!
//! 管理亮暗双色卡、当前主题模式和动画设置，支持"跟随系统".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    system_detection, ColorCard, ColorRole, MotionSettings, MotionTable, MotionToken, Result,
    ThemingError,
};

/// 当前使用的主题模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    /// 使用亮色主题
    #[default]
    Light,
    /// 使用暗色主题
    Dark,
    /// 跟随系统主题
    System,
}

impl ThemeMode {
    /// 返回主题模式的显示名称 (硬编码中文)
    pub fn display_name(&self) -> &'static str {
        match self {
            ThemeMode::Light => "亮色",
            ThemeMode::Dark => "暗色",
            ThemeMode::System => "跟随系统",
        }
    }
}

/// 从 Slint 传递过来的主题设置 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlintThemePayload {
    #[serde(flatten)]
    pub colors: HashMap<String, String>,
    pub reduce_motion: bool,
    pub speed_factor: f64,
}

/// 主题管理器——加载内置色卡和管理当前模式
pub struct ThemeManager {
    /// 亮色和暗色两套色卡
    light_card: ColorCard,
    dark_card: ColorCard,
    /// 动效表
    motion_table: MotionTable,
    /// 运动设置 (reduce_motion + speed_factor)
    motion_settings: MotionSettings,
    /// 当前主题模式
    current_mode: ThemeMode,
}

impl ThemeManager {
    /// 创建新的主题管理器 (使用内置色卡和动效表)
    pub fn built_in() -> Self {
        let light_json = include_str!("assets/light.json");
        let dark_json = include_str!("assets/dark.json");
        let motion_json = include_str!("assets/motion.json");

        Self::from_json_strings(light_json, dark_json, motion_json).expect("应能加载内置主题")
    }

    /// 从 JSON 字符串构建主题管理器
    pub fn from_json_strings(light_json: &str, dark_json: &str, motion_json: &str) -> Result<Self> {
        let light_card: ColorCard = serde_json::from_str(light_json)?;
        let dark_card: ColorCard = serde_json::from_str(dark_json)?;
        let motion_table: MotionTable = serde_json::from_str(motion_json)?;

        // 验证色卡完整性
        light_card.validate().map_err(|_| {
            ThemingError::MissingRequiredColors(vec![
                ColorRole::PrimaryBackground,
                ColorRole::TextPrimary,
                ColorRole::PrimaryForeground,
                ColorRole::AccentColor,
            ])
        })?;

        Ok(Self {
            light_card,
            dark_card,
            motion_table,
            motion_settings: MotionSettings::new(),
            current_mode: ThemeMode::Light,
        })
    }

    /// 获取当前激活的色卡
    pub fn active_colors(&self) -> &ColorCard {
        match self.current_mode {
            ThemeMode::System => match system_detection::detect_system_color_scheme() {
                system_detection::SystemColorScheme::Light => &self.light_card,
                system_detection::SystemColorScheme::Dark => &self.dark_card,
            },
            ThemeMode::Light => &self.light_card,
            ThemeMode::Dark => &self.dark_card,
        }
    }

    /// 获取指定角色名的颜色值
    pub fn get_color(&self, role: ColorRole) -> Option<&str> {
        self.active_colors().get(role)
    }

    /// 获取指定角色的 ARGB 格式颜色值
    pub fn get_color_argb(&self, role: ColorRole) -> Result<u32> {
        let hex = self
            .get_color(role)
            .ok_or(ThemingError::ColorRoleNotFound(format!("{:?})", role)))?;
        ColorCard::parse_hex_to_argb(hex)
    }

    /// 设置当前主题模式
    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.current_mode = mode;
    }

    /// 获取当前主题模式
    pub fn current_mode(&self) -> ThemeMode {
        self.current_mode
    }

    /// 获取动效表
    pub fn motion_table(&self) -> &MotionTable {
        &self.motion_table
    }

    /// 获取运动设置
    pub fn motion_settings(&self) -> &MotionSettings {
        &self.motion_settings
    }

    /// 设置运动设置
    pub fn set_motion_settings(&mut self, settings: MotionSettings) {
        self.motion_settings = settings;
    }

    /// 获取有效时长 (考虑所有设置)
    pub fn effective_duration(&self, token: MotionToken) -> f64 {
        self.motion_table.effective_duration(
            token,
            self.motion_settings.reduce_motion,
            self.motion_settings.speed_factor,
        )
    }

    /// 检查是否启用了任何动画
    pub fn has_any_animation(&self) -> bool {
        self.motion_settings.has_any_animation()
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::built_in()
    }
}
