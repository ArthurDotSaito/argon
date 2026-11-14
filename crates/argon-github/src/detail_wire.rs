use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::wire::{
    Actor, WireIssue, WireIssueState, WirePr, WireRepository, WireReviewState, WireReviewer,
    WireRollup, null_as_default,
};

pub(crate) struct Lenient<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match serde_json::from_value(value) {
            Ok(item) => Ok(Self(Some(item))),
            Err(error) => {
                tracing::warn!(%error, "skipping malformed graphql node");
                Ok(Self(None))
            }
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageFlags {
    #[serde(default)]
    has_next_page: bool,
    #[serde(default)]
    has_previous_page: bool,
}

#[derive(Deserialize)]
#[serde(
    rename_all = "camelCase",
    default,
    bound(deserialize = "T: serde::de::DeserializeOwned")
)]
pub(crate) struct PagedConnection<T> {
    page_info: PageFlags,
    nodes: Option<Vec<Option<Lenient<T>>>>,
}

impl<T> Default for PagedConnection<T> {
    fn default() -> Self {
        Self {
            page_info: PageFlags::default(),
            nodes: None,
        }
    }
}

impl<T> PagedConnection<T> {
    pub(crate) fn has_next_page(&self) -> bool {
        self.page_info.has_next_page
    }

    pub(crate) fn has_previous_page(&self) -> bool {
        self.page_info.has_previous_page
    }

    pub(crate) fn into_items(self) -> Vec<T> {
        self.nodes
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|node| node.0)
            .collect()
    }
}

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
    #[serde(default)]
    pub(crate) base_ref_oid: Option<String>,
    pub(crate) body: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) labels: PagedConnection<WireLabel>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) all_commits: PagedConnection<WireDetailCommitNode>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) review_threads: PagedConnection<WireThread>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) timeline_items: PagedConnection<WireTimelineItem>,
}

#[derive(Deserialize)]
pub(crate) struct IssueDetailData {
    pub(crate) node: Option<WireIssueNode>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
pub(crate) enum WireIssueNode {
    Issue(Box<WireIssueDetail>),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireIssueDetail {
    #[serde(flatten)]
    pub(crate) issue: WireIssue,
    pub(crate) body: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) timeline_items: PagedConnection<WireTimelineItem>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) linked_pull_requests: PagedConnection<WirePr>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) sub_issues: PagedConnection<WireSubIssue>,
}

#[derive(Deserialize)]
pub(crate) struct WireSubIssue {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) state: WireIssueState,
    pub(crate) repository: WireRepository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireMentionsData {
    pub(crate) nodes: Vec<Option<Lenient<WireMentionIssue>>>,
}

impl WireMentionsData {
    pub(crate) fn into_issues(self) -> Vec<WireMentionIssue> {
        self.nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.0)
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireMentionIssue {
    pub(crate) id: String,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) author: Option<Actor>,
    pub(crate) body: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) timeline_items: PagedConnection<WireMentionComment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireMentionComment {
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) author: Option<Actor>,
    pub(crate) body: String,
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
    #[serde(default)]
    pub(crate) is_outdated: bool,
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
    pub(crate) start_line: Option<u32>,
    pub(crate) original_line: Option<u32>,
    pub(crate) original_start_line: Option<u32>,
    pub(crate) diff_side: WireDiffSide,
    pub(crate) start_diff_side: Option<WireDiffSide>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) comments: PagedConnection<WireThreadComment>,
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
    AssignedEvent(WireAssignEvent),
    UnassignedEvent(WireAssignEvent),
    UnlabeledEvent(WireLabeledEvent),
    ReopenedEvent(WireActorEvent),
    CrossReferencedEvent(WireCrossReferencedEvent),
    ConnectedEvent(WireConnectedEvent),
    MilestonedEvent(WireMilestonedEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireAssignEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) assignee: Option<WireAssignee>,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub(crate) struct WireAssignee {
    pub(crate) login: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireCrossReferencedEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) source: WireReferenced,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireConnectedEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) subject: WireReferenced,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
pub(crate) enum WireReferenced {
    Issue(WireReferencedItem),
    PullRequest(WireReferencedItem),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(crate) struct WireReferencedItem {
    pub(crate) number: u64,
    pub(crate) repository: WireRepository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireMilestonedEvent {
    pub(crate) actor: Option<Actor>,
    pub(crate) milestone_title: String,
    pub(crate) created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireIssueComment {
    pub(crate) id: String,
    pub(crate) author: Option<Actor>,
    pub(crate) body: String,
    pub(crate) created_at: DateTime<Utc>,
    #[serde(default)]
    pub(crate) url: Option<String>,
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
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) comments: PagedConnection<WireReviewComment>,
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
