use std::path::Path;

use argon_core::{Config, ConfigError, Paths};

pub fn load(paths: &Paths) -> Config {
    let path = paths.config_file();
    match Config::load(&path) {
        Ok(config) => config,
        Err(error) => {
            report(&path, &error);
            let mut config = Config::default();
            config.orgs.initialized = true;
            config
        }
    }
}

fn report(path: &Path, error: &ConfigError) {
    let shown = path.display();
    match error {
        ConfigError::Parse { source, .. } => {
            match source.span().and_then(|span| line_column(path, span.start)) {
                Some((line, column)) => {
                    tracing::warn!(path = %shown, line, column, "invalid config, using defaults");
                }
                None => tracing::warn!(path = %shown, "invalid config, using defaults"),
            }
        }
        ConfigError::Io { source, .. } => {
            tracing::warn!(path = %shown, kind = %source.kind(), "unreadable config, using defaults");
        }
        ConfigError::Serialize(_) => {
            tracing::warn!(path = %shown, "invalid config, using defaults");
        }
    }
}

fn line_column(path: &Path, offset: usize) -> Option<(usize, usize)> {
    let text = std::fs::read_to_string(path).ok()?;
    let before = text.get(..offset)?;
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |tail| tail.chars().count())
        + 1;
    Some((line, column))
}
