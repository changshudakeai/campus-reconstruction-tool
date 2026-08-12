//! xtask cache-report —— Cargo 构建产物的只读体积与重复代际统计。
//!
//! 本命令只遍历 Cargo 实际 `target_directory` 并读取文件元数据；不会删除、
//! 移动或改写任何构建产物。输出按固定键和稳定排序生成，便于清理前后对比。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const GIB: u64 = 1024 * 1024 * 1024;
const TARGET_WARNING_BYTES: u64 = 30 * GIB;
const TARGET_SEVERE_BYTES: u64 = 60 * GIB;
const FREE_SPACE_SEVERE_BYTES: u64 = 50 * GIB;
const TOP_LIMIT: usize = 20;

const DIRECTORY_KEYS: [&str; 4] = ["debug/deps", "debug/incremental", "debug/build", "release"];
const ARTIFACT_KEYS: [&str; 4] = ["PDB", "EXE", "RLIB", "RMETA"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Severity {
    Normal,
    Warning,
    Severe,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "正常",
            Self::Warning => "警告",
            Self::Severe => "严重警告",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FileStats {
    count: u64,
    bytes: u64,
}

#[derive(Debug, Default)]
struct LogicalTargetStats {
    generations: HashSet<String>,
    files: u64,
    bytes: u64,
}

#[derive(Debug)]
struct ScanResult {
    total: FileStats,
    directories: BTreeMap<&'static str, FileStats>,
    artifacts: BTreeMap<&'static str, FileStats>,
    logical_targets: HashMap<String, LogicalTargetStats>,
}

impl Default for ScanResult {
    fn default() -> Self {
        Self {
            total: FileStats::default(),
            directories: DIRECTORY_KEYS
                .into_iter()
                .map(|key| (key, FileStats::default()))
                .collect(),
            artifacts: ARTIFACT_KEYS
                .into_iter()
                .map(|key| (key, FileStats::default()))
                .collect(),
            logical_targets: HashMap::new(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct RankedTarget {
    name: String,
    generations: usize,
    files: u64,
    bytes: u64,
}

fn normalize_cargo_artifact(file_name: &str) -> (&str, Option<&str>) {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(before_extension, _)| before_extension);
    let Some((logical_name, hash)) = stem.rsplit_once('-') else {
        return (file_name, None);
    };
    if hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        (logical_name, Some(hash))
    } else {
        (file_name, None)
    }
}

fn target_severity(bytes: u64) -> Severity {
    if bytes > TARGET_SEVERE_BYTES {
        Severity::Severe
    } else if bytes > TARGET_WARNING_BYTES {
        Severity::Warning
    } else {
        Severity::Normal
    }
}

fn free_space_severity(bytes: u64) -> Severity {
    if bytes < FREE_SPACE_SEVERE_BYTES {
        Severity::Severe
    } else {
        Severity::Normal
    }
}

fn artifact_kind(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    ARTIFACT_KEYS
        .into_iter()
        .find(|kind| extension.eq_ignore_ascii_case(kind))
}

fn directory_bucket(relative: &Path) -> Option<&'static str> {
    let mut components = relative.components();
    let first = components.next()?.as_os_str().to_str()?;
    if first.eq_ignore_ascii_case("release") {
        return Some("release");
    }
    if !first.eq_ignore_ascii_case("debug") {
        return None;
    }
    let second = components.next()?.as_os_str().to_str()?;
    DIRECTORY_KEYS.into_iter().find(|key| {
        key.strip_prefix("debug/")
            .is_some_and(|tail| second.eq_ignore_ascii_case(tail))
    })
}

fn add_file(result: &mut ScanResult, target: &Path, path: &Path, bytes: u64) {
    result.total.count += 1;
    result.total.bytes += bytes;

    if let Ok(relative) = path.strip_prefix(target) {
        if let Some(bucket) = directory_bucket(relative) {
            let stats = result.directories.get_mut(bucket).expect("固定目录键存在");
            stats.count += 1;
            stats.bytes += bytes;
        }
    }

    if let Some(kind) = artifact_kind(path) {
        let stats = result.artifacts.get_mut(kind).expect("固定产物键存在");
        stats.count += 1;
        stats.bytes += bytes;
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    let (logical_name, hash) = normalize_cargo_artifact(file_name);
    let Some(hash) = hash else {
        return;
    };
    let stats = result
        .logical_targets
        .entry(logical_name.to_owned())
        .or_default();
    stats.generations.insert(hash.to_ascii_lowercase());
    stats.files += 1;
    stats.bytes += bytes;
}

fn scan_target(target: &Path) -> anyhow::Result<ScanResult> {
    let mut result = ScanResult::default();
    if !target.exists() {
        return Ok(result);
    }

    let mut pending = vec![target.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                add_file(&mut result, target, &path, entry.metadata()?.len());
            }
        }
    }
    Ok(result)
}

fn ranked_targets(result: &ScanResult) -> Vec<RankedTarget> {
    result
        .logical_targets
        .iter()
        .map(|(name, stats)| RankedTarget {
            name: name.clone(),
            generations: stats.generations.len(),
            files: stats.files,
            bytes: stats.bytes,
        })
        .collect()
}

fn sort_by_generations(targets: &mut [RankedTarget]) {
    targets.sort_by(|left, right| {
        right
            .generations
            .cmp(&left.generations)
            .then_with(|| right.bytes.cmp(&left.bytes))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn sort_by_bytes(targets: &mut [RankedTarget]) {
    targets.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| right.generations.cmp(&left.generations))
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn unix_milliseconds() -> anyhow::Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

fn format_bytes(bytes: u64) -> String {
    format!("{:.2} GiB ({bytes} bytes)", bytes as f64 / GIB as f64)
}

#[cfg(windows)]
fn available_space(path: &Path) -> anyhow::Result<u64> {
    #[allow(
        clippy::disallowed_methods,
        reason = "xtask 需调用 Windows PowerShell 读取目标卷剩余空间，不参与业务执行"
    )]
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$item=Get-Item -LiteralPath $env:MCREBUILD_CACHE_REPORT_PATH; \
             [Console]::Out.Write((Get-PSDrive -Name $item.PSDrive.Name).Free)",
        ])
        .env("MCREBUILD_CACHE_REPORT_PATH", path)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "PowerShell 查询磁盘剩余空间失败: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}

#[cfg(not(windows))]
fn available_space(path: &Path) -> anyhow::Result<u64> {
    #[allow(
        clippy::disallowed_methods,
        reason = "xtask 在非 Windows 环境调用 df 读取目标卷剩余空间，不参与业务执行"
    )]
    let output = std::process::Command::new("df")
        .args(["-Pk"])
        .arg(path)
        .output()?;
    anyhow::ensure!(output.status.success(), "df 查询磁盘剩余空间失败");
    let stdout = String::from_utf8(output.stdout)?;
    let available_kib = stdout
        .lines()
        .last()
        .and_then(|line| line.split_whitespace().nth(3))
        .ok_or_else(|| anyhow::anyhow!("无法解析 df 输出"))?
        .parse::<u64>()?;
    Ok(available_kib * 1024)
}

fn report_ranked(title: &str, targets: &[RankedTarget]) {
    println!("{title}");
    if targets.is_empty() {
        println!("  （无）");
        return;
    }
    for (index, target) in targets.iter().take(TOP_LIMIT).enumerate() {
        println!(
            "  {:>2}. {} | generations={} | files={} | {}",
            index + 1,
            target.name,
            target.generations,
            target.files,
            format_bytes(target.bytes)
        );
    }
}

/// `cargo xtask cache-report` 入口：只读扫描实际 Cargo target 目录。
pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    let started_unix_ms = unix_milliseconds()?;
    let started = Instant::now();
    let metadata = crate::workspace_metadata(root)?;
    let target = PathBuf::from(metadata.target_directory.as_str());
    let result = scan_target(&target)?;
    let free_bytes = available_space(if target.exists() { &target } else { root })?;
    let finished_unix_ms = unix_milliseconds()?;

    println!("cache-report schema: 1");
    println!("measurement_started_unix_ms: {started_unix_ms}");
    println!("measurement_finished_unix_ms: {finished_unix_ms}");
    println!("measurement_elapsed_ms: {}", started.elapsed().as_millis());
    println!("target_directory: {}", target.display());
    println!(
        "target_total: {} [{}] files={}",
        format_bytes(result.total.bytes),
        target_severity(result.total.bytes).label(),
        result.total.count
    );

    println!("directories:");
    for key in DIRECTORY_KEYS {
        let stats = result.directories.get(key).expect("固定目录键存在");
        println!(
            "  {key}: {} files={}",
            format_bytes(stats.bytes),
            stats.count
        );
    }

    println!("artifacts:");
    for key in ARTIFACT_KEYS {
        let stats = result.artifacts.get(key).expect("固定产物键存在");
        println!(
            "  {key}: {} files={}",
            format_bytes(stats.bytes),
            stats.count
        );
    }

    let mut by_generations = ranked_targets(&result);
    by_generations.retain(|target| target.generations > 1);
    sort_by_generations(&mut by_generations);
    report_ranked("top_duplicate_generations:", &by_generations);

    let mut by_bytes = ranked_targets(&result);
    sort_by_bytes(&mut by_bytes);
    report_ranked("top_logical_targets_by_size:", &by_bytes);

    println!(
        "disk_free: {} [{}]",
        format_bytes(free_bytes),
        free_space_severity(free_bytes).label()
    );
    if target_severity(result.total.bytes) == Severity::Severe
        || free_space_severity(free_bytes) == Severity::Severe
    {
        println!("action: 仅告警；未自动清理任何 Cargo 缓存");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_a_trailing_cargo_hash_from_artifact_names() {
        assert_eq!(
            normalize_cargo_artifact("s1_08_boundary_export_flow-a1b2c3d4e5f60718.pdb"),
            ("s1_08_boundary_export_flow", Some("a1b2c3d4e5f60718"))
        );
        assert_eq!(
            normalize_cargo_artifact("libshared_domain_types-ABCDEF0123456789.rlib"),
            ("libshared_domain_types", Some("ABCDEF0123456789"))
        );
        assert_eq!(
            normalize_cargo_artifact("campus-tool-dev.exe"),
            ("campus-tool-dev.exe", None)
        );
        assert_eq!(
            normalize_cargo_artifact("report-deadbeef.txt"),
            ("report-deadbeef.txt", None)
        );
    }

    #[test]
    fn target_thresholds_match_the_cache_policy() {
        assert_eq!(target_severity(30 * GIB), Severity::Normal);
        assert_eq!(target_severity(30 * GIB + 1), Severity::Warning);
        assert_eq!(target_severity(60 * GIB), Severity::Warning);
        assert_eq!(target_severity(60 * GIB + 1), Severity::Severe);
    }

    #[test]
    fn low_free_space_is_severe() {
        assert_eq!(free_space_severity(50 * GIB), Severity::Normal);
        assert_eq!(free_space_severity(50 * GIB - 1), Severity::Severe);
    }

    #[test]
    fn classifies_requested_directory_and_artifact_buckets() {
        assert_eq!(
            directory_bucket(Path::new("debug/deps/a.pdb")),
            Some("debug/deps")
        );
        assert_eq!(
            directory_bucket(Path::new("release/deps/a.exe")),
            Some("release")
        );
        assert_eq!(artifact_kind(Path::new("a.PdB")), Some("PDB"));
        assert_eq!(artifact_kind(Path::new("a.d")), None);
    }

    #[test]
    fn rankings_are_deterministic_on_ties() {
        let mut targets = vec![
            RankedTarget {
                name: "zeta".to_owned(),
                generations: 2,
                files: 2,
                bytes: 10,
            },
            RankedTarget {
                name: "alpha".to_owned(),
                generations: 2,
                files: 2,
                bytes: 10,
            },
        ];
        sort_by_generations(&mut targets);
        assert_eq!(targets[0].name, "alpha");
    }
}
