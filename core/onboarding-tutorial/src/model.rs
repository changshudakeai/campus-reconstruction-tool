//! 里程碑提示点与教程状态机（ADR-0020 第一节）
//!
//! 状态机：`NotStarted → InProgress → Completed`；"重新查看教程"把状态
//! 拨回 `NotStarted`（跳过可逆，规矩④）。
//!
//! 提示点清单已按 ADR-0028 拍板为三泡（首进列表·步骤条亮相·评审亮相）——
//! F2 `TutorialStep` 枚举对应改造完成。

/// 里程碑提示点（ADR-0028：三泡清单——拆除旧两泡 + 新增两泡）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TutorialStep {
    /// 首次进入方案列表页（气泡：点这里新建方案）【保留现有】
    PlanListIntro,
    /// 步骤条首次亮相时的气泡（ADR-0028：顶上这五格就是全部流程）【新增】
    StepperIntro,
    /// 评审步骤页首次亮相时的气泡（ADR-0028：每条候选给个态度）【新增】
    ReviewIntro,
}

impl TutorialStep {
    /// 提示点稳定标识符（引导进度存储用，与界面文案无关）
    pub fn id(&self) -> &'static str {
        match self {
            Self::PlanListIntro => "plan_list_intro",
            Self::StepperIntro => "stepper_intro",
            Self::ReviewIntro => "review_intro",
        }
    }

    /// 气泡文案的文本键（`tutorial.*`，由 B6 解析；文案定稿归 T19 界面审核）
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::PlanListIntro => "tutorial.step_plan_list",
            Self::StepperIntro => "tutorial.step_stepper_intro",
            Self::ReviewIntro => "tutorial.step_review_intro",
        }
    }
}

/// 全部提示点，顺序即任务链先后（ADR-0028：三泡清单）
pub const ALL_STEPS: [TutorialStep; 3] = [
    TutorialStep::PlanListIntro,
    TutorialStep::StepperIntro,
    TutorialStep::ReviewIntro,
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
