use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::sync::Arc;

use argon_core::{Hunk, LineKind, Side, parse_patch};
use argon_github::{FileStatus, PrFile, ViewedState};
use gpui_kit::component::diff::{DiffFile, DiffParseOptions};

use super::file_tree::file_order;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileBody {
    Ready,
    Empty,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinePos {
    pub order: usize,
    pub hunk: usize,
}

#[derive(Clone)]
pub struct FileDiff {
    pub meta: FileMeta,
    pub body: FileBody,
    pub document: DiffFile,
    pub order: Arc<HashMap<(u32, Side), LinePos>>,
    pub fingerprint: u64,
    pub viewed: bool,
}

impl FileDiff {
    pub fn has_line(&self, anchor: (u32, Side)) -> bool {
        self.order.contains_key(&anchor)
    }

    pub fn line_pos(&self, anchor: (u32, Side)) -> Option<LinePos> {
        self.order.get(&anchor).copied()
    }
}

pub fn synthetic_patch(meta: &FileMeta, body: Option<&str>) -> String {
    let path = &meta.path;
    let old = meta.previous_path.as_deref().unwrap_or(path);
    let (from, to) = match meta.status {
        FileStatus::Added => ("/dev/null".to_string(), format!("b/{path}")),
        FileStatus::Removed => (format!("a/{old}"), "/dev/null".to_string()),
        _ => (format!("a/{old}"), format!("b/{path}")),
    };
    let mut out = format!("diff --git a/{old} b/{path}\n");
    if old != path {
        out.push_str(&format!("rename from {old}\nrename to {path}\n"));
    }
    match meta.status {
        FileStatus::Added => out.push_str("new file mode 100644\n"),
        FileStatus::Removed => out.push_str("deleted file mode 100644\n"),
        _ => {}
    }
    let Some(body) = body else {
        out.push_str(&format!("Binary files {from} and {to} differ\n"));
        return out;
    };
    out.push_str(&format!("--- {from}\n+++ {to}\n"));
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn patch_order(hunks: &[Hunk]) -> HashMap<(u32, Side), LinePos> {
    let mut order = HashMap::new();
    let lines = hunks
        .iter()
        .enumerate()
        .flat_map(|(hunk, h)| h.lines.iter().map(move |line| (hunk, line)));
    for (ix, (hunk, line)) in lines.enumerate() {
        let pos = LinePos { order: ix, hunk };
        if let (Some(old), false) = (line.old_no, line.kind == LineKind::Added) {
            order.insert((old, Side::Left), pos);
        }
        if let (Some(new), false) = (line.new_no, line.kind == LineKind::Removed) {
            order.insert((new, Side::Right), pos);
        }
    }
    order
}

fn fingerprint(meta: &FileMeta, patch: Option<&str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    meta.path.hash(&mut hasher);
    meta.previous_path.hash(&mut hasher);
    patch.hash(&mut hasher);
    hasher.finish()
}

fn document(meta: &FileMeta, body: Option<&str>) -> Option<DiffFile> {
    let patch = synthetic_patch(meta, body);
    let options = DiffParseOptions::new().truncated_hunks(true);
    match DiffFile::parse_with(&patch, options) {
        Ok(mut files) if !files.is_empty() => Some(files.swap_remove(0)),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(message = %error.message(), line = error.line(), path = meta.path, "failed to build diff document");
            None
        }
    }
}

pub fn prepare(file: PrFile, viewed: bool) -> FileDiff {
    let meta = FileMeta {
        path: file.path,
        previous_path: file.previous_path.filter(|previous| !previous.is_empty()),
        status: file.status,
        additions: file.additions,
        deletions: file.deletions,
    };
    let fingerprint = fingerprint(&meta, file.patch.as_deref());
    let parsed = match file.patch.as_deref() {
        None => Err(FileBody::Missing),
        Some(patch) if patch.trim().is_empty() => Ok((FileBody::Empty, Vec::new(), "")),
        Some(patch) => match parse_patch(patch) {
            Ok(hunks) if hunks.is_empty() => Ok((FileBody::Empty, hunks, "")),
            Ok(hunks) => Ok((FileBody::Ready, hunks, patch)),
            Err(error) => {
                tracing::warn!(%error, path = meta.path, "failed to parse file patch");
                Err(FileBody::Invalid)
            }
        },
    };
    let (body, lines, document) = match parsed {
        Ok((body, hunks, patch)) => match document(&meta, Some(patch)) {
            Some(document) => (body, patch_order(&hunks), Some(document)),
            None => (FileBody::Invalid, HashMap::new(), None),
        },
        Err(body) => (body, HashMap::new(), None),
    };
    let document = document
        .or_else(|| self::document(&meta, None))
        .unwrap_or_else(|| DiffFile::unchanged(meta.path.clone(), ""));
    FileDiff {
        meta,
        body,
        document,
        order: Arc::new(lines),
        fingerprint,
        viewed,
    }
}

pub fn prepare_all(
    files: Vec<PrFile>,
    viewed: Option<&HashMap<String, ViewedState>>,
) -> Vec<FileDiff> {
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let order = file_order(&paths);
    let mut slots: Vec<Option<PrFile>> = files.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|ix| slots.get_mut(ix)?.take())
        .map(|file| {
            let viewed = viewed
                .and_then(|viewed| viewed.get(&file.path))
                .is_some_and(|state| *state == ViewedState::Viewed);
            prepare(file, viewed)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::diff::DiffFileStatus;

    use super::*;

    fn named(path: &str, status: FileStatus, patch: Option<&str>) -> PrFile {
        PrFile {
            path: path.to_string(),
            previous_path: None,
            status,
            additions: 1,
            deletions: 1,
            patch: patch.map(str::to_string),
            sha: None,
        }
    }

    fn file(patch: Option<&str>) -> PrFile {
        named("src/a.rs", FileStatus::Modified, patch)
    }

    #[test]
    fn missing_patch_becomes_binary_document() {
        let prepared = prepare(file(None), false);
        assert_eq!(prepared.body, FileBody::Missing);
        assert!(prepared.document.is_binary());
        assert_eq!(prepared.document.path().as_ref(), "src/a.rs");
    }

    #[test]
    fn blank_patch_is_empty() {
        assert_eq!(prepare(file(Some("")), false).body, FileBody::Empty);
    }

    #[test]
    fn broken_patch_is_invalid() {
        let prepared = prepare(file(Some("@@ nonsense @@\n+x")), false);
        assert_eq!(prepared.body, FileBody::Invalid);
        assert_eq!(prepared.document.path().as_ref(), "src/a.rs");
    }

    #[test]
    fn valid_patch_keeps_path_and_line_sides() {
        let prepared = prepare(file(Some("@@ -1,2 +1,2 @@\n a\n-old\n+new")), false);
        assert_eq!(prepared.body, FileBody::Ready);
        assert_eq!(prepared.document.path().as_ref(), "src/a.rs");
        assert_eq!(prepared.document.additions(), 1);
        assert!(prepared.has_line((1, Side::Left)));
        assert!(prepared.has_line((1, Side::Right)));
        assert!(prepared.has_line((2, Side::Left)));
        assert!(prepared.has_line((2, Side::Right)));
        assert!(!prepared.has_line((3, Side::Right)));
        assert_eq!(
            prepared.line_pos((1, Side::Left)).map(|pos| pos.order),
            Some(0)
        );
        assert_eq!(
            prepared.line_pos((1, Side::Right)).map(|pos| pos.order),
            Some(0)
        );
        assert_eq!(
            prepared.line_pos((2, Side::Left)).map(|pos| pos.order),
            Some(1)
        );
        assert_eq!(
            prepared.line_pos((2, Side::Right)).map(|pos| pos.order),
            Some(2)
        );
    }

    #[test]
    fn added_and_removed_files_keep_their_status() {
        let added = prepare(
            named("new.ts", FileStatus::Added, Some("@@ -0,0 +1 @@\n+x")),
            false,
        );
        assert_eq!(added.document.status(), DiffFileStatus::Added);
        assert_eq!(added.document.path().as_ref(), "new.ts");
        let removed = prepare(
            named("old.ts", FileStatus::Removed, Some("@@ -1 +0,0 @@\n-x")),
            false,
        );
        assert_eq!(removed.document.status(), DiffFileStatus::Deleted);
        assert_eq!(removed.document.path().as_ref(), "old.ts");
    }

    #[test]
    fn renames_are_reported_with_both_paths() {
        let mut renamed = named("src/new.rs", FileStatus::Renamed, Some(""));
        renamed.previous_path = Some("src/old.rs".to_string());
        let prepared = prepare(renamed, false);
        assert_eq!(prepared.body, FileBody::Empty);
        assert_eq!(prepared.document.status(), DiffFileStatus::Renamed);
        assert_eq!(
            prepared.document.original_path().map(|path| path.as_ref()),
            Some("src/old.rs")
        );
    }

    #[test]
    fn prepare_all_orders_like_the_tree_and_reads_viewed_state() {
        let files = vec![
            named("README.md", FileStatus::Modified, None),
            named("src/b.rs", FileStatus::Modified, None),
            named("src/a.rs", FileStatus::Modified, None),
        ];
        let viewed = HashMap::from([
            ("src/a.rs".to_string(), ViewedState::Viewed),
            ("src/b.rs".to_string(), ViewedState::Dismissed),
        ]);
        let summary: Vec<(String, bool)> = prepare_all(files, Some(&viewed))
            .into_iter()
            .map(|file| (file.meta.path, file.viewed))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("src/a.rs".to_string(), true),
                ("src/b.rs".to_string(), false),
                ("README.md".to_string(), false),
            ]
        );
    }
}
