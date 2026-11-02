use std::path::Path;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

const LOG_ENV: &str = "ARGON_LOG";
const LOG_FILE: &str = "argon.log";
const DEFAULT_FILTER: &str = "info";

pub fn init(log_dir: &Path) {
    let filter =
        EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    let appender = match RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE)
        .build(log_dir)
    {
        Ok(appender) => appender,
        Err(error) => {
            eprintln!(
                "failed to open log directory {}: {error}",
                log_dir.display()
            );
            return;
        }
    };
    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(appender)
        .try_init();
    if let Err(error) = installed {
        eprintln!("failed to install log subscriber: {error}");
    }
}
