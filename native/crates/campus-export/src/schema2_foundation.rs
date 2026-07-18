use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationManifest {
    pub project_id: String,
    pub project_revision: u64,
    pub compatibility_profile_id: String,
    pub dataset_bundle_id: String,
    pub schematic: FoundationManifestSchematic,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationManifestSchematic {
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
    pub width: usize,
    pub height: usize,
    pub length: usize,
}

pub fn write_foundation_manifest(path: &Path, manifest: &FoundationManifest) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}
