//! MCRebuild V2 —— 颜色色卡模块
//!
//! 定义颜色角色枚举和色卡数据结构，遵循 ADR-0023 色卡硬约束。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ThemingError;

/// 所有颜色角色的统一命名 (ADR-0023)
///
/// 代码中永远不写颜色号，只写这些角色名;切主题就是换色卡.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorRole {
    /// 主背景色 (如窗口背景)
    PrimaryBackground,
    /// 次背景色 (如面板背景)
    SecondaryBackground,
    /// 第三背景色 (如卡片背景)
    TertiaryBackground,
    /// 覆盖层背景 (如弹窗、菜单)
    OverlayBackground,
    /// 主文本色 (如标题、正文)
    TextPrimary,
    /// 次要文本色 (如辅助说明)
    TextSecondary,
    /// 第三文本色 (如禁用文本)
    TextTertiary,
    /// 强调文本色 (如链接、高亮)
    TextAccent,
    /// 主前景色 (如按钮主体)
    PrimaryForeground,
    /// 次前景色 (如图标、边框)
    SecondaryForeground,
    /// 强调色 (如选中状态、活跃指示)
    AccentColor,
    /// 成功色 (如完成标记)
    SuccessColor,
    /// 警告色 (如注意提示)
    WarningColor,
    /// 错误色 (如删除标记)
    ErrorColor,
    /// 信息色 (如提示信息)
    InfoColor,
    /// 边框色
    BorderColor,
    /// 阴影色
    ShadowColor,
    /// 高亮色 (如搜索匹配)
    HighlightColor,
    /// 选择色 (如选中文本)
    SelectionColor,
}

impl ColorRole {
    /// 返回该颜色角色在 Slint theme 中的属性名
    pub fn slint_property_name(&self) -> String {
        // 使用 serde 序列化然后转为 kebab-case(不带引号)
        format!("--{}", serde_json::to_string(self).unwrap().trim_matches('"').replace('_', "-"))
    }

    /// 验证色卡是否包含所有必需的角色
    pub fn validate_card(colors: &HashMap<Self, String>) -> Result<(), Vec<ColorRole>> {
        let required = [
            ColorRole::PrimaryBackground,
            ColorRole::TextPrimary,
            ColorRole::PrimaryForeground,
            ColorRole::AccentColor,
        ];
        missing_roles(colors, &required)
    }
}

fn missing_roles(colors: &HashMap<ColorRole, String>, required: &[ColorRole]) -> Result<(), Vec<ColorRole>> {
    let missing: Vec<_> = required.iter()
        .filter(|r| !colors.contains_key(*r))
        .copied()
        .collect();
    if missing.is_empty() { Ok(()) } else { Err(missing) }
}

/// 单张色卡数据——一个主题的所有颜色值
///
/// 每个主题对应一张独立的 JSON 配置文件，包含所有颜色角色的 hex 值.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorCard {
    /// 色卡名称 (human-readable)
    pub name: String,
    /// 颜色角色名 → hex 颜色值的映射
    #[serde(flatten)]
    pub colors: HashMap<ColorRole, String>,
}

impl ColorCard {
    /// 获取指定角色名的颜色值 (hex 字符串)
    pub fn get(&self, role: ColorRole) -> Option<&str> {
        self.colors.get(&role).map(|s| s.as_str())
    }

    /// 获取指定角色名的颜色值，如果缺失则 panic(用于内部调用)
    pub fn get_required(&self, role: ColorRole) -> Result<&str, ThemingError> {
        self.get(role).ok_or(ThemingError::ColorRoleNotFound(format!("{:?})", role)))
    }

    /// 验证色卡完整性
    pub fn validate(&self) -> Result<(), Vec<ColorRole>> {
        ColorRole::validate_card(&self.colors)
    }

    /// 从 hex 字符串解析为 u32 ARGB 格式
    pub fn parse_hex_to_argb(hex: &str) -> Result<u32, ThemingError> {
        let hex = hex.trim_start_matches('#');
        match hex.len() {
            6 => u32::from_str_radix(hex, 16)
                .map(|rgb| (0xFF << 24) | rgb)  // 默认完全不透明
                .map_err(|_| ThemingError::InvalidColorValue { value: hex.to_string() }),
            8 => u32::from_str_radix(hex, 16)
                .map_err(|_| ThemingError::InvalidColorValue { value: hex.to_string() }),
            _ => Err(ThemingError::InvalidColorValue { value: hex.to_string() }),
        }
    }
}
