//! F5 评审工作台集成测试
//!
//! 覆盖缝 4 全流程：一次性读入 → 纯内存三态评审（状态变更操作）→
//! 批量确认闸 → 暂停/恢复 → 封账批量写回。

use data_persistence::{
    CandidateEligibility, CandidateProjection, CandidateProjectionsApi, CandidateShape,
    CandidateValidation, Database, RawObservation, RawObservationsApi, ReviewDecisionsApi,
};
use review_workbench::CandidateKey;
use review_workbench::{
    CommandOutcome, Error, ReviewWorkbench, StateChange, BATCH_REMOVE_CONFIRM_THRESHOLD,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

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
            CandidateProjection::new(
                &observation.entity_id,
                &plan_key,
                &observation.id,
                &observation.data_source_tag,
                &observation.entity_id,
                "default",
                observation.entity_type,
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
    CandidateKey::new(CandidateCategory::Building, format!("way/b{index}"))
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
    assert_eq!(highlighted_cards[0].entity_id, "way/b2");
    let highlighted_objects: Vec<_> = view.map_objects.iter().filter(|o| o.highlighted).collect();
    assert_eq!(highlighted_objects.len(), 1);
    assert_eq!(highlighted_objects[0].entity_id, "way/b2");

    // 右栏信息面板展示高亮候选的详情（类别、标签、状态）
    let info = view.info_panel.expect("高亮时信息面板可见");
    assert_eq!(info.title, "way/b2");
    assert_eq!(info.category_key, "collection.category_building");
    assert!(info.tags.is_empty());

    workbench.clear_highlight();
    assert!(workbench.view().info_panel.is_none());

    // 高亮不存在的候选是数据不一致信号
    let bogus = CandidateKey::new(CandidateCategory::Sports, "way/none");
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
    assert_eq!(view.cards[0].title, "way/w0");
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
            CandidateKey::new(CandidateCategory::Road, "way/r0"),
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
