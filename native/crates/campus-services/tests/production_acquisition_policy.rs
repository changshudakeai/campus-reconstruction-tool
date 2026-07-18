use std::fs;
use std::path::{Path, PathBuf};

fn source(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .expect("production acquisition source must be readable")
        .replace("\r\n", "\n")
}

fn without_debug_only_legacy_acquisition(source: &str) -> String {
    const START: &str = "// BEGIN DEBUG-ONLY LEGACY ACQUISITION";
    const END: &str = "// END DEBUG-ONLY LEGACY ACQUISITION";
    let mut remaining = source;
    let mut production = String::new();
    while let Some(start) = remaining.find(START) {
        production.push_str(&remaining[..start]);
        let debug_only = &remaining[start + START.len()..];
        let end = debug_only
            .find(END)
            .expect("debug-only legacy acquisition block must have an end marker");
        remaining = &debug_only[end + END.len()..];
    }
    production.push_str(remaining);
    production
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("campus-services must live under native/crates")
        .to_path_buf()
}

#[test]
fn production_desktop_contains_only_the_controlled_v1_acquisition_route() {
    let root = workspace_root();
    let desktop =
        without_debug_only_legacy_acquisition(&source(root.join("apps/campus-native/src/main.rs")));
    let services = without_debug_only_legacy_acquisition(&source(
        root.join("crates/campus-services/src/lib.rs"),
    ));
    let production_client = source(root.join("apps/campus-native/src/v11_acquisition_client.rs"));
    let release_sources = format!("{desktop}\n{services}");

    for forbidden in [
        "overpass-api.de",
        "overpass.kumi.systems",
        "OVERTURE_BUILDING_ENDPOINT",
        "CAMPUS_DATA_SERVICE_URL",
        "query_open_map_data",
        "query_overture_buildings",
        "query_campus_data",
    ] {
        assert!(
            !release_sources.contains(forbidden),
            "production source still contains forbidden acquisition route {forbidden}"
        );
    }
    assert!(desktop.contains("CAMPUS_ACQUISITION_SERVICE_URL"));
    assert!(production_client.contains("HttpsTransport"));
    assert!(production_client.contains("AcquisitionClient"));
}

#[test]
fn fixture_transport_and_fixture_bootstraps_are_debug_only() {
    let root = workspace_root();
    let acquisition = source(root.join("crates/campus-services/src/acquisition.rs"));
    let production_client = source(root.join("apps/campus-native/src/v11_acquisition_client.rs"));

    assert!(acquisition.contains("#[cfg(debug_assertions)]\npub mod fixture_transport"));
    assert!(
        production_client.contains("#[cfg(debug_assertions)]\npub fn bootstrap_fixture_if_enabled")
    );
    assert!(
        !production_client
            .contains("#[cfg(not(debug_assertions))]\npub fn bootstrap_fixture_if_enabled"),
        "release builds must not expose a fixture acquisition entry point"
    );
}
