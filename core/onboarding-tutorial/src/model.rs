//! 里程碑提示点与教程状态机（ADR-0020 第一节）
//!
//! 状态机：`NotStarted → InProgress → Completed`；"重新查看教程"把状态
//! 拨回 `NotStarted`（跳过可逆，规矩④）。
//!
//! 提示点清单**不在本阶段固化**（ADR-0020 后果条：避免对着还不存在的
//! 界面空谈位置）——此处只预留 T17 工单指定的三个里程碑钩子，
//! 完整清单待界面成型后的开发版审核时扩容（T19）。

/// 里程碑提示点（预留钩子，界面审核后扩容）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TutorialStep {
    /// 首次进入方案列表页（气泡：点这里新建方案）
    PlanListIntro,
    /// 首次完成数据采集（气泡：下一步去评审工作台）
    CollectionCompleted,
    /// 首次完成导出（气泡：去导出清单看成果）
    ExportCompleted,
}

impl TutorialStep {
    /// 提示点稳定标识符（引导进度存储用，与界面文案无关）
    pub fn id(&self) -> &'static str {
        match self {
            Self::PlanListIntro => "plan_list_intro",
            Self::CollectionCompleted => "collection_completed",
            Self::ExportCompleted => "export_completed",
        }
    }

    /// 气泡文案的文本键（`tutorial.*`，由 B6 解析；文案定稿归 T19 界面审核）
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::PlanListIntro => "tutorial.step_plan_list",
            Self::CollectionCompleted => "tutorial.step_collection_done",
            Self::ExportCompleted => "tutorial.step_export_done",
        }
    }
}

/// 全部提示点，顺序即任务链先后（首进方案列表 → 采集完成 → 导出完成）
pub const ALL_STEPS: [TutorialStep; 3] = [
    TutorialStep::PlanListIntro,
    TutorialStep::CollectionCompleted,
    TutorialStep::ExportCompleted,
];

/// 教程状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TutorialStatus {
    /// 尚未见过任何气泡（首次使用或"重新查看教程"之后）
    NotStarted,
    /// 部分提示点已见过
    InProgress,
    /// 全部提示点已见过，或用户一键全跳
    Completed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_ids_are_unique() {
        for (i, a) in ALL_STEPS.iter().enumerate() {
            for b in &ALL_STEPS[i + 1..] {
                assert_ne!(a.id(), b.id());
                assert_ne!(a.message_key(), b.message_key());
            }
        }
    }

    #[test]
    fn message_keys_use_tutorial_prefix() {
        for step in ALL_STEPS {
            assert!(step.message_key().starts_with("tutorial."));
        }
    }
}
