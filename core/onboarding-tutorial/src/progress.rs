//! 引导进度的应用级持久化（ADR-0020：状态为应用级，不分方案）
//!
//! 持久化到 B2 app_settings 两个键：
//! - `onboarding_progress`：`{ "seen": [提示点 ID], "skipped_all": bool }` 的 JSON；
//! - `onboarding_completed_at`：完成时刻（RFC3339 毫秒文本；空字符串 = 未完成，
//!   因 `AppSettingsApi` 无删除键接口，重看时以空字符串清空）。
//!
//! 只经 `AppSettingsApi` 公开 trait 读写，不触碰 SQL。
//! "只教一次"（规矩③）的根据就在这里：进度是应用级的，
//! 建第二个方案时 `seen` 里已有记录，全程安静。

use std::collections::BTreeSet;

use chrono::{SecondsFormat, Utc};
use data_persistence::{AppSettingKey, AppSettingsApi};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::{TutorialStatus, TutorialStep, ALL_STEPS};

/// `onboarding_progress` 键内的 JSON 结构
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredProgress {
    /// 已见过的提示点稳定 ID（见 [`TutorialStep::id`]）
    seen: BTreeSet<String>,
    /// 是否一键全跳（规矩②）
    skipped_all: bool,
}

/// 引导进度 —— 应用级状态（已见过哪些提示点、是否全跳、完成时刻）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TutorialProgress {
    seen: BTreeSet<String>,
    skipped_all: bool,
    completed_at: Option<String>,
}

impl TutorialProgress {
    /// 创建空进度（首次使用 / "重新查看教程"之后的初始态）
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 B2 加载引导进度（无记录时返回空进度）
    pub fn load(db: &impl AppSettingsApi) -> Result<Self> {
        let stored: StoredProgress = match db.get_setting(AppSettingKey::OnboardingProgress)? {
            Some(json) => serde_json::from_str(&json)?,
            None => StoredProgress::default(),
        };
        let completed_at = db
            .get_setting(AppSettingKey::OnboardingCompletedAt)?
            .filter(|text| !text.is_empty());
        Ok(Self {
            seen: stored.seen,
            skipped_all: stored.skipped_all,
            completed_at,
        })
    }

    /// 把当前进度写回 B2（两个键一并覆盖）
    pub fn save(&self, db: &mut impl AppSettingsApi) -> Result<()> {
        let stored = StoredProgress {
            seen: self.seen.clone(),
            skipped_all: self.skipped_all,
        };
        let json = serde_json::to_string(&stored)?;
        db.set_setting(AppSettingKey::OnboardingProgress, &json)?;
        db.set_setting(
            AppSettingKey::OnboardingCompletedAt,
            self.completed_at.as_deref().unwrap_or(""),
        )?;
        Ok(())
    }

    /// 当前状态机位置
    pub fn status(&self) -> TutorialStatus {
        if self.skipped_all || ALL_STEPS.iter().all(|step| self.has_seen(*step)) {
            TutorialStatus::Completed
        } else if self.seen.is_empty() {
            TutorialStatus::NotStarted
        } else {
            TutorialStatus::InProgress
        }
    }

    /// 某提示点是否已见过（规矩③"只教一次"的判定依据）
    pub fn has_seen(&self, step: TutorialStep) -> bool {
        self.seen.contains(step.id())
    }

    /// 是否还没见过任何气泡——第一个气泡才附"跳过全部引导"（规矩②）
    pub fn is_first_bubble(&self) -> bool {
        self.seen.is_empty() && !self.skipped_all
    }

    /// 记录提示点已见（规矩①"每泡可关"落点）；看完最后一个提示点时
    /// 盖上完成时刻章
    pub fn mark_seen(&mut self, step: TutorialStep) {
        self.seen.insert(step.id().to_owned());
        if self.status() == TutorialStatus::Completed {
            self.stamp_completed();
        }
    }

    /// 一键全跳（规矩②）：所有气泡永不再出现，直接盖完成章
    pub fn skip_all(&mut self) {
        self.skipped_all = true;
        self.stamp_completed();
    }

    /// 重新查看教程（规矩④）：进度清零、完成章作废，回到 `NotStarted`
    pub fn restart(&mut self) {
        self.seen.clear();
        self.skipped_all = false;
        self.completed_at = None;
    }

    /// 完成时刻（RFC3339 毫秒文本；未完成时为 None）
    pub fn completed_at(&self) -> Option<&str> {
        self.completed_at.as_deref()
    }

    /// 盖完成时刻章（幂等：已有时刻不覆盖）
    fn stamp_completed(&mut self) {
        if self.completed_at.is_none() {
            self.completed_at = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_persistence::Database;

    #[test]
    fn status_walks_not_started_to_completed() {
        let mut progress = TutorialProgress::new();
        assert_eq!(progress.status(), TutorialStatus::NotStarted);
        assert!(progress.completed_at().is_none());

        progress.mark_seen(TutorialStep::PlanListIntro);
        assert_eq!(progress.status(), TutorialStatus::InProgress);
        assert!(progress.completed_at().is_none());

        progress.mark_seen(TutorialStep::CollectionCompleted);
        progress.mark_seen(TutorialStep::ExportCompleted);
        assert_eq!(progress.status(), TutorialStatus::Completed);
        assert!(progress.completed_at().is_some(), "看完最后一泡即盖完成章");
    }

    #[test]
    fn skip_all_completes_immediately() {
        let mut progress = TutorialProgress::new();
        progress.skip_all();
        assert_eq!(progress.status(), TutorialStatus::Completed);
        assert!(progress.completed_at().is_some());
        assert!(!progress.is_first_bubble());
    }

    #[test]
    fn restart_returns_to_not_started() {
        let mut progress = TutorialProgress::new();
        progress.skip_all();
        progress.restart();
        assert_eq!(progress.status(), TutorialStatus::NotStarted);
        assert!(progress.completed_at().is_none());
        assert!(progress.is_first_bubble());
    }

    #[test]
    fn progress_roundtrips_through_app_settings() {
        let mut db = Database::open_in_memory().expect("内存库可打开");
        let mut progress = TutorialProgress::load(&db).unwrap();
        assert_eq!(progress.status(), TutorialStatus::NotStarted);

        progress.mark_seen(TutorialStep::PlanListIntro);
        progress.save(&mut db).unwrap();

        // 重新加载（模拟重启应用）：进度仍被记住
        let reloaded = TutorialProgress::load(&db).unwrap();
        assert!(reloaded.has_seen(TutorialStep::PlanListIntro));
        assert_eq!(reloaded.status(), TutorialStatus::InProgress);
    }

    #[test]
    fn restart_clears_stored_completed_at() {
        let mut db = Database::open_in_memory().expect("内存库可打开");
        let mut progress = TutorialProgress::new();
        progress.skip_all();
        progress.save(&mut db).unwrap();
        assert!(TutorialProgress::load(&db)
            .unwrap()
            .completed_at()
            .is_some());

        // 重看：完成章作废并落库（空字符串 = 未完成）
        progress.restart();
        progress.save(&mut db).unwrap();
        let reloaded = TutorialProgress::load(&db).unwrap();
        assert!(reloaded.completed_at().is_none());
        assert_eq!(reloaded.status(), TutorialStatus::NotStarted);
    }
}
