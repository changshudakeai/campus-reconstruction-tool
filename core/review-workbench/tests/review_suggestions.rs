//! F5 评审工作台的轻量建议、筛选、应用与撤销集成测试。

mod common;

use common::{candidate_key_part, write_raw_observation};
use data_persistence::{
    CandidateDisplay, CandidateNameSource, CandidateProjectionDraft, CandidateProjectionsApi,
    CandidateShape, CandidateSourceIdentity, Database, ReviewableValidation,
};
use review_workbench::{
    CandidateKey, CommandOutcome, ConfidenceFilter, ConfidenceTier, Error, ReviewWorkbench,
    StateChange, SuggestionAction, SuggestionCategory,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

/// 建议夹具：混合信号的已发布 Reviewable 候选。
///
/// - b1 教学楼甲（干净）→ 建议保留（高）
/// - b2 教学楼乙（干净、独立位置）→ 建议保留（高）
/// - b3 教学楼甲（与 b1 同来源实体 + 同几何 = 重复投影）→ 建议剔除（低）
/// - b4 未命名建筑 → 建议人工确认（未命名，中）
/// - b5 实验楼（几何自动修复）→ 建议人工确认（形状经修复，中）
/// - b6 实验楼乙（建筑点形状可疑）→ 建议人工确认（形状可疑，低）
/// - r1 道路（未命名）→ 建议人工确认（未命名，中）
/// - w1 游泳池（干净水体）→ 建议保留（高）
fn suggestion_fixture() -> (Database, PlanId) {
    let mut db = Database::open_in_memory().expect("内存库");
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();
    for (entity_id, category, title) in [
        ("way/b1", CandidateCategory::Building, "教学楼甲"),
        ("way/b2", CandidateCategory::Building, "教学楼乙"),
        ("way/b4", CandidateCategory::Building, "way/b4"),
        ("way/b5", CandidateCategory::Building, "实验楼"),
        ("way/b6", CandidateCategory::Building, "实验楼乙"),
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
        "way/b6",
        "outer",
        CandidateCategory::Building,
        "实验楼乙",
        vec![("building".to_owned(), "lab".to_owned())],
        CandidateShape::point(serde_json::json!([121.4, 31.4])),
        ReviewableValidation::Retained,
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
    for filter in ConfidenceFilter::ALL {
        let _ = workbench.confidence_filter_count(filter);
        workbench.set_confidence_filter(filter);
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
    assert_eq!(workbench.export_summary().pending_count, 8);
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
        assert_eq!(suggestion.confidence_tier(), ConfidenceTier::Medium);
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
    assert_eq!(duplicate.confidence_tier(), ConfidenceTier::Low);
    let repaired_key = suggestion_key(&db, &plan_id, "way/b5", "outer");
    let repaired = &by_id[&repaired_key.candidate_id];
    assert_eq!(repaired.category, SuggestionCategory::NeedsAttention);
    assert_eq!(repaired.action, SuggestionAction::HumanReview);
    assert_eq!(repaired.confidence_tier(), ConfidenceTier::Medium);
    let medium_key = suggestion_key(&db, &plan_id, "way/b6", "outer");
    let medium = &by_id[&medium_key.candidate_id];
    assert_eq!(medium.action, SuggestionAction::HumanReview);
    assert_eq!(medium.confidence_tier(), ConfidenceTier::Low);
    for key in [
        suggestion_key(&db, &plan_id, "way/b2", "outer"),
        suggestion_key(&db, &plan_id, "way/w1", "outer"),
    ] {
        let suggestion = &by_id[&key.candidate_id];
        assert_eq!(suggestion.category, SuggestionCategory::NoActionNeeded);
        assert_eq!(suggestion.action, SuggestionAction::Keep);
        assert_eq!(suggestion.confidence_tier(), ConfidenceTier::High);
    }
    for (_, suggestion) in by_id {
        assert!(!suggestion.reason_key.is_empty());
        assert!(!suggestion.summary_key.is_empty());
    }
}

#[test]
fn confidence_filters_count_tiers_and_combine_with_category() {
    let (db, plan_id) = suggestion_fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // T51：芯片计数按当前激活分类统计（建筑：高 2 / 中 2 / 低 2）。
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::All), 6);
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::High), 2);
    assert_eq!(
        workbench.confidence_filter_count(ConfidenceFilter::Medium),
        2
    );
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::Low), 2);

    let view = workbench.view();
    assert_eq!(view.confidence_filters.len(), 4);
    let high_tab = view
        .confidence_filters
        .iter()
        .find(|tab| tab.filter == ConfidenceFilter::High)
        .expect("高置信芯片存在");
    assert_eq!(high_tab.count, 2);
    assert!(!high_tab.active);

    let mut workbench = workbench;
    workbench.set_confidence_filter(ConfidenceFilter::High);
    let view = workbench.view();
    assert!(view.apply_suggestions_enabled);
    assert_eq!(
        view.cards
            .iter()
            .filter(|card| card.suggestion.is_some())
            .count(),
        2,
        "建筑类别 + 高置信筛选组合后只显示高置信（建议保留）候选"
    );
    assert!(view.cards.iter().all(|card| card
        .suggestion
        .as_ref()
        .is_some_and(|s| s.action_key == "review.suggestion_action_keep")));

    // 切到水域分类：芯片计数随之变为该分类内的高/中/低分布。
    workbench.set_active_category(CandidateCategory::Water);
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::All), 1);
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::High), 1);
    assert_eq!(
        workbench.confidence_filter_count(ConfidenceFilter::Medium),
        0
    );
    assert_eq!(workbench.confidence_filter_count(ConfidenceFilter::Low), 0);
}

#[test]
fn apply_suggestions_confirmation_cancel_leaves_state_untouched() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // T51：一键应用范围 = 全部尚未保留的高置信候选（跨类别），不依赖当前芯片。
    let outcome = workbench.apply_suggestions().unwrap();
    let CommandOutcome::NeedsSuggestionConfirmation(request) = outcome else {
        panic!("一键应用必须先弹确认，实际 {outcome:?}");
    };
    assert_eq!(request.count, 3);
    assert_eq!(request.keep_count, 3);
    assert_eq!(request.remove_count, 0);
    assert!(request.reason_lines.iter().any(|line| line.count == 3));

    assert_eq!(
        workbench.state_of(&suggestion_key(&db, &plan_id, "way/b1", "outer")),
        Some(ReviewState::Pending)
    );
    assert!(workbench.view().pending_suggestion_apply.is_some());

    workbench.cancel_suggestion_apply().unwrap();
    assert!(workbench.view().pending_suggestion_apply.is_none());
    for (source_entity_id, geometry_part_id) in [
        ("way/b1", "duplicate"),
        ("way/b2", "outer"),
        ("way/w1", "outer"),
    ] {
        assert_eq!(
            workbench.state_of(&suggestion_key(
                &db,
                &plan_id,
                source_entity_id,
                geometry_part_id
            )),
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
    let keep_targets = vec![
        suggestion_key(&db, &plan_id, "way/b1", "duplicate"),
        suggestion_key(&db, &plan_id, "way/b2", "outer"),
        suggestion_key(&db, &plan_id, "way/w1", "outer"),
    ];

    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 3);

    let outcome = workbench.confirm_suggestion_apply().unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 3 });
    for key in &keep_targets {
        assert_eq!(workbench.state_of(key), Some(ReviewState::Keep));
    }
    assert!(
        !workbench.apply_suggestions_enabled(),
        "全部高置信候选已保留后一键应用应禁用"
    );

    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("已记录最近一批");
    assert_eq!(batch.keep_count, 3);
    assert_eq!(batch.remove_count, 0);
    assert_eq!(batch.targets.len(), 3);
    assert_eq!(batch.before_states.len(), 3);
    assert!(batch
        .before_states
        .iter()
        .all(|(_, state)| *state == ReviewState::Pending));
    assert!(workbench.can_undo_suggestion_apply());
    assert!(workbench.view().undo_available);

    let changed = workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(changed, 3);
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
fn apply_suggestions_keeps_only_high_confidence_and_never_removes() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 3);
    assert_eq!(request.keep_count, 3);
    assert_eq!(request.remove_count, 0, "T51：一键应用不得剔除任何候选");
    let changed = workbench.confirm_suggestion_apply().unwrap();
    assert_eq!(changed, CommandOutcome::Applied { changed: 3 });
    assert_eq!(request.reason_lines.len(), 1);
    assert_eq!(
        workbench.state_of(&suggestion_key(&db, &plan_id, "way/b1", "duplicate")),
        Some(ReviewState::Keep),
        "重复对中的前序高置信投影应被保留"
    );
    // 中/低置信（含被建议剔除的重复投影）保持待定，绝不自动剔除。
    for (source_entity_id, geometry_part_id) in [
        ("way/b1", "outer"),
        ("way/b4", "outer"),
        ("way/b5", "outer"),
        ("way/b6", "outer"),
        ("way/r1", "outer"),
    ] {
        assert_eq!(
            workbench.state_of(&suggestion_key(
                &db,
                &plan_id,
                source_entity_id,
                geometry_part_id
            )),
            Some(ReviewState::Pending),
            "{source_entity_id}/{geometry_part_id} 不得被一键应用改变"
        );
    }
}

#[test]
fn undo_is_rejected_after_seal_but_batch_record_remains_traceable() {
    let (mut db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
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
    assert_eq!(batch.keep_count, 3);
    assert_eq!(batch.reason_lines.len(), 1);
}

#[test]
fn only_most_recent_batch_is_undoable() {
    let (db, plan_id) = suggestion_fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    let high_keys = vec![
        suggestion_key(&db, &plan_id, "way/b1", "duplicate"),
        suggestion_key(&db, &plan_id, "way/b2", "outer"),
        suggestion_key(&db, &plan_id, "way/w1", "outer"),
    ];
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    for key in &high_keys {
        assert_eq!(workbench.state_of(key), Some(ReviewState::Keep));
    }

    // 把 b2 改回待定后再应用一批：只含 b2 一个目标。
    workbench
        .submit(StateChange::single(
            high_keys[1].clone(),
            ReviewState::Pending,
        ))
        .unwrap();
    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 1, "第二批只应包含改回待定的高置信候选");
    workbench.confirm_suggestion_apply().unwrap();
    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("最近一批存在");
    assert_eq!(batch.keep_count, 1);
    assert_eq!(batch.remove_count, 0);

    // 撤销只覆盖最近一批：b2 回待定，b1/w1 保持保留。
    workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(
        workbench.state_of(&high_keys[1]),
        Some(ReviewState::Pending)
    );
    for key in [&high_keys[0], &high_keys[2]] {
        assert_eq!(workbench.state_of(key), Some(ReviewState::Keep));
    }
    assert!(!workbench.can_undo_suggestion_apply());
}

#[test]
fn cards_and_map_objects_are_sorted_high_to_low() {
    let (db, plan_id) = suggestion_fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    let view = workbench.view();

    // 建筑类别（默认激活）：高（重复对中的前序 b1、b2）→ 中（未命名 b4、
    // 修复 b5）→ 低（被建议剔除的 b1 后序投影、点形状可疑 b6）。
    let expected_building_order = vec![
        suggestion_key(&db, &plan_id, "way/b1", "duplicate").candidate_id,
        suggestion_key(&db, &plan_id, "way/b2", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b4", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b5", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b1", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b6", "outer").candidate_id,
    ];
    let card_ids: Vec<String> = view
        .cards
        .iter()
        .map(|card| card.candidate_id.clone())
        .collect();
    assert_eq!(
        card_ids, expected_building_order,
        "卡片必须按 高→中→低 排序，同档按稳定候选 ID"
    );

    // 地图对象（跨类别）：高 b1 前序投影/b2/w1 → 中（b4、b5、r1）→
    // 低（b1 后序、b6）。
    let expected_map_order = vec![
        suggestion_key(&db, &plan_id, "way/b1", "duplicate").candidate_id,
        suggestion_key(&db, &plan_id, "way/b2", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/w1", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b4", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b5", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/r1", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b1", "outer").candidate_id,
        suggestion_key(&db, &plan_id, "way/b6", "outer").candidate_id,
    ];
    let object_ids: Vec<String> = view
        .map_objects
        .iter()
        .map(|object| object.candidate_id.clone())
        .collect();
    assert_eq!(
        object_ids, expected_map_order,
        "地图对象必须与列表同序（高置信优先加载）"
    );
}
