//! 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在快照中，PR diff 可见。
//!
//! 简单方式：检查所有公开类型可实例化、关键行为可调用。

use data_persistence::Database;
use localization::{Language, Localization};
use onboarding_tutorial::{
    BubblePlacement, Error, OnboardingTutorial, SettingsEntryView, TutorialBubble,
    TutorialProgress, TutorialStatus, TutorialStep, ALL_STEPS,
};

#[test]
fn public_api_types_exist() {
    // 常量：三个里程碑钩子（T17 预留；扩容归 T19 界面审核）
    assert_eq!(ALL_STEPS.len(), 3);

    // TutorialStep：稳定 ID + tutorial.* 文本键
    assert_eq!(TutorialStep::PlanListIntro.id(), "plan_list_intro");
    assert_eq!(
        TutorialStep::StepperIntro.message_key(),
        "tutorial.step_stepper_intro"
    );

    // BubblePlacement：占位值（定稿归 T19 界面审核）
    let placement = BubblePlacement::placeholder();
    assert_eq!(placement, BubblePlacement::default());
    assert!(placement.width > 0.0);

    // TutorialProgress：状态机走位 + 落库往返
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let mut progress = TutorialProgress::load(&db).unwrap();
    assert_eq!(progress.status(), TutorialStatus::NotStarted);
    assert!(progress.is_first_bubble());
    progress.mark_seen(TutorialStep::PlanListIntro);
    assert!(progress.has_seen(TutorialStep::PlanListIntro));
    assert_eq!(progress.status(), TutorialStatus::InProgress);
    progress.skip_all();
    assert_eq!(progress.status(), TutorialStatus::Completed);
    assert!(progress.completed_at().is_some());
    progress.save(&mut db).unwrap();
    assert!(TutorialProgress::load(&db)
        .unwrap()
        .completed_at()
        .is_some());
    progress.restart();
    assert_eq!(progress.status(), TutorialStatus::NotStarted);
    progress.save(&mut db).unwrap(); // 清零落库，下段从干净状态加载
    let _ = TutorialProgress::new();

    // OnboardingTutorial：气泡索取 / 关泡 / 全跳 / 重看 / 设置页入口
    let l10n = Localization::new(Language::ZhCn).expect("内嵌 zh-CN 资源必定可用");
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();
    let bubble: TutorialBubble = tutorial
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .expect("首泡必在");
    assert_eq!(bubble.step, TutorialStep::PlanListIntro);
    assert!(!bubble.message.is_empty());
    assert!(bubble.skip_all_label.is_some());
    tutorial
        .dismiss(&mut db, TutorialStep::PlanListIntro)
        .unwrap();
    tutorial.skip_all(&mut db, &l10n).unwrap();
    assert_eq!(tutorial.status(), TutorialStatus::Completed);
    tutorial.restart(&mut db).unwrap();
    assert_eq!(tutorial.progress().status(), TutorialStatus::NotStarted);
    let entry: SettingsEntryView = tutorial.settings_entry(&l10n);
    assert!(!entry.replay_label.is_empty());
    let _ = OnboardingTutorial::new();

    // Error（#[non_exhaustive]）：Display 可用
    let err: Error = serde_json::from_str::<serde_json::Value>("not-json")
        .map_err(Error::from)
        .unwrap_err();
    assert!(!err.to_string().is_empty());
}
