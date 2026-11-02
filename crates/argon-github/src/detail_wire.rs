use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::wire::{Actor, Connection, PageInfo, WirePr, WireReviewState, WireReviewer, WireRollup};

#[derive(Deserialize)]
pub(crate) struct DetailData {
    pub(crate) node: Option<WireDetail>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireDetail {
    #[serde(flatten)]
    pub(crate) pr: WirePr,
    pub(crate) head_ref_oid: String,
    pub(crate) body: Option<String>,
    pub(crate) labels: Connection<WireLabel>,
    pub(crate) all_commits: Connection<WireDetailCommitNode>,
    pub(crate) review_threads: Connection<WireThread>,
    pub(crate) timeline_items: WireTimeline,
}

#[derive(Deserialize)]
pub(crate) struct WireLabel {
    pub(crate) name: String,
    pub(crate) color: String,
}

#[derive(Deserialize)]
pub(crate) struct WireDetailCommitNode {
    pub(crate) commit: WireDetailCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireDetailCommit {
    pub(crate) oid: String,
    pub(crate) message_headline: String,
    pub(crate) message_body: Option<String>,
    pub(crate) committed_date: DateTime<Utc>,
    pub(crate) additions: u32,
    pub(crate) deletions: u32,
    pub(crate) author: Option<WireGitActor>,
    pub(crate) status_check_rollup: Option<WireRollup>,
}

#[derive(Deserialize)]
pub(crate) struct WireGitActor {
    pub(crate) name: Option<String>,
    pub(crate) user: Option<Actor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireThread {
    pub(crate) id: String,
    pub(crate) is_resolved: bool,
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) start_line: Option<u32>,
    pub(crate) diff_side: WireDiffSide,
    pub(crate) comments: Connection<WireThreadComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireThreadComment {
    pub(crate) id: String,
    pub(crate) author: Option<Actor>,
    pub(crate) body: String,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireDiffSide {
    Left,
    Right,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireTimeline {
    pub(crate) page_info: PageInfo,
    pub(crate) nodes: Option<Vec<Option<WireTimelineItem>>>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
pub(crate) enum WireTimelineItem {
    IssueComment(WireIssueComment),
    PullRequestReview(WireReviewItem),
    PullRequestCommit(WireCommitItem),
    ReviewRequestedEvent(WireReviewRequestedEvent),
    LabeledEvent(WireLabeledEvent),
    HeadRefForcePushedEvent(WireActorEvent),
    MergedEvent(WireActorEvent),
    ClosedEvent(WireActorEvent),
    ReadyForReviewEvent(WireActorEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireIssueComment {
    pub(crate) id: String,
    pub(crate) author: Option<Actor>,
    pub(crate) body: String,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReviewItem {
    pub(crate) id: String,
    pub(crate) author: Option<Actor>,
    pub(crate) state: WireReviewState,
    pub(crate) body: String,
    pub(crate) submitted_at: Option<DateTime<Utc>>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) comments: Connection<WireReviewComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReviewComment {
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) body: String,
    pub(crate) diff_hunk: String,
}

#[derive(Deserialize)]
pub(crate) struct WireCommitItem {
    pub(crate) commit: WireTimelineCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireTimelineCommit {
    pub(crate) oid: String,
    pub(crate) message_headline: String,
    pub(crate) committed_date: DateTime<Utc>,
    pub(crate) author: Option<WireGitActor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReviewRequestedEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) requested_reviewer: Option<WireReviewer>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireLabeledEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) label: WireEventLabel,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub(crate) struct WireEventLabel {
    pub(crate) name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireActorEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) created_at: DateTime<Utc>,
}
