//! xtask tidy —— 规模与卫生红线（执法清单 2.1，仿 rustc tidy / rust-analyzer）。
//!
//! 检查项：
//! 1. 单文件行数红线：`.rs` / `.slint` 文件 ≤ 1000 行（ADR-0017 十戒第 10 条）；
//!    豁免必须在文件内留显式标记 `<marker-filelength>: <理由>`，无理由的标记同样违规。
//! 2. 模块文档：每个 `.rs` 文件必须以 `//!` 模块文档开头（tests/、build.rs 豁免）。
//! 3. 半成品禁令：源码不得含待办/待修字样入主干（字样清单见 `forbidden_words`）。
//! 4. 单 crate 源文件数 ≤ 10（十戒第 10 条，由 `crate_file_count` 检查）。
//!
//! 所有检查是纯函数，本文件内附单元测试；`tests/enforcement.rs` 把
//! 真实 workspace 的全量扫描写成 `#[test]`（执法即测试）。

use std::path::{Path, PathBuf};

/// 单文件行数红线（ADR-0017：新代码 1000 行；rustc 老代码库才放宽到 3000）。
pub(crate) const MAX_FILE_LINES: usize = 1000;

/// 单 crate `src/` 下源文件数上限（ADR-0017 十戒第 10 条）。
pub(crate) const MAX_CRATE_SOURCE_FILES: usize = 10;

/// 行数豁免标记（运行时拼接，避免本文件自身含完整标记字样）。
fn filelength_marker() -> String {
    ["ignore-tidy-", "filelength"].concat()
}

/// 半成品字样（运行时拼接，避免 tidy 自查时误伤本文件）。
fn forbidden_words() -> [String; 3] {
    [
        ["TO", "DO"].concat(),
        ["FIX", "ME"].concat(),
        ["XX", "X"].concat(),
    ]
}

/// 对单个源文件内容执行全部 tidy 检查，返回违规描述（空 = 通过）。
pub(crate) fn check_file(path_display: &str, content: &str) -> Vec<String> {
    let mut violations = Vec::new();
    // Windows 编辑器常带 UTF-8 BOM，不得干扰首行 `//!` 判定。
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let is_rust = path_display.ends_with(".rs");

    check_line_limit(path_display, content, &mut violations);
    if is_rust {
        check_module_doc(path_display, content, &mut violations);
        check_forbidden_words(path_display, content, &mut violations);
    }
    violations
}

/// 检查 1：行数红线与豁免留痕。
fn check_line_limit(path_display: &str, content: &str, violations: &mut Vec<String>) {
    let marker = filelength_marker();
    let line_count = content.lines().count();
    let marker_line = content.lines().find(|line| line.contains(&marker));

    match marker_line {
        Some(line) => {
            // 豁免留痕纪律：标记必须带理由（`<marker>: 理由`）。
            let reason = line
                .split_once(&format!("{marker}:"))
                .map(|(_, rest)| rest.trim())
                .unwrap_or("");
            if reason.is_empty() {
                violations.push(format!(
                    "{path_display}: 行数豁免标记缺少理由，格式应为 `// {marker}: <为什么必须超长>`"
                ));
            }
        }
        None if line_count > MAX_FILE_LINES => {
            violations.push(format!(
                "{path_display}: 文件 {line_count} 行，超过 {MAX_FILE_LINES} 行红线（如确需豁免，\
                 在文件内添加 `// {marker}: <理由>` 并接受守门人评审）"
            ));
        }
        None => {}
    }
}

/// 检查 2：每个 .rs 文件必须以 `//!` 模块文档开头。
fn check_module_doc(path_display: &str, content: &str, violations: &mut Vec<String>) {
    if is_module_doc_exempt(path_display) {
        return;
    }
    let first_meaningful = content.lines().find(|line| !line.trim().is_empty());
    let has_doc = matches!(first_meaningful, Some(line) if line.trim_start().starts_with("//!"));
    if !has_doc {
        violations.push(format!(
            "{path_display}: 缺少模块文档——文件必须以 `//!` 开头自述职责（仿 rust-analyzer tidy）"
        ));
    }
}

/// tests/ 目录与 build.rs 不要求模块文档。
fn is_module_doc_exempt(path_display: &str) -> bool {
    let normalized = path_display.replace('\\', "/");
    normalized.contains("/tests/") || normalized.ends_with("build.rs")
}

/// 检查 3：待办/待修字样禁止入主干（半成品用工单追踪，不留在代码里）。
fn check_forbidden_words(path_display: &str, content: &str, violations: &mut Vec<String>) {
    for word in forbidden_words() {
        for (idx, line) in content.lines().enumerate() {
            if line.contains(&word) {
                violations.push(format!(
                    "{path_display}:{}: 含 {word} 字样——半成品记入工单追踪器，不入主干",
                    idx + 1
                ));
            }
        }
    }
}

/// 检查 4：单 crate `src/` 源文件数 ≤ 10（输入为文件清单，纯函数便于测试）。
pub(crate) fn check_crate_file_count(crate_name: &str, source_files: &[PathBuf]) -> Vec<String> {
    if source_files.len() > MAX_CRATE_SOURCE_FILES {
        vec![format!(
            "crate `{crate_name}`: src/ 下有 {} 个源文件，超过 {MAX_CRATE_SOURCE_FILES} 个上限\
             （ADR-0017 十戒第 10 条）——考虑拆分 crate",
            source_files.len()
        )]
    } else {
        Vec::new()
    }
}

/// 需要接受 tidy 扫描的文件扩展名。
fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs" | "slint")
    )
}

/// 扫描时跳过的目录（构建产物、版本库、本地工单等非源码目录）。
fn is_skipped_dir(name: &str) -> bool {
    matches!(name, ".git" | "target" | ".scratch" | "node_modules") || name.starts_with('.')
}

/// 收集 workspace 根下所有待检查源文件。
pub(crate) fn collect_source_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_dir() {
                if !is_skipped_dir(&name) {
                    pending.push(path);
                }
            } else if is_source_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// 对真实 workspace 执行全量 tidy，返回所有违规（空 = 通过）。
pub(crate) fn workspace_violations(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut violations = Vec::new();
    for file in collect_source_files(root)? {
        let content = std::fs::read_to_string(&file)?;
        violations.extend(check_file(&file.display().to_string(), &content));
    }
    // 检查 4：每个成员 crate 的源文件数。
    let metadata = crate::workspace_metadata(root)?;
    for package in metadata.workspace_packages() {
        let src_dir = package
            .manifest_path
            .parent()
            .map(|dir| dir.join("src"))
            .filter(|dir| dir.exists());
        if let Some(src_dir) = src_dir {
            let sources: Vec<PathBuf> = collect_source_files(src_dir.as_std_path())?
                .into_iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
                .collect();
            violations.extend(check_crate_file_count(&package.name, &sources));
        }
    }
    Ok(violations)
}

/// `cargo xtask tidy` 入口：打印违规并以非零码退出（CI 阻断）。
pub(crate) fn run(root: &Path) -> anyhow::Result<()> {
    let violations = workspace_violations(root)?;
    if violations.is_empty() {
        println!("tidy: 全部通过（行数红线 / 模块文档 / 半成品禁令 / 文件数上限）");
        return Ok(());
    }
    for violation in &violations {
        println!("tidy 违规: {violation}");
    }
    anyhow::bail!("tidy 检查失败：{} 处违规", violations.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(lines: usize, first_line: &str) -> String {
        let mut content = String::from(first_line);
        for _ in 1..lines {
            content.push_str("\nlet x = 1;");
        }
        content
    }

    #[test]
    fn file_within_limit_and_documented_passes() {
        let content = rs(50, "//! 模块文档。");
        assert!(check_file("src/lib.rs", &content).is_empty());
    }

    #[test]
    fn file_over_1000_lines_is_reported() {
        let content = rs(1001, "//! 模块文档。");
        let violations = check_file("src/big.rs", &content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("超过 1000 行红线"));
    }

    #[test]
    fn oversized_file_with_reasoned_marker_is_exempt() {
        let marker = super::filelength_marker();
        let first = format!("//! 文档。 // {marker}: 生成的映射表，评审已批准");
        let content = rs(1500, &first);
        assert!(check_file("src/generated.rs", &content).is_empty());
    }

    #[test]
    fn marker_without_reason_is_a_violation() {
        let marker = super::filelength_marker();
        let first = format!("//! 文档。 // {marker}:");
        let content = rs(10, &first);
        let violations = check_file("src/sneaky.rs", &content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("缺少理由"));
    }

    #[test]
    fn utf8_bom_does_not_break_module_doc_check() {
        let content = format!("\u{feff}{}", rs(5, "//! 模块文档。"));
        assert!(check_file("src/bom.rs", &content).is_empty());
    }

    #[test]
    fn missing_module_doc_is_reported() {
        let violations = check_file("src/lib.rs", "fn main() {}");
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("模块文档"));
    }

    #[test]
    fn tests_dir_and_build_rs_are_doc_exempt() {
        assert!(check_file("crates/x/tests/api.rs", "fn t() {}").is_empty());
        assert!(check_file("crates/x/build.rs", "fn main() {}").is_empty());
    }

    #[test]
    fn forbidden_words_are_reported_with_line_numbers() {
        let word = ["TO", "DO"].concat();
        let content = format!("//! 文档。\n// {word}: 以后再说");
        let violations = check_file("src/lazy.rs", &content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("src/lazy.rs:2"));
    }

    #[test]
    fn slint_files_only_get_line_limit_check() {
        // .slint 无模块文档要求，但受行数红线约束。
        let content = "component Foo {}\n".repeat(1001);
        let violations = check_file("ui/app.slint", &content);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("红线"));
    }

    #[test]
    fn crate_with_more_than_10_source_files_is_reported() {
        let files: Vec<PathBuf> = (0..11).map(|i| PathBuf::from(format!("f{i}.rs"))).collect();
        let violations = check_crate_file_count("demo", &files);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("超过 10 个"));
        assert!(check_crate_file_count("demo", &files[..10]).is_empty());
    }
}
