use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{GithubError, Result};

#[derive(Deserialize)]
pub(crate) struct Envelope<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlMessage>>,
}

#[derive(Deserialize)]
struct GraphQlMessage {
    message: String,
}

impl<T> Envelope<T> {
    pub(crate) fn into_data(self) -> Result<T> {
        if let Some(errors) = self.errors.filter(|errors| !errors.is_empty()) {
            return Err(GithubError::GraphQl(
                errors.into_iter().map(|error| error.message).collect(),
            ));
        }
        self.data
            .ok_or_else(|| GithubError::GraphQl(vec!["response without data".to_string()]))
    }
}

#[derive(Deserialize)]
pub(crate) struct Connection<T> {
    nodes: Option<Vec<Option<T>>>,
}

impl<T> Connection<T> {
    pub(crate) fn into_items(self) -> Vec<T> {
        self.nodes.into_iter().flatten().flatten().collect()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchData {
    pub(crate) search: SearchConnection,
    pub(crate) rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchConnection {
    pub(crate) page_info: PageInfo,
    pub(crate) nodes: Vec<Option<WirePr>>,
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
    pub(crate) comments: TotalCount,
    pub(crate) review_requests: Connection<WireReviewRequest>,
    pub(crate) assignees: Connection<Actor>,
    pub(crate) latest_reviews: Option<Connection<WireReview>>,
    pub(crate) commits: Connection<WireCommitNode>,
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

#[derive(Deserialize)]
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
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireMergeable {
    Mergeable,
    Conflicting,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum WireRollupState {
    Success,
    Failure,
    Pending,
    Expected,
    Error,
}

#[derive(Deserialize)]
pub(crate) struct ViewerData {
    pub(crate) viewer: WireViewer,
}

#[derive(Deserialize)]
pub(crate) struct WireViewer {
    pub(crate) login: String,
    pub(crate) organizations: Connection<WireViewerOrg>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireViewerOrg {
    pub(crate) login: String,
    pub(crate) viewer_can_administer: bool,
}
