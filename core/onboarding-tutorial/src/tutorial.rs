//! 气泡编排与四条规矩（ADR-0020 第二节）+ 设置页入口
//!
//! 窗口契约：壳向 F2 要当前步骤的气泡位置和文案（[`OnboardingTutorial::bubble_for`]），
//! F2 读取 B2 app_settings 中的引导进度决定给不给。气泡是纯数据 ViewModel，
//! 零 slint，呈现由壳负责；与各页面/设置页的实际接线归 T19。
//!
//! **位置只做占位，不定稿**：[`BubblePlacement`] 的数值待界面成型后由
//! 产品负责人在开发版审核时敲定（ADR-0020 第三节）。

use data_persistence::AppSettingsApi;
use localization::Localization;

use crate::error::Result;
use crate::model::{TutorialStatus, TutorialStep};
use crate::progress::TutorialProgress;

/// 气泡位置（逻辑像素）。
///
/// ⚠️ 占位值，不是定稿：具体坐标与宽度待 T19 界面成型后的开发版审核
/// 时逐泡敲定（ADR-0020：避免对着还不存在的界面空谈位置）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BubblePlacement {
    /// 气泡左上角横坐标
    pub x: f32,
    /// 气泡左上角纵坐标
    pub y: f32,
    /// 气泡宽度
    pub width: f32,
}

impl BubblePlacement {
    /// 统一占位位置（界面审核前所有气泡共用）
    pub const fn placeholder() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 280.0,
        }
    }
}

impl Default for BubblePlacement {
    fn default() -> Self {
        Self::placeholder()
    }
}

/// 气泡 ViewModel（纯数据，壳绑定呈现）
#[derive(Debug, Clone, PartialEq)]
pub struct TutorialBubble {
    /// 所属提示点
    pub step: TutorialStep,
    /// 气泡位置（占位，定稿归 T19 界面审核）
    pub placement: BubblePlacement,
    /// 文案的文本键（`tutorial.*`，供壳追溯来源）
    pub message_key: &'static str,
    /// 成品文案（B6 解析后）
    pub message: String,
    /// "知道了"按钮文字（规矩①：每泡可关）
    pub dismiss_label: String,
    /// "跳过全部引导"按钮文字——仅第一个气泡附带（规矩②），其余为 None
    pub skip_all_label: Option<String>,
}

/// 设置页"重新查看教程"入口的视图数据（规矩④，按钮接线归 T19）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEntryView {
    /// 按钮文字（"重新查看教程"）
    pub replay_label: String,
}

/// F2 教程编排入口 —— 壳的唯一对话对象
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnboardingTutorial {
    progress: TutorialProgress,
}

impl OnboardingTutorial {
    /// 创建空进度的教程（测试/首次使用）
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 B2 加载引导进度（应用启动时壳调用一次）
    pub fn load(db: &impl AppSettingsApi) -> Result<Self> {
        Ok(Self {
            progress: TutorialProgress::load(db)?,
        })
    }

    /// 当前状态机位置
    pub fn status(&self) -> TutorialStatus {
        self.progress.status()
    }

    /// 当前引导进度（只读；设置页可据 `completed_at` 展示完成时刻）
    pub fn progress(&self) -> &TutorialProgress {
        &self.progress
    }

    /// 壳到达某提示点时索取气泡。
    ///
    /// 规矩③"只教一次"：该提示点已见过、或引导已完成（含一键全跳）
    /// 时返回 None——界面保持安静。第一个气泡附"跳过全部引导"（规矩②）。
    pub fn bubble_for(&self, step: TutorialStep, l10n: &Localization) -> Option<TutorialBubble> {
        if self.status() == TutorialStatus::Completed || self.progress.has_seen(step) {
            return None;
        }
        let skip_all_label = self
            .progress
            .is_first_bubble()
            .then(|| l10n.t("tutorial.skip_all_button"));
        Some(TutorialBubble {
            step,
            placement: BubblePlacement::placeholder(),
            message_key: step.message_key(),
            message: l10n.t(step.message_key()),
            dismiss_label: l10n.t("tutorial.dismiss_button"),
            skip_all_label,
        })
    }

    /// 规矩①"每泡可关"：用户点"知道了"，该提示点记为已见并落库，
    /// 此后不再显示；看完全部提示点即转 `Completed`。
    pub fn dismiss(&mut self, db: &mut impl AppSettingsApi, step: TutorialStep) -> Result<()> {
        self.progress.mark_seen(step);
        self.progress.save(db)
    }

    /// 规矩②"一键全跳"：直接转 `Completed`，所有气泡永不再出现；
    /// 经 B7 info 级留底"设置里可重看"（不打扰，仅进公告栏）。
    pub fn skip_all(&mut self, db: &mut impl AppSettingsApi, l10n: &Localization) -> Result<()> {
        self.progress.skip_all();
        self.progress.save(db)?;
        notification_center::info(
            l10n.t("tutorial.source_tag"),
            l10n.t("tutorial.skip_notice_title"),
            l10n.t("tutorial.skip_notice_body"),
        );
        Ok(())
    }

    /// 规矩④"可以重看"：设置页"重新查看教程"按钮的落点——进度清零
    /// 落库，回到 `NotStarted`，气泡将再次出现。
    pub fn restart(&mut self, db: &mut impl AppSettingsApi) -> Result<()> {
        self.progress.restart();
        self.progress.save(db)
    }

    /// 设置页入口视图（按钮由 T19 接线到 [`restart`](Self::restart)）
    pub fn settings_entry(&self, l10n: &Localization) -> SettingsEntryView {
        SettingsEntryView {
            replay_label: l10n.t("tutorial.replay_button"),
        }
    }
}
