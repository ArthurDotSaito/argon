use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::events::PermissionOption;
use crate::util::json_text;

const READ_ONLY_KINDS: [&str; 2] = ["read", "search"];
const ALLOW_ONCE: &str = "allow_once";
const PATH_KEYS: [&str; 5] = ["path", "file_path", "filePath", "abs_path", "directory"];
const PATTERN_KEYS: [&str; 3] = ["pattern", "glob", "include"];
const GLOB_CHARS: [char; 4] = ['*', '?', '[', '{'];

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

pub fn options(request: &Value) -> Vec<PermissionOption> {
    request
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|option| {
                    Some(PermissionOption {
                        id: json_text(option, "optionId")?,
                        label: json_text(option, "name").unwrap_or_default(),
                        kind: json_text(option, "kind").unwrap_or_default(),
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

fn canonical(path: &Path) -> PathBuf {
    let normalized = normalize(path);
    let mut existing = normalized.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = dunce::canonicalize(existing) {
            let mut full = resolved;
            for part in rest.iter().rev() {
                full.push(part);
            }
            return full;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return normalized,
        }
    }
}

fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/').to_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn is_inside(cwd: &Path, candidate: &str) -> bool {
    let candidate = Path::new(candidate);
    let joined = if candidate.is_absolute() || candidate.has_root() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    let root = comparable(&canonical(cwd));
    let target = comparable(&canonical(&joined));
    target == root || target.starts_with(&format!("{root}/"))
}

fn literal_prefix(pattern: &str) -> &str {
    let cut = pattern.find(GLOB_CHARS).unwrap_or(pattern.len());
    &pattern[..cut]
}

fn is_path_like(pattern: &str) -> bool {
    let path = Path::new(pattern);
    path.is_absolute()
        || path.has_root()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
}

fn touched_paths(tool_call: &Value) -> Vec<String> {
    let mut paths: Vec<String> = tool_call
        .get("locations")
        .and_then(Value::as_array)
        .map(|locations| {
            locations
                .iter()
                .filter_map(|location| json_text(location, "path"))
                .collect()
        })
        .unwrap_or_default();
    if let Some(input) = tool_call.get("rawInput") {
        paths.extend(PATH_KEYS.iter().filter_map(|key| json_text(input, key)));
        paths.extend(
            PATTERN_KEYS
                .iter()
                .filter_map(|key| json_text(input, key))
                .filter(|pattern| is_path_like(pattern))
                .map(|pattern| literal_prefix(&pattern).to_owned()),
        );
    }
    paths
}

fn is_local_read(kind: &str, tool_call: &Value, cwd: &Path) -> bool {
    if !READ_ONLY_KINDS.contains(&kind) {
        return false;
    }
    let paths = touched_paths(tool_call);
    if paths.is_empty() {
        return kind == "search";
    }
    paths.iter().all(|path| is_inside(cwd, path))
}

pub fn decide(request: &Value, known_kind: Option<&str>, cwd: &Path) -> PermissionDecision {
    let tool_call = request.get("toolCall").cloned().unwrap_or(Value::Null);
    let kind = json_text(&tool_call, "kind")
        .or_else(|| known_kind.map(str::to_owned))
        .unwrap_or_else(|| "other".to_owned());
    let options = options(request);
    let allow_once = options
        .iter()
        .find(|option| option.kind == ALLOW_ONCE)
        .map(|option| option.id.clone());
    match allow_once {
        Some(option_id) if is_local_read(&kind, &tool_call, cwd) => {
            PermissionDecision::Allow { option_id }
        }
        _ => PermissionDecision::Ask(PermissionPrompt {
            title: json_text(&tool_call, "title").unwrap_or_else(|| kind.clone()),
            kind,
            options,
        }),
    }
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
