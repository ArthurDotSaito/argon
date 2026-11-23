use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::PermissionMode;

pub const DEFAULT_HOST: &str = "github.com";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrKey {
    pub host: String,
    pub id: PrId,
}

impl PrKey {
    pub fn new(host: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            id: PrId(id.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoKey {
    pub host: String,
    pub repo: String,
}

impl RepoKey {
    pub fn new(host: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            repo: repo.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChecksState {
    Success,
    Failure,
    Pending,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Involvement {
    pub requested: bool,
    pub assigned: bool,
    pub authored: bool,
    pub reviewed: bool,
    pub mentioned: bool,
    pub commented: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubjectKind {
    Pr,
    Issue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionKind {
    Review,
    Plan,
    Summary,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Plan => "plan",
            Self::Summary => "summary",
        }
    }

    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Plan | Self::Summary)
    }

    pub fn pinned_mode(self, requested: PermissionMode) -> PermissionMode {
        match self {
            Self::Review => requested,
            Self::Plan => PermissionMode::Supervised,
            Self::Summary if requested == PermissionMode::FullAccess => PermissionMode::FullAccess,
            Self::Summary => PermissionMode::Supervised,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueStateReason {
    Completed,
    NotPlanned,
    Duplicate,
    Reopened,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueLabel {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPr {
    pub repo: String,
    pub number: u64,
    pub state: PrState,
    pub is_draft: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubIssues {
    pub total: u32,
    pub completed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueRef {
    pub repo: String,
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestReview {
    pub author: String,
    pub state: ReviewState,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: PrId,
    pub host: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub is_draft: bool,
    pub state: PrState,
    pub base: String,
    pub head: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub comments: u32,
    pub review_decision: Option<ReviewDecision>,
    pub checks: Option<ChecksState>,
    pub mergeable: Mergeable,
    pub requested_reviewers: Vec<String>,
    pub assignees: Vec<String>,
    pub latest_reviews: Vec<LatestReview>,
    pub head_committed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub involvement: Involvement,
    #[serde(default)]
    pub closing_issues: Vec<IssueRef>,
}

impl PullRequest {
    pub fn key(&self) -> PrKey {
        PrKey {
            host: self.host.clone(),
            id: self.id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: PrId,
    pub host: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub state: IssueState,
    pub state_reason: Option<IssueStateReason>,
    pub issue_type: Option<String>,
    pub labels: Vec<IssueLabel>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub comments: u32,
    pub linked_prs: Vec<LinkedPr>,
    pub sub_issues: Option<SubIssues>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_mention_at: Option<DateTime<Utc>>,
    pub involvement: Involvement,
}

impl Issue {
    pub fn key(&self) -> PrKey {
        PrKey {
            host: self.host.clone(),
            id: self.id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn pull_request_json_without_closing_issues_deserializes() {
        let mut value = serde_json::to_value(fixtures::pr()).unwrap();
        value.as_object_mut().unwrap().remove("closing_issues");
        value["involvement"] = serde_json::json!({
            "requested": true,
            "assigned": false,
            "authored": false,
            "reviewed": false
        });

        let parsed: PullRequest = serde_json::from_value(value).unwrap();

        assert!(parsed.closing_issues.is_empty());
        assert!(parsed.involvement.requested);
    }

    #[test]
    fn involvement_json_without_new_fields_deserializes() {
        let parsed: Involvement = serde_json::from_str(
            r#"{"requested":false,"assigned":true,"authored":false,"reviewed":false}"#,
        )
        .unwrap();

        assert!(parsed.assigned);
        assert!(!parsed.mentioned);
        assert!(!parsed.commented);
    }

    #[test]
    fn issue_round_trips_through_json() {
        let issue = fixtures::issue();

        let json = serde_json::to_string(&issue).unwrap();
        let parsed: Issue = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, issue);
    }

    #[test]
    fn issue_json_without_last_mention_at_deserializes() {
        let mut value = serde_json::to_value(fixtures::issue()).unwrap();
        value.as_object_mut().unwrap().remove("last_mention_at");

        let parsed: Issue = serde_json::from_value(value).unwrap();

        assert_eq!(parsed.last_mention_at, None);
    }

    #[test]
    fn session_kind_as_str_matches_persisted_values() {
        assert_eq!(SessionKind::Review.as_str(), "review");
        assert_eq!(SessionKind::Plan.as_str(), "plan");
        assert_eq!(SessionKind::Summary.as_str(), "summary");
    }

    #[test]
    fn only_plan_and_summary_sessions_are_read_only() {
        assert!(!SessionKind::Review.is_read_only());
        assert!(SessionKind::Plan.is_read_only());
        assert!(SessionKind::Summary.is_read_only());
    }

    #[test]
    fn pinned_mode_keeps_full_access_only_for_review_and_summary() {
        use PermissionMode::{AcceptEdits, FullAccess, Supervised};
        for mode in [Supervised, AcceptEdits, FullAccess] {
            assert_eq!(SessionKind::Review.pinned_mode(mode), mode);
            assert_eq!(SessionKind::Plan.pinned_mode(mode), Supervised);
        }
        assert_eq!(SessionKind::Summary.pinned_mode(Supervised), Supervised);
        assert_eq!(SessionKind::Summary.pinned_mode(AcceptEdits), Supervised);
        assert_eq!(SessionKind::Summary.pinned_mode(FullAccess), FullAccess);
    }
}
