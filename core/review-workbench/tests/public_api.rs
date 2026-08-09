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
        text_keys::PAUSE,
        text_keys::RESUME,
        text_keys::CONFIRM_BUTTON,
        text_keys::CANCEL_BUTTON,
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
