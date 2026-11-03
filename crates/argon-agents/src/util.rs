use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;

pub(crate) enum Resolved {
    Path(PathBuf),
    Missing,
    Failed(String),
}

pub(crate) fn resolve_command(name: &Path) -> Resolved {
    match which::which(name) {
        Ok(path) => Resolved::Path(path),
        Err(which::Error::CannotFindBinaryPath) => Resolved::Missing,
        Err(error) => Resolved::Failed(error.to_string()),
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn json_text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}
