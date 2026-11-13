use std::collections::{HashMap, HashSet};

use argon_core::{Involvement, Issue, PullRequest};
use chrono::{DateTime, Duration, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::client::GithubClient;
use crate::convert::{into_issues, into_pull_requests};
use crate::error::{GithubError, Result};
use crate::wire::{HydrateData, IdsData, IssueHydrateData, RateLimit};

const IDS_QUERY: &str = include_str!("../queries/inbox_ids.graphql");
const HYDRATE_QUERY: &str = concat!(
    include_str!("../queries/inbox_hydrate.graphql"),
    include_str!("../queries/pull_request_fields.graphql")
);
const ISSUE_HYDRATE_QUERY: &str = concat!(
    include_str!("../queries/issue_hydrate.graphql"),
    include_str!("../queries/issue_fields.graphql")
);
const PAGE_SIZE: usize = 100;
const ORG_PAGE_SIZE: usize = 50;
const MIN_BATCH: usize = 10;
const HYDRATE_CHUNK: usize = 40;
const SEARCH_RESULT_LIMIT: usize = 1000;
const RATE_LIMIT_FLOOR: u32 = 200;
const SEARCH_CONCURRENCY: usize = 3;
const HYDRATE_CONCURRENCY: usize = 3;

#[derive(Clone, Copy)]
enum Kind {
    Pr,
    Issue,
}

impl Kind {
    fn qualifier(self) -> &'static str {
        match self {
            Self::Pr => "is:pr",
            Self::Issue => "is:issue",
        }
    }
}

struct Search {
    query: String,
    involvement: Involvement,
    page_size: usize,
    limit: usize,
    org: Option<String>,
}

struct Found {
    involvement: Involvement,
    ids: Vec<(String, DateTime<Utc>)>,
    has_more: bool,
    org: Option<String>,
}

trait Hydrated {
    fn id(&self) -> &str;
    fn updated_at(&self) -> DateTime<Utc>;
    fn set_involvement(&mut self, involvement: Involvement);
}

impl Hydrated for PullRequest {
    fn id(&self) -> &str {
        &self.id.0
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn set_involvement(&mut self, involvement: Involvement) {
        self.involvement = involvement;
    }
}

impl Hydrated for Issue {
    fn id(&self) -> &str {
        &self.id.0
    }

    fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn set_involvement(&mut self, involvement: Involvement) {
        self.involvement = involvement;
    }
}

trait HydratePage: DeserializeOwned {
    type Item: Hydrated;
    const OPERATION: &'static str;
    const QUERY: &'static str;

    fn rate_limit(&self) -> &RateLimit;
    fn into_items(self, host: &str) -> Vec<Self::Item>;
}

impl HydratePage for HydrateData {
    type Item = PullRequest;
    const OPERATION: &'static str = "hydrate";
    const QUERY: &'static str = HYDRATE_QUERY;

    fn rate_limit(&self) -> &RateLimit {
        &self.rate_limit
    }

    fn into_items(self, host: &str) -> Vec<PullRequest> {
        into_pull_requests(self.nodes, host)
    }
}

impl HydratePage for IssueHydrateData {
    type Item = Issue;
    const OPERATION: &'static str = "issue_hydrate";
    const QUERY: &'static str = ISSUE_HYDRATE_QUERY;

    fn rate_limit(&self) -> &RateLimit {
        &self.rate_limit
    }

    fn into_items(self, host: &str) -> Vec<Issue> {
        into_issues(self.nodes, host)
    }
}

pub struct Fetched<T> {
    pub items: Vec<T>,
    pub kept: HashSet<String>,
    pub unchanged: Vec<(String, Involvement)>,
}

pub struct OrgFetch<T> {
    pub fetched: Fetched<T>,
    pub orgs_with_more: Vec<String>,
}

struct Plan {
    pending: Vec<(String, Involvement)>,
    unchanged: Vec<(String, Involvement)>,
    kept: HashSet<String>,
}

pub fn ids_to_hydrate(
    found: &[(String, DateTime<Utc>)],
    known_updated: &HashMap<String, DateTime<Utc>>,
) -> (Vec<String>, HashSet<String>) {
    let mut all = HashSet::with_capacity(found.len());
    let mut stale = Vec::new();
    for (id, updated_at) in found {
        if all.insert(id.clone()) && known_updated.get(id) != Some(updated_at) {
            stale.push(id.clone());
        }
    }
    (stale, all)
}

fn plan_hydration(
    found: Vec<Found>,
    known_updated: &HashMap<String, DateTime<Utc>>,
    skip: &HashSet<String>,
) -> Plan {
    let seen: Vec<(String, DateTime<Utc>)> = found
        .iter()
        .flat_map(|found| found.ids.iter().cloned())
        .collect();
    let (stale, kept) = ids_to_hydrate(&seen, known_updated);
    let stale: HashSet<String> = stale.into_iter().collect();
    let (pending, unchanged) = merge_ids(into_batches(found))
        .into_iter()
        .filter(|(id, _)| !skip.contains(id))
        .partition(|(id, _)| stale.contains(id));
    Plan {
        pending,
        unchanged,
        kept,
    }
}

impl GithubClient {
    pub async fn fetch_important(&self, closed_window_days: i64) -> Result<Fetched<PullRequest>> {
        let found = self
            .run_searches(important_searches(&since(closed_window_days)))
            .await?;
        self.hydrate_plan::<HydrateData>(plan_hydration(found, &HashMap::new(), &HashSet::new()))
            .await
    }

    pub async fn fetch_orgs(
        &self,
        orgs: &[(String, usize)],
        known: &HashSet<String>,
        known_updated: &HashMap<String, DateTime<Utc>>,
        closed_window_days: i64,
    ) -> Result<OrgFetch<PullRequest>> {
        let found = self
            .run_searches(org_searches(Kind::Pr, orgs, &since(closed_window_days)))
            .await?;
        let orgs_with_more = orgs_with_more(&found);
        let fetched = self
            .hydrate_plan::<HydrateData>(plan_hydration(found, known_updated, known))
            .await?;
        Ok(OrgFetch {
            fetched,
            orgs_with_more,
        })
    }

    pub async fn fetch_important_issues(
        &self,
        known_updated: &HashMap<String, DateTime<Utc>>,
        closed_window_days: i64,
    ) -> Result<Fetched<Issue>> {
        let found = self
            .run_searches(important_issue_searches(&since(closed_window_days)))
            .await?;
        self.hydrate_plan::<IssueHydrateData>(plan_hydration(found, known_updated, &HashSet::new()))
            .await
    }

    pub async fn fetch_org_issues(
        &self,
        orgs: &[(String, usize)],
        known: &HashSet<String>,
        known_updated: &HashMap<String, DateTime<Utc>>,
        closed_window_days: i64,
    ) -> Result<OrgFetch<Issue>> {
        let found = self
            .run_searches(org_searches(Kind::Issue, orgs, &since(closed_window_days)))
            .await?;
        let orgs_with_more = orgs_with_more(&found);
        let fetched = self
            .hydrate_plan::<IssueHydrateData>(plan_hydration(found, known_updated, known))
            .await?;
        Ok(OrgFetch {
            fetched,
            orgs_with_more,
        })
    }

    async fn hydrate_plan<P: HydratePage>(&self, plan: Plan) -> Result<Fetched<P::Item>> {
        let Plan {
            pending,
            unchanged,
            mut kept,
        } = plan;
        let requested: Vec<String> = pending.iter().map(|(id, _)| id.clone()).collect();
        let items = self.hydrate_all::<P>(pending).await?;
        let returned: HashSet<&str> = items.iter().map(|item| item.id()).collect();
        for id in requested
            .iter()
            .filter(|id| !returned.contains(id.as_str()))
        {
            kept.remove(id);
        }
        Ok(Fetched {
            items,
            kept,
            unchanged,
        })
    }

    async fn run_searches(&self, searches: Vec<Search>) -> Result<Vec<Found>> {
        stream::iter(searches)
            .map(|search| async move { self.search_ids(&search).await })
            .buffered(SEARCH_CONCURRENCY)
            .try_collect()
            .await
    }

    async fn hydrate_all<P: HydratePage>(
        &self,
        merged: Vec<(String, Involvement)>,
    ) -> Result<Vec<P::Item>> {
        let chunks: Vec<Vec<String>> = merged
            .chunks(HYDRATE_CHUNK)
            .map(|chunk| chunk.iter().map(|(id, _)| id.clone()).collect())
            .collect();
        let hydrated: Vec<Vec<P::Item>> = stream::iter(chunks)
            .map(|chunk| async move { self.hydrate::<P>(&chunk).await })
            .buffered(HYDRATE_CONCURRENCY)
            .try_collect()
            .await?;
        Ok(apply_involvement(
            hydrated.into_iter().flatten().collect(),
            merged,
        ))
    }

    async fn search_ids(&self, search: &Search) -> Result<Found> {
        let mut ids = Vec::new();
        let mut fetched = 0;
        let mut after: Option<String> = None;
        let mut page_size = search.page_size;
        let mut has_next_page = false;
        while fetched < search.limit {
            let first = page_size.min(search.limit - fetched);
            let variables = json!({ "query": search.query, "after": after, "first": first });
            let data: IdsData = match self.graphql("search_ids", IDS_QUERY, variables).await {
                Err(GithubError::Http(error))
                    if is_gateway_failure(&error) && page_size / 2 >= MIN_BATCH =>
                {
                    page_size /= 2;
                    continue;
                }
                result => result?,
            };
            check_rate_limit(&data.rate_limit)?;
            page_size = (page_size * 2).min(search.page_size);
            fetched += data.search.nodes.len();
            ids.extend(
                data.search
                    .nodes
                    .into_iter()
                    .flatten()
                    .filter_map(|node| Some((node.id?, node.updated_at?))),
            );
            has_next_page = data.search.page_info.has_next_page;
            if !has_next_page {
                break;
            }
            after = data.search.page_info.end_cursor;
        }
        if fetched >= SEARCH_RESULT_LIMIT {
            tracing::warn!(
                query = search.query,
                "search reached the result limit, results may be truncated"
            );
        }
        Ok(Found {
            involvement: search.involvement,
            ids,
            has_more: has_next_page && fetched < SEARCH_RESULT_LIMIT,
            org: search.org.clone(),
        })
    }

    async fn hydrate<P: HydratePage>(&self, ids: &[String]) -> Result<Vec<P::Item>> {
        let mut items = Vec::new();
        let mut pending: Vec<&[String]> = vec![ids];
        while let Some(batch) = pending.pop() {
            let variables = json!({ "ids": batch });
            let data: P = match self.graphql(P::OPERATION, P::QUERY, variables).await {
                Err(GithubError::Http(error))
                    if is_gateway_failure(&error) && batch.len() / 2 >= MIN_BATCH =>
                {
                    let (first, second) = batch.split_at(batch.len() / 2);
                    pending.push(second);
                    pending.push(first);
                    continue;
                }
                result => result?,
            };
            check_rate_limit(data.rate_limit())?;
            items.extend(data.into_items(&self.host));
        }
        Ok(items)
    }
}

fn apply_involvement<T: Hydrated>(mut items: Vec<T>, merged: Vec<(String, Involvement)>) -> Vec<T> {
    let involvement: HashMap<String, Involvement> = merged.into_iter().collect();
    for item in &mut items {
        if let Some(found) = involvement.get(item.id()) {
            item.set_involvement(*found);
        }
    }
    items.sort_by(|left, right| {
        right
            .updated_at()
            .cmp(&left.updated_at())
            .then_with(|| left.id().cmp(right.id()))
    });
    items
}

fn orgs_with_more(found: &[Found]) -> Vec<String> {
    found
        .iter()
        .filter(|found| found.has_more)
        .filter_map(|found| found.org.clone())
        .collect()
}

fn check_rate_limit(rate_limit: &RateLimit) -> Result<()> {
    if rate_limit.remaining < RATE_LIMIT_FLOOR {
        return Err(GithubError::RateLimited {
            reset_at: rate_limit.reset_at,
        });
    }
    Ok(())
}

fn is_gateway_failure(error: &reqwest::Error) -> bool {
    error.status().is_some_and(|status| {
        matches!(
            status,
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
        )
    })
}

fn since(closed_window_days: i64) -> String {
    (Utc::now() - Duration::days(closed_window_days))
        .format("%Y-%m-%d")
        .to_string()
}

fn flagged_search(query: &str, involvement: Involvement) -> Search {
    Search {
        query: query.to_string(),
        involvement,
        page_size: PAGE_SIZE,
        limit: SEARCH_RESULT_LIMIT,
        org: None,
    }
}

fn important_searches(since: &str) -> Vec<Search> {
    vec![
        flagged_search(
            "is:pr is:open review-requested:@me",
            Involvement {
                requested: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            "is:pr is:open assignee:@me",
            Involvement {
                assigned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:pr author:@me updated:>={since}"),
            Involvement {
                authored: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            "is:pr is:open reviewed-by:@me -author:@me",
            Involvement {
                reviewed: true,
                ..Involvement::default()
            },
        ),
    ]
}

fn important_issue_searches(since: &str) -> Vec<Search> {
    vec![
        flagged_search(
            "is:issue is:open assignee:@me",
            Involvement {
                assigned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue author:@me updated:>={since}"),
            Involvement {
                authored: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue is:open mentions:@me updated:>={since}"),
            Involvement {
                mentioned: true,
                ..Involvement::default()
            },
        ),
        flagged_search(
            &format!("is:issue is:open commenter:@me -author:@me updated:>={since}"),
            Involvement {
                commented: true,
                ..Involvement::default()
            },
        ),
    ]
}

fn org_searches(kind: Kind, orgs: &[(String, usize)], since: &str) -> Vec<Search> {
    let qualifier = kind.qualifier();
    orgs.iter()
        .map(|(org, depth)| Search {
            query: format!("{qualifier} org:{org} updated:>={since} sort:updated-desc"),
            involvement: Involvement::default(),
            page_size: ORG_PAGE_SIZE,
            limit: ((*depth).max(1) * ORG_PAGE_SIZE).min(SEARCH_RESULT_LIMIT),
            org: Some(org.clone()),
        })
        .collect()
}

fn into_batches(found: Vec<Found>) -> Vec<(Involvement, Vec<String>)> {
    found
        .into_iter()
        .map(|found| {
            let ids = found.ids.into_iter().map(|(id, _)| id).collect();
            (found.involvement, ids)
        })
        .collect()
}

fn merge_ids(batches: Vec<(Involvement, Vec<String>)>) -> Vec<(String, Involvement)> {
    let mut merged: Vec<(String, Involvement)> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for (involvement, ids) in batches {
        for id in ids {
            match positions.get(&id) {
                Some(&index) => merged[index].1 = union(merged[index].1, involvement),
                None => {
                    positions.insert(id.clone(), merged.len());
                    merged.push((id, involvement));
                }
            }
        }
    }
    merged
}

fn union(left: Involvement, right: Involvement) -> Involvement {
    Involvement {
        requested: left.requested || right.requested,
        assigned: left.assigned || right.assigned,
        authored: left.authored || right.authored,
        reviewed: left.reviewed || right.reviewed,
        mentioned: left.mentioned || right.mentioned,
        commented: left.commented || right.commented,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::tests::{fixture_issues, fixture_prs};
    use crate::wire::Envelope;

    #[test]
    fn hydrate_query_spreads_and_defines_the_shared_fragment() {
        assert!(HYDRATE_QUERY.contains("...PullRequestFields"));
        assert!(HYDRATE_QUERY.contains("fragment PullRequestFields on PullRequest"));
    }

    #[test]
    fn merge_ors_involvement_of_the_same_pr_across_searches() {
        let ids: Vec<String> = fixture_prs().into_iter().map(|pr| pr.id.0).collect();
        let requested = Involvement {
            requested: true,
            ..Involvement::default()
        };
        let authored = Involvement {
            authored: true,
            ..Involvement::default()
        };
        let reviewed = Involvement {
            reviewed: true,
            ..Involvement::default()
        };
        let batches = vec![
            (requested, vec![ids[0].clone(), ids[1].clone()]),
            (authored, vec![ids[0].clone()]),
            (Involvement::default(), vec![ids[1].clone(), ids[2].clone()]),
            (reviewed, vec![ids[0].clone(), ids[3].clone()]),
        ];

        let merged = merge_ids(batches);

        let merged_ids: Vec<&str> = merged.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(merged_ids, vec![&ids[0], &ids[1], &ids[2], &ids[3]]);
        assert_eq!(
            merged[0].1,
            Involvement {
                requested: true,
                authored: true,
                reviewed: true,
                ..Involvement::default()
            }
        );
        assert_eq!(merged[1].1, requested);
        assert_eq!(merged[2].1, Involvement::default());
        assert_eq!(merged[3].1, reviewed);
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn found_with(involvement: Involvement, ids: &[(&str, i64)]) -> Found {
        Found {
            involvement,
            ids: ids
                .iter()
                .map(|(id, seconds)| (id.to_string(), at(*seconds)))
                .collect(),
            has_more: false,
            org: None,
        }
    }

    fn pair(id: &str, seconds: i64) -> (String, DateTime<Utc>) {
        (id.to_string(), at(seconds))
    }

    #[test]
    fn ids_to_hydrate_selects_new_changed_and_skips_equal() {
        let found = vec![
            pair("new", 100),
            pair("same", 200),
            pair("changed", 300),
            pair("older_in_search", 50),
        ];
        let known = HashMap::from([
            pair("same", 200),
            pair("changed", 250),
            pair("older_in_search", 60),
            pair("gone", 10),
        ]);

        let (stale, all) = ids_to_hydrate(&found, &known);

        assert_eq!(stale, vec!["new", "changed", "older_in_search"]);
        assert_eq!(
            all,
            HashSet::from(["new", "same", "changed", "older_in_search"].map(String::from))
        );
    }

    #[test]
    fn ids_to_hydrate_with_empty_database_hydrates_everything_once() {
        let found = vec![pair("a", 1), pair("b", 2), pair("a", 1)];

        let (stale, all) = ids_to_hydrate(&found, &HashMap::new());

        assert_eq!(stale, vec!["a", "b"]);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn ids_to_hydrate_with_nothing_found_is_empty() {
        let known = HashMap::from([pair("a", 1)]);
        let (stale, all) = ids_to_hydrate(&[], &known);
        assert!(stale.is_empty() && all.is_empty());
    }

    #[test]
    fn plan_keeps_every_found_id_but_hydrates_only_stale_ones_with_merged_involvement() {
        let requested = Involvement {
            requested: true,
            ..Involvement::default()
        };
        let authored = Involvement {
            authored: true,
            ..Involvement::default()
        };
        let found = vec![
            found_with(requested, &[("stale", 20), ("fresh", 10)]),
            found_with(authored, &[("fresh", 10), ("fresh_too", 5)]),
        ];
        let known = HashMap::from([pair("stale", 19), pair("fresh", 10), pair("fresh_too", 5)]);

        let plan = plan_hydration(found, &known, &HashSet::new());

        assert_eq!(plan.pending, vec![("stale".to_string(), requested)]);
        assert_eq!(
            plan.unchanged,
            vec![
                (
                    "fresh".to_string(),
                    Involvement {
                        requested: true,
                        authored: true,
                        ..Involvement::default()
                    }
                ),
                ("fresh_too".to_string(), authored),
            ]
        );
        assert_eq!(plan.kept.len(), 3);
    }

    #[test]
    fn plan_skips_ids_owned_by_the_important_phase_but_still_keeps_them() {
        let found = vec![found_with(
            Involvement::default(),
            &[("owned", 1), ("other", 2), ("unchanged_owned", 3)],
        )];
        let owned = HashSet::from(["owned".to_string(), "unchanged_owned".to_string()]);
        let known = HashMap::from([pair("unchanged_owned", 3)]);

        let plan = plan_hydration(found, &known, &owned);

        assert_eq!(
            plan.pending,
            vec![("other".to_string(), Involvement::default())]
        );
        assert!(plan.unchanged.is_empty());
        assert_eq!(plan.kept.len(), 3);
    }

    #[test]
    fn ids_page_fixture_skips_non_pull_request_nodes() {
        let envelope: Envelope<IdsData> =
            serde_json::from_str(include_str!("../tests/fixtures/search_page.json")).unwrap();
        let data = envelope.into_data(Utc::now()).unwrap().data;
        let ids: Vec<String> = data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "PR_kwDOOrbita0412",
                "PR_kwDOOrbita0398",
                "PR_kwDOOrbita0080"
            ]
        );
        assert!(data.search.page_info.has_next_page);
        assert_eq!(data.rate_limit.remaining, 4990);
    }

    #[test]
    fn important_searches_cover_the_four_me_queries_without_a_page_cap() {
        let searches = important_searches("2026-10-26");
        let queries: Vec<&str> = searches
            .iter()
            .map(|search| search.query.as_str())
            .collect();
        assert_eq!(
            queries,
            vec![
                "is:pr is:open review-requested:@me",
                "is:pr is:open assignee:@me",
                "is:pr author:@me updated:>=2026-10-26",
                "is:pr is:open reviewed-by:@me -author:@me",
            ]
        );
        assert!(
            searches
                .iter()
                .all(|search| search.page_size == PAGE_SIZE && search.limit == SEARCH_RESULT_LIMIT)
        );
    }

    #[test]
    fn org_searches_fetch_depth_pages_of_fifty() {
        let orgs = vec![
            ("orbita-health".to_string(), 1),
            ("nimbus".to_string(), 3),
            ("vela".to_string(), 0),
            ("atlas".to_string(), 40),
        ];
        let searches = org_searches(Kind::Pr, &orgs, "2026-10-26");
        let plan: Vec<(&str, usize, usize)> = searches
            .iter()
            .map(|search| (search.query.as_str(), search.page_size, search.limit))
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:pr org:orbita-health updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50
                ),
                (
                    "is:pr org:nimbus updated:>=2026-10-26 sort:updated-desc",
                    50,
                    150
                ),
                (
                    "is:pr org:vela updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50
                ),
                (
                    "is:pr org:atlas updated:>=2026-10-26 sort:updated-desc",
                    50,
                    1000
                ),
            ]
        );
        assert!(
            searches
                .iter()
                .all(|search| search.involvement == Involvement::default())
        );
    }

    #[test]
    fn issue_hydrate_query_is_separate_and_defines_its_own_fragment() {
        assert!(ISSUE_HYDRATE_QUERY.contains("... on Issue"));
        assert!(ISSUE_HYDRATE_QUERY.contains("...IssueFields"));
        assert!(ISSUE_HYDRATE_QUERY.contains("fragment IssueFields on Issue"));
        assert!(!ISSUE_HYDRATE_QUERY.contains("... on PullRequest"));
        assert!(!HYDRATE_QUERY.contains("... on Issue"));
    }

    #[test]
    fn ids_query_accepts_pull_request_and_issue_nodes() {
        assert!(IDS_QUERY.contains("... on PullRequest"));
        assert!(IDS_QUERY.contains("... on Issue"));
    }

    #[test]
    fn ids_page_fixture_with_issue_nodes_skips_empty_nodes() {
        let envelope: Envelope<IdsData> =
            serde_json::from_str(include_str!("../tests/fixtures/search_page_issues.json"))
                .unwrap();
        let data = envelope.into_data(Utc::now()).unwrap().data;
        let ids: Vec<String> = data
            .search
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|node| node.id)
            .collect();
        assert_eq!(ids, vec!["I_kwDOOrbita0101", "I_kwDOOrbita0087"]);
        assert!(!data.search.page_info.has_next_page);
    }

    #[test]
    fn important_issue_searches_cover_the_four_me_queries_without_a_page_cap() {
        let searches = important_issue_searches("2026-10-26");
        let plan: Vec<(&str, Involvement)> = searches
            .iter()
            .map(|search| (search.query.as_str(), search.involvement))
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:issue is:open assignee:@me",
                    Involvement {
                        assigned: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue author:@me updated:>=2026-10-26",
                    Involvement {
                        authored: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue is:open mentions:@me updated:>=2026-10-26",
                    Involvement {
                        mentioned: true,
                        ..Involvement::default()
                    }
                ),
                (
                    "is:issue is:open commenter:@me -author:@me updated:>=2026-10-26",
                    Involvement {
                        commented: true,
                        ..Involvement::default()
                    }
                ),
            ]
        );
        assert!(searches.iter().all(|search| {
            search.page_size == PAGE_SIZE
                && search.limit == SEARCH_RESULT_LIMIT
                && search.org.is_none()
        }));
    }

    #[test]
    fn org_issue_searches_use_the_same_pages_and_limits_as_pull_requests() {
        let orgs = vec![
            ("orbita-health".to_string(), 1),
            ("nimbus".to_string(), 3),
            ("atlas".to_string(), 40),
        ];
        let searches = org_searches(Kind::Issue, &orgs, "2026-10-26");
        let plan: Vec<(&str, usize, usize, Option<&str>)> = searches
            .iter()
            .map(|search| {
                (
                    search.query.as_str(),
                    search.page_size,
                    search.limit,
                    search.org.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            plan,
            vec![
                (
                    "is:issue org:orbita-health updated:>=2026-10-26 sort:updated-desc",
                    50,
                    50,
                    Some("orbita-health")
                ),
                (
                    "is:issue org:nimbus updated:>=2026-10-26 sort:updated-desc",
                    50,
                    150,
                    Some("nimbus")
                ),
                (
                    "is:issue org:atlas updated:>=2026-10-26 sort:updated-desc",
                    50,
                    1000,
                    Some("atlas")
                ),
            ]
        );
    }

    #[test]
    fn orgs_with_more_names_each_org_by_its_own_search() {
        let found = |org: Option<&str>, has_more: bool| Found {
            involvement: Involvement::default(),
            ids: Vec::new(),
            has_more,
            org: org.map(str::to_string),
        };
        let results = vec![
            found(Some("vela"), false),
            found(Some("nimbus"), true),
            found(None, true),
            found(Some("atlas"), true),
        ];
        assert_eq!(orgs_with_more(&results), vec!["nimbus", "atlas"]);
    }

    #[test]
    fn issue_merge_ors_mentioned_and_commented_flags_and_sorts_by_update() {
        let issues = fixture_issues();
        let id = |number: u64| -> String {
            issues
                .iter()
                .find(|issue| issue.number == number)
                .unwrap()
                .id
                .0
                .clone()
        };
        let flag = |mentioned: bool, commented: bool, assigned: bool| Involvement {
            mentioned,
            commented,
            assigned,
            ..Involvement::default()
        };
        let merged = merge_ids(vec![
            (flag(false, false, true), vec![id(101), id(130)]),
            (flag(true, false, false), vec![id(101), id(87)]),
            (flag(false, true, false), vec![id(101), id(92)]),
        ]);

        let applied = apply_involvement(issues, merged);

        let numbers: Vec<u64> = applied.iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, vec![101, 130, 87, 92, 64]);
        let involvement: Vec<Involvement> = applied.iter().map(|issue| issue.involvement).collect();
        assert_eq!(
            involvement,
            vec![
                flag(true, true, true),
                flag(false, false, true),
                flag(true, false, false),
                flag(false, true, false),
                Involvement::default(),
            ]
        );
    }
}
