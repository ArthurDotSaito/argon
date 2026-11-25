use std::collections::HashMap;

use argon_core::{ChecksState, Issue, IssueState, PrId, PullRequest, ReviewState, Side};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use serde_json::json;

use crate::client::GithubClient;
use crate::detail_convert::{into_issue_detail, into_pr_detail, latest_mention_times};
use crate::detail_wire::{
    DetailData, IssueDetailData, WireIssueNode, WireMentionIssue, WireMentionsData,
};
use crate::error::{GithubError, Result};

const DETAIL_QUERY: &str = concat!(
    include_str!("../queries/detail.graphql"),
    include_str!("../queries/pull_request_fields.graphql")
);
const ISSUE_DETAIL_QUERY: &str = concat!(
    include_str!("../queries/issue_detail.graphql"),
    include_str!("../queries/issue_fields.graphql"),
    include_str!("../queries/pull_request_fields.graphql")
);
const MENTIONS_QUERY: &str = include_str!("../queries/issue_mentions.graphql");
const MENTION_CHUNK: usize = 40;
const MENTION_CONCURRENCY: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct PrDetail {
    pub pr: PullRequest,
    pub body: String,
    pub head_oid: String,
    pub base_oid: Option<String>,
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
        author_is_bot: bool,
        body: String,
        created_at: DateTime<Utc>,
        url: Option<String>,
    },
    Review {
        id: String,
        author: String,
        author_is_bot: bool,
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
    Assigned {
        actor: String,
        assignee: String,
        at: DateTime<Utc>,
    },
    Unassigned {
        actor: String,
        assignee: String,
        at: DateTime<Utc>,
    },
    Unlabeled {
        actor: String,
        label: String,
        at: DateTime<Utc>,
    },
    Reopened {
        actor: String,
        at: DateTime<Utc>,
    },
    CrossReferenced {
        actor: String,
        repo: String,
        number: u64,
        is_pull_request: bool,
        at: DateTime<Utc>,
    },
    Milestoned {
        actor: String,
        milestone: String,
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
    pub outdated: bool,
    pub path: String,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub original_line: Option<u32>,
    pub original_start_line: Option<u32>,
    pub side: Side,
    pub start_side: Option<Side>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct IssueDetail {
    pub issue: Issue,
    pub body: String,
    pub timeline: Vec<TimelineItem>,
    pub linked_prs: Vec<PullRequest>,
    pub sub_issues: Vec<SubIssueRow>,
    pub truncated_timeline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubIssueRow {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub state: IssueState,
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

    pub async fn issue_detail(&self, id: &PrId) -> Result<IssueDetail> {
        let data: IssueDetailData = self
            .graphql("issue_detail", ISSUE_DETAIL_QUERY, json!({ "id": id.0 }))
            .await?;
        match data.node {
            Some(WireIssueNode::Issue(wire)) => Ok(into_issue_detail(*wire, &self.host)),
            Some(WireIssueNode::Other) => Err(GithubError::GraphQl(vec![format!(
                "node {} is not an issue",
                id.0
            )])),
            None => Err(GithubError::GraphQl(vec![format!(
                "issue {} not found",
                id.0
            )])),
        }
    }

    pub async fn latest_mentions(
        &self,
        ids: &[String],
        login: &str,
        since: &HashMap<String, DateTime<Utc>>,
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        let chunks: Vec<&[String]> = ids.chunks(MENTION_CHUNK).collect();
        let pages: Vec<Vec<WireMentionIssue>> = stream::iter(chunks)
            .map(|chunk| async move {
                let data: WireMentionsData = self
                    .graphql("issue_mentions", MENTIONS_QUERY, json!({ "ids": chunk }))
                    .await?;
                Ok::<_, GithubError>(data.into_issues())
            })
            .buffered(MENTION_CONCURRENCY)
            .try_collect()
            .await?;
        Ok(latest_mention_times(
            pages.into_iter().flatten(),
            login,
            since,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_query_spreads_and_defines_the_shared_fragment() {
        assert!(DETAIL_QUERY.contains("...PullRequestFields"));
        assert!(DETAIL_QUERY.contains("fragment PullRequestFields on PullRequest"));
    }

    #[test]
    fn issue_detail_query_defines_both_shared_fragments() {
        assert!(ISSUE_DETAIL_QUERY.contains("...IssueFields"));
        assert!(ISSUE_DETAIL_QUERY.contains("fragment IssueFields on Issue"));
        assert!(ISSUE_DETAIL_QUERY.contains("...PullRequestFields"));
        assert!(ISSUE_DETAIL_QUERY.contains("fragment PullRequestFields on PullRequest"));
    }

    #[test]
    fn issue_detail_query_aliases_the_linked_prs_to_avoid_a_field_conflict() {
        assert!(
            ISSUE_DETAIL_QUERY
                .contains("linkedPullRequests: closedByPullRequestsReferences(first: 10)")
        );
    }

    #[test]
    fn issue_detail_query_asks_for_the_comment_url() {
        let comment = ISSUE_DETAIL_QUERY
            .split("... on IssueComment {")
            .nth(1)
            .and_then(|rest| rest.split("... on").next())
            .unwrap();
        assert!(comment.contains("url"));
    }

    #[test]
    fn mentions_query_asks_for_the_last_twenty_comments_only() {
        assert!(MENTIONS_QUERY.contains("nodes(ids: $ids)"));
        assert!(MENTIONS_QUERY.contains("timelineItems(last: 20, itemTypes: [ISSUE_COMMENT])"));
    }
}
