use campus_export::inspect_schematic;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const FULL_FLOW_CAMPUSES: [&str; 5] = [
    "putuo",
    "sjtu-minhang",
    "wuhan-university",
    "xiamen-siming",
    "xian-jiaotong-xingqing",
];
const NEGATIVE_CAMPUS: &str = "nyu-shanghai-qiantan";
const CATEGORIES: [&str; 5] = ["building", "circulation", "water", "vegetation", "sports"];
const RISKS: [&str; 9] = [
    "large-request-preflight",
    "continuation-cancellation-replay",
    "complex-relations",
    "multipolygons",
    "alias-disambiguation",
    "overlapping-buildings",
    "whole-part-buildings",
    "sports-containment",
    "honest-unavailability",
];

pub(crate) fn write_report(input_path: &Path, report_path: &Path) -> Result<Value, String> {
    if report_path.exists() {
        return Err("live/Axiom evidence report already exists".into());
    }
    let input_bytes = std::fs::read(input_path).map_err(|error| error.to_string())?;
    let record: Value = serde_json::from_slice(&input_bytes).map_err(|error| error.to_string())?;
    let root = input_path
        .parent()
        .ok_or("operator record has no parent directory")?;
    let mut blockers = Vec::<Value>::new();
    let mut inspections = Vec::<Value>::new();
    require_string(&record, "/candidateId", "candidate identity", &mut blockers);
    require_string(&record, "/commit", "candidate commit", &mut blockers);
    validate_environment(&record, &mut blockers);
    validate_secret_safety(&record, &mut blockers);
    match record.pointer("/releaseBlockerObservations").and_then(Value::as_array) {
        Some(observations) if observations.is_empty() => {}
        Some(observations) => blocker(&mut blockers, "observed-failures", &format!("{} crash, rejected import, unknown block, empty output, material misplacement, Manifest mismatch, or missing-negative-case observation(s) were recorded", observations.len())),
        None => blocker(&mut blockers, "observed-failures", "releaseBlockerObservations must explicitly record an empty list"),
    }
    validate_campuses(root, &record, &mut inspections, &mut blockers);
    let report = json!({
        "status": if blockers.is_empty() { "pass" } else { "fail" },
        "version": env!("CARGO_PKG_VERSION"),
        "candidateId": record.pointer("/candidateId").cloned().unwrap_or(Value::Null),
        "commit": record.pointer("/commit").cloned().unwrap_or(Value::Null),
        "operatorRecordSha256": format!("{:x}", Sha256::digest(&input_bytes)),
        "campusCount": record.pointer("/campuses").and_then(Value::as_array).map_or(0, Vec::len),
        "schematicInspections": inspections,
        "releaseBlockers": blockers,
    });
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        report_path,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(report)
}

fn validate_environment(record: &Value, blockers: &mut Vec<Value>) {
    for (pointer, label) in [
        (
            "/controlledService/deploymentId",
            "controlled-service deployment",
        ),
        ("/controlledService/version", "controlled-service version"),
        (
            "/controlledService/datasetBundlePolicy",
            "dataset-bundle policy",
        ),
        ("/minecraft/version", "Minecraft version"),
        ("/axiom/version", "Axiom version"),
        (
            "/axiom/instanceDigestSha256",
            "Minecraft/Axiom instance digest",
        ),
    ] {
        require_string(record, pointer, label, blockers);
    }
    if record
        .pointer("/controlledService/healthStatus")
        .and_then(Value::as_str)
        != Some("pass")
    {
        blocker(
            blockers,
            "controlled-service",
            "pinned controlled service health did not pass",
        );
    }
    if record.pointer("/minecraft/version").and_then(Value::as_str) != Some("26.1.2") {
        blocker(
            blockers,
            "minecraft-profile",
            "Minecraft Java Edition 26.1.2 was not recorded",
        );
    }
    if !is_sha256(record.pointer("/axiom/instanceDigestSha256")) {
        blocker(
            blockers,
            "axiom-instance",
            "Minecraft/Axiom instance digest is not SHA-256",
        );
    }
}

fn validate_secret_safety(record: &Value, blockers: &mut Vec<Value>) {
    let text = record.to_string().to_ascii_lowercase();
    for forbidden in [
        "gaode_js_api_key",
        "gaode_security_code",
        "authorization",
        "bearer ",
    ] {
        if text.contains(forbidden) {
            blocker(
                blockers,
                "secret-safety",
                &format!("operator record contains forbidden credential material: {forbidden}"),
            );
        }
    }
    for name in [
        "GAODE_JS_API_KEY",
        "VITE_GAODE_JS_API_KEY",
        "GAODE_SECURITY_CODE",
        "VITE_GAODE_SECURITY_CODE",
        "CAMPUS_ACQUISITION_SERVICE_SECRET",
    ] {
        if let Ok(secret) = std::env::var(name) {
            if !secret.is_empty() && record.to_string().contains(&secret) {
                blocker(
                    blockers,
                    "secret-safety",
                    &format!("operator record leaked configured {name}"),
                );
            }
        }
    }
}

fn validate_campuses(
    root: &Path,
    record: &Value,
    inspections: &mut Vec<Value>,
    blockers: &mut Vec<Value>,
) {
    let Some(campuses) = record.pointer("/campuses").and_then(Value::as_array) else {
        blocker(
            blockers,
            "six-campus-matrix",
            "campuses must contain the accepted six-campus matrix",
        );
        return;
    };
    let ids = campuses
        .iter()
        .filter_map(|campus| campus.get("campusId").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    for id in FULL_FLOW_CAMPUSES.into_iter().chain([NEGATIVE_CAMPUS]) {
        if !ids.contains(id) {
            blocker(blockers, id, "accepted campus is missing");
        }
    }
    if campuses.len() != 6 || ids.len() != 6 {
        blocker(
            blockers,
            "six-campus-matrix",
            "matrix must contain exactly six unique accepted campuses",
        );
    }
    let mut exercised = BTreeSet::<String>::new();
    for campus in campuses {
        let id = campus
            .get("campusId")
            .and_then(Value::as_str)
            .unwrap_or("unknown-campus");
        validate_campus_common(root, id, campus, blockers);
        validate_risk_exercises(root, id, campus, &mut exercised, blockers);
        if FULL_FLOW_CAMPUSES.contains(&id) {
            validate_full_flow(root, id, campus, inspections, blockers);
        } else if id == NEGATIVE_CAMPUS {
            validate_negative(id, campus, blockers);
        }
    }
    for risk in RISKS {
        if !exercised.contains(risk) {
            blocker(
                blockers,
                "risk-matrix",
                &format!("required risk was not exercised: {risk}"),
            );
        }
    }
}

fn validate_campus_common(root: &Path, id: &str, campus: &Value, blockers: &mut Vec<Value>) {
    for (pointer, label) in [
        ("/campusName", "campus name"),
        ("/gaode/poiId", "Gaode POI identity"),
        ("/gaode/address", "Gaode address"),
        (
            "/gaode/providerDiagnostics",
            "redacted provider diagnostics",
        ),
        ("/evidence/datasetBundleId", "Dataset Bundle identity"),
        (
            "/evidence/acquisitionLicenceManifest",
            "Acquisition Licence Manifest",
        ),
    ] {
        if campus
            .pointer(pointer)
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            blocker(blockers, id, &format!("missing {label}"));
        }
    }
    let coordinates_ok = campus
        .pointer("/gaode/coordinates")
        .and_then(Value::as_array)
        .is_some_and(|items| items.len() == 2 && items.iter().all(|item| item.as_f64().is_some()));
    if !coordinates_ok {
        blocker(blockers, id, "Gaode coordinates are missing or invalid");
    }
    if !is_sha256(campus.pointer("/evidence/datasetBundleDigestSha256")) {
        blocker(blockers, id, "Dataset Bundle digest is not SHA-256");
    }
    let snapshots_ok = campus
        .pointer("/evidence/providerSnapshots")
        .and_then(Value::as_array)
        .is_some_and(|snapshots| {
            !snapshots.is_empty()
                && snapshots.iter().all(|snapshot| {
                    non_empty_string(snapshot.get("provider"))
                        && non_empty_string(snapshot.get("snapshotId"))
                        && is_sha256(snapshot.get("contentDigestSha256"))
                })
        });
    if !snapshots_ok {
        blocker(blockers, id, "provider snapshots are empty or incomplete");
    }
    for (pointer, label) in [
        ("/evidence/ruleVersions", "rule versions"),
        ("/evidence/limits", "request limits"),
    ] {
        if campus
            .pointer(pointer)
            .and_then(Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
        {
            blocker(blockers, id, &format!("{label} must not be empty"));
        }
    }
    let digests_ok = campus
        .pointer("/evidence/contentDigests")
        .and_then(Value::as_object)
        .is_some_and(|digests| {
            !digests.is_empty() && digests.values().all(|value| is_sha256(Some(value)))
        });
    if !digests_ok {
        blocker(blockers, id, "content digests are empty or invalid");
    }
    for category in CATEGORIES {
        let report_ok = campus
            .pointer(&format!("/evidence/coverageReports/{category}"))
            .and_then(Value::as_object)
            .is_some_and(|report| {
                let status = report.get("status").and_then(Value::as_str);
                let allowed_status = if id == NEGATIVE_CAMPUS {
                    status == Some("not-run-boundary-unavailable")
                } else {
                    matches!(
                        status,
                        Some("complete" | "complete-empty" | "partial" | "failed")
                    )
                };
                allowed_status
                    && is_sha256(report.get("reportDigestSha256"))
                    && non_empty_string(report.get("classificationRuleVersion"))
                    && report
                        .get("queryScope")
                        .and_then(Value::as_object)
                        .is_some_and(|scope| !scope.is_empty())
                    && report
                        .get("limits")
                        .and_then(Value::as_object)
                        .is_some_and(|limits| !limits.is_empty())
                    && report.get("knownGaps").and_then(Value::as_array).is_some()
                    && report
                        .get("providerSnapshotIds")
                        .and_then(Value::as_array)
                        .is_some_and(|ids| {
                            !ids.is_empty() && ids.iter().all(|value| non_empty_string(Some(value)))
                        })
            });
        if !report_ok {
            blocker(
                blockers,
                id,
                &format!("{category} Coverage Report has an invalid status or incomplete scope, limits, rules, gaps, snapshots, or digest"),
            );
        }
    }
    if let Some(path) = campus
        .pointer("/evidence/acquisitionLicenceManifest")
        .and_then(Value::as_str)
    {
        let manifest_ok = resolve_evidence_path(root, path)
            .and_then(|path| std::fs::read(path).map_err(|error| error.to_string()))
            .and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
            })
            .is_ok_and(|manifest| {
                manifest
                    .get("sources")
                    .and_then(Value::as_array)
                    .is_some_and(|sources| !sources.is_empty())
            });
        if !manifest_ok {
            blocker(
                blockers,
                id,
                "Acquisition Licence Manifest is missing, unsafe, or has no sources",
            );
        }
    }
}

fn validate_risk_exercises(
    root: &Path,
    campus_id: &str,
    campus: &Value,
    exercised: &mut BTreeSet<String>,
    blockers: &mut Vec<Value>,
) {
    let Some(risks) = campus.get("riskExercises").and_then(Value::as_array) else {
        blocker(blockers, campus_id, "riskExercises must be an array");
        return;
    };
    for risk in risks {
        let Some(risk_id) = risk.get("riskId").and_then(Value::as_str) else {
            blocker(blockers, campus_id, "risk exercise is missing riskId");
            continue;
        };
        let evidence_ok = risk
            .get("evidence")
            .and_then(Value::as_array)
            .is_some_and(|paths| {
                !paths.is_empty()
                    && paths.iter().all(|value| {
                        value
                            .as_str()
                            .is_some_and(|path| resolve_evidence_path(root, path).is_ok())
                    })
            });
        if evidence_ok {
            exercised.insert(risk_id.to_string());
        } else {
            blocker(
                blockers,
                campus_id,
                &format!("risk exercise {risk_id} has no existing result evidence"),
            );
        }
    }
}

fn validate_full_flow(
    root: &Path,
    id: &str,
    campus: &Value,
    inspections: &mut Vec<Value>,
    blockers: &mut Vec<Value>,
) {
    for (pointer, reason) in [
        ("/workflow/boundaryConfirmed", "boundary was not confirmed"),
        (
            "/workflow/fiveCategoryReviewComplete",
            "five-category review is incomplete",
        ),
        ("/workflow/saveReopenPassed", "save/reopen did not pass"),
        (
            "/workflow/currentRevisionGenerated",
            "current revision was not generated",
        ),
    ] {
        if campus.pointer(pointer).and_then(Value::as_bool) != Some(true) {
            blocker(blockers, id, reason);
        }
    }
    if campus
        .pointer("/workflow/projectRevision")
        .and_then(Value::as_u64)
        .is_none_or(|revision| revision == 0)
        || campus
            .pointer("/workflow/projectId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        blocker(
            blockers,
            id,
            "current project identity or revision is missing",
        );
    }
    let outputs = campus
        .pointer("/outputs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let foundations = outputs
        .iter()
        .filter(|output| output.get("kind").and_then(Value::as_str) == Some("foundation"))
        .count();
    let detailed = outputs
        .iter()
        .filter(|output| output.get("kind").and_then(Value::as_str) == Some("detailed"))
        .count();
    if foundations != 1 {
        blocker(
            blockers,
            id,
            "exactly one current-revision Foundation output is required",
        );
    }
    if (id == "putuo" && detailed != 1) || (id != "putuo" && detailed != 0) {
        blocker(
            blockers,
            id,
            "only Putuo must contain one representative Detailed output",
        );
    }
    for output in &outputs {
        validate_output(root, id, campus, output, inspections, blockers);
    }
}

fn validate_negative(id: &str, campus: &Value, blockers: &mut Vec<Value>) {
    if campus
        .pointer("/workflow/boundaryStatus")
        .and_then(Value::as_str)
        != Some("boundary-unavailable")
    {
        blocker(
            blockers,
            id,
            "negative case did not preserve boundary-unavailable",
        );
    }
    for action in ["retry", "back", "refresh"] {
        if campus
            .pointer(&format!("/workflow/recoveryActions/{action}"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            blocker(
                blockers,
                id,
                &format!("negative case did not prove {action}"),
            );
        }
    }
    if campus
        .pointer("/workflow/fabricatedGeometry")
        .and_then(Value::as_bool)
        != Some(false)
        || campus
            .pointer("/workflow/fiveCategoryReviewEntered")
            .and_then(Value::as_bool)
            != Some(false)
        || campus
            .pointer("/workflow/exportAttempted")
            .and_then(Value::as_bool)
            != Some(false)
    {
        blocker(
            blockers,
            id,
            "negative case fabricated geometry or entered prohibited review/export",
        );
    }
    if campus
        .pointer("/workflow/diagnosticLineage")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        blocker(
            blockers,
            id,
            "negative case is missing persisted diagnostic lineage",
        );
    }
    if campus
        .pointer("/outputs")
        .and_then(Value::as_array)
        .is_some_and(|outputs| !outputs.is_empty())
    {
        blocker(
            blockers,
            id,
            "boundary-unavailable case must not have outputs",
        );
    }
}

fn validate_output(
    root: &Path,
    campus_id: &str,
    campus: &Value,
    output: &Value,
    inspections: &mut Vec<Value>,
    blockers: &mut Vec<Value>,
) {
    let kind = output
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let Some(schematic_name) = output.get("schematic").and_then(Value::as_str) else {
        blocker(blockers, campus_id, "output is missing schematic path");
        return;
    };
    let Some(repeat_name) = output.get("determinismRepeat").and_then(Value::as_str) else {
        blocker(
            blockers,
            campus_id,
            "output is missing deterministic repeat path",
        );
        return;
    };
    let Ok(schematic_path) = resolve_evidence_path(root, schematic_name) else {
        blocker(blockers, campus_id, "schematic path is missing or unsafe");
        return;
    };
    let Ok(repeat_path) = resolve_evidence_path(root, repeat_name) else {
        blocker(
            blockers,
            campus_id,
            "deterministic repeat path is missing or unsafe",
        );
        return;
    };
    let inspection = match inspect_schematic(&schematic_path) {
        Ok(value) => value,
        Err(error) => {
            blocker(
                blockers,
                campus_id,
                &format!("schematic inspection failed: {error}"),
            );
            return;
        }
    };
    let repeat = match inspect_schematic(&repeat_path) {
        Ok(value) => value,
        Err(error) => {
            blocker(
                blockers,
                campus_id,
                &format!("repeat schematic inspection failed: {error}"),
            );
            return;
        }
    };
    if inspection.sponge_version != 3
        || inspection.data_version != 3955
        || inspection.offset != [0, 0, 0]
    {
        blocker(
            blockers,
            campus_id,
            "Sponge profile, DataVersion, or origin/offset is invalid",
        );
    }
    if inspection.content_sha256 != repeat.content_sha256 {
        blocker(
            blockers,
            campus_id,
            "schematic voxel content is not deterministic",
        );
    }
    let file_sha = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&schematic_path).unwrap_or_default())
    );
    if kind == "foundation" {
        let expected = FoundationManifestExpectation {
            project_id: campus
                .pointer("/workflow/projectId")
                .and_then(Value::as_str),
            project_revision: campus
                .pointer("/workflow/projectRevision")
                .and_then(Value::as_u64),
            dataset_bundle_id: campus
                .pointer("/evidence/datasetBundleId")
                .and_then(Value::as_str),
            schematic_name,
            file_sha256: &file_sha,
            inspection: &inspection,
        };
        validate_foundation_manifest(root, campus_id, output, &expected, blockers);
    }
    validate_axiom_import(root, campus_id, output, &file_sha, blockers);
    inspections.push(json!({"campusId": campus_id, "kind": kind, "file": schematic_name, "fileSha256": file_sha, "inspection": inspection}));
}

struct FoundationManifestExpectation<'a> {
    project_id: Option<&'a str>,
    project_revision: Option<u64>,
    dataset_bundle_id: Option<&'a str>,
    schematic_name: &'a str,
    file_sha256: &'a str,
    inspection: &'a campus_export::SchematicInspection,
}

fn validate_foundation_manifest(
    root: &Path,
    campus_id: &str,
    output: &Value,
    expected: &FoundationManifestExpectation<'_>,
    blockers: &mut Vec<Value>,
) {
    let Some(name) = output.get("foundationManifest").and_then(Value::as_str) else {
        blocker(
            blockers,
            campus_id,
            "Foundation output is missing its Manifest",
        );
        return;
    };
    let Ok(path) = resolve_evidence_path(root, name) else {
        blocker(
            blockers,
            campus_id,
            "Foundation Manifest path is missing or unsafe",
        );
        return;
    };
    let Ok(manifest) = std::fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        })
    else {
        blocker(blockers, campus_id, "Foundation Manifest is invalid JSON");
        return;
    };
    let expected_file = Path::new(expected.schematic_name)
        .file_name()
        .and_then(|name| name.to_str());
    let schematic_bytes = std::fs::metadata(root.join(expected.schematic_name))
        .map(|item| item.len())
        .ok();
    let matches = manifest.pointer("/projectId").and_then(Value::as_str) == expected.project_id
        && manifest.pointer("/projectRevision").and_then(Value::as_u64)
            == expected.project_revision
        && manifest.pointer("/datasetBundleId").and_then(Value::as_str)
            == expected.dataset_bundle_id
        && manifest
            .pointer("/compatibilityProfileId")
            .and_then(Value::as_str)
            == Some("minecraft-java-26.1.2-axiom-v1")
        && manifest
            .pointer("/schematic/fileName")
            .and_then(Value::as_str)
            == expected_file
        && manifest
            .pointer("/schematic/sha256")
            .and_then(Value::as_str)
            == Some(expected.file_sha256)
        && manifest.pointer("/schematic/bytes").and_then(Value::as_u64) == schematic_bytes
        && manifest.pointer("/schematic/width").and_then(Value::as_u64)
            == Some(expected.inspection.dimensions[0] as u64)
        && manifest
            .pointer("/schematic/height")
            .and_then(Value::as_u64)
            == Some(expected.inspection.dimensions[1] as u64)
        && manifest
            .pointer("/schematic/length")
            .and_then(Value::as_u64)
            == Some(expected.inspection.dimensions[2] as u64);
    if !matches {
        blocker(
            blockers,
            campus_id,
            "Foundation Manifest does not correspond to the inspected schematic",
        );
    }
}

fn validate_axiom_import(
    root: &Path,
    campus_id: &str,
    output: &Value,
    file_sha: &str,
    blockers: &mut Vec<Value>,
) {
    let Some(import) = output.get("axiomImport") else {
        blocker(blockers, campus_id, "Axiom import evidence is missing");
        return;
    };
    if import.get("status").and_then(Value::as_str) != Some("pass")
        || import.get("fileSha256").and_then(Value::as_str) != Some(file_sha)
        || import
            .get("durationMs")
            .and_then(Value::as_u64)
            .is_none_or(|value| value == 0)
        || import
            .get("blockCount")
            .and_then(Value::as_u64)
            .is_none_or(|value| value == 0)
        || import
            .get("bounds")
            .and_then(Value::as_array)
            .is_none_or(|value| value.len() != 6)
    {
        blocker(
            blockers,
            campus_id,
            "Axiom import status, hash, duration, bounds, or block count is invalid",
        );
    }
    let screenshots = import
        .get("screenshots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let attestation = import.get("visualAttestation");
    if screenshots.len() < 2
        || screenshots.iter().any(|value| {
            value
                .as_str()
                .is_none_or(|path| validate_png_screenshot(root, path).is_err())
        })
        || attestation
            .and_then(|value| value.get("orientationAndPlacement"))
            .and_then(Value::as_bool)
            != Some(true)
        || attestation
            .and_then(|value| value.get("principalFeatures"))
            .and_then(Value::as_bool)
            != Some(true)
    {
        blocker(blockers, campus_id, "Axiom evidence requires valid screenshots and explicit orientation/placement and principal-feature attestation");
    }
}

fn validate_png_screenshot(root: &Path, relative: &str) -> Result<(), String> {
    let path = resolve_evidence_path(root, relative)?;
    let reader = image::ImageReader::open(path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    if reader.format() != Some(image::ImageFormat::Png) {
        return Err("screenshot is not PNG".into());
    }
    let decoded = reader.decode().map_err(|error| error.to_string())?;
    let (width, height) = (decoded.width(), decoded.height());
    if width < 640 || height < 360 {
        return Err("screenshot is below 640x360".into());
    }
    Ok(())
}

fn resolve_evidence_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("evidence path escapes the operator-record directory".into());
    }
    let path = root.join(relative);
    if !path.is_file()
        || std::fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .len()
            == 0
    {
        return Err("evidence file is missing or empty".into());
    }
    Ok(path)
}

fn require_string(record: &Value, pointer: &str, label: &str, blockers: &mut Vec<Value>) {
    if record
        .pointer(pointer)
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        blocker(blockers, label, &format!("missing {label}"));
    }
}
fn is_sha256(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
fn non_empty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty())
}
fn blocker(blockers: &mut Vec<Value>, case_id: &str, reason: &str) {
    blockers.push(json!({"caseId": case_id, "reason": reason}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use campus_export::{write_schematic, VoxelModel};

    #[test]
    fn incomplete_operator_record_is_a_release_blocker() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, br#"{"candidateId":"candidate","commit":"commit"}"#).unwrap();

        let report = write_report(&input, &output).unwrap();

        assert_eq!(report["status"], "fail");
        assert!(report["releaseBlockers"].as_array().unwrap().len() >= 3);
        assert!(output.is_file());
    }

    #[test]
    fn complete_six_campus_record_passes_the_public_evidence_seam() {
        let root = tempfile::tempdir().unwrap();
        let sha = "a".repeat(64);
        let mut campuses = Vec::new();
        for id in FULL_FLOW_CAMPUSES.into_iter().chain([NEGATIVE_CAMPUS]) {
            let campus_root = root.path().join(id);
            std::fs::create_dir_all(&campus_root).unwrap();
            std::fs::write(
                campus_root.join("licence.json"),
                br#"{"sources":[{"provider":"osm","licence":"ODbL"}]}"#,
            )
            .unwrap();
            let risk_ids: &[&str] = match id {
                "sjtu-minhang" => &[
                    "large-request-preflight",
                    "continuation-cancellation-replay",
                    "overlapping-buildings",
                    "whole-part-buildings",
                    "sports-containment",
                ],
                "wuhan-university" => &["complex-relations"],
                "xiamen-siming" => &["alias-disambiguation"],
                "xian-jiaotong-xingqing" => &["multipolygons"],
                NEGATIVE_CAMPUS => &["honest-unavailability"],
                _ => &[],
            };
            let risks = risk_ids
                .iter()
                .map(|risk_id| {
                    let name = format!("risk-{risk_id}.json");
                    std::fs::write(campus_root.join(&name), b"{\"status\":\"pass\"}").unwrap();
                    json!({"riskId":risk_id,"evidence":[format!("{id}/{name}")]})
                })
                .collect::<Vec<_>>();
            let workflow = if id == NEGATIVE_CAMPUS {
                json!({"boundaryStatus":"boundary-unavailable","recoveryActions":{"retry":true,"back":true,"refresh":true},"diagnosticLineage":["event-1"],"fabricatedGeometry":false,"fiveCategoryReviewEntered":false,"exportAttempted":false})
            } else {
                json!({"projectId":id,"projectRevision":1,"boundaryConfirmed":true,"fiveCategoryReviewComplete":true,"saveReopenPassed":true,"currentRevisionGenerated":true})
            };
            let mut outputs = Vec::new();
            if id != NEGATIVE_CAMPUS {
                outputs.push(test_output(root.path(), id, "foundation", true));
                if id == "putuo" {
                    outputs.push(test_output(root.path(), id, "detailed", false));
                }
            }
            let coverage_status = if id == NEGATIVE_CAMPUS {
                "not-run-boundary-unavailable"
            } else {
                "complete"
            };
            campuses.push(json!({
                "campusId": id, "campusName": id,
                "gaode":{"poiId":"poi","address":"address","coordinates":[121.0,31.0],"providerDiagnostics":"redacted"},
                "evidence":{"datasetBundleId":"bundle","datasetBundleDigestSha256":sha,"providerSnapshots":[{"provider":"osm","snapshotId":"osm-1","contentDigestSha256":sha}],"coverageReports":{"building":coverage(&sha, coverage_status),"circulation":coverage(&sha, coverage_status),"water":coverage(&sha, coverage_status),"vegetation":coverage(&sha, coverage_status),"sports":coverage(&sha, coverage_status)},"acquisitionLicenceManifest":format!("{id}/licence.json"),"ruleVersions":{"conflation":"v1"},"limits":{"maxTiles":100},"contentDigests":{"observations":sha}},
                "riskExercises":risks,"workflow":workflow,"outputs":outputs
            }));
        }
        let record = json!({
            "candidateId":"candidate","commit":"commit",
            "controlledService":{"deploymentId":"deployment","version":"v1","healthStatus":"pass","datasetBundlePolicy":"pinned"},
            "minecraft":{"version":"26.1.2"},
            "axiom":{"version":"test","instanceDigestSha256":"b".repeat(64)},
            "releaseBlockerObservations":[],"campuses":campuses
        });
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

        let report = write_report(&input, &output).unwrap();

        assert_eq!(
            report["status"],
            "pass",
            "{}",
            serde_json::to_string_pretty(&report).unwrap()
        );
        assert_eq!(report["schematicInspections"].as_array().unwrap().len(), 6);
    }

    fn test_output(root: &Path, campus_id: &str, kind: &str, foundation: bool) -> Value {
        let directory = root.join(campus_id);
        let file_name = format!("{kind}.schem");
        let repeat_name = format!("{kind}-repeat.schem");
        let schematic = directory.join(&file_name);
        let repeat = directory.join(&repeat_name);
        let model = VoxelModel {
            width: 2,
            height: 1,
            length: 2,
            palette: vec!["minecraft:air".into(), "minecraft:stone".into()],
            blocks: vec![0, 1, 1, 0],
        };
        write_schematic(&schematic, kind, &model).unwrap();
        std::fs::copy(&schematic, &repeat).unwrap();
        let file_sha = format!("{:x}", Sha256::digest(std::fs::read(&schematic).unwrap()));
        let shot_a = directory.join(format!("{kind}-orientation.png"));
        let shot_b = directory.join(format!("{kind}-features.png"));
        write_test_png(&shot_a);
        write_test_png(&shot_b);
        let mut output = json!({
            "kind":kind,"schematic":format!("{campus_id}/{file_name}"),"determinismRepeat":format!("{campus_id}/{repeat_name}"),
            "axiomImport":{"status":"pass","fileSha256":file_sha,"durationMs":1,"bounds":[0,0,0,2,1,2],"blockCount":2,"screenshots":[format!("{campus_id}/{}",shot_a.file_name().unwrap().to_string_lossy()),format!("{campus_id}/{}",shot_b.file_name().unwrap().to_string_lossy())],"visualAttestation":{"orientationAndPlacement":true,"principalFeatures":true}}
        });
        if foundation {
            let manifest_name = "foundation-manifest.json";
            let bytes = std::fs::metadata(&schematic).unwrap().len();
            std::fs::write(directory.join(manifest_name), serde_json::to_vec_pretty(&json!({"projectId":campus_id,"projectRevision":1,"compatibilityProfileId":"minecraft-java-26.1.2-axiom-v1","datasetBundleId":"bundle","schematic":{"fileName":file_name,"bytes":bytes,"sha256":file_sha,"width":2,"height":1,"length":2}})).unwrap()).unwrap();
            output["foundationManifest"] = json!(format!("{campus_id}/{manifest_name}"));
        }
        output
    }

    fn coverage(sha: &str, status: &str) -> Value {
        json!({"status":status,"reportDigestSha256":sha,"providerSnapshotIds":["osm-1"],"classificationRuleVersion":"v1","queryScope":{"campusBoundary":"confirmed"},"limits":{"maxObservations":100},"knownGaps":[]})
    }

    fn write_test_png(path: &Path) {
        image::RgbImage::new(1280, 720).save(path).unwrap();
    }
}
