use argon_core::{DraftComment, ReviewError, ReviewEvent, Side};

use crate::review::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftSummary {
    pub id: i64,
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub label: String,
    pub excerpt: String,
}

const EXCERPT_CHARS: usize = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventOptions {
    pub judge: bool,
}

impl EventOptions {
    pub fn for_viewer(author: &str, viewer: Option<&str>) -> Self {
        let own = viewer.is_some_and(|viewer| viewer.eq_ignore_ascii_case(author));
        Self { judge: !own }
    }

    pub fn allows(self, event: ReviewEvent) -> bool {
        self.judge || event == ReviewEvent::Comment
    }

    pub fn coerce(self, event: ReviewEvent) -> ReviewEvent {
        if self.allows(event) {
            event
        } else {
            ReviewEvent::Comment
        }
    }
}

pub fn location(draft: &DraftComment) -> String {
    let side = match draft.side {
        Side::Left => labels::OLD_SIDE_SUFFIX,
        Side::Right => "",
    };
    match draft.start_line.filter(|start| *start != draft.line) {
        Some(start) => format!("{}:{start}–{}{side}", draft.path, draft.line),
        None => format!("{}:{}{side}", draft.path, draft.line),
    }
}

pub fn excerpt(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if line.chars().count() <= EXCERPT_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(EXCERPT_CHARS).collect();
    format!("{}…", cut.trim_end())
}

pub fn summarize(drafts: &[DraftComment]) -> Vec<DraftSummary> {
    let mut items: Vec<DraftSummary> = drafts
        .iter()
        .map(|draft| DraftSummary {
            id: draft.id,
            path: draft.path.clone(),
            line: draft.line,
            side: draft.side,
            label: location(draft),
            excerpt: excerpt(&draft.body),
        })
        .collect();
    items.sort_by(|a, b| {
        (a.path.as_str(), a.line, a.side == Side::Right, a.id).cmp(&(
            b.path.as_str(),
            b.line,
            b.side == Side::Right,
            b.id,
        ))
    });
    items
}

pub fn head_moved(drafts: &[DraftComment], diff_head: Option<&str>) -> bool {
    let Some(diff_head) = diff_head else {
        return false;
    };
    drafts
        .iter()
        .filter_map(|draft| draft.head_oid.as_deref())
        .any(|head| head != diff_head)
}

pub fn is_ranged(draft: &DraftComment) -> bool {
    draft.start_line.is_some_and(|start| start != draft.line)
}

const LINE_HINTS: [&str; 5] = ["line", "thread", "hunk", "position", "diff"];

pub fn suspect_drafts(message: &str, drafts: &[DraftComment]) -> Vec<i64> {
    let message = message.to_lowercase();
    if !LINE_HINTS.iter().any(|hint| message.contains(hint)) {
        return Vec::new();
    }
    let (ranged, single): (Vec<&DraftComment>, Vec<&DraftComment>) =
        drafts.iter().partition(|draft| is_ranged(draft));
    let only_ranges = message.contains("start_line") || message.contains("startline");
    let suspects: Vec<&DraftComment> = if only_ranges && !ranged.is_empty() {
        ranged
    } else {
        ranged.into_iter().chain(single).collect()
    };
    suspects.into_iter().map(|draft| draft.id).collect()
}

pub fn comment_body(text: &str) -> Option<String> {
    let body = text.trim();
    (!body.is_empty()).then(|| body.to_string())
}

pub fn review_error(error: &ReviewError, drafts: &[DraftComment]) -> String {
    let at = |id: i64| {
        drafts
            .iter()
            .find(|draft| draft.id == id)
            .map(location)
            .unwrap_or_else(|| labels::UNKNOWN_DRAFT.to_string())
    };
    match error {
        ReviewError::Empty => labels::REVIEW_EMPTY.to_string(),
        ReviewError::RequestChangesNeedsBody => labels::REQUEST_CHANGES_NEEDS_BODY.to_string(),
        ReviewError::InvalidRange { draft_id } => labels::invalid_range(&at(*draft_id)),
        ReviewError::EmptyComment { draft_id } => labels::empty_comment(&at(*draft_id)),
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{DraftSource, PrKey};

    use super::*;

    fn draft(id: i64, path: &str, line: u32, side: Side, start: Option<u32>) -> DraftComment {
        DraftComment {
            id,
            pr: PrKey::new("github.com", "PR_1"),
            path: path.to_string(),
            line,
            side,
            start_line: start,
            start_side: start.map(|_| side),
            body: format!("body {id}"),
            source: DraftSource::User,
            session_id: None,
            head_oid: None,
        }
    }

    fn at_head(id: i64, head: Option<&str>) -> DraftComment {
        DraftComment {
            head_oid: head.map(str::to_string),
            ..draft(id, "a.ts", 1, Side::Right, None)
        }
    }

    #[test]
    fn own_pr_only_allows_comment() {
        let own = EventOptions::for_viewer("Ana", Some("ana"));
        assert!(!own.judge);
        assert!(own.allows(ReviewEvent::Comment));
        assert!(!own.allows(ReviewEvent::Approve));
        assert!(!own.allows(ReviewEvent::RequestChanges));
        assert_eq!(own.coerce(ReviewEvent::Approve), ReviewEvent::Comment);
    }

    #[test]
    fn someone_elses_pr_allows_every_event() {
        let other = EventOptions::for_viewer("bruno", Some("ana"));
        assert!(other.allows(ReviewEvent::Approve));
        assert!(other.allows(ReviewEvent::RequestChanges));
        assert_eq!(
            other.coerce(ReviewEvent::RequestChanges),
            ReviewEvent::RequestChanges
        );
    }

    #[test]
    fn unknown_viewer_does_not_block_events() {
        assert!(EventOptions::for_viewer("bruno", None).judge);
    }

    #[test]
    fn summary_is_sorted_by_path_and_line_with_ranges() {
        let drafts = vec![
            draft(1, "src/b.ts", 4, Side::Right, None),
            draft(2, "src/a.ts", 20, Side::Right, Some(15)),
            draft(3, "src/a.ts", 7, Side::Left, None),
        ];
        let labels: Vec<String> = summarize(&drafts).into_iter().map(|s| s.label).collect();
        assert_eq!(
            labels,
            vec![
                format!("src/a.ts:7{}", labels::OLD_SIDE_SUFFIX),
                "src/a.ts:15–20".to_string(),
                "src/b.ts:4".to_string(),
            ]
        );
    }

    #[test]
    fn excerpt_takes_the_first_non_blank_line() {
        assert_eq!(excerpt("  curto  "), "curto");
        assert_eq!(excerpt("\n  \n  segunda linha\nterceira"), "segunda linha");
        assert_eq!(excerpt(" \n\t"), "");
    }

    #[test]
    fn excerpt_cuts_long_lines_on_char_boundaries() {
        let long = "ação ".repeat(30);
        let cut = excerpt(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= EXCERPT_CHARS + 1);
        assert!(long.starts_with(cut.trim_end_matches('…')));
        let exact = "é".repeat(EXCERPT_CHARS);
        assert_eq!(excerpt(&exact), exact);
    }

    #[test]
    fn summary_carries_the_draft_excerpt() {
        let drafts = vec![draft(1, "a.ts", 3, Side::Right, None)];
        assert_eq!(summarize(&drafts)[0].excerpt, "body 1");
    }

    #[test]
    fn start_equal_to_line_is_a_single_line() {
        assert_eq!(
            location(&draft(1, "a.rs", 9, Side::Right, Some(9))),
            "a.rs:9"
        );
    }

    #[test]
    fn head_moved_when_a_persisted_draft_head_differs_from_the_diff() {
        let same = vec![at_head(1, Some("abc")), at_head(2, None)];
        assert!(!head_moved(&same, Some("abc")));
        assert!(!head_moved(&same, None));
        assert!(!head_moved(&[at_head(1, None)], Some("def")));
        assert!(head_moved(&same, Some("def")));
    }

    #[test]
    fn line_errors_suspect_ranges_first() {
        let drafts = vec![
            draft(1, "a.ts", 4, Side::Right, None),
            draft(2, "a.ts", 9, Side::Right, Some(6)),
        ];
        let line_error = "pull_request_review_thread.line must be part of the diff";
        assert_eq!(suspect_drafts(line_error, &drafts), vec![2, 1]);
        let range_error = "pull_request_review_thread.start_line must be part of the same hunk";
        assert_eq!(suspect_drafts(range_error, &drafts), vec![2]);
        assert!(suspect_drafts("Resource not accessible by integration", &drafts).is_empty());
    }

    #[test]
    fn blank_comment_has_no_body() {
        assert_eq!(comment_body("  \n\t "), None);
        assert_eq!(comment_body("  ok \n"), Some("ok".to_string()));
    }

    #[test]
    fn review_errors_name_the_draft_location() {
        let drafts = vec![draft(5, "src/a.ts", 12, Side::Right, None)];
        assert_eq!(
            review_error(&ReviewError::EmptyComment { draft_id: 5 }, &drafts),
            labels::empty_comment("src/a.ts:12")
        );
        assert_eq!(
            review_error(&ReviewError::InvalidRange { draft_id: 9 }, &drafts),
            labels::invalid_range(labels::UNKNOWN_DRAFT)
        );
        assert_eq!(
            review_error(&ReviewError::RequestChangesNeedsBody, &drafts),
            labels::REQUEST_CHANGES_NEEDS_BODY
        );
    }
}
