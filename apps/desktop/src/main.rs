//! campus-tool-dev —— 开发版桌面应用入口（ADR-0014 开发版形态）。
//!
//! 薄壳原则：入口只调 `run_dev`，全部装配逻辑在 `desktop_shell::runtime`。

use anyhow::Result;

fn main() -> Result<()> {
    // T35 验收 3：真机走查证明"WebView drop 发生在 IPC 回调返回之后"。
    // 设置 MCREBUILD_LOG_FILE=<文件路径> 时把 log（含 DEBUG）写进该文件；
    // 未设置时保持原行为（log 无输出）。std-only，不引入新依赖。
    init_log_file();
    desktop_shell::run_dev()
}

/// T35：极简文件 logger（`MCREBUILD_LOG_FILE` 环境变量启用）。
///
/// 应用内 `log::*` 调用已遍布（B11 方向），但此前没有任何 logger 初始化，
/// 真机无法观察"IPC 回调进入/返回"与"下一拍 drop"的顺序。启用后可按
/// `map_webview` 目标过滤出 T35 验收证据行。
fn init_log_file() {
    let Some(path) = std::env::var_os("MCREBUILD_LOG_FILE") else {
        return;
    };
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::Mutex;

    struct FileLogger {
        file: Mutex<std::fs::File>,
    }

    impl log::Log for FileLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Debug
        }

        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            let Ok(mut file) = self.file.lock() else {
                return;
            };
            let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let _ = writeln!(
                file,
                "{timestamp} [{}] {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }

        fn flush(&self) {}
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("无法打开 MCREBUILD_LOG_FILE {path:?}: {error}"));
    let logger: &'static FileLogger = Box::leak(Box::new(FileLogger {
        file: Mutex::new(file),
    }));
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Debug);
}
