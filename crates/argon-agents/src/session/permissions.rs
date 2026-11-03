use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::events::PermissionOption;

const READ_ONLY_LOCAL_KINDS: [&str; 2] = ["read", "search"];
const READ_ONLY_REMOTE_KINDS: [&str; 1] = ["fetch"];
const ALLOW_ONCE: &str = "allow_once";
const PATH_KEYS: [&str; 4] = ["path", "file_path", "filePath", "abs_path"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub title: String,
    pub kind: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow { option_id: String },
    Ask(PermissionPrompt),
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn options(request: &Value) -> Vec<PermissionOption> {
    request
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| {
                    Some(PermissionOption {
                        id: text(option, "optionId")?,
                        label: text(option, "name").unwrap_or_default(),
                        kind: text(option, "kind").unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn is_inside(cwd: &Path, candidate: &str) -> bool {
    let candidate = Path::new(candidate);
    let resolved = if candidate.is_absolute() || candidate.has_root() {
        normalize(candidate)
    } else {
        normalize(&cwd.join(candidate))
    };
    let root = comparable(&normalize(cwd));
    let target = comparable(&resolved);
    let root = root.trim_end_matches('/');
    target == root || target.starts_with(&format!("{root}/"))
}

fn touched_paths(tool_call: &Value) -> Vec<String> {
    let mut paths: Vec<String> = tool_call
        .get("locations")
        .and_then(Value::as_array)
        .map(|locations| {
            locations
                .iter()
                .filter_map(|location| text(location, "path"))
                .collect()
        })
        .unwrap_or_default();
    if let Some(input) = tool_call.get("rawInput") {
        paths.extend(PATH_KEYS.iter().filter_map(|key| text(input, key)));
    }
    paths
}

pub fn decide(request: &Value, known_kind: Option<&str>, cwd: &Path) -> PermissionDecision {
    let tool_call = request.get("toolCall").cloned().unwrap_or(Value::Null);
    let kind = text(&tool_call, "kind")
        .or_else(|| known_kind.map(str::to_owned))
        .unwrap_or_else(|| "other".to_owned());
    let options = options(request);
    let allow_once = options
        .iter()
        .find(|option| option.kind == ALLOW_ONCE)
        .map(|option| option.id.clone());
    let read_only = if READ_ONLY_REMOTE_KINDS.contains(&kind.as_str()) {
        true
    } else if READ_ONLY_LOCAL_KINDS.contains(&kind.as_str()) {
        touched_paths(&tool_call)
            .iter()
            .all(|path| is_inside(cwd, path))
    } else {
        false
    };
    match allow_once {
        Some(option_id) if read_only => PermissionDecision::Allow { option_id },
        _ => PermissionDecision::Ask(PermissionPrompt {
            title: text(&tool_call, "title").unwrap_or_else(|| kind.clone()),
            kind,
            options,
        }),
    }
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
