use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

const LOG_ENV: &str = "ARGON_LOG";
const LOG_FILE: &str = "argon.log";
const DEFAULT_FILTER: &str = "info";
const MAX_LOG_FILES: usize = 7;

pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    let filter =
        EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let appender = match RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE)
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
    {
        Ok(appender) => appender,
        Err(error) => {
            eprintln!(
                "failed to open log directory {}: {error}",
                log_dir.display()
            );
            return None;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(writer)
        .try_init();
    if let Err(error) = installed {
        eprintln!("failed to install log subscriber: {error}");
        return None;
    }
    install_panic_hook();
    Some(guard)
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        tracing::error!(thread = name, panic = %info, "thread panicked");
        default_hook(info);
    }));
}
