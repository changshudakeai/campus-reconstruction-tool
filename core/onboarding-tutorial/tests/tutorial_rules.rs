//! 四条规矩的行为测试（ADR-0020 第二节）
//!
//! 逐条验证：① 每泡可关 ② 一键全跳 ③ 只教一次 ④ 设置可重看，
//! 以及三个里程碑钩子与状态机的走位。

use data_persistence::Database;
use localization::{Language, Localization};
use onboarding_tutorial::{OnboardingTutorial, TutorialStatus, TutorialStep, ALL_STEPS};

fn l10n() -> Localization {
    Localization::new(Language::ZhCn).expect("内嵌 zh-CN 资源必定可用")
}

#[test]
fn rule_1_dismiss_closes_bubble_and_persists() {
    // 规矩①：气泡上有"知道了"，点击即消失，且重启后不再出现
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let l10n = l10n();
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();

    let bubble = tutorial
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .expect("首次到达提示点必有气泡");
    assert_eq!(bubble.dismiss_label, "知道了");
    assert!(!bubble.message.is_empty());
    assert_eq!(bubble.message_key, "tutorial.step_plan_list");

    tutorial
        .dismiss(&mut db, TutorialStep::PlanListIntro)
        .unwrap();
    assert!(tutorial
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .is_none());

    // 重启应用：该泡仍不再出现，其余提示点照常
    let reloaded = OnboardingTutorial::load(&db).unwrap();
    assert!(reloaded
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .is_none());
    assert!(reloaded
        .bubble_for(TutorialStep::StepperIntro, &l10n)
        .is_some());
}

#[test]
fn rule_2_skip_all_silences_everything_forever() {
    // 规矩②：第一个气泡附"跳过全部引导"；选择后所有气泡永不再出现
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let l10n = l10n();
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();

    let first = tutorial
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .unwrap();
    assert_eq!(first.skip_all_label.as_deref(), Some("跳过全部引导"));

    tutorial.skip_all(&mut db, &l10n).unwrap();
    assert_eq!(tutorial.status(), TutorialStatus::Completed);
    assert!(tutorial.progress().completed_at().is_some());
    for step in ALL_STEPS {
        assert!(tutorial.bubble_for(step, &l10n).is_none());
    }

    // 重启应用：仍然全程安静
    let reloaded = OnboardingTutorial::load(&db).unwrap();
    assert_eq!(reloaded.status(), TutorialStatus::Completed);
    for step in ALL_STEPS {
        assert!(reloaded.bubble_for(step, &l10n).is_none());
    }
}

#[test]
fn rule_2_skip_all_option_only_on_first_bubble() {
    // "跳过全部引导"只随第一个气泡出现；关过一泡后不再附带
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let l10n = l10n();
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();

    tutorial
        .dismiss(&mut db, TutorialStep::PlanListIntro)
        .unwrap();
    let second = tutorial
        .bubble_for(TutorialStep::StepperIntro, &l10n)
        .unwrap();
    assert!(second.skip_all_label.is_none());
}

#[test]
fn rule_3_each_step_teaches_only_once() {
    // 规矩③：三个提示点逐个走完，全程只教一次；建第二个方案时全程安静
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let l10n = l10n();
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();
    assert_eq!(tutorial.status(), TutorialStatus::NotStarted);

    for step in ALL_STEPS {
        assert!(tutorial.bubble_for(step, &l10n).is_some());
        tutorial.dismiss(&mut db, step).unwrap();
        assert!(tutorial.bubble_for(step, &l10n).is_none());
    }
    assert_eq!(tutorial.status(), TutorialStatus::Completed);
    assert!(
        tutorial.progress().completed_at().is_some(),
        "看完最后一泡即盖 onboarding_completed_at 章"
    );

    // 建第二个方案（重新加载模拟新流程）：全程安静
    let second_plan = OnboardingTutorial::load(&db).unwrap();
    for step in ALL_STEPS {
        assert!(second_plan.bubble_for(step, &l10n).is_none());
    }
}

#[test]
fn rule_4_replay_from_settings_is_reversible() {
    // 规矩④：设置里"重新查看教程"后气泡再次出现——跳过可逆
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let l10n = l10n();
    let mut tutorial = OnboardingTutorial::load(&db).unwrap();

    // 设置页入口文字备好（按钮接线归 T19）
    assert_eq!(tutorial.settings_entry(&l10n).replay_label, "重新查看教程");

    tutorial.skip_all(&mut db, &l10n).unwrap();
    tutorial.restart(&mut db).unwrap();
    assert_eq!(tutorial.status(), TutorialStatus::NotStarted);

    // 重启应用：重看生效且完成章已作废
    let reloaded = OnboardingTutorial::load(&db).unwrap();
    assert_eq!(reloaded.status(), TutorialStatus::NotStarted);
    assert!(reloaded.progress().completed_at().is_none());
    let bubble = reloaded
        .bubble_for(TutorialStep::PlanListIntro, &l10n)
        .expect("重看后气泡再次出现");
    assert!(bubble.skip_all_label.is_some(), "重看后的第一泡照旧可全跳");
}

#[test]
fn milestone_hooks_cover_ticket_reserved_points() {
    // T19B-5A 改造：三泡清单（首进列表·步骤条亮相·评审亮相）
    assert_eq!(
        ALL_STEPS,
        [
            TutorialStep::PlanListIntro,
            TutorialStep::StepperIntro,
            TutorialStep::ReviewIntro,
        ]
    );
}
