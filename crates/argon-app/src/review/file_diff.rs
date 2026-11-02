use std::collections::HashMap;
use std::sync::Arc;

use argon_core::{Hunk, SplitRow, parse_patch, split_rows};
use argon_github::{FileStatus, PrFile, ViewedState};

use super::file_tree::file_order;
use super::highlight::FileHighlights;

const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiff {
    pub hunks: Vec<Hunk>,
    pub split: Vec<Vec<SplitRow>>,
}

impl ParsedDiff {
    pub fn new(mut hunks: Vec<Hunk>) -> Self {
        for line in hunks.iter_mut().flat_map(|hunk| hunk.lines.iter_mut()) {
            if line.text.contains('\t') {
                line.text = line.text.replace('\t', &" ".repeat(TAB_WIDTH));
            }
        }
        let split = hunks.iter().map(split_rows).collect();
        Self { hunks, split }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileBody {
    Ready(Arc<ParsedDiff>),
    Empty,
    Missing,
    Invalid,
}

impl FileBody {
    pub fn diff(&self) -> Option<&Arc<ParsedDiff>> {
        match self {
            Self::Ready(diff) => Some(diff),
            Self::Empty | Self::Missing | Self::Invalid => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
}

pub struct FileDiff {
    pub meta: FileMeta,
    pub body: FileBody,
    pub highlights: Option<Arc<FileHighlights>>,
    pub viewed: bool,
    pub collapsed: bool,
}

pub fn prepare(file: PrFile) -> (FileMeta, FileBody) {
    let body = match file.patch.as_deref() {
        None => FileBody::Missing,
        Some(patch) if patch.trim().is_empty() => FileBody::Empty,
        Some(patch) => match parse_patch(patch) {
            Ok(hunks) if hunks.is_empty() => FileBody::Empty,
            Ok(hunks) => FileBody::Ready(Arc::new(ParsedDiff::new(hunks))),
            Err(error) => {
                tracing::warn!(%error, path = file.path, "failed to parse file patch");
                FileBody::Invalid
            }
        },
    };
    let meta = FileMeta {
        path: file.path,
        previous_path: file.previous_path,
        status: file.status,
        additions: file.additions,
        deletions: file.deletions,
    };
    (meta, body)
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
            let (meta, body) = prepare(file);
            FileDiff {
                meta,
                body,
                highlights: None,
                viewed,
                collapsed: viewed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(patch: Option<&str>) -> PrFile {
        named("src/a.rs", patch)
    }

    fn named(path: &str, patch: Option<&str>) -> PrFile {
        PrFile {
            path: path.to_string(),
            previous_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            patch: patch.map(str::to_string),
            sha: None,
        }
    }

    #[test]
    fn missing_patch_is_reported_as_missing() {
        assert_eq!(prepare(file(None)).1, FileBody::Missing);
    }

    #[test]
    fn blank_patch_is_empty() {
        assert_eq!(prepare(file(Some(""))).1, FileBody::Empty);
    }

    #[test]
    fn broken_patch_is_invalid() {
        assert_eq!(
            prepare(file(Some("@@ nonsense @@\n+x"))).1,
            FileBody::Invalid
        );
    }

    #[test]
    fn valid_patch_is_parsed_with_tabs_expanded() {
        let (meta, body) = prepare(file(Some("@@ -1 +1 @@\n-\told\n+\tnew")));
        assert_eq!(meta.path, "src/a.rs");
        let diff = body.diff().expect("parsed");
        assert_eq!(diff.hunks[0].lines[1].text, "    new");
        assert_eq!(diff.split[0].len(), 1);
    }

    #[test]
    fn prepare_all_orders_like_the_tree_and_collapses_viewed_files() {
        let files = vec![
            named("README.md", None),
            named("src/b.rs", None),
            named("src/a.rs", None),
        ];
        let viewed = HashMap::from([
            ("src/a.rs".to_string(), ViewedState::Viewed),
            ("src/b.rs".to_string(), ViewedState::Dismissed),
        ]);
        let prepared = prepare_all(files, Some(&viewed));
        let summary: Vec<(&str, bool, bool)> = prepared
            .iter()
            .map(|file| (file.meta.path.as_str(), file.viewed, file.collapsed))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("src/a.rs", true, true),
                ("src/b.rs", false, false),
                ("README.md", false, false),
            ]
        );
    }
}
