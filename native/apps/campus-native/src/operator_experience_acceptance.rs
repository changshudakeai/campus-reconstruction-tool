use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

pub(crate) fn write_report(input_path: &Path, report_path: &Path) -> Result<Value, String> {
    if report_path.exists() {
        return Err("operator-experience evidence report already exists".into());
    }
    let input_bytes = std::fs::read(input_path).map_err(|error| error.to_string())?;
    let record: Value = serde_json::from_slice(&input_bytes).map_err(|error| error.to_string())?;
    let root = input_path
        .parent()
        .ok_or("operator record has no parent directory")?;
    let mut blockers = Vec::new();

    require_string(&record, "/candidateId", "candidate identity", &mut blockers);
    require_string(&record, "/commit", "candidate commit", &mut blockers);
    validate_independence(&record, &mut blockers);
    validate_run(root, &record, &mut blockers);
    validate_guidance_and_shortcuts(root, &record, &mut blockers);
    validate_localisation(root, &record, &mut blockers);
    validate_scaling(root, &record, &mut blockers);
    validate_secret_safety(root, &record, &input_bytes, &mut blockers);
    match record
        .pointer("/releaseBlockerObservations")
        .and_then(Value::as_array)
    {
        Some(observations) if observations.is_empty() => {}
        Some(observations) => blocker(
            &mut blockers,
            "observed-failures",
            &format!(
                "{} Release Blocker observation(s) were recorded",
                observations.len()
            ),
        ),
        None => blocker(
            &mut blockers,
            "observed-failures",
            "releaseBlockerObservations must explicitly record an empty list",
        ),
    }

    let report = json!({
        "status": if blockers.is_empty() { "pass" } else { "fail" },
        "version": env!("CARGO_PKG_VERSION"),
        "candidateId": record.pointer("/candidateId").cloned().unwrap_or(Value::Null),
        "commit": record.pointer("/commit").cloned().unwrap_or(Value::Null),
        "operatorRecordSha256": format!("{:x}", Sha256::digest(&input_bytes)),
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

fn validate_independence(record: &Value, blockers: &mut Vec<Value>) {
    for (pointer, expected, reason) in [
        (
            "/operator/attested",
            true,
            "operator attestation is missing",
        ),
        (
            "/operator/isDeveloper",
            false,
            "operator is part of development",
        ),
        (
            "/operator/receivedGoalOnly",
            true,
            "operator received a step-by-step script",
        ),
        (
            "/operator/assistanceRequired",
            false,
            "operator required assistance",
        ),
        (
            "/environment/standardUser",
            true,
            "run did not use a standard Windows user",
        ),
        (
            "/environment/repositoryPresent",
            false,
            "repository was present",
        ),
        (
            "/environment/toolchainsPresent",
            false,
            "toolchains were present",
        ),
        (
            "/environment/fixturesPresent",
            false,
            "fixtures were present",
        ),
        (
            "/environment/developerUtilitiesPresent",
            false,
            "developer utilities were present",
        ),
    ] {
        if record.pointer(pointer).and_then(Value::as_bool) != Some(expected) {
            blocker(blockers, "operator-independence", reason);
        }
    }
    if record
        .pointer("/environment/windowsVersion")
        .and_then(Value::as_str)
        .is_none_or(|value| {
            !value.to_ascii_lowercase().contains("windows 11")
                || !value.to_ascii_lowercase().contains("x64")
        })
    {
        blocker(
            blockers,
            "operator-environment",
            "Windows 11 x64 was not recorded",
        );
    }
    for (installed, expected, label) in [
        (
            "/environment/installedCandidateId",
            "/candidateId",
            "candidate ID",
        ),
        (
            "/environment/installedCommit",
            "/commit",
            "candidate commit",
        ),
    ] {
        if record.pointer(installed) != record.pointer(expected)
            || !non_empty_string(record.pointer(installed))
        {
            blocker(
                blockers,
                "installed-identity",
                &format!("installed {label} does not match the operator record"),
            );
        }
    }
}

fn validate_run(root: &Path, record: &Value, blockers: &mut Vec<Value>) {
    if record
        .pointer("/independentRun/status")
        .and_then(Value::as_str)
        != Some("pass")
    {
        blocker(blockers, "independent-run", "independent run did not pass");
    }
    for (field, label) in [
        (
            "campusSearchedAndConfirmed",
            "search and confirm a Campus Target",
        ),
        (
            "campusScopedProjectCreated",
            "create a campus-scoped project",
        ),
        ("boundaryConfirmed", "confirm a Campus Boundary"),
        (
            "fiveCategoryReviewCompleted",
            "complete five-category review",
        ),
        ("saveReopenCompleted", "save and reopen"),
        ("generated", "generate"),
        ("exported", "export"),
    ] {
        if record
            .pointer(&format!("/independentRun/steps/{field}"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            blocker(
                blockers,
                "independent-run",
                &format!("operator did not {label}"),
            );
        }
    }
    validate_evidence_array(
        root,
        record.pointer("/independentRun/evidence"),
        "independent-run",
        blockers,
    );
    for field in [
        "hesitations",
        "errors",
        "assistance",
        "abandonments",
        "blockingDeveloperExplanations",
    ] {
        if record
            .pointer(&format!("/independentRun/{field}"))
            .and_then(Value::as_array)
            .is_none()
        {
            blocker(
                blockers,
                "operator-observations",
                &format!("{field} must be recorded explicitly"),
            );
        }
    }
    if record
        .pointer("/independentRun/assistance")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("affectedIndependence").and_then(Value::as_bool) != Some(false)
            })
        })
    {
        blocker(
            blockers,
            "operator-independence",
            "assistance affected independence or was not classified",
        );
    }
    if record
        .pointer("/independentRun/abandonments")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("affectedCompletion").and_then(Value::as_bool) != Some(false))
        })
    {
        blocker(
            blockers,
            "operator-independence",
            "abandonment affected completion or was not classified",
        );
    }
    if record
        .pointer("/independentRun/blockingDeveloperExplanations")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
    {
        blocker(
            blockers,
            "operator-independence",
            "a blocking developer explanation was recorded",
        );
    }
}

fn validate_guidance_and_shortcuts(root: &Path, record: &Value, blockers: &mut Vec<Value>) {
    for (pointer, label) in [
        (
            "/guidance/firstRunAppeared",
            "first-run guidance did not appear",
        ),
        ("/guidance/skipWorked", "guidance could not be skipped"),
        (
            "/guidance/teachesCampusBeforeProject",
            "guidance did not teach campus-before-project",
        ),
        ("/guidance/reopenedBy/f1", "guidance did not reopen with F1"),
        (
            "/guidance/reopenedBy/questionMark",
            "guidance did not reopen with ?",
        ),
        (
            "/guidance/reopenedBy/settings",
            "guidance did not reopen through Settings",
        ),
        (
            "/shortcuts/fixedSetExercised",
            "fixed shortcut set was not exercised",
        ),
        (
            "/shortcuts/disabledReasonsVisible",
            "unavailable actions did not explain themselves",
        ),
        (
            "/shortcuts/escapeUnwindsOneLayer",
            "Esc did not unwind one layer",
        ),
    ] {
        if record.pointer(pointer).and_then(Value::as_bool) != Some(true) {
            blocker(blockers, "guidance-shortcuts", label);
        }
    }
    for context in [
        "textFocus",
        "modalStack",
        "activeMapTool",
        "vertexSelection",
        "history",
        "workflowStage",
    ] {
        if record
            .pointer(&format!("/shortcuts/contexts/{context}"))
            .and_then(Value::as_bool)
            != Some(true)
        {
            blocker(
                blockers,
                "shortcut-contexts",
                &format!("shortcut context was not exercised: {context}"),
            );
        }
    }
    validate_evidence_array(
        root,
        record.pointer("/guidance/evidence"),
        "guidance-shortcuts",
        blockers,
    );
    validate_evidence_array(
        root,
        record.pointer("/shortcuts/evidence"),
        "guidance-shortcuts",
        blockers,
    );
}

fn validate_localisation(root: &Path, record: &Value, blockers: &mut Vec<Value>) {
    let full_run = record
        .pointer("/localisation/fullRunLocale")
        .and_then(Value::as_str);
    if !matches!(full_run, Some("zh-CN" | "en")) {
        blocker(
            blockers,
            "localisation",
            "full run locale must be zh-CN or en",
        );
    }
    let Some(sweeps) = record
        .pointer("/localisation/sweeps")
        .and_then(Value::as_array)
    else {
        blocker(
            blockers,
            "localisation",
            "Chinese and English sweeps are missing",
        );
        return;
    };
    let locales = sweeps
        .iter()
        .filter_map(|sweep| sweep.get("locale").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if locales != BTreeSet::from(["en", "zh-CN"]) || sweeps.len() != 2 {
        blocker(
            blockers,
            "localisation",
            "exactly one zh-CN and one en sweep are required",
        );
    }
    for sweep in sweeps {
        let locale = sweep
            .get("locale")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if sweep.get("allScreens").and_then(Value::as_bool) != Some(true)
            || sweep.get("keyErrorStates").and_then(Value::as_bool) != Some(true)
            || sweep
                .get("defects")
                .and_then(Value::as_array)
                .is_none_or(|items| !items.is_empty())
        {
            blocker(blockers, "localisation", &format!("{locale} sweep is incomplete or contains mixed-language/mojibake/layout defects"));
        }
        validate_evidence_array(root, sweep.get("evidence"), "localisation", blockers);
    }
}

fn validate_scaling(root: &Path, record: &Value, blockers: &mut Vec<Value>) {
    let Some(scales) = record.pointer("/scaling").and_then(Value::as_array) else {
        blocker(
            blockers,
            "windows-scaling",
            "Windows scale evidence is missing",
        );
        return;
    };
    let percents = scales
        .iter()
        .filter_map(|item| item.get("percent").and_then(Value::as_u64))
        .collect::<BTreeSet<_>>();
    if percents != BTreeSet::from([100, 125, 150]) || scales.len() != 3 {
        blocker(
            blockers,
            "windows-scaling",
            "exactly 100%, 125%, and 150% scale evidence is required",
        );
    }
    for scale in scales {
        let percent = scale.get("percent").and_then(Value::as_u64).unwrap_or(0);
        for field in [
            "principalFlowPassed",
            "visibleFocus",
            "modalsEscapable",
            "keyboardCoreNonMap",
        ] {
            if scale.get(field).and_then(Value::as_bool) != Some(true) {
                blocker(
                    blockers,
                    "windows-scaling",
                    &format!("{percent}% scale failed {field}"),
                );
            }
        }
        validate_evidence_array(root, scale.get("evidence"), "windows-scaling", blockers);
    }
}

fn validate_secret_safety(
    root: &Path,
    record: &Value,
    input_bytes: &[u8],
    blockers: &mut Vec<Value>,
) {
    if record
        .pointer("/secretSafety/fieldsMaskedByDefault")
        .and_then(Value::as_bool)
        != Some(true)
    {
        blocker(
            blockers,
            "secret-safety",
            "secret fields were not masked by default",
        );
    }
    if record
        .pointer("/secretSafety/credentialFindings")
        .and_then(Value::as_array)
        .is_none_or(|items| !items.is_empty())
    {
        blocker(
            blockers,
            "secret-safety",
            "credential findings are missing or non-empty",
        );
    }
    if contains_forbidden_secret_material(input_bytes) {
        blocker(
            blockers,
            "secret-safety",
            "operator record contains forbidden credential material",
        );
    }
    let Some(artifacts) = record
        .pointer("/secretSafety/scannedArtifacts")
        .and_then(Value::as_array)
    else {
        blocker(
            blockers,
            "secret-safety",
            "secret-safe artifact scan is missing",
        );
        return;
    };
    if artifacts.is_empty() {
        blocker(
            blockers,
            "secret-safety",
            "at least one screenshot/log/project/portable/evidence artifact must be scanned",
        );
    }
    let artifact_kinds = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("kind").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let required_kinds = BTreeSet::from([
        "evidence",
        "log",
        "portable-project",
        "project",
        "screenshot",
    ]);
    if artifact_kinds != required_kinds {
        blocker(
            blockers,
            "secret-safety",
            "scanned artifact classes must cover screenshot, log, project, portable-project, and evidence exactly",
        );
    }
    let screenshot_artifacts = artifacts
        .iter()
        .filter(|artifact| artifact.get("kind").and_then(Value::as_str) == Some("screenshot"))
        .filter_map(|artifact| artifact.get("path").and_then(Value::as_str))
        .map(normalise_evidence_path)
        .collect::<BTreeSet<_>>();
    for artifact in artifacts
        .iter()
        .filter(|artifact| artifact.get("kind").and_then(Value::as_str) == Some("screenshot"))
    {
        if artifact
            .get("visualInspectionPassed")
            .and_then(Value::as_bool)
            != Some(true)
        {
            blocker(
                blockers,
                "secret-safety",
                "every screenshot requires a passed human visual inspection for exposed credentials",
            );
        }
    }
    for screenshot in referenced_screenshots(record) {
        if !screenshot_artifacts.contains(&screenshot) {
            blocker(
                blockers,
                "secret-safety",
                &format!("referenced screenshot was not included in the secret scan: {screenshot}"),
            );
        }
    }
    match evidence_directory_screenshots(root) {
        Ok(screenshots) => {
            for screenshot in screenshots {
                if !screenshot_artifacts.contains(&screenshot) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("evidence-directory screenshot lacks secret review: {screenshot}"),
                    );
                }
            }
        }
        Err(error) => blocker(
            blockers,
            "secret-safety",
            &format!("could not enumerate evidence screenshots: {error}"),
        ),
    }
    match evidence_directory_files(root) {
        Ok(files) => {
            for (relative, bytes) in files {
                if is_archive_path(&relative) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("unsupported archive evidence must be unpacked before review: {relative}"),
                    );
                }
                if contains_forbidden_secret_material(&bytes) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("evidence file contains credential material: {relative}"),
                    );
                }
                if let Some(name) = configured_secret_leak(&bytes) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("evidence file leaked configured {name}: {relative}"),
                    );
                }
            }
        }
        Err(error) => blocker(
            blockers,
            "secret-safety",
            &format!("could not scan evidence files: {error}"),
        ),
    }
    for artifact in artifacts {
        let Some(relative) = artifact.get("path").and_then(Value::as_str) else {
            blocker(
                blockers,
                "secret-safety",
                "scanned artifact path is missing",
            );
            continue;
        };
        match read_evidence(root, relative) {
            Ok(bytes) => {
                let actual = format!("{:x}", Sha256::digest(&bytes));
                if artifact.get("sha256").and_then(Value::as_str) != Some(actual.as_str()) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("scanned artifact digest mismatch: {relative}"),
                    );
                }
                if contains_forbidden_secret_material(&bytes) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("scanned artifact contains credential material: {relative}"),
                    );
                }
                if let Some(name) = configured_secret_leak(&bytes) {
                    blocker(
                        blockers,
                        "secret-safety",
                        &format!("scanned artifact leaked configured {name}: {relative}"),
                    );
                }
            }
            Err(error) => blocker(
                blockers,
                "secret-safety",
                &format!("invalid scanned artifact {relative}: {error}"),
            ),
        }
    }
    if let Some(name) = configured_secret_leak(input_bytes) {
        blocker(
            blockers,
            "secret-safety",
            &format!("operator record leaked configured {name}"),
        );
    }
}

fn validate_evidence_array(
    root: &Path,
    value: Option<&Value>,
    case_id: &str,
    blockers: &mut Vec<Value>,
) {
    let Some(paths) = value.and_then(Value::as_array) else {
        blocker(blockers, case_id, "evidence list is missing");
        return;
    };
    if paths.is_empty() {
        blocker(blockers, case_id, "evidence list is empty");
    }
    for relative in paths {
        if relative
            .as_str()
            .is_none_or(|path| read_evidence(root, path).is_err())
        {
            blocker(
                blockers,
                case_id,
                "evidence path is unsafe, missing, or empty",
            );
        }
    }
}

fn read_evidence(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("evidence path escapes the operator-record directory".into());
    }
    let bytes = std::fs::read(root.join(relative)).map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        Err("evidence file is empty".into())
    } else {
        Ok(bytes)
    }
}

fn contains_forbidden_secret_material(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes)
        .to_ascii_lowercase()
        .replace("authorization: bearer [redacted]", "")
        .replace("authorization: bearer <redacted>", "")
        .replace("\"authorization\":\"bearer [redacted]\"", "")
        .replace("\"authorization\": \"bearer [redacted]\"", "");
    [
        "authorization: bearer ",
        "\"authorization\":\"bearer ",
        "\"authorization\": \"bearer ",
        "gaode_js_api_key=",
        "gaode_security_code=",
        "campus_acquisition_service_secret=",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn referenced_screenshots(record: &Value) -> BTreeSet<String> {
    let mut screenshots = BTreeSet::new();
    for pointer in [
        "/independentRun/evidence",
        "/guidance/evidence",
        "/shortcuts/evidence",
    ] {
        collect_screenshot_paths(record.pointer(pointer), &mut screenshots);
    }
    for collection in ["/localisation/sweeps", "/scaling"] {
        if let Some(items) = record.pointer(collection).and_then(Value::as_array) {
            for item in items {
                collect_screenshot_paths(item.get("evidence"), &mut screenshots);
            }
        }
    }
    screenshots
}

fn collect_screenshot_paths(value: Option<&Value>, screenshots: &mut BTreeSet<String>) {
    let Some(paths) = value.and_then(Value::as_array) else {
        return;
    };
    for path in paths.iter().filter_map(Value::as_str) {
        if is_screenshot_path(path) {
            screenshots.insert(normalise_evidence_path(path));
        }
    }
}

fn evidence_directory_files(root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut Vec<(String, Vec<u8>)>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if file_type.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                files.push((normalise_evidence_path(&relative), bytes));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn evidence_directory_screenshots(root: &Path) -> Result<BTreeSet<String>, String> {
    fn visit(
        root: &Path,
        directory: &Path,
        screenshots: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            if file_type.is_dir() {
                visit(root, &entry.path(), screenshots)?;
            } else if file_type.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy()
                    .to_string();
                if is_screenshot_path(&relative) {
                    screenshots.insert(normalise_evidence_path(&relative));
                }
            }
        }
        Ok(())
    }

    let mut screenshots = BTreeSet::new();
    visit(root, root, &mut screenshots)?;
    Ok(screenshots)
}

fn is_archive_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".zip", ".7z", ".rar", ".tar", ".gz", ".tgz", ".bz2", ".xz"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn is_screenshot_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".tif", ".tiff",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

fn normalise_evidence_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn configured_secret_leak(bytes: &[u8]) -> Option<&'static str> {
    for name in [
        "GAODE_JS_API_KEY",
        "VITE_GAODE_JS_API_KEY",
        "GAODE_SECURITY_CODE",
        "VITE_GAODE_SECURITY_CODE",
        "CAMPUS_ACQUISITION_SERVICE_SECRET",
    ] {
        if let Ok(secret) = std::env::var(name) {
            if !secret.is_empty()
                && bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes())
            {
                return Some(name);
            }
        }
    }
    None
}

fn require_string(record: &Value, pointer: &str, label: &str, blockers: &mut Vec<Value>) {
    if !non_empty_string(record.pointer(pointer)) {
        blocker(blockers, label, &format!("missing {label}"));
    }
}

fn non_empty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn blocker(blockers: &mut Vec<Value>, case_id: &str, reason: &str) {
    blockers.push(json!({"caseId":case_id,"reason":reason}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn complete_independent_operator_record_passes_the_public_evidence_seam() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "goal.png",
            "campus.png",
            "review.png",
            "export.png",
            "guidance.png",
            "shortcuts.png",
            "zh-sweep.png",
            "en-sweep.png",
            "scale-100.png",
            "scale-125.png",
            "scale-150.png",
        ] {
            std::fs::write(root.path().join(name), b"evidence").unwrap();
        }
        let mut scanned_artifacts = [
            "goal.png",
            "campus.png",
            "review.png",
            "export.png",
            "guidance.png",
            "shortcuts.png",
            "zh-sweep.png",
            "en-sweep.png",
            "scale-100.png",
            "scale-125.png",
            "scale-150.png",
        ]
        .into_iter()
        .map(|name| {
            let path = root.path().join(name);
            json!({"kind":"screenshot","path":name,"sha256":format!("{:x}", Sha256::digest(std::fs::read(path).unwrap())),"visualInspectionPassed":true})
        })
        .collect::<Vec<_>>();
        scanned_artifacts.extend(
            [
                ("log", "diagnostics.log"),
                ("project", "project.campus.json"),
                ("portable-project", "portable.campus.json"),
                ("evidence", "evidence.json"),
            ]
            .into_iter()
            .map(|(kind, name)| {
                let path = root.path().join(name);
                std::fs::write(&path, b"provider request completed [REDACTED]").unwrap();
                json!({"kind":kind,"path":name,"sha256":format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))})
            }),
        );
        let record = json!({
            "candidateId":"candidate-1",
            "commit":"0123456789abcdef",
            "operator":{
                "name":"Independent operator",
                "attested":true,
                "isDeveloper":false,
                "receivedGoalOnly":true,
                "assistanceRequired":false
            },
            "environment":{
                "windowsVersion":"Windows 11 x64",
                "standardUser":true,
                "repositoryPresent":false,
                "toolchainsPresent":false,
                "fixturesPresent":false,
                "developerUtilitiesPresent":false,
                "installedCandidateId":"candidate-1",
                "installedCommit":"0123456789abcdef"
            },
            "independentRun":{
                "status":"pass",
                "steps":{
                    "campusSearchedAndConfirmed":true,
                    "campusScopedProjectCreated":true,
                    "boundaryConfirmed":true,
                    "fiveCategoryReviewCompleted":true,
                    "saveReopenCompleted":true,
                    "generated":true,
                    "exported":true
                },
                "evidence":["goal.png","campus.png","review.png","export.png"],
                "hesitations":[],"errors":[],"assistance":[],"abandonments":[],
                "blockingDeveloperExplanations":[]
            },
            "guidance":{
                "firstRunAppeared":true,"skipWorked":true,"teachesCampusBeforeProject":true,
                "reopenedBy":{"f1":true,"questionMark":true,"settings":true},
                "evidence":["guidance.png"]
            },
            "shortcuts":{
                "fixedSetExercised":true,
                "contexts":{"textFocus":true,"modalStack":true,"activeMapTool":true,"vertexSelection":true,"history":true,"workflowStage":true},
                "disabledReasonsVisible":true,"escapeUnwindsOneLayer":true,
                "evidence":["shortcuts.png"]
            },
            "localisation":{
                "fullRunLocale":"zh-CN",
                "sweeps":[
                    {"locale":"zh-CN","allScreens":true,"keyErrorStates":true,"defects":[],"evidence":["zh-sweep.png"]},
                    {"locale":"en","allScreens":true,"keyErrorStates":true,"defects":[],"evidence":["en-sweep.png"]}
                ]
            },
            "scaling":[
                {"percent":100,"principalFlowPassed":true,"visibleFocus":true,"modalsEscapable":true,"keyboardCoreNonMap":true,"evidence":["scale-100.png"]},
                {"percent":125,"principalFlowPassed":true,"visibleFocus":true,"modalsEscapable":true,"keyboardCoreNonMap":true,"evidence":["scale-125.png"]},
                {"percent":150,"principalFlowPassed":true,"visibleFocus":true,"modalsEscapable":true,"keyboardCoreNonMap":true,"evidence":["scale-150.png"]}
            ],
            "secretSafety":{
                "fieldsMaskedByDefault":true,
                "scannedArtifacts":scanned_artifacts,
                "credentialFindings":[]
            },
            "releaseBlockerObservations":[]
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
        assert!(report["releaseBlockers"].as_array().unwrap().is_empty());
        assert!(output.is_file());
    }

    #[test]
    fn missing_independence_and_leaked_credentials_are_release_blockers() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(
            &input,
            br#"{"candidateId":"candidate","commit":"commit","notes":"Authorization: Bearer exposed"}"#,
        )
        .unwrap();

        let report = write_report(&input, &output).unwrap();

        assert_eq!(report["status"], "fail");
        let blockers = report["releaseBlockers"].as_array().unwrap();
        assert!(blockers.len() >= 5);
        assert!(blockers
            .iter()
            .any(|item| item["caseId"] == "secret-safety"));
    }

    #[test]
    fn every_secret_bearing_artifact_class_must_be_scanned() {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("diagnostics.log");
        std::fs::write(&log, b"safe").unwrap();
        let screenshot = root.path().join("screen.png");
        std::fs::write(&screenshot, b"safe image").unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, serde_json::to_vec(&json!({
            "candidateId":"candidate", "commit":"commit",
            "secretSafety":{
                "fieldsMaskedByDefault":true,
                "scannedArtifacts":[
                    {"kind":"log","path":"diagnostics.log","sha256":format!("{:x}", Sha256::digest(b"safe"))},
                    {"kind":"screenshot","path":"screen.png","sha256":format!("{:x}", Sha256::digest(b"safe image"))}
                ],
                "credentialFindings":[]
            },
            "releaseBlockerObservations":[]
        })).unwrap()).unwrap();

        let report = write_report(&input, &output).unwrap();

        assert!(report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"]
                        .as_str()
                        .unwrap()
                        .contains("artifact classes")
            }));
        assert!(report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"]
                        .as_str()
                        .unwrap()
                        .contains("visual inspection")
            }));
    }

    #[test]
    fn blocking_help_abandonment_and_missing_interaction_evidence_are_blockers() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, serde_json::to_vec(&json!({
            "candidateId":"candidate", "commit":"commit",
            "operator":{"assistanceRequired":false},
            "independentRun":{
                "status":"pass", "evidence":[], "hesitations":[], "errors":[],
                "assistance":["developer explained the next workflow step"],
                "abandonments":["operator could not continue"],
                "blockingDeveloperExplanations":["developer completed boundary confirmation"]
            },
            "guidance":{
                "firstRunAppeared":true,"skipWorked":true,"teachesCampusBeforeProject":true,
                "reopenedBy":{"f1":true,"questionMark":true,"settings":true}
            },
            "shortcuts":{
                "fixedSetExercised":true,
                "contexts":{"textFocus":true,"modalStack":true,"activeMapTool":true,"vertexSelection":true,"history":true,"workflowStage":true},
                "disabledReasonsVisible":true,"escapeUnwindsOneLayer":true
            },
            "releaseBlockerObservations":[]
        })).unwrap()).unwrap();

        let report = write_report(&input, &output).unwrap();
        let blockers = report["releaseBlockers"].as_array().unwrap();

        assert!(blockers
            .iter()
            .any(|item| item["caseId"] == "operator-independence"
                && item["reason"].as_str().unwrap().contains("assistance")));
        assert!(blockers
            .iter()
            .any(|item| item["caseId"] == "operator-independence"
                && item["reason"].as_str().unwrap().contains("abandonment")));
        assert!(blockers
            .iter()
            .any(|item| item["caseId"] == "operator-independence"
                && item["reason"]
                    .as_str()
                    .unwrap()
                    .contains("developer explanation")));
        assert!(blockers
            .iter()
            .any(|item| item["caseId"] == "guidance-shortcuts"
                && item["reason"].as_str().unwrap().contains("evidence")));
    }

    #[test]
    fn redacted_authorization_text_is_not_a_credential_leak() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(
            &input,
            br#"{"candidateId":"candidate","commit":"commit","notes":"Authorization: Bearer [REDACTED]"}"#,
        )
        .unwrap();

        let report = write_report(&input, &output).unwrap();

        assert!(!report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"]
                        .as_str()
                        .unwrap()
                        .contains("forbidden credential material")
            }));
    }

    #[test]
    fn evidence_capture_help_does_not_disqualify_independent_completion() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, serde_json::to_vec(&json!({
            "candidateId":"candidate", "commit":"commit",
            "operator":{"assistanceRequired":false},
            "independentRun":{
                "assistance":[{"observation":"release owner explained how to save a screenshot","affectedIndependence":false}],
                "abandonments":[], "blockingDeveloperExplanations":[]
            }
        })).unwrap()).unwrap();

        let report = write_report(&input, &output).unwrap();

        assert!(!report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "operator-independence"
                    && item["reason"].as_str().unwrap().contains("assistance")
            }));
    }

    #[test]
    fn every_screenshot_in_the_evidence_directory_requires_visual_review() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "reviewed.png",
            "extra.bmp",
            "log.txt",
            "project.json",
            "portable.json",
            "notes.json",
        ] {
            std::fs::write(root.path().join(name), b"safe").unwrap();
        }
        let artifact = |kind: &str, path: &str, visual: Option<bool>| {
            let mut value =
                json!({"kind":kind,"path":path,"sha256":format!("{:x}", Sha256::digest(b"safe"))});
            if let Some(visual) = visual {
                value["visualInspectionPassed"] = json!(visual);
            }
            value
        };
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(&input, serde_json::to_vec(&json!({
            "candidateId":"candidate", "commit":"commit",
            "secretSafety":{
                "fieldsMaskedByDefault":true,"credentialFindings":[],
                "scannedArtifacts":[
                    artifact("screenshot","reviewed.png",Some(true)),
                    artifact("log","log.txt",None), artifact("project","project.json",None),
                    artifact("portable-project","portable.json",None), artifact("evidence","notes.json",None)
                ]
            },
            "releaseBlockerObservations":[]
        })).unwrap()).unwrap();

        let report = write_report(&input, &output).unwrap();

        assert!(report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"].as_str().unwrap().contains("extra.bmp")
            }));
    }

    #[test]
    fn unlisted_evidence_files_are_still_scanned_for_credentials() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("unlisted.log"),
            b"Authorization: Bearer exposed-token",
        )
        .unwrap();
        std::fs::write(root.path().join("opaque.zip"), b"compressed").unwrap();
        for name in [
            "screen.png",
            "listed.log",
            "project.json",
            "portable.json",
            "notes.json",
        ] {
            std::fs::write(root.path().join(name), b"safe").unwrap();
        }
        let artifact = |kind: &str, path: &str, visual: Option<bool>| {
            let mut value =
                json!({"kind":kind,"path":path,"sha256":format!("{:x}", Sha256::digest(b"safe"))});
            if let Some(visual) = visual {
                value["visualInspectionPassed"] = json!(visual);
            }
            value
        };
        let input = root.path().join("operator-record.json");
        let output = root.path().join("report.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&json!({
                "candidateId":"candidate", "commit":"commit",
                "secretSafety":{
                    "fieldsMaskedByDefault":true,
                    "credentialFindings":[],
                    "scannedArtifacts":[
                        artifact("screenshot","screen.png",Some(true)),
                        artifact("log","listed.log",None),
                        artifact("project","project.json",None),
                        artifact("portable-project","portable.json",None),
                        artifact("evidence","notes.json",None)
                    ]
                },
                "releaseBlockerObservations":[]
            }))
            .unwrap(),
        )
        .unwrap();

        let report = write_report(&input, &output).unwrap();

        assert!(report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"].as_str().unwrap().contains("unlisted.log")
            }));
        assert!(report["releaseBlockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["caseId"] == "secret-safety"
                    && item["reason"].as_str().unwrap().contains("opaque.zip")
            }));
    }
}
