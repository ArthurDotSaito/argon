use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

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

pub(crate) async fn read_line_lossy<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut any_bytes = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            break;
        }
        any_bytes = true;
        let (consumed, complete) = match chunk.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (chunk.len(), false),
        };
        let room = max_bytes.saturating_sub(line.len());
        line.extend_from_slice(&chunk[..consumed.min(room)]);
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    if !any_bytes {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&line);
    Ok(Some(text.trim_end_matches(['\r', '\n']).to_owned()))
}
