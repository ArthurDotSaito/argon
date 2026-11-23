use std::path::{Component, Path, PathBuf};

use argon_core::{PermissionMode, SessionKind, checked_local_folder};
use serde_json::Value;

use super::events::PermissionOption;
use crate::util::json_text;
use crate::worktree::is_related_dir_name;

const READ_ONLY_KINDS: &[&str] = &["read", "search"];
const FULL_ACCESS_READ_ONLY_KINDS: &[&str] = &["read", "search", "execute", "fetch", "think"];
const LOCAL_EDIT_KINDS: &[&str] = &["read", "search", "edit"];
pub(crate) const PERMISSIONS_KIND: &str = "permissions";
const DENIED_WHEN_READ_ONLY_KINDS: &[&str] =
    &["edit", "delete", "move", "switch_mode", PERMISSIONS_KIND];
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
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];
const SEGMENT_BREAKS: [char; 5] = ['/', '\\', '{', '}', ','];
const PARENT_SEGMENT: &str = "..";
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
    Deny { option_id: String },
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

fn has_parent_segment(text: &str) -> bool {
    text.split(SEGMENT_BREAKS)
        .any(|segment| segment == PARENT_SEGMENT)
}

fn is_unresolvable(candidate: &str) -> bool {
    candidate.starts_with(HOME_PREFIX)
        || candidate.contains(EXPANSION_CHARS)
        || has_parent_segment(candidate)
}

fn target_of(cwd: &Path, candidate: &str) -> Option<String> {
    if is_unresolvable(candidate) {
        return None;
    }
    let candidate = Path::new(candidate);
    let joined = if candidate.is_absolute() || candidate.has_root() {
        candidate.to_path_buf()
    } else {
        cwd.join(candidate)
    };
    Some(comparable(&canonical(&joined)))
}

fn is_under(root: &Path, target: &str) -> bool {
    let root = comparable(&canonical(root));
    target == root || target.starts_with(&format!("{root}/"))
}

fn is_usable_read_root(root: &Path) -> bool {
    root.is_absolute() && canonical(root).parent().is_some()
}

fn is_valid_read_root(root: &Path) -> bool {
    let Ok(resolved) = dunce::canonicalize(root) else {
        return false;
    };
    resolved.is_dir()
        && resolved.parent().is_some()
        && resolved
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_related_dir_name)
}

pub(crate) fn valid_read_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .filter(|root| {
            let valid = is_valid_read_root(root);
            if !valid {
                tracing::warn!(path = %root.display(), "ignoring an invalid read-only folder");
            }
            valid
        })
        .cloned()
        .collect()
}

pub(crate) fn valid_local_roots(roots: &[PathBuf], home: Option<&Path>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for root in roots {
        match checked_local_folder(root, home) {
            Ok(resolved) => {
                let resolved = dunce::simplified(&resolved).to_path_buf();
                if !kept.contains(&resolved) {
                    kept.push(resolved);
                }
            }
            Err(reason) => {
                tracing::warn!(path = %root.display(), ?reason, "ignoring an invalid local folder");
            }
        }
    }
    kept
}

pub fn is_inside(cwd: &Path, candidate: &str) -> bool {
    target_of(cwd, candidate).is_some_and(|target| is_under(cwd, &target))
}

fn is_readable(cwd: &Path, read_roots: &[PathBuf], candidate: &str) -> bool {
    target_of(cwd, candidate).is_some_and(|target| {
        is_under(cwd, &target)
            || read_roots
                .iter()
                .filter(|root| is_usable_read_root(root))
                .any(|root| is_under(root, &target))
    })
}

fn literal_prefix(pattern: &str) -> &str {
    let Some(cut) = pattern.find(GLOB_CHARS) else {
        return pattern;
    };
    let head = &pattern[..cut];
    match head.rfind(PATH_SEPARATORS) {
        Some(separator) => &head[..=separator],
        None => head,
    }
}

fn scoped_prefix(pattern: &str) -> String {
    if has_parent_segment(pattern) {
        return pattern.to_owned();
    }
    match literal_prefix(pattern) {
        "" => ".".to_owned(),
        prefix => prefix.to_owned(),
    }
}

fn is_path_like(pattern: &str) -> bool {
    let path = Path::new(pattern);
    path.is_absolute() || path.has_root() || has_parent_segment(pattern)
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

fn is_readable_read(ask: &PermissionAsk, cwd: &Path, read_roots: &[PathBuf]) -> bool {
    READ_ONLY_KINDS.contains(&ask.kind.as_str())
        && !ask.paths.is_empty()
        && ask
            .paths
            .iter()
            .all(|path| is_readable(cwd, read_roots, path))
}

fn auto_allowed(
    ask: &PermissionAsk,
    cwd: &Path,
    read_roots: &[PathBuf],
    mode: PermissionMode,
) -> bool {
    match mode {
        PermissionMode::Supervised => is_readable_read(ask, cwd, read_roots),
        PermissionMode::AcceptEdits => {
            is_local(ask, cwd, LOCAL_EDIT_KINDS) || is_readable_read(ask, cwd, read_roots)
        }
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

fn option_of_kind(ask: &PermissionAsk, kind: &str) -> Option<String> {
    ask.options
        .iter()
        .find(|option| option.kind == kind)
        .map(|option| option.id.clone())
}

fn is_write_kind(ask: &PermissionAsk) -> bool {
    DENIED_WHEN_READ_ONLY_KINDS.contains(&ask.kind.as_str())
}

fn denied_when_read_only(ask: &PermissionAsk) -> Option<String> {
    if is_write_kind(ask) {
        option_of_kind(ask, REJECT_ONCE)
    } else {
        None
    }
}

pub fn decide(
    ask: &PermissionAsk,
    cwd: &Path,
    read_roots: &[PathBuf],
    mode: PermissionMode,
    kind: SessionKind,
) -> PermissionDecision {
    let (mode, read_roots) = if !kind.is_read_only() {
        (mode, &[][..])
    } else if FULL_ACCESS_READ_ONLY_KINDS.contains(&ask.kind.as_str()) {
        (kind.pinned_mode(mode), read_roots)
    } else {
        (PermissionMode::Supervised, read_roots)
    };
    if kind.is_read_only()
        && let Some(option_id) = denied_when_read_only(ask)
    {
        return PermissionDecision::Deny { option_id };
    }
    match option_of_kind(ask, ALLOW_ONCE) {
        Some(option_id) if auto_allowed(ask, cwd, read_roots, mode) => {
            PermissionDecision::Allow { option_id }
        }
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
