use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::model::{PrKey, Side};

impl Side {
    pub fn as_graphql(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }

    pub fn from_graphql(value: &str) -> Option<Side> {
        match value {
            "LEFT" => Some(Self::Left),
            "RIGHT" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum DraftSource {
    User,
    Agent,
}

impl DraftSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    pub fn parse(value: &str) -> Option<DraftSource> {
        match value {
            "user" => Some(Self::User),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewEvent {
    pub fn as_graphql(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }

    pub fn from_graphql(value: &str) -> Option<ReviewEvent> {
        match value {
            "COMMENT" => Some(Self::Comment),
            "APPROVE" => Some(Self::Approve),
            "REQUEST_CHANGES" => Some(Self::RequestChanges),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftComment {
    pub id: i64,
    pub pr: PrKey,
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start_line: Option<u32>,
    pub start_side: Option<Side>,
    pub body: String,
    pub source: DraftSource,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftReview {
    pub body: String,
    pub event: ReviewEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftThreadInput {
    pub path: String,
    pub line: u32,
    pub side: Side,
    pub start_line: Option<u32>,
    pub start_side: Option<Side>,
    pub body: String,
}

impl DraftThreadInput {
    fn multiline_start(&self) -> Option<(u32, Side)> {
        let start_line = self.start_line?;
        let start_side = self.start_side.unwrap_or(self.side);
        (start_line != self.line || start_side != self.side).then_some((start_line, start_side))
    }
}

impl From<&DraftComment> for DraftThreadInput {
    fn from(draft: &DraftComment) -> Self {
        Self {
            path: draft.path.clone(),
            line: draft.line,
            side: draft.side,
            start_line: draft.start_line,
            start_side: draft.start_side,
            body: draft.body.clone(),
        }
    }
}

impl Serialize for DraftThreadInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let start = self.multiline_start();
        let fields = if start.is_some() { 6 } else { 4 };
        let mut state = serializer.serialize_struct("DraftThreadInput", fields)?;
        state.serialize_field("path", &self.path)?;
        state.serialize_field("line", &self.line)?;
        state.serialize_field("side", self.side.as_graphql())?;
        if let Some((start_line, start_side)) = start {
            state.serialize_field("startLine", &start_line)?;
            state.serialize_field("startSide", start_side.as_graphql())?;
        }
        state.serialize_field("body", &self.body)?;
        state.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewSubmission {
    pub body: String,
    pub event: ReviewEvent,
    pub threads: Vec<DraftThreadInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("review has no body and no comments")]
    Empty,
    #[error("requesting changes requires a review body")]
    RequestChangesNeedsBody,
    #[error("draft {draft_id} starts after the line it ends on")]
    InvalidRange { draft_id: i64 },
    #[error("draft {draft_id} has an empty comment")]
    EmptyComment { draft_id: i64 },
}

fn has_inverted_range(draft: &DraftComment) -> bool {
    let Some(start_line) = draft.start_line else {
        return false;
    };
    match (draft.start_side.unwrap_or(draft.side), draft.side) {
        (Side::Right, Side::Left) => true,
        (start_side, side) if start_side == side => start_line > draft.line,
        _ => false,
    }
}

pub fn build_submission(
    body: &str,
    event: ReviewEvent,
    drafts: &[DraftComment],
) -> Result<ReviewSubmission, ReviewError> {
    if let Some(draft) = drafts.iter().find(|draft| has_inverted_range(draft)) {
        return Err(ReviewError::InvalidRange { draft_id: draft.id });
    }
    if let Some(draft) = drafts.iter().find(|draft| draft.body.trim().is_empty()) {
        return Err(ReviewError::EmptyComment { draft_id: draft.id });
    }
    let body = if body.trim().is_empty() { "" } else { body };
    match event {
        ReviewEvent::RequestChanges if body.is_empty() => {
            return Err(ReviewError::RequestChangesNeedsBody);
        }
        ReviewEvent::Comment if body.is_empty() && drafts.is_empty() => {
            return Err(ReviewError::Empty);
        }
        _ => {}
    }
    Ok(ReviewSubmission {
        body: body.to_string(),
        event,
        threads: drafts.iter().map(DraftThreadInput::from).collect(),
    })
}

pub fn single_comment_submission(draft: &DraftComment) -> ReviewSubmission {
    ReviewSubmission {
        body: String::new(),
        event: ReviewEvent::Comment,
        threads: vec![DraftThreadInput::from(draft)],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn draft(id: i64, path: &str, line: u32, side: Side) -> DraftComment {
        DraftComment {
            id,
            pr: PrKey::new("github.com", "PR_kwDOOrbita0412"),
            path: path.to_string(),
            line,
            side,
            start_line: None,
            start_side: None,
            body: format!("comment {id}"),
            source: DraftSource::User,
            session_id: None,
        }
    }

    fn ranged(id: i64, start_line: u32, line: u32) -> DraftComment {
        DraftComment {
            start_line: Some(start_line),
            start_side: Some(Side::Right),
            ..draft(id, "src/reschedule/slot-finder.ts", line, Side::Right)
        }
    }

    fn threads_json(submission: &ReviewSubmission) -> serde_json::Value {
        serde_json::to_value(&submission.threads).unwrap()
    }

    #[test]
    fn multiline_draft_becomes_a_ranged_thread() {
        let submission = build_submission("", ReviewEvent::Comment, &[ranged(1, 10, 14)]).unwrap();
        assert_eq!(
            threads_json(&submission),
            json!([{
                "path": "src/reschedule/slot-finder.ts",
                "line": 14,
                "side": "RIGHT",
                "startLine": 10,
                "startSide": "RIGHT",
                "body": "comment 1"
            }])
        );
    }

    #[test]
    fn left_side_draft_serializes_as_left() {
        let submission = build_submission(
            "",
            ReviewEvent::Comment,
            &[draft(1, "src/legacy/old.ts", 7, Side::Left)],
        )
        .unwrap();
        assert_eq!(
            threads_json(&submission),
            json!([{
                "path": "src/legacy/old.ts",
                "line": 7,
                "side": "LEFT",
                "body": "comment 1"
            }])
        );
    }

    #[test]
    fn start_is_omitted_when_equal_to_the_end() {
        let same = DraftComment {
            start_line: Some(9),
            start_side: Some(Side::Right),
            ..draft(1, "a.ts", 9, Side::Right)
        };
        let side_defaulted = DraftComment {
            start_line: Some(9),
            ..draft(2, "b.ts", 9, Side::Left)
        };
        let submission =
            build_submission("", ReviewEvent::Comment, &[same, side_defaulted]).unwrap();
        for thread in threads_json(&submission).as_array().unwrap() {
            let object = thread.as_object().unwrap();
            assert!(!object.contains_key("startLine"));
            assert!(!object.contains_key("startSide"));
        }
    }

    #[test]
    fn start_on_another_side_is_kept_even_on_the_same_line() {
        let crossing = DraftComment {
            start_line: Some(9),
            start_side: Some(Side::Left),
            ..draft(1, "a.ts", 9, Side::Right)
        };
        let submission = build_submission("", ReviewEvent::Comment, &[crossing]).unwrap();
        assert_eq!(
            threads_json(&submission),
            json!([{
                "path": "a.ts",
                "line": 9,
                "side": "RIGHT",
                "startLine": 9,
                "startSide": "LEFT",
                "body": "comment 1"
            }])
        );
    }

    #[test]
    fn start_without_side_defaults_to_the_thread_side() {
        let defaulted = DraftComment {
            start_line: Some(3),
            ..draft(1, "a.ts", 5, Side::Left)
        };
        let submission = build_submission("", ReviewEvent::Comment, &[defaulted]).unwrap();
        assert_eq!(threads_json(&submission)[0]["startSide"], json!("LEFT"));
    }

    #[test]
    fn submission_keeps_draft_order_and_body() {
        let drafts = [
            draft(2, "b.ts", 4, Side::Right),
            draft(1, "a.ts", 8, Side::Right),
        ];
        let submission = build_submission("Ship it", ReviewEvent::Approve, &drafts).unwrap();
        assert_eq!(submission.body, "Ship it");
        assert_eq!(submission.event, ReviewEvent::Approve);
        let paths: Vec<&str> = submission.threads.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, ["b.ts", "a.ts"]);
    }

    #[test]
    fn comment_without_body_and_threads_is_empty() {
        assert_eq!(
            build_submission("", ReviewEvent::Comment, &[]),
            Err(ReviewError::Empty)
        );
    }

    #[test]
    fn whitespace_body_counts_as_empty() {
        assert_eq!(
            build_submission("  \n\t ", ReviewEvent::Comment, &[]),
            Err(ReviewError::Empty)
        );
        assert_eq!(
            build_submission(
                " \n ",
                ReviewEvent::RequestChanges,
                &[draft(1, "a.ts", 1, Side::Right)]
            ),
            Err(ReviewError::RequestChangesNeedsBody)
        );
        let submission = build_submission(
            "  ",
            ReviewEvent::Comment,
            &[draft(1, "a.ts", 1, Side::Right)],
        )
        .unwrap();
        assert_eq!(submission.body, "");
    }

    #[test]
    fn comment_with_only_threads_or_only_body_is_valid() {
        assert!(
            build_submission(
                "",
                ReviewEvent::Comment,
                &[draft(1, "a.ts", 1, Side::Right)]
            )
            .is_ok()
        );
        assert!(build_submission("note", ReviewEvent::Comment, &[]).is_ok());
    }

    #[test]
    fn request_changes_needs_a_body_even_with_threads() {
        assert_eq!(
            build_submission("", ReviewEvent::RequestChanges, &[]),
            Err(ReviewError::RequestChangesNeedsBody)
        );
        assert_eq!(
            build_submission(
                "",
                ReviewEvent::RequestChanges,
                &[draft(1, "a.ts", 1, Side::Right)]
            ),
            Err(ReviewError::RequestChangesNeedsBody)
        );
        assert!(build_submission("fix this", ReviewEvent::RequestChanges, &[]).is_ok());
    }

    #[test]
    fn approve_may_go_without_body_and_threads() {
        let submission = build_submission("", ReviewEvent::Approve, &[]).unwrap();
        assert_eq!(submission.body, "");
        assert!(submission.threads.is_empty());
    }

    #[test]
    fn inverted_range_on_the_same_side_names_the_draft() {
        let drafts = [ranged(1, 3, 5), ranged(7, 20, 12)];
        assert_eq!(
            build_submission("", ReviewEvent::Comment, &drafts),
            Err(ReviewError::InvalidRange { draft_id: 7 })
        );
        let defaulted_side = DraftComment {
            start_line: Some(9),
            ..draft(4, "a.ts", 5, Side::Right)
        };
        assert_eq!(
            build_submission("", ReviewEvent::Approve, &[defaulted_side]),
            Err(ReviewError::InvalidRange { draft_id: 4 })
        );
    }

    #[test]
    fn start_on_the_right_ending_on_the_left_is_invalid() {
        let backwards = DraftComment {
            start_line: Some(2),
            start_side: Some(Side::Right),
            ..draft(3, "a.ts", 9, Side::Left)
        };
        assert_eq!(
            build_submission("", ReviewEvent::Comment, &[backwards]),
            Err(ReviewError::InvalidRange { draft_id: 3 })
        );
    }

    #[test]
    fn blank_draft_body_names_the_draft() {
        let blank = DraftComment {
            body: " 
	"
            .to_string(),
            ..draft(6, "a.ts", 1, Side::Right)
        };
        let drafts = [draft(5, "b.ts", 2, Side::Right), blank];
        for event in [
            ReviewEvent::Comment,
            ReviewEvent::Approve,
            ReviewEvent::RequestChanges,
        ] {
            assert_eq!(
                build_submission("body", event, &drafts),
                Err(ReviewError::EmptyComment { draft_id: 6 })
            );
        }
    }

    #[test]
    fn invalid_range_is_reported_before_an_empty_comment() {
        let blank = DraftComment {
            body: String::new(),
            ..draft(1, "a.ts", 1, Side::Right)
        };
        assert_eq!(
            build_submission("", ReviewEvent::RequestChanges, &[blank, ranged(2, 9, 4)]),
            Err(ReviewError::InvalidRange { draft_id: 2 })
        );
    }

    #[test]
    fn empty_comment_is_reported_before_the_body_rules() {
        let blank = DraftComment {
            body: String::new(),
            ..draft(1, "a.ts", 1, Side::Right)
        };
        assert_eq!(
            build_submission("", ReviewEvent::RequestChanges, &[blank]),
            Err(ReviewError::EmptyComment { draft_id: 1 })
        );
    }

    #[test]
    fn start_after_end_is_allowed_across_sides() {
        let crossing = DraftComment {
            start_line: Some(30),
            start_side: Some(Side::Left),
            ..draft(1, "a.ts", 5, Side::Right)
        };
        assert!(build_submission("", ReviewEvent::Comment, &[crossing]).is_ok());
    }

    #[test]
    fn submission_serializes_with_graphql_enum_names() {
        let submission = build_submission(
            "Needs work",
            ReviewEvent::RequestChanges,
            &[draft(1, "a.ts", 2, Side::Left)],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&submission).unwrap(),
            json!({
                "body": "Needs work",
                "event": "REQUEST_CHANGES",
                "threads": [{ "path": "a.ts", "line": 2, "side": "LEFT", "body": "comment 1" }]
            })
        );
        for (event, name) in [
            (ReviewEvent::Comment, "COMMENT"),
            (ReviewEvent::Approve, "APPROVE"),
        ] {
            assert_eq!(serde_json::to_value(event).unwrap(), json!(name));
            assert_eq!(event.as_graphql(), name);
            assert_eq!(ReviewEvent::from_graphql(name), Some(event));
        }
        assert_eq!(ReviewEvent::from_graphql("PENDING"), None);
    }

    #[test]
    fn thread_input_serializes_exactly_like_the_github_input() {
        let input = DraftThreadInput {
            path: "src/a.ts".to_string(),
            line: 12,
            side: Side::Right,
            start_line: Some(8),
            start_side: Some(Side::Right),
            body: "multi \"line\"\nbody".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            json!({
                "path": "src/a.ts",
                "line": 12,
                "side": "RIGHT",
                "startLine": 8,
                "startSide": "RIGHT",
                "body": "multi \"line\"\nbody"
            })
        );
    }

    #[test]
    fn side_and_source_round_trip_through_their_text_form() {
        for side in [Side::Left, Side::Right] {
            assert_eq!(Side::from_graphql(side.as_graphql()), Some(side));
        }
        assert_eq!(Side::from_graphql("left"), None);
        for source in [DraftSource::User, DraftSource::Agent] {
            assert_eq!(DraftSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(DraftSource::parse("bot"), None);
    }

    #[test]
    fn single_comment_submission_is_a_body_less_comment_with_one_thread() {
        let drafted = ranged(5, 2, 4);
        let submission = single_comment_submission(&drafted);
        assert_eq!(submission.body, "");
        assert_eq!(submission.event, ReviewEvent::Comment);
        assert_eq!(submission.threads, vec![DraftThreadInput::from(&drafted)]);
    }
}
