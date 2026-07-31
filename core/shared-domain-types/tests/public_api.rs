//! 公开 API 快照测试（执法清单 2.5，模板见 docs/developer-guide/enforcement.md）
//!
//! 任何公开类型、方法、trait、derive 的增删改都会让快照比对失败，
//! 必须显式运行 `UPDATE_SNAPSHOTS=yes` 重新生成快照，diff 现形于 PR。
//!
//! 构建 rustdoc JSON 需要 nightly 工具链（仅此测试用，主工具链仍由
//! rust-toolchain.toml 固定为 1.96.0）。

#[test]
fn public_api_snapshot() {
    // 构建 rustdoc JSON 需要 nightly 工具链（仅此测试用，主工具链仍是 stable/1.96.0）。
    let rustdoc_json = rustdoc_json::Builder::default()
        .toolchain(public_api::MINIMUM_NIGHTLY_RUST_VERSION)
        .build()
        .unwrap();
    let api = public_api::Builder::from_rustdoc_json(rustdoc_json)
        .build()
        .unwrap();
    // 快照不一致 = 测试失败；确认变更合理后用
    // `UPDATE_SNAPSHOTS=yes cargo test` 更新快照并把 diff 提交评审。
    api.assert_eq_or_update("tests/snapshots/public-api.txt");
}
