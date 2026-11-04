use std::path::{Component, Path, PathBuf};

use argon_core::PermissionMode;
use serde_json::Value;

use super::events::PermissionOption;
use crate::util::json_text;

const READ_ONLY_KINDS: &[&str] = &["read", "search"];
const LOCAL_EDIT_KINDS: &[&str] = &["read", "search", "edit"];
pub(crate) const ALLOW_ONCE: &str = "allow_once";
pub(crate) const ALLOW_ALWAYS: &str = "allow_always";
pub(crate) const REJECT_ONCE: &str = "reject_once";
const PATH_KEYS: [&str; 6] = [
    "path",
    "file_path",
    "filePath",
    "abs_path",
    "directory",
    "notebook_path",
];
const PATTERN_KEYS: [&str; 3] = ["pattern", "glob", "include"];
const REGEX_KEY: &str = "pattern";
const GLOB_CHARS: [char; 4] = ['*', '?', '[', '{'];
const HOME_PREFIX: char = '~';
const EXPANSION_CHARS: [char; 2] = ['$', '%'];
const NARROW_TITLE_KINDS: &[&str] = &["read", "search", "edit"];
const NARROW_TITLE_LIMIT: usize = 300;
const WIDE_TITLE_LIMIT: usize = 2000;
const TITLE_CUT_MARKER: &str = " …";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub title: String,
    pub kind: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAsk {
    pub tool_call_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub paths: Vec<String>,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionAnswer {
    Selected(String),
    Cancelled,
}

pub(crate) type PermissionReply =
    Box<dyn FnOnce(PermissionAnswer) -> std::result::Result<(), String> + Send>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow { option_id: String },
    Ask(PermissionPrompt),
}

fn options(request: &Value) -> Vec<PermissionOption> {
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

fn is_unresolvable(candidate: &str) -> bool {
    candidate.starts_with(HOME_PREFIX)
        || candidate.contains(EXPANSION_CHARS)
        || Path::new(candidate)
            .components()
            .any(|component| component == Component::ParentDir)
}

pub fn is_inside(cwd: &Path, candidate: &str) -> bool {
    if is_unresolvable(candidate) {
        return false;
    }
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

fn scoped_prefix(pattern: &str) -> String {
    match literal_prefix(pattern) {
        "" => ".".to_owned(),
        prefix => prefix.to_owned(),
    }
}

fn is_path_like(pattern: &str) -> bool {
    let path = Path::new(pattern);
    path.is_absolute()
        || path.has_root()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
}

pub(crate) fn input_paths(input: &Value) -> Vec<String> {
    let mut paths: Vec<String> = PATH_KEYS
        .iter()
        .filter_map(|key| json_text(input, key))
        .collect();
    paths.extend(
        PATTERN_KEYS
            .iter()
            .filter_map(|key| json_text(input, key).map(|value| (*key, value)))
            .filter(|(key, pattern)| *key != REGEX_KEY || is_path_like(pattern))
            .map(|(_, pattern)| scoped_prefix(&pattern)),
    );
    paths
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
        paths.extend(input_paths(input));
    }
    paths
}

pub(crate) fn acp_ask(request: &Value, known_kind: Option<&str>) -> PermissionAsk {
    let tool_call = request.get("toolCall").cloned().unwrap_or(Value::Null);
    let kind = json_text(&tool_call, "kind")
        .or_else(|| known_kind.map(str::to_owned))
        .unwrap_or_else(|| "other".to_owned());
    PermissionAsk {
        tool_call_id: json_text(&tool_call, "toolCallId"),
        title: json_text(&tool_call, "title").unwrap_or_else(|| kind.clone()),
        kind,
        paths: touched_paths(&tool_call),
        options: options(request),
    }
}

fn grantable_for_session(kind: &str) -> bool {
    LOCAL_EDIT_KINDS.contains(&kind)
}

const NATIVE_OPTIONS: [(&str, &str); 3] = [
    (ALLOW_ONCE, "Permitir"),
    (ALLOW_ALWAYS, "Permitir nesta sessão"),
    (REJECT_ONCE, "Negar"),
];

fn native_options(include: impl Fn(&str) -> bool) -> Vec<PermissionOption> {
    NATIVE_OPTIONS
        .into_iter()
        .filter(|(id, _)| include(id))
        .map(|(id, label)| PermissionOption {
            id: id.to_owned(),
            label: label.to_owned(),
            kind: id.to_owned(),
        })
        .collect()
}

pub(crate) fn native_options_for(kind: &str) -> Vec<PermissionOption> {
    native_options(|id| id != ALLOW_ALWAYS || grantable_for_session(kind))
}

pub(crate) fn native_options_with_session() -> Vec<PermissionOption> {
    native_options(|_| true)
}

pub(crate) fn paths_contained(ask: &PermissionAsk, cwd: &Path) -> bool {
    !ask.paths.is_empty() && ask.paths.iter().all(|path| is_inside(cwd, path))
}

pub(crate) fn session_grant_applies(ask: &PermissionAsk, cwd: &Path) -> bool {
    grantable_for_session(&ask.kind) && paths_contained(ask, cwd)
}

fn is_local(ask: &PermissionAsk, cwd: &Path, kinds: &[&str]) -> bool {
    kinds.contains(&ask.kind.as_str()) && paths_contained(ask, cwd)
}

fn auto_allowed(ask: &PermissionAsk, cwd: &Path, mode: PermissionMode) -> bool {
    match mode {
        PermissionMode::Supervised => is_local(ask, cwd, READ_ONLY_KINDS),
        PermissionMode::AcceptEdits => is_local(ask, cwd, LOCAL_EDIT_KINDS),
        PermissionMode::FullAccess => true,
    }
}

fn is_hidden_format(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{FEFF}'
    )
}

fn title_limit(kind: &str) -> usize {
    if NARROW_TITLE_KINDS.contains(&kind) {
        NARROW_TITLE_LIMIT
    } else {
        WIDE_TITLE_LIMIT
    }
}

pub(crate) fn prompt_title(title: &str, kind: &str) -> String {
    let visible: String = title
        .chars()
        .map(|c| {
            if c.is_control() || is_hidden_format(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let flat = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let limit = title_limit(kind);
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit).collect();
    format!("{}{TITLE_CUT_MARKER}", cut.trim_end())
}

pub fn decide(ask: &PermissionAsk, cwd: &Path, mode: PermissionMode) -> PermissionDecision {
    let allow_once = ask
        .options
        .iter()
        .find(|option| option.kind == ALLOW_ONCE)
        .map(|option| option.id.clone());
    match allow_once {
        Some(option_id) if auto_allowed(ask, cwd, mode) => PermissionDecision::Allow { option_id },
        _ => PermissionDecision::Ask(PermissionPrompt {
            title: ask.title.clone(),
            kind: ask.kind.clone(),
            options: ask.options.clone(),
        }),
    }
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
