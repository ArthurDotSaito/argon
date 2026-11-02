use argon_core::{ChecksState, PrId, PullRequest, ReviewState, Side};
use chrono::{DateTime, Utc};
use serde_json::json;

use crate::client::GithubClient;
use crate::detail_convert::into_pr_detail;
use crate::detail_wire::DetailData;
use crate::error::{GithubError, Result};

const DETAIL_QUERY: &str = include_str!("../queries/detail.graphql");

#[derive(Debug, Clone, PartialEq)]
pub struct PrDetail {
    pub pr: PullRequest,
    pub body: String,
    pub head_oid: String,
    pub labels: Vec<Label>,
    pub reviewers: Vec<ReviewerStatus>,
    pub timeline: Vec<TimelineItem>,
    pub truncated_timeline: bool,
    pub threads: Vec<ReviewThread>,
    pub truncated_threads: bool,
    pub commits: Vec<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerStatus {
    pub login: String,
    pub state: Option<ReviewState>,
    pub requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub path: String,
    pub line: Option<u32>,
    pub body: String,
    pub diff_hunk: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineItem {
    Comment {
        id: String,
        author: String,
        body: String,
        created_at: DateTime<Utc>,
    },
    Review {
        id: String,
        author: String,
        state: ReviewState,
        body: String,
        submitted_at: DateTime<Utc>,
        comments: Vec<ReviewComment>,
    },
    Commit {
        oid: String,
        headline: String,
        author: String,
        committed_at: DateTime<Utc>,
    },
    ReviewRequested {
        actor: String,
        reviewer: String,
        at: DateTime<Utc>,
    },
    Labeled {
        actor: String,
        label: String,
        at: DateTime<Utc>,
    },
    HeadRefForcePushed {
        actor: String,
        at: DateTime<Utc>,
    },
    Merged {
        actor: String,
        at: DateTime<Utc>,
    },
    Closed {
        actor: String,
        at: DateTime<Utc>,
    },
    ReadyForReview {
        actor: String,
        at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub id: String,
    pub resolved: bool,
    pub path: String,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub side: Side,
    pub comments: Vec<ThreadComment>,
    pub truncated_comments: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub oid: String,
    pub headline: String,
    pub body: String,
    pub author_name: String,
    pub author_login: Option<String>,
    pub committed_at: DateTime<Utc>,
    pub additions: u32,
    pub deletions: u32,
    pub checks: Option<ChecksState>,
}

impl GithubClient {
    pub async fn pr_detail(&self, id: &PrId) -> Result<PrDetail> {
        let data: DetailData = self
            .graphql("pr_detail", DETAIL_QUERY, json!({ "id": id.0 }))
            .await?;
        let wire = data.node.ok_or_else(|| {
            GithubError::GraphQl(vec![format!("pull request {} not found", id.0)])
        })?;
        Ok(into_pr_detail(wire, &self.host))
    }
}
