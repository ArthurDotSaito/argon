use argon_core::{DraftComment, ReviewError, ReviewEvent, Side};

use crate::review::labels;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftSummary {
    pub id: i64,
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub label: String,
}

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

pub fn summarize(drafts: &[DraftComment]) -> Vec<DraftSummary> {
    let mut items: Vec<DraftSummary> = drafts
        .iter()
        .map(|draft| DraftSummary {
            id: draft.id,
            path: draft.path.clone(),
            line: draft.line,
            side: draft.side,
            label: location(draft),
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

pub fn head_moved(session_head: Option<&str>, current: Option<&str>) -> bool {
    matches!((session_head, current), (Some(session), Some(current)) if session != current)
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
    fn start_equal_to_line_is_a_single_line() {
        assert_eq!(
            location(&draft(1, "a.rs", 9, Side::Right, Some(9))),
            "a.rs:9"
        );
    }

    #[test]
    fn head_moved_only_when_session_head_differs() {
        assert!(!head_moved(None, Some("abc")));
        assert!(!head_moved(Some("abc"), Some("abc")));
        assert!(!head_moved(Some("abc"), None));
        assert!(head_moved(Some("abc"), Some("def")));
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
