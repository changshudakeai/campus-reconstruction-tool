//! F5 public API snapshot and localization contract tests.

use localization::{Language, Localization};
use review_workbench::text_keys;

#[test]
fn public_api_snapshot() {
    let rustdoc_json = rustdoc_json::Builder::default()
        .toolchain(public_api::MINIMUM_NIGHTLY_RUST_VERSION)
        .build()
        .unwrap();
    let api = public_api::Builder::from_rustdoc_json(rustdoc_json)
        .build()
        .unwrap();
    api.assert_eq_or_update("tests/snapshots/public-api.txt");
}

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
        text_keys::INFO_SOURCE,
        text_keys::CONFIRM_BUTTON,
        text_keys::CANCEL_BUTTON,
        text_keys::CONFIDENCE_FILTERS_LABEL,
        text_keys::CONFIDENCE_FILTER_TAB,
        text_keys::FILTER_ALL,
        text_keys::FILTER_HIGH,
        text_keys::FILTER_MEDIUM,
        text_keys::FILTER_LOW,
        text_keys::APPLY_SUGGESTIONS,
        text_keys::UNDO_SUGGESTIONS,
        text_keys::SUGGESTION_CATEGORY_UNNAMED,
        text_keys::SUGGESTION_CATEGORY_NEEDS_ATTENTION,
        text_keys::SUGGESTION_CATEGORY_NO_ACTION,
        text_keys::SUGGESTION_ACTION_KEEP,
        text_keys::SUGGESTION_ACTION_HUMAN_REVIEW,
        text_keys::SUGGESTION_ACTION_REMOVE,
        text_keys::SUGGESTION_REASON_UNNAMED,
        text_keys::SUGGESTION_REASON_OVERLAP,
        text_keys::SUGGESTION_REASON_EXACT_DUPLICATE,
        text_keys::SUGGESTION_REASON_DUPLICATE_SUSPECT,
        text_keys::SUGGESTION_REASON_SUSPICIOUS_SHAPE,
        text_keys::SUGGESTION_REASON_REPAIRED,
        text_keys::SUGGESTION_REASON_SPARSE_TAGS,
        text_keys::SUGGESTION_REASON_MISSING_SOURCE,
        text_keys::SUGGESTION_REASON_ISOLATED,
        text_keys::SUGGESTION_REASON_MISSING_LATEST,
        text_keys::SUGGESTION_REASON_KEEP,
        text_keys::SUGGESTION_SUMMARY_UNNAMED,
        text_keys::SUGGESTION_SUMMARY_OVERLAP,
        text_keys::SUGGESTION_SUMMARY_EXACT_DUPLICATE,
        text_keys::SUGGESTION_SUMMARY_DUPLICATE_SUSPECT,
        text_keys::SUGGESTION_SUMMARY_SUSPICIOUS_SHAPE,
        text_keys::SUGGESTION_SUMMARY_REPAIRED,
        text_keys::SUGGESTION_SUMMARY_SPARSE_TAGS,
        text_keys::SUGGESTION_SUMMARY_MISSING_SOURCE,
        text_keys::SUGGESTION_SUMMARY_ISOLATED,
        text_keys::SUGGESTION_SUMMARY_MISSING_LATEST,
        text_keys::SUGGESTION_SUMMARY_KEEP,
        text_keys::APPLY_SUGGEST_CONFIRM_TITLE,
        text_keys::APPLY_SUGGEST_CONFIRM_BODY,
        text_keys::APPLY_SUGGEST_REASON_LABEL,
        text_keys::APPLY_SUGGEST_REASON_LINE,
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
