//! MCRebuild V2 —— 动效表配置模块
//!
//! 集中定义所有动画参数 (ADR-0023 §4),遵循硬约束:
//! - 节奏全部集中于一张全局动效表，代码禁止写死时长数字
//! - "减少动画"开关一键全关
//! - Codex 手感基准：快而淡、不弹跳、安静连贯

use serde::{Deserialize, Serialize};

/// 动效表中预定义的时长类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MotionToken {
    /// 快速动画 (按钮反馈、淡入淡出等，≤0.2 秒)
    Fast,
    /// 中等速度动画 (里程碑时刻，如首次进入方案列表、采集完成、导出完成)
    Medium,
    /// 慢速动画 (一般不使用)
    Slow,
}

/// 单个时长的详细配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DurationConfig {
    /// 持续时间 (秒)
    pub duration: f64,
    /// 缓动曲线类型
    #[serde(default = "default_easing")]
    pub easing: EasingType,
}

/// 缓动曲线类型 (Codex 手感：EaseOut for fast)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EasingType {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

fn default_easing() -> EasingType { EasingType::Linear }

/// 完整的动效表配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionTable {
    /// 快速动画
    pub fast: DurationConfig,
    /// 中等速度动画
    pub medium: DurationConfig,
    /// 慢速动画
    pub slow: DurationConfig,
}

/// "减少动画"设置和速度因子
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionSettings {
    /// "减少动画"开关——一键全关 (ADR-0023 §4)
    #[serde(default = "default_reduce_motion")]
    pub reduce_motion: bool,
    /// 全局速度因子 (1.0=正常，0.5=一半速度，0.2=四分之一速度)
    #[serde(default = "default_speed_factor")]
    pub speed_factor: f64,
}

fn default_reduce_motion() -> bool { false }
fn default_speed_factor() -> f64 { 1.0 }

impl MotionTable {
    /// 获取某个 token 的原始时长 (不考虑 reduce_motion 和 speed_factor)
    pub fn get_duration(&self, token: MotionToken) -> f64 {
        match token {
            MotionToken::Fast => self.fast.duration,
            MotionToken::Medium => self.medium.duration,
            MotionToken::Slow => self.slow.duration,
        }
    }

    /// 获取有效时长 (考虑 reduce_motion 和 speed_factor)
    pub fn effective_duration(&self, token: MotionToken, reduce_motion: bool, speed_factor: f64) -> f64 {
        if reduce_motion { return 0.0; }  // 减少动画模式返回 0
        self.get_duration(token) * speed_factor
    }
}

impl MotionSettings {
    /// 创建默认的运动设置
    pub fn new() -> Self {
        Self { reduce_motion: false, speed_factor: 1.0 }
    }

    /// 检查是否启用了任何动画
    pub fn has_any_animation(&self) -> bool { !self.reduce_motion }
}

impl Default for MotionSettings {
    fn default() -> Self { Self::new() }
}
