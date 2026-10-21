//! Logging: console (stderr) plus a daily rolling file in the project's log
//! directory. Level from `laurabrod.toml` (`[log] level`) or `RUST_LOG`.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Initialise logging. Keep the returned guard alive for the process.
pub fn init(root: &Path, file_logging: bool) -> Option<WorkerGuard> {
    let app = lb_core::project::files::read_toml_or_default::<lb_core::config::AppConfig>(&root.join(lb_core::project::APP_FILE))
        .unwrap_or_default();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{},ffmpeg=warn,wgpu_hal=off,wgpu_core=warn,naga=warn,egui_wgpu=warn,eframe=warn", app.log.level))
    });
    let console = fmt::layer().with_writer(std::io::stderr).with_target(false).compact();
    if !file_logging {
        let _ = tracing_subscriber::registry().with(filter).with(console).try_init();
        return None;
    }
    let dir = lb_core::project::paths::resolve(root, &app.log.dir);
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::daily(&dir, "laurabrod.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let file = fmt::layer().with_writer(writer).with_ansi(false);
    let _ = tracing_subscriber::registry().with(filter).with(console).with(file).try_init();
    Some(guard)
}
