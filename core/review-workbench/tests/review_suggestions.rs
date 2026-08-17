//! F5 评审工作台的轻量建议、筛选、应用与撤销集成测试。

mod common;

use common::{candidate_key_part, write_raw_observation};
use data_persistence::{
    CandidateDisplay, CandidateNameSource, CandidateProjectionDraft, CandidateProjectionsApi,
    CandidateShape, CandidateSourceIdentity, Database, ReviewableValidation,
};
use review_workbench::{
    CandidateKey, CommandOutcome, Error, ReviewWorkbench, SuggestFilter, SuggestionAction,
    SuggestionCategory,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

/// 建议夹具：混合信号的已发布 Reviewable 候选。
///
/// - b1 教学楼甲（干净）→ 建议保留
/// - b2 教学楼乙（干净、独立位置）→ 建议保留
/// - b3 教学楼甲（与 b1 同来源实体 + 同几何 = 重复投影）→ 建议剔除
/// - b4 未命名建筑 → 建议人工确认（未命名）
/// - b5 实验楼（几何自动修复）→ 建议人工确认（形状经修复）
/// - r1 道路（未命名）→ 建议人工确认（未命名）
/// - w1 游泳池（干净水体）→ 建议保留
fn suggestion_fixture() -> (Database, PlanId) {
    let mut db = Database::open_in_memory().expect("内存库");
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();
    for (entity_id, category, title) in [
        ("way/b1", CandidateCategory::Building, "教学楼甲"),
        ("way/b2", CandidateCategory::Building, "教学楼乙"),
        ("way/b4", CandidateCategory::Building, "way/b4"),
        ("way/b5", CandidateCategory::Building, "实验楼"),
        ("way/r1", CandidateCategory::Road, "way/r1"),
        ("way/w1", CandidateCategory::Water, "游泳池"),
    ] {
        write_raw_observation(&mut db, &plan_id, entity_id, title, category);
    }

    fn ring(offset: f64) -> serde_json::Value {
        serde_json::json!([
            [121.4 + offset, 31.2],
            [121.5 + offset, 31.2],
            [121.5 + offset, 31.3],
            [121.4 + offset, 31.3],
            [121.4 + offset, 31.2]
        ])
    }

    let mut projections = Vec::new();
    let mut push = |source_entity_id: &str,
                    geometry_part_id: &str,
                    category: CandidateCategory,
                    title: &str,
                    tags: Vec<(String, String)>,
                    shape: CandidateShape,
                    validation: ReviewableValidation,
                    name_source: CandidateNameSource| {
        projections.push(
            CandidateProjectionDraft::reviewable(
                CandidateSourceIdentity::new("overpass", source_entity_id, geometry_part_id),
                category,
                CandidateDisplay::new(title, tags),
                shape,
                validation,
            )
            .with_name_source(name_source),
        );
    };

    push(
        "way/b1",
        "outer",
        CandidateCategory::Building,
        "教学楼甲",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.0)),
        ReviewableValidation::Retained,
        CandidateNameSource::Osm,
    );
    push(
        "way/b2",
        "outer",
        CandidateCategory::Building,
        "教学楼乙",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.2)),
        ReviewableValidation::Retained,
        CandidateNameSource::Osm,
    );
    push(
        "way/b1",
        "duplicate",
        CandidateCategory::Building,
        "教学楼甲",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.0)),
        ReviewableValidation::Retained,
        CandidateNameSource::Osm,
    );
    push(
        "way/b4",
        "outer",
        CandidateCategory::Building,
        "way/b4",
        Vec::new(),
        CandidateShape::polygon(ring(0.4)),
        ReviewableValidation::Retained,
        CandidateNameSource::Unnamed,
    );
    push(
        "way/b5",
        "outer",
        CandidateCategory::Building,
        "实验楼",
        vec![("building".to_owned(), "lab".to_owned())],
        CandidateShape::polygon(ring(0.6)),
        ReviewableValidation::Repaired,
        CandidateNameSource::Osm,
    );
    push(
        "way/r1",
        "outer",
        CandidateCategory::Road,
        "way/r1",
        vec![("highway".to_owned(), "footway".to_owned())],
        CandidateShape::line_string(serde_json::json!([[121.4, 31.1], [121.6, 31.1]])),
        ReviewableValidation::Retained,
        CandidateNameSource::Unnamed,
    );
    push(
        "way/w1",
        "outer",
        CandidateCategory::Water,
        "游泳池",
        vec![("leisure".to_owned(), "swimming_pool".to_owned())],
        CandidateShape::polygon(ring(0.8)),
        ReviewableValidation::Retained,
        CandidateNameSource::Osm,
    );

    db.publish_candidate_batch(&plan_key, "suggestion-boundary", &projections)
        .expect("建议夹具批次发布");
    (db, plan_id)
}

fn suggestion_key(
    db: &Database,
    plan_id: &PlanId,
    source_entity_id: &str,
    geometry_part_id: &str,
) -> CandidateKey {
    candidate_key_part(db, plan_id, source_entity_id, Some(geometry_part_id))
}

fn visible_card_keys(workbench: &ReviewWorkbench) -> Vec<CandidateKey> {
    workbench
        .view()
        .cards
        .into_iter()
        .map(|card| CandidateKey::new(card.candidate_id))
        .collect()
}

#[test]
fn generating_suggestions_never_changes_review_state() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    let before: Vec<(CandidateKey, ReviewState)> = workbench
        .suggestions()
        .iter()
        .map(|(key, _)| ((*key).clone(), workbench.state_of(key).unwrap()))
        .collect();

    for (key, suggestion) in workbench.suggestions() {
        assert!(workbench.suggestion_of(key).is_some());
        assert!(!suggestion.reason_key.is_empty());
    }
    for filter in SuggestFilter::ALL {
        let _ = workbench.suggestion_filter_count(filter);
        workbench.toggle_suggestion_filter(filter);
    }
    let _ = workbench.view();

    let after: Vec<(CandidateKey, ReviewState)> = workbench
        .suggestions()
        .iter()
        .map(|(key, _)| ((*key).clone(), workbench.state_of(key).unwrap()))
        .collect();
    assert_eq!(
        before, after,
        "仅生成建议/筛选不得改变 ReviewState（验收 5）"
    );
    assert_eq!(workbench.export_summary().pending_count, 7);
}

#[test]
fn suggestions_are_deterministic_across_loads() {
    let (db, plan_id) = suggestion_fixture();
    let first = ReviewWorkbench::load(&db, &plan_id).unwrap();
    let second = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(first.suggestions(), second.suggestions());
}

#[test]
fn suggestion_rules_cover_all_required_categories_with_readable_reasons() {
    let (db, plan_id) = suggestion_fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    let by_id: std::collections::HashMap<_, _> = workbench
        .suggestions()
        .into_iter()
        .map(|(key, suggestion)| (key.candidate_id.clone(), suggestion))
        .collect();

    for key in [
        suggestion_key(&db, &plan_id, "way/b4", "outer"),
        suggestion_key(&db, &plan_id, "way/r1", "outer"),
    ] {
        let suggestion = &by_id[&key.candidate_id];
        assert_eq!(suggestion.category, SuggestionCategory::Unnamed);
        assert_eq!(suggestion.action, SuggestionAction::HumanReview);
    }
    let duplicate_pair = [
        suggestion_key(&db, &plan_id, "way/b1", "outer"),
        suggestion_key(&db, &plan_id, "way/b1", "duplicate"),
    ];
    let pair_suggestions: Vec<_> = duplicate_pair
        .iter()
        .map(|key| &by_id[&key.candidate_id])
        .collect();
    assert_eq!(
        pair_suggestions
            .iter()
            .filter(|suggestion| suggestion.action == SuggestionAction::Remove)
            .count(),
        1
    );
    assert_eq!(
        pair_suggestions
            .iter()
            .filter(|suggestion| suggestion.action == SuggestionAction::Keep)
            .count(),
        1
    );
    let duplicate = pair_suggestions
        .iter()
        .find(|suggestion| suggestion.action == SuggestionAction::Remove)
        .expect("重复对中有一个建议剔除");
    assert_eq!(duplicate.category, SuggestionCategory::NeedsAttention);
    let repaired_key = suggestion_key(&db, &plan_id, "way/b5", "outer");
    let repaired = &by_id[&repaired_key.candidate_id];
    assert_eq!(repaired.category, SuggestionCategory::NeedsAttention);
    assert_eq!(repaired.action, SuggestionAction::HumanReview);
    for key in [
        suggestion_key(&db, &plan_id, "way/b2", "outer"),
        suggestion_key(&db, &plan_id, "way/w1", "outer"),
    ] {
        let suggestion = &by_id[&key.candidate_id];
        assert_eq!(suggestion.category, SuggestionCategory::NoActionNeeded);
        assert_eq!(suggestion.action, SuggestionAction::Keep);
    }
    for (_, suggestion) in by_id {
        assert!(!suggestion.reason_key.is_empty());
        assert!(!suggestion.summary_key.is_empty());
    }
}

#[test]
fn suggestion_filters_combine_with_category_and_count_accurately() {
    let (db, plan_id) = suggestion_fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    assert_eq!(workbench.suggestion_filter_count(SuggestFilter::Unnamed), 2);
    assert_eq!(
        workbench.suggestion_filter_count(SuggestFilter::SuggestKeep),
        3
    );
    assert_eq!(
        workbench.suggestion_filter_count(SuggestFilter::SuggestHumanReview),
        3
    );
    assert_eq!(
        workbench.suggestion_filter_count(SuggestFilter::SuggestRemove),
        1
    );
    assert_eq!(
        workbench.suggestion_filter_count(SuggestFilter::NeedsAttention),
        2
    );

    let view = workbench.view();
    assert_eq!(view.suggestion_filters.len(), 5);
    let keep_tab = view
        .suggestion_filters
        .iter()
        .find(|tab| tab.filter == SuggestFilter::SuggestKeep)
        .expect("建议保留标签存在");
    assert_eq!(keep_tab.count, 3);
    assert!(!keep_tab.active);

    let mut workbench = workbench;
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestKeep);
    let view = workbench.view();
    assert!(view.apply_suggestions_enabled);
    assert_eq!(
        view.cards
            .iter()
            .filter(|card| card.suggestion.is_some())
            .count(),
        2,
        "建筑类别 + 建议保留筛选组合后只显示建议保留的候选"
    );
    assert!(view.cards.iter().all(|card| card
        .suggestion
        .as_ref()
        .is_some_and(|s| s.action_key == "review.suggestion_action_keep")));
}

#[test]
fn apply_suggestions_confirmation_cancel_leaves_state_untouched() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestKeep);

    let outcome = workbench.apply_suggestions().unwrap();
    let CommandOutcome::NeedsSuggestionConfirmation(request) = outcome else {
        panic!("一键应用必须先弹确认，实际 {outcome:?}");
    };
    assert_eq!(request.count, 2);
    assert_eq!(request.keep_count, 2);
    assert_eq!(request.remove_count, 0);
    assert!(request.reason_lines.iter().any(|line| line.count == 2));

    assert_eq!(
        workbench.state_of(&suggestion_key(&db, &plan_id, "way/b1", "outer")),
        Some(ReviewState::Pending)
    );
    assert!(workbench.view().pending_suggestion_apply.is_some());

    workbench.cancel_suggestion_apply().unwrap();
    assert!(workbench.view().pending_suggestion_apply.is_none());
    for source_entity_id in ["way/b1", "way/b2"] {
        assert_eq!(
            workbench.state_of(&suggestion_key(&db, &plan_id, source_entity_id, "outer",)),
            Some(ReviewState::Pending),
            "取消后状态不得改变"
        );
    }
    assert!(!workbench.can_undo_suggestion_apply());
    assert!(matches!(
        workbench.cancel_suggestion_apply(),
        Err(Error::NoSuggestionApplyPending)
    ));
}

#[test]
fn apply_suggestions_confirm_writes_states_and_undo_restores_them() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestKeep);
    let keep_targets = visible_card_keys(&workbench);
    assert_eq!(keep_targets.len(), 2);

    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 2);

    let outcome = workbench.confirm_suggestion_apply().unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 2 });
    for key in &keep_targets {
        assert_eq!(workbench.state_of(key), Some(ReviewState::Keep));
    }

    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("已记录最近一批");
    assert_eq!(batch.keep_count, 2);
    assert_eq!(batch.remove_count, 0);
    assert_eq!(batch.targets.len(), 2);
    assert_eq!(batch.before_states.len(), 2);
    assert!(batch
        .before_states
        .iter()
        .all(|(_, state)| *state == ReviewState::Pending));
    assert!(workbench.can_undo_suggestion_apply());
    assert!(workbench.view().undo_available);

    let changed = workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(changed, 2);
    for key in &keep_targets {
        assert_eq!(
            workbench.state_of(key),
            Some(ReviewState::Pending),
            "撤销后必须恢复应用前状态"
        );
    }
    assert!(!workbench.can_undo_suggestion_apply());
    assert!(matches!(
        workbench.undo_last_suggestion_apply(),
        Err(Error::NoSuggestionApplyToUndo)
    ));
}

#[test]
fn apply_suggestions_scope_respects_active_category_and_remove_batch() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestRemove);
    let remove_targets = visible_card_keys(&workbench);
    assert_eq!(remove_targets.len(), 1);

    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 1);
    assert_eq!(request.remove_count, 1);
    let changed = workbench.confirm_suggestion_apply().unwrap();
    assert_eq!(changed, CommandOutcome::Applied { changed: 1 });
    assert_eq!(
        workbench.state_of(&remove_targets[0]),
        Some(ReviewState::Remove)
    );

    let mut water = workbench;
    water.set_active_category(CandidateCategory::Water);
    water.toggle_suggestion_filter(SuggestFilter::SuggestKeep);
    let CommandOutcome::NeedsSuggestionConfirmation(request) = water.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 1);
    assert_eq!(request.keep_count, 1);
    assert_eq!(request.reason_lines.len(), 1);
    assert_eq!(
        water.confirm_suggestion_apply().unwrap(),
        CommandOutcome::Applied { changed: 1 }
    );
    assert_eq!(
        water.state_of(&suggestion_key(&db, &plan_id, "way/w1", "outer")),
        Some(ReviewState::Keep)
    );
}

#[test]
fn undo_is_rejected_after_seal_but_batch_record_remains_traceable() {
    let (mut db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestKeep);
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    assert!(workbench.can_undo_suggestion_apply());

    workbench.seal(&mut db).unwrap();
    assert!(!workbench.can_undo_suggestion_apply(), "封账后不可撤销");
    assert!(matches!(
        workbench.undo_last_suggestion_apply(),
        Err(Error::AlreadySealed)
    ));
    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("封账后追溯记录仍可读");
    assert_eq!(batch.keep_count, 2);
    assert_eq!(batch.reason_lines.len(), 1);
}

#[test]
fn only_most_recent_batch_is_undoable() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestKeep);
    let keep_targets = visible_card_keys(&workbench);
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestRemove);
    let remove_target = workbench
        .suggestions()
        .into_iter()
        .find(|(_, suggestion)| suggestion.action == SuggestionAction::Remove)
        .map(|(key, _)| key.clone())
        .expect("存在唯一建议剔除对象");
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("最近一批为剔除批");
    assert_eq!(batch.remove_count, 1);

    workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(
        workbench.state_of(&remove_target),
        Some(ReviewState::Pending)
    );
    for key in &keep_targets {
        assert_eq!(workbench.state_of(key), Some(ReviewState::Keep));
    }
}
