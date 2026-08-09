//! T36：MCREBUILD_LOG_FILE 指向的文件日志（真机走查取证用，零新依赖）。
//!
//! 只在该环境变量非空时安装；安装失败静默跳过（日志仍可走调试器输出）。

use std::sync::Mutex;

pub(crate) fn init() {
    let Ok(path) = std::env::var("MCREBUILD_LOG_FILE") else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(_) => return,
    };
    let logger = DiagnosticFileLogger {
        file: Mutex::new(Some(file)),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}

/// 最小 log::Log 实现：时间戳 + 级别 + target + 消息，逐行追加写文件。
struct DiagnosticFileLogger {
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for DiagnosticFileLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        use std::io::Write;
        let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
        let line = format!(
            "{timestamp} [{:<5}] {}: {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        let mut guard = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = guard.as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        use std::io::Write;
        let mut guard = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(file) = guard.as_mut() {
            let _ = file.flush();
        }
    }
}
