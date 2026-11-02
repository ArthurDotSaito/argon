use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrId(pub String);

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
pub struct Involvement {
    pub requested: bool,
    pub assigned: bool,
    pub authored: bool,
    pub reviewed: bool,
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
}
