//! F5 公开 API 快照测试（执法清单 2.5）
//!
//! 任何公开类型的增删都会反映在本测试与 snapshots/public-api.txt 中，
//! PR diff 可见。附带 B6 国际化验收：本 crate 产出的全部文本键必须在
//! zh-CN.json 中逐条可解析（ADR-0005，文案外置）。

use data_persistence::{
    CandidateDisplay, CandidateEligibility, CandidateProjection, CandidateProjectionsApi,
    CandidateShape, CandidateValidation, Database, RawObservation, RawObservationsApi,
};
use localization::{Language, Localization};
use review_workbench::{
    text_keys, Candidate, CandidateKey, CommandOutcome, ConfirmationRequest, Error,
    ReviewWorkbench, StateChange, WorkbenchView, BATCH_REMOVE_CONFIRM_THRESHOLD,
};
use shared_domain_types::{CandidateCategory, PlanId, ReviewState};

#[test]
fn public_api_types_exist() {
    // 常量：批量剔除确认阈值（ADR-0016）
    assert_eq!(BATCH_REMOVE_CONFIRM_THRESHOLD, 5);

    // CandidateKey / Candidate：复用 B1 的类别与三态枚举（不重新定义）
    let projection = CandidateProjection::new(
        "overpass:way/1:outer",
        "plan",
        "raw-1",
        "overpass",
        "way/1",
        "outer",
        CandidateCategory::Building,
        CandidateDisplay::new(
            "教学楼",
            vec![
                ("building".to_owned(), "school".to_owned()),
                ("name".to_owned(), "教学楼".to_owned()),
            ],
        ),
        CandidateShape::polygon(serde_json::json!([
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [0.0, 0.0]
        ])),
        CandidateValidation::Retained,
        CandidateEligibility::Reviewable,
    );
    let key = CandidateKey::new(CandidateCategory::Building, &projection.candidate_id);
    assert_eq!(key.category, CandidateCategory::Building);
    assert_eq!(key.candidate_id, "overpass:way/1:outer");
    let candidate = Candidate::from_projection(&projection);
    assert_eq!(candidate.title, "教学楼");
    assert_eq!(candidate.state, ReviewState::Pending);

    // StateChange：明确的状态变更操作（B8 接口预留，ADR-0022）
    let change = StateChange::single(key.clone(), ReviewState::Keep);
    assert!(!change.needs_confirmation());

    // ReviewWorkbench：进台一次性读入（缝 4）
    let mut db = Database::open_in_memory().expect("内存库可打开");
    let plan_id = PlanId::generate();
    db.write_raw_observations(&[RawObservation::new(
        plan_id.to_string(),
        CandidateCategory::Building,
        "way/1",
        serde_json::json!({ "tags": { "name": "教学楼" } }),
        "overpass",
    )])
    .unwrap();
    let batch = db.prepare_candidate_batch(&plan_id.to_string()).unwrap();
    let mut projection = projection;
    projection.plan_id = plan_id.to_string();
    db.write_candidate_projections(&batch.id, &[projection])
        .unwrap();
    db.publish_candidate_batch(&batch.id).unwrap();
    let mut workbench = ReviewWorkbench::load(&db, &plan_id).unwrap();
    assert_eq!(workbench.plan_id(), plan_id.to_string());
    assert_eq!(workbench.candidate_count(), 1);

    // 状态变更 + 视图产出
    let outcome: CommandOutcome = workbench.submit(change).unwrap();
    assert_eq!(outcome, CommandOutcome::Applied { changed: 1 });
    let view: WorkbenchView = workbench.view();
    assert_eq!(view.title_key, "review.workbench_title");
    assert!(!view.sealed);

    // 封账写回 + 汇总（缝 4/缝 5）
    let summary = workbench.seal(&mut db).unwrap();
    assert_eq!(summary.keep_total, 1);
    assert!(workbench.is_sealed());

    // Error #[non_exhaustive]：带类型错误可匹配
    let err: Error = workbench
        .submit(StateChange::single(key, ReviewState::Remove))
        .unwrap_err();
    assert!(matches!(err, Error::AlreadySealed));
    assert!(!err.to_string().is_empty());

    // ConfirmationRequest 走弹窗文本键（弹窗铁律 ADR-0021 + 文案外置 ADR-0005）
    let request: Option<ConfirmationRequest> = view.pending_confirmation;
    assert!(request.is_none());
}

/// B6 国际化验收：本 crate 产出的全部文本键在 zh-CN.json 中逐条可解析
/// （`t()` 查不到键时原样返回键名——据此断言解析结果不等于键名）。
#[test]
fn every_emitted_text_key_resolves_in_zh_cn() {
    let l10n = Localization::new(Language::ZhCn).expect("zh-CN.json 可加载");
    let keys = [
        text_keys::WORKBENCH_TITLE,
        text_keys::STATE_PENDING,
        text_keys::STATE_KEEP,
        text_keys::STATE_REJECT,
        text_keys::STATE_LABEL,
        text_keys::SELECT_ALL,
        text_keys::DESELECT_ALL,
        text_keys::SET_KEEP,
        text_keys::SET_REJECT,
        text_keys::SET_PENDING,
        text_keys::BATCH_REJECT_CONFIRM_TITLE,
        text_keys::BATCH_REJECT_CONFIRM_BODY,
        text_keys::SELECTED_COUNT,
        text_keys::ITEM_COUNT,
        text_keys::PENDING_COUNT,
        text_keys::INFO_CATEGORY,
        text_keys::INFO_TAGS,
        text_keys::PAUSE,
        text_keys::RESUME,
        text_keys::CONFIRM_BUTTON,
        text_keys::CANCEL_BUTTON,
        // 类别显示名（与 tag-rules.json 同源的 collection 命名空间既有键）
        "collection.category_building",
        "collection.category_road",
        "collection.category_water",
        "collection.category_vegetation",
        "collection.category_sports",
        "collection.category_other",
    ];
    for key in keys {
        assert_ne!(l10n.t(key), key, "文本键 {key} 在 zh-CN.json 中缺失");
    }
}
