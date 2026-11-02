use std::collections::HashMap;
use std::future::Future;

use argon_core::PrId;
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::{Value, json};

use crate::client::GithubClient;
use crate::error::{GithubError, Result};
use crate::mutations::require_payload;
use crate::wire::PageInfo;

const VIEWED_FILES_QUERY: &str = include_str!("../queries/viewed_files.graphql");
const MARK_VIEWED_QUERY: &str = include_str!("../queries/mark_file_viewed.graphql");
const UNMARK_VIEWED_QUERY: &str = include_str!("../queries/unmark_file_viewed.graphql");
const MAX_PAGES: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewedState {
    Viewed,
    Unviewed,
    Dismissed,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireViewedState {
    Viewed,
    Unviewed,
    Dismissed,
    #[serde(other)]
    Unknown,
}

impl From<WireViewedState> for ViewedState {
    fn from(state: WireViewedState) -> Self {
        match state {
            WireViewedState::Viewed => Self::Viewed,
            WireViewedState::Dismissed => Self::Dismissed,
            WireViewedState::Unviewed | WireViewedState::Unknown => Self::Unviewed,
        }
    }
}

#[derive(Deserialize)]
struct ViewedData {
    node: Option<ViewedNode>,
}

#[derive(Deserialize)]
struct ViewedNode {
    files: Option<ViewedFiles>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewedFiles {
    nodes: Option<Vec<Option<ViewedFile>>>,
    page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewedFile {
    path: Option<String>,
    viewer_viewed_state: Option<WireViewedState>,
}

#[derive(Deserialize)]
struct ViewedMutationData {
    #[serde(alias = "markFileAsViewed", alias = "unmarkFileAsViewed")]
    payload: Option<IgnoredAny>,
}

struct ViewedPage {
    entries: Vec<(String, ViewedState)>,
    next_cursor: Option<String>,
}

fn into_page(data: ViewedData, pr: &PrId) -> Result<ViewedPage> {
    let node = data
        .node
        .ok_or_else(|| GithubError::GraphQl(vec![format!("pull request {} not found", pr.0)]))?;
    let Some(files) = node.files else {
        return Ok(ViewedPage {
            entries: Vec::new(),
            next_cursor: None,
        });
    };
    let entries = files
        .nodes
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|file| {
            let state = file
                .viewer_viewed_state
                .map_or(ViewedState::Unviewed, Into::into);
            Some((file.path?, state))
        })
        .collect();
    let next_cursor = files
        .page_info
        .filter(|info| info.has_next_page)
        .and_then(|info| info.end_cursor);
    Ok(ViewedPage {
        entries,
        next_cursor,
    })
}

async fn collect_viewed<F, Fut>(mut fetch: F) -> Result<HashMap<String, ViewedState>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<ViewedPage>>,
{
    let mut states = HashMap::new();
    let mut cursor = None;
    for _ in 0..MAX_PAGES {
        let page = fetch(cursor).await?;
        states.extend(page.entries);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok(states),
        }
    }
    tracing::warn!(
        kept = states.len(),
        "github returned more files than the page cap, viewed states truncated"
    );
    Ok(states)
}

fn viewed_files_variables(pr: &PrId, after: Option<&str>) -> Value {
    json!({ "id": pr.0, "after": after })
}

fn viewed_variables(pr: &PrId, path: &str) -> Value {
    json!({ "input": { "pullRequestId": pr.0, "path": path } })
}

fn viewed_mutation(viewed: bool) -> (&'static str, &'static str) {
    if viewed {
        ("markFileAsViewed", MARK_VIEWED_QUERY)
    } else {
        ("unmarkFileAsViewed", UNMARK_VIEWED_QUERY)
    }
}

impl GithubClient {
    pub async fn viewed_files(&self, pr: &PrId) -> Result<HashMap<String, ViewedState>> {
        collect_viewed(|cursor| async move {
            let data: ViewedData = self
                .graphql(
                    "viewed_files",
                    VIEWED_FILES_QUERY,
                    viewed_files_variables(pr, cursor.as_deref()),
                )
                .await?;
            into_page(data, pr)
        })
        .await
    }

    pub async fn set_viewed(&self, pr: &PrId, path: &str, viewed: bool) -> Result<()> {
        let (mutation, query) = viewed_mutation(viewed);
        let data: ViewedMutationData = self
            .graphql(mutation, query, viewed_variables(pr, path))
            .await?;
        require_payload(data.payload, mutation)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::wire::Envelope;

    const VIEWED_FILES: &str = include_str!("../tests/fixtures/viewed_files.json");

    fn pr() -> PrId {
        PrId("PR_kwDOOrbita0412".to_string())
    }

    fn fixture_page() -> ViewedPage {
        let envelope: Envelope<ViewedData> = serde_json::from_str(VIEWED_FILES).unwrap();
        let parsed = envelope.into_data(Utc::now()).unwrap();
        assert_eq!(parsed.partial_paths.len(), 1);
        into_page(parsed.data, &pr()).unwrap()
    }

    fn page(paths: &[&str], next: Option<&str>) -> ViewedPage {
        ViewedPage {
            entries: paths
                .iter()
                .map(|path| (path.to_string(), ViewedState::Viewed))
                .collect(),
            next_cursor: next.map(str::to_string),
        }
    }

    #[test]
    fn fixture_maps_every_viewed_state_and_skips_empty_nodes() {
        let page = fixture_page();
        let entries: Vec<(&str, ViewedState)> = page
            .entries
            .iter()
            .map(|(path, state)| (path.as_str(), *state))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("src/reschedule/slot-finder.ts", ViewedState::Viewed),
                ("src/reschedule/retry-budget.ts", ViewedState::Unviewed),
                ("docs/adr/0007-retry.md", ViewedState::Dismissed),
                ("assets/logo.png", ViewedState::Unviewed),
                ("src/legacy/old.ts", ViewedState::Unviewed),
            ]
        );
        assert_eq!(page.next_cursor.as_deref(), Some("Y3Vyc29yOjEwMA=="));
    }

    #[test]
    fn missing_node_is_an_error_and_missing_files_is_an_empty_page() {
        let missing: ViewedData = serde_json::from_str(r#"{ "node": null }"#).unwrap();
        assert!(matches!(
            into_page(missing, &pr()),
            Err(GithubError::GraphQl(_))
        ));
        let empty: ViewedData = serde_json::from_str(r#"{ "node": { "files": null } }"#).unwrap();
        let page = into_page(empty, &pr()).unwrap();
        assert!(page.entries.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn last_page_has_no_cursor_even_with_an_end_cursor() {
        let data: ViewedData = serde_json::from_str(
            r#"{ "node": { "files": { "nodes": [], "pageInfo": { "hasNextPage": false, "endCursor": "abc" } } } }"#,
        )
        .unwrap();
        assert!(into_page(data, &pr()).unwrap().next_cursor.is_none());
    }

    #[tokio::test]
    async fn pages_are_followed_by_cursor_and_merged() {
        let mut cursors = Vec::new();
        let states = collect_viewed(|cursor| {
            cursors.push(cursor.clone());
            let next = match cursor.as_deref() {
                None => page(&["a.ts"], Some("c1")),
                Some("c1") => page(&["b.ts"], Some("c2")),
                _ => page(&["c.ts"], None),
            };
            async move { Ok(next) }
        })
        .await
        .unwrap();
        assert_eq!(states.len(), 3);
        assert_eq!(
            cursors,
            vec![None, Some("c1".to_string()), Some("c2".to_string())]
        );
    }

    #[tokio::test]
    async fn pagination_stops_at_the_page_cap() {
        let mut calls = 0;
        let states = collect_viewed(|_| {
            calls += 1;
            let path = format!("file-{calls}.ts");
            async move { Ok(page(&[path.as_str()], Some("more"))) }
        })
        .await
        .unwrap();
        assert_eq!(calls, MAX_PAGES);
        assert_eq!(states.len(), MAX_PAGES as usize);
    }

    #[tokio::test]
    async fn page_errors_propagate() {
        let result = collect_viewed(|_| async { Err(GithubError::Decode("boom".into())) }).await;
        assert!(result.is_err());
    }

    #[test]
    fn viewed_files_variables_carry_id_and_cursor() {
        assert_eq!(
            viewed_files_variables(&pr(), None),
            json!({ "id": "PR_kwDOOrbita0412", "after": null })
        );
        assert_eq!(
            viewed_files_variables(&pr(), Some("c1")),
            json!({ "id": "PR_kwDOOrbita0412", "after": "c1" })
        );
    }

    #[test]
    fn viewed_variables_carry_pull_request_and_path() {
        assert_eq!(
            viewed_variables(&pr(), "src/a \"b\".ts"),
            json!({ "input": { "pullRequestId": "PR_kwDOOrbita0412", "path": "src/a \"b\".ts" } })
        );
    }

    #[test]
    fn viewed_flag_selects_the_matching_mutation() {
        let (mark, mark_query) = viewed_mutation(true);
        assert_eq!(mark, "markFileAsViewed");
        assert!(mark_query.contains("markFileAsViewed(input: $input)"));
        assert!(mark_query.contains("$input: MarkFileAsViewedInput!"));
        let (unmark, unmark_query) = viewed_mutation(false);
        assert_eq!(unmark, "unmarkFileAsViewed");
        assert!(unmark_query.contains("unmarkFileAsViewed(input: $input)"));
        assert!(unmark_query.contains("$input: UnmarkFileAsViewedInput!"));
    }

    #[test]
    fn viewed_files_document_matches_its_variables() {
        assert!(VIEWED_FILES_QUERY.contains("$id: ID!"));
        assert!(VIEWED_FILES_QUERY.contains("files(first: 100, after: $after)"));
        assert!(VIEWED_FILES_QUERY.contains("viewerViewedState"));
    }

    #[test]
    fn mutation_payloads_deserialize_under_either_field_name() {
        for json in [
            r#"{ "markFileAsViewed": { "clientMutationId": null } }"#,
            r#"{ "unmarkFileAsViewed": { "clientMutationId": null } }"#,
        ] {
            let data: ViewedMutationData = serde_json::from_str(json).unwrap();
            assert!(require_payload(data.payload, "viewed").is_ok());
        }
    }

    #[test]
    fn null_mutation_payload_is_an_error() {
        let data: ViewedMutationData =
            serde_json::from_str(r#"{ "markFileAsViewed": null }"#).unwrap();
        assert!(matches!(
            require_payload(data.payload, "markFileAsViewed"),
            Err(GithubError::GraphQl(_))
        ));
    }
}
