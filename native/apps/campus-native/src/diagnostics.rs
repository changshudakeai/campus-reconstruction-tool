use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_FILES: usize = 10;

static GLOBAL: OnceLock<Diagnostics> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl DiagnosticLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticRecord {
    pub id: String,
    pub log_path: PathBuf,
}

pub struct Diagnostics {
    session_id: String,
    log_path: PathBuf,
    sequence: AtomicU64,
    writer: Mutex<()>,
}

impl Diagnostics {
    pub fn start_in(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = directory.as_ref();
        fs::create_dir_all(directory)?;
        prune_old_logs(directory);
        let started_at = unix_millis();
        let session_id = format!("{started_at}-{}", std::process::id());
        let log_path = directory.join(format!("session-{session_id}.jsonl"));
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        Ok(Self {
            session_id,
            log_path,
            sequence: AtomicU64::new(0),
            writer: Mutex::new(()),
        })
    }

    pub fn record(
        &self,
        level: DiagnosticLevel,
        event: &str,
        message: &str,
        context: &[(&str, &str)],
    ) -> io::Result<DiagnosticRecord> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let incident_id = format!("{}-{sequence:06}", self.session_id);
        let mut context_value = Map::new();
        for (key, value) in context {
            context_value.insert(
                (*key).to_string(),
                Value::String(sanitise_context_value(key, value)),
            );
        }
        let task = context_value
            .get("task")
            .and_then(Value::as_str)
            .filter(|value| *value != REDACTED)
            .unwrap_or(event);
        let redacted_code = context_value
            .get("code")
            .and_then(Value::as_str)
            .filter(|value| *value != REDACTED)
            .unwrap_or(event);
        let project_revision = context
            .iter()
            .find(|(key, _)| *key == "project_revision")
            .and_then(|(_, value)| value.parse::<u64>().ok());
        let bundle_revision = context_value
            .get("bundle_revision")
            .and_then(Value::as_str)
            .filter(|value| *value != REDACTED);
        let recovery_result = context_value
            .get("recovery_result")
            .and_then(Value::as_str)
            .filter(|value| *value != REDACTED)
            .unwrap_or("not-attempted");
        let entry = json!({
            "timestamp_unix_ms": unix_millis(),
            "level": level.as_str(),
            "event": safe_identifier(event),
            "message": sanitise_message(message),
            "incident_id": incident_id,
            "session_id": self.session_id,
            "process_id": std::process::id(),
            "thread": format!("{:?}", std::thread::current().id()),
            "candidate": "v1.1.0",
            "version": env!("CARGO_PKG_VERSION"),
            "task": safe_identifier(task),
            "redacted_code": safe_identifier(redacted_code),
            "bundle_revision": bundle_revision,
            "project_revision": project_revision,
            "recovery_result": safe_identifier(recovery_result),
            "context": context_value,
        });
        let bytes = serde_json::to_vec(&entry).map_err(io::Error::other)?;
        let _guard = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("diagnostic writer lock poisoned"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(DiagnosticRecord {
            id: incident_id,
            log_path: self.log_path.clone(),
        })
    }

    #[cfg(test)]
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

pub fn initialise(directory: impl AsRef<Path>) -> io::Result<&'static Diagnostics> {
    if let Some(existing) = GLOBAL.get() {
        return Ok(existing);
    }
    let diagnostics = Diagnostics::start_in(directory)?;
    let _ = GLOBAL.set(diagnostics);
    GLOBAL
        .get()
        .ok_or_else(|| io::Error::other("diagnostics failed to initialise"))
}

pub fn record(
    level: DiagnosticLevel,
    event: &str,
    message: &str,
    context: &[(&str, &str)],
) -> Option<DiagnosticRecord> {
    GLOBAL
        .get()
        .and_then(|diagnostics| diagnostics.record(level, event, message, context).ok())
}

const REDACTED: &str = "[REDACTED]";

fn safe_identifier(value: &str) -> String {
    let value = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ':')
        })
        .take(96)
        .collect::<String>();
    if value.is_empty() {
        "unknown".into()
    } else {
        value
    }
}

fn sanitise_context_value(key: &str, value: &str) -> String {
    let normalized = key.to_ascii_lowercase();
    if [
        "api_key",
        "security_code",
        "credential",
        "password",
        "token",
        "secret",
        "review_content",
        "project_file",
    ]
    .iter()
    .any(|private| normalized.contains(private))
    {
        return REDACTED.into();
    }
    if normalized.contains("path") || normalized == "executable" {
        return if contains_personal_path(value) {
            REDACTED.into()
        } else {
            value
                .rsplit(['/', '\\'])
                .next()
                .filter(|component| !component.is_empty())
                .unwrap_or(REDACTED)
                .to_string()
        };
    }
    if matches!(
        normalized.as_str(),
        "task"
            | "code"
            | "candidate"
            | "version"
            | "bundle_revision"
            | "project_revision"
            | "recovery_result"
    ) {
        return safe_identifier(value);
    }
    if normalized == "source" || normalized == "backtrace" {
        return sanitise_message(value);
    }
    REDACTED.into()
}

fn contains_personal_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with(r"\\")
        || value.contains("/Users/")
        || value.contains("/home/")
}

fn sanitise_message(message: &str) -> String {
    let prefix = message
        .split_once(['{', '['])
        .map_or(message, |(prefix, _)| prefix);
    let mut output = Vec::new();
    for token in prefix.split_whitespace().take(80) {
        let normalized = token.to_ascii_lowercase();
        let private_assignment = [
            "api_key=",
            "security_code=",
            "credential=",
            "password=",
            "token=",
            "secret=",
        ]
        .iter()
        .any(|marker| normalized.contains(marker));
        if private_assignment {
            output.push(REDACTED.to_string());
        } else if contains_personal_path(token) {
            output.push("[REDACTED-PATH]".into());
        } else {
            output.push(token.to_string());
        }
    }
    let mut scrubbed = output.join(" ");
    if scrubbed.len() > 512 {
        scrubbed.truncate(512);
    }
    if scrubbed.is_empty() {
        "diagnostic detail redacted".into()
    } else {
        scrubbed
    }
}

pub fn log_directory() -> Option<PathBuf> {
    GLOBAL
        .get()
        .and_then(|diagnostics| diagnostics.log_path.parent().map(Path::to_path_buf))
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn prune_old_logs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut logs = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("session-") && name.ends_with(".jsonl"))
        })
        .collect::<Vec<_>>();
    logs.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH)
    });
    let remove_count = logs.len().saturating_sub(MAX_LOG_FILES - 1);
    for entry in logs.into_iter().take(remove_count) {
        let _ = fs::remove_file(entry.path());
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticLevel, Diagnostics};

    fn temp_log_directory(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "campus-diagnostics-test-{}-{test_name}",
            std::process::id()
        ))
    }

    #[test]
    fn records_structured_incidents_with_unique_ids() {
        let directory = temp_log_directory("records");
        let _ = std::fs::remove_dir_all(&directory);
        let diagnostics = Diagnostics::start_in(&directory).unwrap();

        let first = diagnostics
            .record(
                DiagnosticLevel::Error,
                "project.save",
                "disk full",
                &[("path", "project.json")],
            )
            .unwrap();
        let second = diagnostics
            .record(DiagnosticLevel::Error, "map.connect", "pipe closed", &[])
            .unwrap();

        assert_ne!(first.id, second.id);
        let lines = std::fs::read_to_string(diagnostics.log_path()).unwrap();
        let entries = lines
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["level"], "error");
        assert_eq!(entries[0]["event"], "project.save");
        assert_eq!(entries[0]["message"], "disk full");
        assert_eq!(entries[0]["context"]["path"], "project.json");
        assert_eq!(entries[1]["incident_id"], second.id);
        assert_eq!(entries[0]["candidate"], "v1.1.0");
        assert_eq!(entries[0]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(entries[0]["task"], "project.save");
        assert_eq!(entries[0]["redacted_code"], "project.save");
        assert_eq!(entries[0]["recovery_result"], "not-attempted");
        assert!(entries[0]["timestamp_unix_ms"].is_number());

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn diagnostics_redact_credentials_paths_and_project_content() {
        let directory = temp_log_directory("privacy");
        let _ = std::fs::remove_dir_all(&directory);
        let diagnostics = Diagnostics::start_in(&directory).unwrap();

        diagnostics
            .record(
                DiagnosticLevel::Error,
                "acquisition.chunk",
                r#"failed at C:\Users\Alice\Campus\secret.campus.json api_key=gaode-secret {\"project\":\"complete\"}"#,
                &[
                    ("path", r#"C:\Users\Alice\Campus\secret.campus.json"#),
                    ("api_key", "gaode-secret"),
                    ("review_content", "a private facade photo description"),
                    ("project_revision", "42"),
                    ("bundle_revision", "bundle-2026-07"),
                    ("recovery_result", "retry-offered"),
                ],
            )
            .unwrap();

        let line = std::fs::read_to_string(diagnostics.log_path()).unwrap();
        assert!(!line.contains("Alice"));
        assert!(!line.contains("gaode-secret"));
        assert!(!line.contains("private facade"));
        assert!(!line.contains("complete"));
        let entry: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(entry["project_revision"], 42);
        assert_eq!(entry["bundle_revision"], "bundle-2026-07");
        assert_eq!(entry["recovery_result"], "retry-offered");
        assert_eq!(entry["context"]["path"], "[REDACTED]");
        assert_eq!(entry["context"]["api_key"], "[REDACTED]");
        assert_eq!(entry["context"]["review_content"], "[REDACTED]");

        let _ = std::fs::remove_dir_all(directory);
    }
}
