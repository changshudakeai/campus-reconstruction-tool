//! F5 评审工作台集成测试
// ignore-tidy-filelength: F5 集成测试集中（三态/批量确认闸/会话/封账/轻量建议筛选与撤销）同属
// 一个用例入口，便于对照验收；失效里程碑：v2.1.0（2026-12-31），届时建议测试拆入独立文件后消除
//!
//! 覆盖缝 4 全流程：一次性读入 → 纯内存三态评审（状态变更操作）→
//! 批量确认闸 → 暂停/恢复 → 封账批量写回。

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateNameSource, CandidateProjection,
    CandidateProjectionsApi, CandidateShape, CandidateValidation, Database, RawObservation,
    RawObservationsApi, ReviewDecisionsApi,
};
use review_workbench::CandidateKey;
use review_workbench::{
    CommandOutcome, Error, ReviewWorkbench, StateChange, SuggestFilter, SuggestionAction,
    SuggestionCategory, BATCH_REMOVE_CONFIRM_THRESHOLD,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};
use std::collections::HashSet;

/// 在内存库里种入原始观测与一批已发布的可评审候选：6 栋建筑 + 2 条道路 + 1 处水域。
fn fixture() -> (Database, PlanId) {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let plan_key = plan_id.to_string();

    let mut observations = Vec::new();
    for index in 0..6 {
        observations.push(RawObservation::new(
            &plan_key,
            CandidateCategory::Building,
            format!("way/b{index}"),
            serde_json::json!({ "tags": { "name": format!("教学楼 {index}"), "building": "school" } }),
            "overpass",
        ));
    }
    for index in 0..2 {
        observations.push(RawObservation::new(
            &plan_key,
            CandidateCategory::Road,
            format!("way/r{index}"),
            serde_json::json!({ "tags": { "highway": "footway" } }),
            "overpass",
        ));
    }
    observations.push(RawObservation::new(
        &plan_key,
        CandidateCategory::Water,
        "way/w0",
        serde_json::json!({ "tags": { "name": "游泳池", "leisure": "swimming_pool" } }),
        "overpass",
    ));
    db.write_raw_observations(&observations)
        .expect("种子观测写入成功");
    let batch = db
        .prepare_candidate_batch(&plan_key)
        .expect("候选批次准备成功");
    let projections: Vec<_> = observations
        .iter()
        .map(|observation| {
            let display = match observation.entity_type {
                CandidateCategory::Building => CandidateDisplay::new(
                    observation.source_data["tags"]["name"]
                        .as_str()
                        .expect("建筑夹具名称")
                        .to_owned(),
                    vec![
                        ("building".to_owned(), "school".to_owned()),
                        (
                            "name".to_owned(),
                            observation.source_data["tags"]["name"]
                                .as_str()
                                .expect("建筑夹具名称")
                                .to_owned(),
                        ),
                    ],
                ),
                CandidateCategory::Road => CandidateDisplay::new(
                    &observation.entity_id,
                    vec![("highway".to_owned(), "footway".to_owned())],
                ),
                CandidateCategory::Water => CandidateDisplay::new(
                    "游泳池",
                    vec![
                        ("leisure".to_owned(), "swimming_pool".to_owned()),
                        ("name".to_owned(), "游泳池".to_owned()),
                    ],
                ),
                _ => unreachable!("夹具只含建筑、道路和水体"),
            };
            CandidateProjection::new(
                format!("overpass:{}:outer", observation.entity_id),
                &plan_key,
                &observation.id,
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
                observation.entity_type,
                display,
                CandidateShape::polygon(serde_json::json!([
                    [121.4, 31.2],
                    [121.5, 31.2],
                    [121.4, 31.3],
                    [121.4, 31.2]
                ])),
                CandidateValidation::Retained,
                CandidateEligibility::Reviewable,
            )
        })
        .collect();
    db.write_candidate_projections(&batch.id, &projections)
        .expect("候选投影写入成功");
    db.publish_candidate_batch(&batch.id)
        .expect("候选批次发布成功");
    (db, plan_id)
}

fn building_key(index: usize) -> CandidateKey {
    CandidateKey::new(format!("overpass:way/b{index}:outer"))
}

fn reviewable_projection(
    plan_id: &PlanId,
    candidate_id: &str,
    source_entity_id: &str,
    title: &str,
    category: CandidateCategory,
) -> CandidateProjection {
    CandidateProjection::new(
        candidate_id,
        plan_id.to_string(),
        format!("raw:{source_entity_id}"),
        "overpass",
        source_entity_id,
        "outer",
        category,
        CandidateDisplay::new(
            title,
            vec![
                ("building".to_owned(), "school".to_owned()),
                ("name".to_owned(), title.to_owned()),
            ],
        ),
        CandidateShape::polygon(serde_json::json!([
            [121.4, 31.2],
            [121.5, 31.2],
            [121.4, 31.3],
            [121.4, 31.2]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    )
}

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
    db.write_raw_observations(&[RawObservation::new(
        &plan_key,
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({ "tags": { "name": "原始观测名称" } }),
        "overpass",
    )])
    .expect("原始观测写入");

    assert_eq!(
        ReviewWorkbench::load(&db, &plan_id)
            .expect("只有原始观测也能进台")
            .candidate_count(),
        0,
        "RawObservation 不能旁路候选投影进入 F5"
    );

    let batch = db.prepare_candidate_batch(&plan_key).expect("准备批次");
    let reviewable = reviewable_projection(
        &plan_id,
        "overpass:way/1:outer",
        "way/1",
        "第一教学楼",
        CandidateCategory::Building,
    );
    let mut isolated = reviewable_projection(
        &plan_id,
        "overpass:way/2:outer",
        "way/2",
        "隔离建筑",
        CandidateCategory::Building,
    );
    isolated.eligibility = CandidateEligibility::Isolated;
    isolated.isolation_reason = Some("self_intersecting".to_owned());
    db.write_candidate_projections(&batch.id, &[reviewable, isolated])
        .expect("投影写入");

    assert_eq!(
        ReviewWorkbench::load(&db, &plan_id)
            .expect("未发布批次不影响进台")
            .candidate_count(),
        0,
        "未发布批次不能进入 F5"
    );

    db.publish_candidate_batch(&batch.id).expect("发布批次");
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).expect("加载已发布候选");
    assert_eq!(workbench.candidate_count(), 1, "Isolated 投影不能进入 F5");
    let key = CandidateKey::new("overpass:way/1:outer");
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
fn stable_candidate_id_roundtrips_through_session_seal_and_reload() {
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    let batch = db
        .prepare_candidate_batch(&plan_id.to_string())
        .expect("准备批次");
    db.write_candidate_projections(
        &batch.id,
        &[reviewable_projection(
            &plan_id,
            "overpass:way/1:outer",
            "way/1",
            "第一教学楼",
            CandidateCategory::Building,
        )],
    )
    .expect("投影写入");
    db.publish_candidate_batch(&batch.id).expect("发布批次");
    let stored = db
        .get_current_candidate_projection(&plan_id.to_string(), "overpass:way/1:outer")
        .expect("读取当前投影")
        .expect("当前投影存在");
    assert_eq!(stored.candidate_id, "overpass:way/1:outer");
    assert_eq!(stored.source_entity_id, "way/1");
    assert_ne!(stored.candidate_id, stored.source_entity_id);

    let key = CandidateKey::new("overpass:way/1:outer");
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).expect("加载候选");
    workbench
        .submit(StateChange::single(key.clone(), ReviewState::Keep))
        .expect("按 candidate_id 修改状态");
    workbench
        .toggle_selected(&key)
        .expect("按 candidate_id 勾选");

    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stable-candidate-session.json");
    workbench.save_session(&session_path).expect("保存会话");
    let mut resumed = ReviewWorkbench::load(&db, &plan_id).expect("重新加载候选");
    resumed.restore_session(&session_path).expect("恢复会话");
    assert_eq!(resumed.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(resumed.selected_count(), 1);

    resumed.seal(&mut db).expect("按 candidate_id 封账");
    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, "overpass:way/1:outer");

    let reloaded = ReviewWorkbench::load(&db, &plan_id).expect("封账后重新加载");
    assert_eq!(reloaded.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(
        reloaded.state_of(&CandidateKey::new("way/1")),
        None,
        "source_entity_id 不能冒充稳定 candidate_id"
    );
}

#[test]
fn category_change_keeps_one_candidate_identity_through_session_seal_and_reload() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let candidate_id = "overpass:way/1:outer";
    let key = CandidateKey::new(candidate_id);

    let first_batch = db.prepare_candidate_batch(&plan_id.to_string()).unwrap();
    db.write_candidate_projections(
        &first_batch.id,
        &[reviewable_projection(
            &plan_id,
            candidate_id,
            "way/1",
            "第一教学楼",
            CandidateCategory::Building,
        )],
    )
    .unwrap();
    db.publish_candidate_batch(&first_batch.id).unwrap();

    let mut first_review = ReviewWorkbench::load(&db, &plan_id).unwrap();
    first_review
        .submit(StateChange::single(key.clone(), ReviewState::Keep))
        .unwrap();
    first_review.toggle_selected(&key).unwrap();
    first_review.highlight(&key).unwrap();
    let session_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("candidate-category-change-session.json");
    first_review.save_session(&session_path).unwrap();
    first_review.seal(&mut db).unwrap();

    let second_batch = db.prepare_candidate_batch(&plan_id.to_string()).unwrap();
    db.write_candidate_projections(
        &second_batch.id,
        &[reviewable_projection(
            &plan_id,
            candidate_id,
            "way/1",
            "校园主路",
            CandidateCategory::Road,
        )],
    )
    .unwrap();
    db.publish_candidate_batch(&second_batch.id).unwrap();

    let mut second_review = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Keep));
    second_review.restore_session(&session_path).unwrap();
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(second_review.selected_count(), 1);
    assert_eq!(second_review.active_category(), CandidateCategory::Road);
    second_review.highlight(&key).unwrap();
    assert_eq!(second_review.highlighted(), Some(&key));
    let highlighted = second_review.view().map_objects;
    assert_eq!(highlighted.len(), 1);
    assert_eq!(highlighted[0].candidate_id, candidate_id);
    assert_eq!(highlighted[0].category, CandidateCategory::Road);
    assert!(highlighted[0].highlighted);
    second_review
        .submit(StateChange::single(key.clone(), ReviewState::Remove))
        .unwrap();
    assert_eq!(second_review.state_of(&key), Some(ReviewState::Remove));
    second_review.seal(&mut db).unwrap();

    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].candidate_id, candidate_id);
    assert_eq!(decisions[0].category, CandidateCategory::Road);
    assert_eq!(decisions[0].review_state, ReviewState::Remove);

    let reloaded = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(reloaded.candidate_count(), 1);
    assert_eq!(reloaded.state_of(&key), Some(ReviewState::Remove));
}

#[test]
fn version_two_session_restores_by_candidate_id_using_current_category() {
    let mut db = Database::open_in_memory().unwrap();
    let plan_id = PlanId::generate();
    let candidate_id = "overpass:way/v2:outer";
    let batch = db.prepare_candidate_batch(&plan_id.to_string()).unwrap();
    db.write_candidate_projections(
        &batch.id,
        &[reviewable_projection(
            &plan_id,
            candidate_id,
            "way/v2",
            "current-building",
            CandidateCategory::Building,
        )],
    )
    .unwrap();
    db.publish_candidate_batch(&batch.id).unwrap();

    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("version-two-session.json");
    let version_two = serde_json::json!({
        "version": 2,
        "plan_id": plan_id.to_string(),
        "active_category": "Road",
        "entries": [{
            "category": "Road",
            "candidate_id": candidate_id,
            "state": "keep",
            "selected": true
        }]
    });
    std::fs::write(
        &session_path,
        serde_json::to_vec_pretty(&version_two).unwrap(),
    )
    .unwrap();

    let key = CandidateKey::new(candidate_id);
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    workbench.restore_session(&session_path).unwrap();

    assert_eq!(workbench.state_of(&key), Some(ReviewState::Keep));
    assert_eq!(workbench.selected_count(), 1);
    assert_eq!(workbench.active_category(), CandidateCategory::Building);
    assert_eq!(
        workbench.view().map_objects[0].category,
        CandidateCategory::Building
    );
}

#[test]
fn load_reads_candidate_set_once_with_pending_initial_state() {
    let (db, plan_id) = fixture();
    let workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    assert_eq!(workbench.candidate_count(), 9);
    // 初始态一律"待定"（ADR-0022）
    assert_eq!(
        workbench.state_of(&building_key(0)),
        Some(ReviewState::Pending)
    );
    // 默认激活第一个有候选的类别抽屉
    assert_eq!(workbench.active_category(), CandidateCategory::Building);
}

#[test]
fn pending_to_keep_transition_via_state_change_operation() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 给定初始状态 Pending → 改为 Keep → 断言状态正确
    let outcome = workbench
        .submit(StateChange::single(building_key(0), ReviewState::Keep))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 1 });
    assert_eq!(
        workbench.state_of(&building_key(0)),
        Some(ReviewState::Keep)
    );

    // 点错了改点另一个状态即可（状态即后悔药）：Keep → Remove → Pending
    workbench
        .submit(StateChange::single(building_key(0), ReviewState::Remove))
        .unwrap();
    workbench
        .submit(StateChange::single(building_key(0), ReviewState::Pending))
        .unwrap();
    assert_eq!(
        workbench.state_of(&building_key(0)),
        Some(ReviewState::Pending)
    );
}

#[test]
fn batch_remove_at_threshold_pops_confirmation_dialog() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 勾选 5 栋建筑后批量剔除 → 弹二次确认弹窗
    for index in 0..BATCH_REMOVE_CONFIRM_THRESHOLD {
        workbench.toggle_selected(&building_key(index)).unwrap();
    }
    let outcome = workbench.submit_for_selected(ReviewState::Remove).unwrap();
    let CommandOutcome::NeedsConfirmation(request) = outcome else {
        panic!("批量剔除 ≥5 项必须先弹二次确认，实际 {outcome:?}");
    };
    assert_eq!(request.count, 5);
    assert_eq!(request.title_key, "review.batch_reject_confirm_title");
    assert_eq!(request.body_key, "review.batch_reject_confirm_body");

    // 弹窗期间视图带确认请求、状态原样不动
    let view = workbench.view();
    assert!(view.pending_confirmation.is_some());
    assert_eq!(
        workbench.state_of(&building_key(0)),
        Some(ReviewState::Pending)
    );

    // 点"确认" → 整批执行
    let confirmed = workbench.confirm_pending().unwrap();
    assert_eq!(confirmed, CommandOutcome::Applied { changed: 5 });
    assert_eq!(
        workbench.state_of(&building_key(4)),
        Some(ReviewState::Remove)
    );
    assert!(workbench.view().pending_confirmation.is_none());
}

#[test]
fn cancel_leaves_states_untouched() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    let targets: Vec<CandidateKey> = (0..6).map(building_key).collect();
    workbench
        .submit(StateChange::batch(targets, ReviewState::Remove))
        .unwrap();
    workbench.cancel_pending().unwrap();

    for index in 0..6 {
        assert_eq!(
            workbench.state_of(&building_key(index)),
            Some(ReviewState::Pending)
        );
    }
    // 没有等待中的确认时 confirm/cancel 都是错误
    assert!(matches!(
        workbench.confirm_pending(),
        Err(Error::NoPendingConfirmation)
    ));
}

#[test]
fn batch_remove_below_threshold_and_harmless_batches_run_directly() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 批量剔除 4 项：直接执行
    let targets: Vec<CandidateKey> = (0..4).map(building_key).collect();
    let outcome = workbench
        .submit(StateChange::batch(targets, ReviewState::Remove))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 4 });

    // 批量改保留 6 项：无害动作不需确认（ADR-0022）
    let targets: Vec<CandidateKey> = (0..6).map(building_key).collect();
    let outcome = workbench
        .submit(StateChange::batch(targets.clone(), ReviewState::Keep))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 6 });

    // 批量改回待定（恢复动作）：同样不需确认
    let outcome = workbench
        .submit(StateChange::batch(targets, ReviewState::Pending))
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 6 });
}

#[test]
fn bulk_buttons_appear_at_two_selections() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    workbench.toggle_selected(&building_key(0)).unwrap();
    assert!(!workbench.bulk_buttons_visible(), "勾选 1 个不显示");

    workbench.toggle_selected(&building_key(1)).unwrap();
    assert!(workbench.bulk_buttons_visible(), "勾选 ≥2 个自动浮现");
    assert!(workbench.view().bulk_buttons_visible);

    // 全选/取消全选作用于当前激活类别
    workbench.select_all_in_active_category();
    assert_eq!(workbench.selected_count(), 6);
    workbench.deselect_all_in_active_category();
    assert_eq!(workbench.selected_count(), 0);
    assert!(!workbench.bulk_buttons_visible());
}

#[test]
fn highlight_links_map_and_cards_both_ways() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 点地图上的对象（或卡片）→ 共用同一份高亮状态，双向联动
    workbench.highlight(&building_key(2)).unwrap();
    let view = workbench.view();
    let highlighted_cards: Vec<_> = view.cards.iter().filter(|c| c.highlighted).collect();
    assert_eq!(highlighted_cards.len(), 1);
    assert_eq!(highlighted_cards[0].candidate_id, "overpass:way/b2:outer");
    let highlighted_objects: Vec<_> = view.map_objects.iter().filter(|o| o.highlighted).collect();
    assert_eq!(highlighted_objects.len(), 1);
    assert_eq!(highlighted_objects[0].candidate_id, "overpass:way/b2:outer");

    // 右栏信息面板展示高亮候选的详情（类别、标签、状态）
    let info = view.info_panel.expect("高亮时信息面板可见");
    assert_eq!(info.title, "教学楼 2");
    assert_eq!(info.category_key, "collection.category_building");
    assert!(info
        .tags
        .contains(&("building".to_owned(), "school".to_owned())));

    workbench.clear_highlight();
    assert!(workbench.view().info_panel.is_none());

    // 高亮不存在的候选是数据不一致信号
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
    let building_tab = &view.category_tabs[0];
    assert_eq!(building_tab.category, CandidateCategory::Building);
    assert_eq!(building_tab.count, 6);
    assert!(building_tab.active);
    // 左栏卡片只显示激活类别；中间地图显示全部候选
    assert_eq!(view.cards.len(), 6);
    assert_eq!(view.map_objects.len(), 9);

    workbench.set_active_category(CandidateCategory::Water);
    let view = workbench.view();
    assert_eq!(view.cards.len(), 1);
    assert_eq!(view.cards[0].title, "游泳池");
}

#[test]
fn drawer_view_carries_source_shape_and_named_flags() {
    // T38：地图标注与"定位到地图"依赖几何（GCJ-02）与来源；详情面板与卡片
    // 标题依赖 named（未命名 → "未命名建筑 #id"）。
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 地图对象：全部候选携带几何种类/坐标与来源
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

    // 未命名候选（道路夹具标题回退为实体 ID）→ named=false，详情面板给出来源
    workbench.set_active_category(CandidateCategory::Road);
    let view = workbench.view();
    let road_card = view
        .cards
        .iter()
        .find(|c| c.candidate_id == "overpass:way/r0:outer")
        .expect("道路卡片存在");
    assert_eq!(road_card.title, "way/r0");
    assert!(!road_card.named, "回退标识的候选必须标记为未命名");

    workbench
        .highlight(&CandidateKey::new("overpass:way/r0:outer"))
        .unwrap();
    let view = workbench.view();
    let info = view.info_panel.expect("高亮时信息面板可见");
    assert_eq!(info.title, "way/r0");
    assert!(!info.named);
    assert_eq!(info.source, "overpass");
    assert_eq!(info.source_label_key, "review.info_source");
}

#[test]
fn pause_and_resume_roundtrip_via_temp_file() {
    let (db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    workbench
        .submit(StateChange::single(building_key(0), ReviewState::Keep))
        .unwrap();
    workbench
        .submit(StateChange::single(building_key(1), ReviewState::Remove))
        .unwrap();
    workbench.toggle_selected(&building_key(2)).unwrap();
    workbench.set_active_category(CandidateCategory::Road);

    // 暂停：内存状态持久化到临时文件（cargo 提供的集成测试临时目录）
    let session_path =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("review-session.json");
    workbench.save_session(&session_path).unwrap();

    // 退出再回来：重新进台一次性读入，再从临时文件恢复进度
    let mut resumed = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(
        resumed.state_of(&building_key(0)),
        Some(ReviewState::Pending)
    );
    resumed.restore_session(&session_path).unwrap();

    assert_eq!(resumed.state_of(&building_key(0)), Some(ReviewState::Keep));
    assert_eq!(
        resumed.state_of(&building_key(1)),
        Some(ReviewState::Remove)
    );
    assert_eq!(resumed.selected_count(), 1);
    assert_eq!(resumed.active_category(), CandidateCategory::Road);

    // 串档保护：别的方案不能吃这份会话文件
    let (other_db, other_plan) = fixture();
    let mut other = ReviewWorkbench::load(&other_db, &other_plan).unwrap();
    assert!(matches!(
        other.restore_session(&session_path),
        Err(Error::SessionPlanMismatch { .. })
    ));
}

#[test]
fn seal_batch_writes_back_and_freezes_review() {
    let (mut db, plan_id) = fixture();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();

    // 保留 2 栋建筑 + 剔除 1 条道路，其余保持待定
    workbench
        .submit(StateChange::batch(
            vec![building_key(0), building_key(1)],
            ReviewState::Keep,
        ))
        .unwrap();
    workbench
        .submit(StateChange::single(
            CandidateKey::new("overpass:way/r0:outer"),
            ReviewState::Remove,
        ))
        .unwrap();

    // 封账前汇总（缝 5 账本）：保留 2、待定 6、剔除 1
    let summary = workbench.export_summary();
    assert_eq!(summary.keep_total, 2);
    assert_eq!(summary.pending_count, 6);
    assert_eq!(summary.remove_count, 1);
    assert_eq!(
        summary.keep_by_category,
        vec![(CandidateCategory::Building, 2)]
    );

    // 封账：一次性批量写回 B2
    let sealed_summary = workbench.seal(&mut db).unwrap();
    assert_eq!(sealed_summary.keep_total, 2);
    assert!(workbench.is_sealed());
    assert!(workbench.view().sealed);

    // 数据库里如实落账（9 条终态，状态逐一对得上）
    let decisions = db.list_review_decisions(&plan_id.to_string()).unwrap();
    assert_eq!(decisions.len(), 9);
    let (pending, keep, remove) = db.count_review_states(&plan_id.to_string()).unwrap();
    assert_eq!((pending, keep, remove), (6, 2, 1));

    // 封账后评审决定不可再改
    assert!(matches!(
        workbench.submit(StateChange::single(building_key(0), ReviewState::Remove)),
        Err(Error::AlreadySealed)
    ));
    assert!(matches!(workbench.seal(&mut db), Err(Error::AlreadySealed)));

    // 重新进台读到的是封账终态（对回内存）
    let reloaded = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(reloaded.state_of(&building_key(0)), Some(ReviewState::Keep));
}

// ── 轻量建议辅助（工单：自动标记 + 可读理由 + 筛选 + 一键应用 + 撤销）──

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
    let (mut db, plan_id) = fixture();
    let plan_key = plan_id.to_string();
    let batch = db
        .prepare_candidate_batch(&plan_key)
        .expect("准备建议夹具批次");

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
    let mut push = |candidate_id: &str,
                    source_entity_id: &str,
                    category: CandidateCategory,
                    title: &str,
                    tags: Vec<(String, String)>,
                    shape: CandidateShape,
                    validation: CandidateValidation,
                    name_source: CandidateNameSource| {
        projections.push(
            CandidateProjection::new(
                candidate_id,
                &plan_key,
                format!("raw:{candidate_id}"),
                "overpass",
                source_entity_id,
                "outer",
                category,
                CandidateDisplay::new(title, tags),
                shape,
                validation,
                CandidateEligibility::Reviewable,
            )
            .with_name_source(name_source),
        );
    };

    push(
        "overpass:way/b1:outer",
        "way/b1",
        CandidateCategory::Building,
        "教学楼甲",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.0)),
        CandidateValidation::Retained,
        CandidateNameSource::Osm,
    );
    push(
        "overpass:way/b2:outer",
        "way/b2",
        CandidateCategory::Building,
        "教学楼乙",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.2)),
        CandidateValidation::Retained,
        CandidateNameSource::Osm,
    );
    // b3 与 b1 同来源实体 + 同几何（重复投影）。
    push(
        "overpass:way/b3:outer",
        "way/b1",
        CandidateCategory::Building,
        "教学楼甲",
        vec![("building".to_owned(), "school".to_owned())],
        CandidateShape::polygon(ring(0.0)),
        CandidateValidation::Retained,
        CandidateNameSource::Osm,
    );
    push(
        "overpass:way/b4:outer",
        "way/b4",
        CandidateCategory::Building,
        "way/b4",
        Vec::new(),
        CandidateShape::polygon(ring(0.4)),
        CandidateValidation::Retained,
        CandidateNameSource::Unnamed,
    );
    push(
        "overpass:way/b5:outer",
        "way/b5",
        CandidateCategory::Building,
        "实验楼",
        vec![("building".to_owned(), "lab".to_owned())],
        CandidateShape::polygon(ring(0.6)),
        CandidateValidation::Repaired,
        CandidateNameSource::Osm,
    );
    push(
        "overpass:way/r1:outer",
        "way/r1",
        CandidateCategory::Road,
        "way/r1",
        vec![("highway".to_owned(), "footway".to_owned())],
        CandidateShape::line_string(serde_json::json!([[121.4, 31.1], [121.6, 31.1]])),
        CandidateValidation::Retained,
        CandidateNameSource::Unnamed,
    );
    push(
        "overpass:way/w1:outer",
        "way/w1",
        CandidateCategory::Water,
        "游泳池",
        vec![("leisure".to_owned(), "swimming_pool".to_owned())],
        CandidateShape::polygon(ring(0.8)),
        CandidateValidation::Retained,
        CandidateNameSource::Osm,
    );

    db.write_candidate_projections(&batch.id, &projections)
        .expect("建议夹具投影写入");
    db.publish_candidate_batch(&batch.id)
        .expect("建议夹具批次发布");
    (db, plan_id)
}

fn suggestion_key(candidate_id: &str) -> CandidateKey {
    CandidateKey::new(candidate_id)
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

    // 生成建议只读候选数据：逐条查询、筛选计数、切换筛选、产出视图。
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

    // 未命名（b4/r1）
    for id in ["overpass:way/b4:outer", "overpass:way/r1:outer"] {
        let suggestion = &by_id[id];
        assert_eq!(suggestion.category, SuggestionCategory::Unnamed);
        assert_eq!(suggestion.action, SuggestionAction::HumanReview);
    }
    // 需要关注（b3 重复投影、b5 自动修复）
    let duplicate = &by_id["overpass:way/b3:outer"];
    assert_eq!(duplicate.category, SuggestionCategory::NeedsAttention);
    assert_eq!(duplicate.action, SuggestionAction::Remove);
    let repaired = &by_id["overpass:way/b5:outer"];
    assert_eq!(repaired.category, SuggestionCategory::NeedsAttention);
    assert_eq!(repaired.action, SuggestionAction::HumanReview);
    // 无需处理（b1/b2/w1 建议保留）
    for id in [
        "overpass:way/b1:outer",
        "overpass:way/b2:outer",
        "overpass:way/w1:outer",
    ] {
        let suggestion = &by_id[id];
        assert_eq!(suggestion.category, SuggestionCategory::NoActionNeeded);
        assert_eq!(suggestion.action, SuggestionAction::Keep);
    }
    // 每条建议都有可读理由文本键（public_api 测试再断言 zh-CN.json 可解析）
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

    // 视图中的筛选标签与类别组合（卡片只显示当前类别）。
    let view = workbench.view();
    assert_eq!(view.suggestion_filters.len(), 5);
    let keep_tab = view
        .suggestion_filters
        .iter()
        .find(|tab| tab.filter == SuggestFilter::SuggestKeep)
        .expect("建议保留标签存在");
    assert_eq!(keep_tab.count, 3);
    assert!(!keep_tab.active);

    // 激活"建议保留"后与建筑类别组合：建筑页 2 张建议保留卡。
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

    // 弹窗期间状态原样不动。
    assert_eq!(
        workbench.state_of(&suggestion_key("overpass:way/b1:outer")),
        Some(ReviewState::Pending)
    );
    assert!(workbench.view().pending_suggestion_apply.is_some());

    // 取消：状态不变、待确认计划清除。
    workbench.cancel_suggestion_apply().unwrap();
    assert!(workbench.view().pending_suggestion_apply.is_none());
    for id in ["overpass:way/b1:outer", "overpass:way/b2:outer"] {
        assert_eq!(
            workbench.state_of(&suggestion_key(id)),
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

    let CommandOutcome::NeedsSuggestionConfirmation(request) =
        workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    assert_eq!(request.count, 2);

    let outcome = workbench.confirm_suggestion_apply().unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 2 });
    assert_eq!(
        workbench.state_of(&suggestion_key("overpass:way/b1:outer")),
        Some(ReviewState::Keep)
    );
    assert_eq!(
        workbench.state_of(&suggestion_key("overpass:way/b2:outer")),
        Some(ReviewState::Keep)
    );

    // 可追溯：批次与理由被记录。
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

    // 撤销上一批：恢复到应用前状态。
    let changed = workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(changed, 2);
    for id in ["overpass:way/b1:outer", "overpass:way/b2:outer"] {
        assert_eq!(
            workbench.state_of(&suggestion_key(id)),
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

    // 建筑类别 + 建议剔除筛选：只有 b3 一个重复投影。
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
        workbench.state_of(&suggestion_key("overpass:way/b3:outer")),
        Some(ReviewState::Remove)
    );

    // 水类别 + 建议保留筛选：只有 w1。
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
        water.state_of(&suggestion_key("overpass:way/w1:outer")),
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
    // 追溯记录仍保留（批次与理由可查），只是不能再撤销。
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
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    // 第二次应用（建议剔除）覆盖上一批撤销点。
    workbench.toggle_suggestion_filter(SuggestFilter::SuggestRemove);
    let CommandOutcome::NeedsSuggestionConfirmation(_) = workbench.apply_suggestions().unwrap()
    else {
        panic!("需要确认");
    };
    workbench.confirm_suggestion_apply().unwrap();
    let batch = workbench
        .last_applied_suggestion_batch()
        .expect("最近一批为剔除批");
    assert_eq!(batch.remove_count, 1);

    // 撤销只回滚最近一批（b3 回到待定），b1/b2 保持上一批的保留。
    workbench.undo_last_suggestion_apply().unwrap();
    assert_eq!(
        workbench.state_of(&suggestion_key("overpass:way/b3:outer")),
        Some(ReviewState::Pending)
    );
    assert_eq!(
        workbench.state_of(&suggestion_key("overpass:way/b1:outer")),
        Some(ReviewState::Keep)
    );
}
