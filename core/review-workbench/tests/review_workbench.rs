//! F5 评审工作台的核心评审、批量确认与可见视图集成测试。

mod common;

use common::{building_key, candidate_key, fixture, reviewable_projection};
use data_persistence::{
    CandidateDisplay, CandidateProjectionDraft, CandidateProjectionsApi, CandidateShape,
    CandidateSourceIdentity, Database, RawObservation, RawObservationsApi,
};
use review_workbench::{CandidateKey, CommandOutcome, Error, ReviewWorkbench, StateChange};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use std::collections::HashSet;

#[test]
fn candidate_key_contains_only_the_stable_candidate_id() {
    let first = CandidateKey::new("overpass:way/1:outer");
    let second = CandidateKey::new("overpass:way/1:outer");

    assert_eq!(first, second);
    assert_eq!(HashSet::from([first, second]).len(), 1);
}

#[test]
fn load_accepts_only_published_reviewable_projections_and_keeps_display_attributes() {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();
    db.write_raw_observations(&[
        RawObservation::new(
            &plan_key,
            CandidateCategory::Building,
            "way/1",
            serde_json::json!({ "tags": { "name": "原始观测名称" } }),
            "overpass",
        ),
        RawObservation::new(
            &plan_key,
            CandidateCategory::Building,
            "way/2",
            serde_json::json!({ "tags": { "name": "隔离建筑" } }),
            "overpass",
        ),
    ])
    .expect("原始观测写入");

    let empty = ReviewWorkbench::load(&db, &plan_id).expect("无候选是合法评审空态");
    assert_eq!(empty.candidate_count(), 0);

    let reviewable = reviewable_projection("way/1", "第一教学楼", CandidateCategory::Building);
    let isolated = CandidateProjectionDraft::isolated(
        CandidateSourceIdentity::new("overpass", "way/2", "outer"),
        CandidateCategory::Building,
        CandidateDisplay::new("隔离建筑", Vec::new()),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        "self_intersecting",
    )
    .expect("合法隔离事实");
    db.publish_candidate_batch(&plan_key, "fixture-boundary", &[reviewable, isolated])
        .expect("原子发布候选批次");
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).expect("加载已发布候选");
    assert_eq!(workbench.candidate_count(), 1, "Isolated 投影不能进入 F5");
    let key = candidate_key(&db, &plan_id, "way/1");
    workbench.highlight(&key).expect("按稳定 candidate_id 高亮");
    let info = workbench.view().info_panel.expect("展示属性可见");
    assert_eq!(info.title, "第一教学楼");
    assert!(info
        .tags
        .contains(&("building".to_owned(), "school".to_owned())));
    assert!(matches!(
        workbench.highlight(&CandidateKey::new("way/1")),
        Err(Error::CandidateNotFound(_))
    ));
}

#[test]
fn load_reads_candidate_set_once_with_pending_initial_state() {
    let (db, plan_id) = fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    assert_eq!(workbench.candidate_count(), 9);
    assert_eq!(
        workbench.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Pending)
    );
    assert_eq!(workbench.active_category(), CandidateCategory::Building);
}

#[test]
fn pending_to_keep_transition_via_state_change_operation() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let outcome = workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Keep,
        ))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 1 });
    assert_eq!(
        workbench.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Keep)
    );

    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Remove,
        ))
        .unwrap();
    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Pending,
        ))
        .unwrap();
    assert_eq!(
        workbench.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Pending)
    );
}

#[test]
fn multi_target_batch_remove_pops_confirmation_dialog_without_threshold() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    for index in 0..5 {
        workbench
            .toggle_selected(&building_key(&db, &plan_id, index))
            .unwrap();
    }
    let outcome = workbench.submit_for_selected(ReviewState::Remove).unwrap();
    let CommandOutcome::NeedsConfirmation(request) = outcome else {
        panic!("批量剔除必须先弹二次确认，实际 {outcome:?}");
    };
    assert_eq!(request.count, 5);
    assert_eq!(request.title_key, "review.batch_reject_confirm_title");
    assert_eq!(request.body_key, "review.batch_reject_confirm_body");

    let view = workbench.view();
    assert!(view.pending_confirmation.is_some());
    assert_eq!(
        workbench.state_of(&building_key(&db, &plan_id, 0)),
        Some(ReviewState::Pending)
    );

    let confirmed = workbench.confirm_pending().unwrap();
    assert_eq!(confirmed, CommandOutcome::Applied { changed: 5 });
    assert_eq!(
        workbench.state_of(&building_key(&db, &plan_id, 4)),
        Some(ReviewState::Remove)
    );
    assert!(workbench.view().pending_confirmation.is_none());
}

#[test]
fn cancel_leaves_states_untouched() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let targets: Vec<CandidateKey> = (0..6)
        .map(|index| building_key(&db, &plan_id, index))
        .collect();
    workbench
        .submit(StateChange::batch(targets, ReviewState::Remove))
        .unwrap();
    workbench.cancel_pending().unwrap();

    for index in 0..6 {
        assert_eq!(
            workbench.state_of(&building_key(&db, &plan_id, index)),
            Some(ReviewState::Pending)
        );
    }
    assert!(matches!(
        workbench.confirm_pending(),
        Err(Error::NoPendingConfirmation)
    ));
}

#[test]
fn single_remove_and_harmless_batches_run_directly() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let single: Vec<CandidateKey> = (0..1)
        .map(|index| building_key(&db, &plan_id, index))
        .collect();
    let outcome = workbench
        .submit(StateChange::batch(single, ReviewState::Remove))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 1 });

    let targets: Vec<CandidateKey> = (0..6)
        .map(|index| building_key(&db, &plan_id, index))
        .collect();
    let outcome = workbench
        .submit(StateChange::batch(targets.clone(), ReviewState::Keep))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 6 });

    let outcome = workbench
        .submit(StateChange::batch(targets, ReviewState::Pending))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 6 });
}

#[test]
fn page_selection_updates_selected_count_without_bulk_visibility_flag() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let first = building_key(&db, &plan_id, 0);
    let second = building_key(&db, &plan_id, 1);
    workbench.toggle_selected(&first).unwrap();
    assert_eq!(workbench.selected_count(), 1);

    // T51：页面级全选由呈现层按当前页切片调用 set_selected。
    let page_keys = [first.clone(), second.clone()];
    let changed = workbench.set_selected(&page_keys, true);
    assert_eq!(changed, 1, "只把尚未勾选的一张卡改为勾选");
    assert_eq!(workbench.selected_count(), 2);

    let changed = workbench.set_selected(&page_keys, false);
    assert_eq!(changed, 2);
    assert_eq!(workbench.selected_count(), 0);
}

#[test]
fn highlight_links_map_and_cards_both_ways() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let highlighted_key = building_key(&db, &plan_id, 2);
    workbench.highlight(&highlighted_key).unwrap();
    let view = workbench.view();
    let highlighted_cards: Vec<_> = view.cards.iter().filter(|c| c.highlighted).collect();
    assert_eq!(highlighted_cards.len(), 1);
    assert_eq!(
        highlighted_cards[0].candidate_id,
        highlighted_key.candidate_id
    );
    let highlighted_objects: Vec<_> = view.map_objects.iter().filter(|o| o.highlighted).collect();
    assert_eq!(highlighted_objects.len(), 1);
    assert_eq!(
        highlighted_objects[0].candidate_id,
        highlighted_key.candidate_id
    );

    let info = view.info_panel.expect("高亮时信息面板可见");
    assert_eq!(info.title, "教学楼 2");
    assert_eq!(info.category_key, "collection.category_building");
    assert!(info
        .tags
        .contains(&("building".to_owned(), "school".to_owned())));

    workbench.clear_highlight();
    assert!(workbench.view().info_panel.is_none());

    let bogus = CandidateKey::new("way/none");
    assert!(matches!(
        workbench.highlight(&bogus),
        Err(Error::CandidateNotFound(_))
    ));
}

#[test]
fn three_pane_view_reflects_active_category() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let view = workbench.view();
    assert_eq!(view.title_key, "review.workbench_title");
    assert_eq!(view.category_tabs.len(), 6);
    assert_eq!(
        view.confidence_filters.len(),
        4,
        "置信度芯片固定为 全部/高/中/低 四个"
    );
    assert_eq!(
        view.state_tabs.len(),
        3,
        "三态分组固定为 待定/保留/剔除 三个"
    );
    assert!(
        view.state_tabs
            .iter()
            .find(|tab| tab.state == ReviewState::Pending)
            .is_some_and(|tab| tab.active),
        "默认激活分组必须是'待定'"
    );
    assert!(
        view.confidence_filters
            .iter()
            .find(|chip| chip.filter == review_workbench::ConfidenceFilter::All)
            .is_some_and(|chip| chip.active),
        "默认激活芯片必须是'全部'"
    );
    let building_tab = &view.category_tabs[0];
    assert_eq!(building_tab.category, CandidateCategory::Building);
    assert_eq!(building_tab.count, 6);
    assert!(building_tab.active);
    assert_eq!(view.cards.len(), 6);
    assert_eq!(view.map_objects.len(), 9);

    workbench.set_active_category(CandidateCategory::Water);
    let view = workbench.view();
    assert_eq!(view.cards.len(), 1);
    assert_eq!(view.cards[0].title, "游泳池");
}

#[test]
fn state_tabs_partition_cards_while_map_cards_stay_unpartitioned() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 默认待定分组：建筑分类全部 6 个候选都在待定。
    assert_eq!(workbench.active_state_tab(), ReviewState::Pending);
    assert_eq!(workbench.state_tab_count(ReviewState::Pending), 6);
    assert_eq!(workbench.state_tab_count(ReviewState::Keep), 0);
    assert_eq!(workbench.state_tab_count(ReviewState::Remove), 0);
    assert_eq!(workbench.view().cards.len(), 6);
    assert_eq!(workbench.view().map_cards.len(), 6);

    // 逐项评审：改为保留/剔除后，卡片离开待定分组并进入对应分组。
    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 0),
            ReviewState::Keep,
        ))
        .unwrap();
    workbench
        .submit(StateChange::single(
            building_key(&db, &plan_id, 1),
            ReviewState::Remove,
        ))
        .unwrap();
    assert_eq!(workbench.state_tab_count(ReviewState::Pending), 4);
    assert_eq!(workbench.state_tab_count(ReviewState::Keep), 1);
    assert_eq!(workbench.state_tab_count(ReviewState::Remove), 1);
    let pending_ids: Vec<String> = workbench
        .view()
        .cards
        .iter()
        .map(|card| card.candidate_id.clone())
        .collect();
    assert_eq!(pending_ids.len(), 4, "待定分组只显示尚未裁决的卡片");

    // 地图概览不随分组变化：始终包含当前分类+筛选下的全部三态。
    assert_eq!(workbench.view().map_cards.len(), 6);

    workbench.set_active_state_tab(ReviewState::Keep);
    assert_eq!(workbench.view().cards.len(), 1);
    assert_eq!(
        workbench.view().cards[0].candidate_id,
        building_key(&db, &plan_id, 0).candidate_id
    );
    workbench.set_active_state_tab(ReviewState::Remove);
    assert_eq!(workbench.view().cards.len(), 1);
    assert_eq!(
        workbench.view().cards[0].candidate_id,
        building_key(&db, &plan_id, 1).candidate_id
    );
}

#[test]
fn drawer_view_carries_source_shape_and_named_flags() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let view = workbench.view();
    assert_eq!(view.map_objects.len(), 9);
    for object in &view.map_objects {
        assert_eq!(object.source, "overpass", "来源必须随地图对象携带");
        assert_eq!(object.shape_kind, "polygon");
        assert!(
            object.shape_coordinates.is_array(),
            "几何坐标必须是数组: {}",
            object.shape_coordinates
        );
    }

    workbench.set_active_category(CandidateCategory::Road);
    let view = workbench.view();
    let road_key = candidate_key(&db, &plan_id, "way/r0");
    let road_card = view
        .cards
        .iter()
        .find(|c| c.candidate_id == road_key.candidate_id)
        .expect("道路卡片存在");
    assert_eq!(road_card.title, "way/r0");
    assert!(!road_card.named, "回退标识的候选必须标记为未命名");

    workbench.highlight(&road_key).unwrap();
    let view = workbench.view();
    let info = view.info_panel.expect("高亮时信息面板可见");
    assert_eq!(info.title, "way/r0");
    assert!(!info.named);
    assert_eq!(info.source, "overpass");
    assert_eq!(info.source_label_key, "review.info_source");
}
