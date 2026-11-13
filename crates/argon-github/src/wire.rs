use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::error::{GithubError, Result};

const RATE_LIMITED: &str = "RATE_LIMITED";

#[derive(Deserialize)]
pub(crate) struct Envelope<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: String,
    path: Option<Vec<Value>>,
}

pub(crate) struct Parsed<T> {
    pub(crate) data: T,
    pub(crate) dropped_nodes: usize,
    pub(crate) dropped_kinds: Vec<String>,
    pub(crate) partial_paths: Vec<String>,
}

impl GraphQlError {
    fn is_rate_limited(&self) -> bool {
        self.kind.as_deref() == Some(RATE_LIMITED)
    }

    fn node_key(&self) -> Option<String> {
        let path = self.path.as_deref()?;
        let depth = match path {
            [
                Value::String(root),
                Value::String(field),
                Value::Number(_),
                ..,
            ] if root == "search" && field == "nodes" => 3,
            [Value::String(root), Value::Number(_), ..] if root == "nodes" => 2,
            _ => return None,
        };
        Some(
            path[..depth]
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("."),
        )
    }

    fn partial_path(&self) -> Option<String> {
        match self.path.as_deref()? {
            [Value::String(root), Value::String(_), ..] if root == "node" => {
                Some(describe_path(self.path.as_deref()?))
            }
            _ => None,
        }
    }

    fn describe(&self) -> String {
        match &self.kind {
            Some(kind) => format!("[{kind}] {}", self.message),
            None => self.message.clone(),
        }
    }
}

impl<T> Envelope<T> {
    pub(crate) fn into_data(self, reset_at: DateTime<Utc>) -> Result<Parsed<T>> {
        let errors = self.errors.unwrap_or_default();
        if errors.iter().any(GraphQlError::is_rate_limited) {
            return Err(GithubError::RateLimited { reset_at });
        }
        let (node_errors, others): (Vec<_>, Vec<_>) =
            errors.iter().partition(|error| error.node_key().is_some());
        let (partial_errors, root_errors): (Vec<_>, Vec<_>) = others
            .into_iter()
            .partition(|error| error.partial_path().is_some());
        if !root_errors.is_empty() {
            return Err(graphql_failure(&root_errors));
        }
        let Some(data) = self.data else {
            let all: Vec<&GraphQlError> = node_errors
                .iter()
                .chain(partial_errors.iter())
                .copied()
                .collect();
            return Err(graphql_failure(&all));
        };
        let mut partial_paths: Vec<String> = partial_errors
            .iter()
            .filter_map(|error| error.partial_path())
            .collect();
        partial_paths.sort();
        partial_paths.dedup();
        let dropped: HashSet<String> = node_errors
            .iter()
            .filter_map(|error| error.node_key())
            .collect();
        let mut dropped_kinds: Vec<String> = node_errors
            .iter()
            .filter_map(|error| error.kind.clone())
            .collect();
        dropped_kinds.sort();
        dropped_kinds.dedup();
        Ok(Parsed {
            data,
            dropped_nodes: dropped.len(),
            dropped_kinds,
            partial_paths,
        })
    }
}

fn describe_path(path: &[Value]) -> String {
    path.iter()
        .map(|segment| match segment {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn graphql_failure(errors: &[&GraphQlError]) -> GithubError {
    if errors.is_empty() {
        return GithubError::GraphQl(vec!["response without data".to_string()]);
    }
    GithubError::GraphQl(errors.iter().map(|error| error.describe()).collect())
}

pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
pub(crate) struct Connection<T> {
    nodes: Option<Vec<Option<T>>>,
}

impl<T> Default for Connection<T> {
    fn default() -> Self {
        Self { nodes: None }
    }
}

impl<T> Connection<T> {
    pub(crate) fn into_items(self) -> Vec<T> {
        self.nodes.into_iter().flatten().flatten().collect()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdsData {
    pub(crate) search: IdsConnection,
    pub(crate) rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdsConnection {
    pub(crate) page_info: PageInfo,
    pub(crate) nodes: Vec<Option<IdNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdNode {
    pub(crate) id: Option<String>,
    #[serde(default)]
    pub(crate) updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HydrateData {
    pub(crate) nodes: Vec<Option<WirePr>>,
    pub(crate) rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IssueHydrateData {
    pub(crate) nodes: Vec<Option<WireIssue>>,
    pub(crate) rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    pub(crate) has_next_page: bool,
    pub(crate) end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimit {
    pub(crate) remaining: u32,
    pub(crate) reset_at: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WirePr {
    pub(crate) id: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) is_draft: bool,
    pub(crate) state: WirePrState,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) closed_at: Option<DateTime<Utc>>,
    pub(crate) author: Option<Actor>,
    pub(crate) repository: WireRepository,
    pub(crate) base_ref_name: String,
    pub(crate) head_ref_name: String,
    pub(crate) additions: u32,
    pub(crate) deletions: u32,
    pub(crate) changed_files: u32,
    pub(crate) review_decision: Option<WireReviewDecision>,
    pub(crate) mergeable: WireMergeable,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) comments: TotalCount,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) review_requests: Connection<WireReviewRequest>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) assignees: Connection<Actor>,
    pub(crate) latest_reviews: Option<Connection<WireReview>>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) commits: Connection<WireCommitNode>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) closing_issues_references: Connection<WireIssueRef>,
}

#[derive(Deserialize)]
pub(crate) struct WireIssueRef {
    pub(crate) number: u64,
    pub(crate) repository: WireRepository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireIssue {
    pub(crate) id: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) state: WireIssueState,
    pub(crate) state_reason: Option<WireIssueStateReason>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) closed_at: Option<DateTime<Utc>>,
    pub(crate) author: Option<Actor>,
    pub(crate) repository: WireRepository,
    pub(crate) issue_type: Option<WireIssueType>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) labels: Connection<WireLabel>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) assignees: Connection<Actor>,
    pub(crate) milestone: Option<WireMilestone>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) comments: TotalCount,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) closed_by_pull_requests_references: Connection<WireLinkedPr>,
    pub(crate) sub_issues_summary: Option<WireSubIssuesSummary>,
}

#[derive(Deserialize)]
pub(crate) struct WireIssueType {
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct WireLabel {
    pub(crate) name: String,
    pub(crate) color: String,
}

#[derive(Deserialize)]
pub(crate) struct WireMilestone {
    pub(crate) title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireLinkedPr {
    pub(crate) number: u64,
    pub(crate) repository: WireRepository,
    pub(crate) state: WirePrState,
    pub(crate) is_draft: bool,
}

#[derive(Deserialize)]
pub(crate) struct WireSubIssuesSummary {
    pub(crate) total: u32,
    pub(crate) completed: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireIssueState {
    Open,
    Closed,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireIssueStateReason {
    Completed,
    NotPlanned,
    Duplicate,
    Reopened,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(crate) struct Actor {
    pub(crate) login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireRepository {
    pub(crate) name_with_owner: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TotalCount {
    pub(crate) total_count: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReviewRequest {
    pub(crate) requested_reviewer: Option<WireReviewer>,
}

#[derive(Deserialize)]
pub(crate) struct WireReviewer {
    pub(crate) login: Option<String>,
    pub(crate) slug: Option<String>,
    pub(crate) organization: Option<WireOrganization>,
}

#[derive(Deserialize)]
pub(crate) struct WireOrganization {
    pub(crate) login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReview {
    pub(crate) author: Option<Actor>,
    pub(crate) state: WireReviewState,
    pub(crate) submitted_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub(crate) struct WireCommitNode {
    pub(crate) commit: WireCommit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireCommit {
    pub(crate) committed_date: DateTime<Utc>,
    pub(crate) status_check_rollup: Option<WireRollup>,
}

#[derive(Deserialize)]
pub(crate) struct WireRollup {
    pub(crate) state: WireRollupState,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WirePrState {
    Open,
    Closed,
    Merged,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireMergeable {
    Mergeable,
    Conflicting,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireRollupState {
    Success,
    Failure,
    Pending,
    Expected,
    Error,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
pub(crate) struct ViewerData {
    pub(crate) viewer: WireViewer,
}

#[derive(Deserialize)]
pub(crate) struct WireViewer {
    pub(crate) id: String,
    pub(crate) login: String,
    pub(crate) organizations: Connection<WireViewerOrg>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireViewerOrg {
    pub(crate) login: String,
    pub(crate) viewer_can_administer: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTIAL_PAGE: &str = include_str!("../tests/fixtures/search_page_partial.json");

    fn reset() -> DateTime<Utc> {
        "2026-11-02T14:00:00Z".parse().unwrap()
    }

    fn envelope(json: &str) -> Envelope<IdsData> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn node_errors_drop_the_node_and_keep_the_rest_of_the_page() {
        let parsed = envelope(PARTIAL_PAGE).into_data(reset()).unwrap();
        assert_eq!(parsed.dropped_nodes, 1);
        assert_eq!(parsed.dropped_kinds, vec!["FORBIDDEN"]);
        let ids: Vec<String> = parsed
            .data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.id)
            .collect();
        assert_eq!(ids, vec!["PR_kwDOOrbita0412", "PR_kwDOOrbita0080"]);
    }

    #[test]
    fn nodes_query_errors_are_node_errors_too() {
        let json = r#"{
            "data": { "nodes": [null], "rateLimit": { "remaining": 4000, "resetAt": "2026-11-02T14:00:00Z" } },
            "errors": [{ "type": "FORBIDDEN", "path": ["nodes", 0], "message": "blocked" }]
        }"#;
        let envelope: Envelope<HydrateData> = serde_json::from_str(json).unwrap();
        let parsed = envelope.into_data(reset()).unwrap();
        assert_eq!(parsed.dropped_nodes, 1);
        assert!(parsed.data.nodes[0].is_none());
    }

    #[test]
    fn root_errors_fail_even_with_partial_data() {
        let json = r#"{
            "data": { "search": { "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] },
                      "rateLimit": { "remaining": 4000, "resetAt": "2026-11-02T14:00:00Z" } },
            "errors": [{ "type": "INTERNAL", "message": "something broke" }]
        }"#;
        match envelope(json).into_data(reset()) {
            Err(GithubError::GraphQl(messages)) => {
                assert_eq!(messages, vec!["[INTERNAL] something broke"]);
            }
            other => panic!("unexpected result: {:?}", other.err()),
        }
    }

    #[test]
    fn null_data_fails_even_when_errors_are_node_errors() {
        let json = r#"{
            "data": null,
            "errors": [{ "type": "FORBIDDEN", "path": ["search", "nodes", 0], "message": "blocked" }]
        }"#;
        assert!(matches!(
            envelope(json).into_data(reset()),
            Err(GithubError::GraphQl(_))
        ));
    }

    #[test]
    fn rate_limited_error_maps_to_rate_limited() {
        let json = r#"{
            "data": null,
            "errors": [{ "type": "RATE_LIMITED", "message": "API rate limit exceeded" }]
        }"#;
        match envelope(json).into_data(reset()) {
            Err(GithubError::RateLimited { reset_at }) => assert_eq!(reset_at, reset()),
            other => panic!("unexpected result: {:?}", other.err()),
        }
    }
}
