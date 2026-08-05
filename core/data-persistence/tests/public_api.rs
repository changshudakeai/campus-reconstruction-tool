//! B2 public API snapshot generated from rustdoc JSON.

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
