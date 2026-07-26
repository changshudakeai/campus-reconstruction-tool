//! 采集任务状态
//!
//! 每次采集作业的生命周期状态标记

/// 采集任务状态
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionJobStatus {
    /// 待执行：已创建但未启动
    Pending,
    /// 进行中：正在采集数据
    InProgress,
    /// 已完成：采集成功结束
    Completed,
    /// 失败：采集过程中出错
    Failed,
}

impl CollectionJobStatus {
    /// 返回中文显示名
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Pending => "待执行",
            Self::InProgress => "进行中",
            Self::Completed => "已完成",
            Self::Failed => "失败",
        }
    }

    /// 判断是否已完成（包含成功与失败）
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }

    /// 判断是否成功完成
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed)
    }
}
